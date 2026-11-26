//! The PHP oracle (research/php-oracle) driven from Rust.
//!
//! The default command is what `research/php-oracle/oracle.sh` runs: `docker run --rm -i
//! twig-oracle:3.31.1 php runner.php` (the image name comes from `TWIG_ORACLE_IMAGE`, like the
//! script's). `TWIG_FUZZ_ORACLE` replaces the whole command, for example with
//! `bash research/php-oracle/oracle.sh` from Git Bash (on Windows a plain `bash` may be WSL's).
//!
//! Two modes:
//!
//! * [`Oracle::run`]: one container per chunk of cases (default 2000), for big batches. Cases lost
//!   to a PHP fatal error earlier in their chunk are run again; a case whose own run died is run
//!   once more alone in a separate process (`"isolation": "process"`), since a redeclared class
//!   or another case's state may have killed it.
//! * [`Oracle::run_served`]: one long-running container (`runner.php --serve`: a batch per input
//!   line, its results as one output line), for the many small batches of the minimiser. A server
//!   that died (fatal error) is restarted for the next batch.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::kit::json::Json;

/// The image `oracle.sh` uses by default.
pub const DEFAULT_IMAGE: &str = "twig-oracle:3.31.1";

/// The repository's `research/php-oracle` directory (build context of the image).
pub fn oracle_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../research/php-oracle")
}

/// The oracle client.
pub struct Oracle {
    command: Vec<String>,
    /// Cases per container in [`Oracle::run`].
    pub chunk: usize,
    server: Option<Server>,
    /// Batches sent to the current server (it is restarted every few hundred batches).
    served: usize,
    /// Containers started so far.
    pub containers: usize,
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Default for Oracle {
    fn default() -> Self {
        Oracle::from_env()
    }
}

impl Oracle {
    /// The oracle command from the environment (see the module documentation).
    pub fn from_env() -> Oracle {
        let command = match std::env::var("TWIG_FUZZ_ORACLE") {
            Ok(command) if !command.trim().is_empty() => {
                command.split_whitespace().map(str::to_owned).collect()
            }
            _ => {
                let image =
                    std::env::var("TWIG_ORACLE_IMAGE").unwrap_or_else(|_| DEFAULT_IMAGE.to_owned());
                [
                    "docker",
                    "run",
                    "--rm",
                    "-i",
                    image.as_str(),
                    "php",
                    "runner.php",
                ]
                .iter()
                .map(|s| (*s).to_owned())
                .collect()
            }
        };
        Oracle::with_command(command)
    }

    /// An oracle running `command` (which must read a batch on stdin and print the results).
    pub fn with_command(command: Vec<String>) -> Oracle {
        Oracle {
            command,
            chunk: 2000,
            server: None,
            served: 0,
            containers: 0,
        }
    }

    /// The command.
    pub fn command(&self) -> &[String] {
        &self.command
    }

    /// Builds the image when `docker image inspect` does not find it (what `oracle.sh` does).
    ///
    /// # Errors
    ///
    /// Docker is missing or the build failed.
    pub fn ensure_image(&self) -> Result<(), String> {
        if self.command.first().map(String::as_str) != Some("docker") {
            return Ok(());
        }
        let image = std::env::var("TWIG_ORACLE_IMAGE").unwrap_or_else(|_| DEFAULT_IMAGE.to_owned());
        let inspect = Command::new("docker")
            .args(["image", "inspect", &image])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("cannot run docker: {e}"))?;
        if inspect.success() {
            return Ok(());
        }
        let status = Command::new("docker")
            .args(["build", "-t", &image])
            .arg(oracle_dir())
            .status()
            .map_err(|e| format!("cannot run docker build: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("docker build -t {image} failed"))
        }
    }

    /// Runs `cases` (case objects) in chunks of [`Oracle::chunk`] and returns one result object per
    /// case, in order.
    ///
    /// # Errors
    ///
    /// The oracle could not be started or printed something that is not a result list.
    pub fn run(&mut self, cases: &[Json], isolation: Option<&str>) -> Result<Vec<Json>, String> {
        let mut results = Vec::with_capacity(cases.len());
        for chunk in cases.chunks(self.chunk.max(1)) {
            results.extend(self.run_chunk(chunk, isolation, false)?);
        }
        Ok(results)
    }

    /// Like [`Oracle::run`], through one long-running server container.
    ///
    /// # Errors
    ///
    /// The server could not be started or answered something that is not a result list.
    pub fn run_served(
        &mut self,
        cases: &[Json],
        isolation: Option<&str>,
    ) -> Result<Vec<Json>, String> {
        let mut results = Vec::with_capacity(cases.len());
        for chunk in cases.chunks(self.chunk.clamp(1, 500)) {
            results.extend(self.run_chunk(chunk, isolation, true)?);
        }
        Ok(results)
    }

    fn run_chunk(
        &mut self,
        cases: &[Json],
        isolation: Option<&str>,
        served: bool,
    ) -> Result<Vec<Json>, String> {
        let mut results = self.send(cases, isolation, served)?;
        // cases that never ran because an earlier case died: run them again
        let mut rounds = 0;
        while rounds < 10 {
            let lost: Vec<usize> = (0..results.len())
                .filter(|&i| phase_of(&results[i]) == Some("lost"))
                .collect();
            if lost.is_empty() {
                break;
            }
            rounds += 1;
            let retry: Vec<Json> = lost.iter().map(|&i| cases[i].clone()).collect();
            let again = self.send(&retry, isolation, served)?;
            for (&i, result) in lost.iter().zip(again) {
                results[i] = result;
            }
        }
        // a case whose own run died: once more alone, in a fresh process
        let fatal: Vec<usize> = (0..results.len())
            .filter(|&i| phase_of(&results[i]) == Some("fatal"))
            .collect();
        if !fatal.is_empty() && isolation != Some("process") {
            let retry: Vec<Json> = fatal.iter().map(|&i| cases[i].clone()).collect();
            let again = self.send(&retry, Some("process"), served)?;
            for (&i, result) in fatal.iter().zip(again) {
                results[i] = result;
            }
        }
        Ok(results)
    }

    fn send(
        &mut self,
        cases: &[Json],
        isolation: Option<&str>,
        served: bool,
    ) -> Result<Vec<Json>, String> {
        let batch = match isolation {
            Some(mode) => Json::Object(vec![
                ("cases".to_owned(), Json::Array(cases.to_vec())),
                ("isolation".to_owned(), Json::str(mode)),
            ]),
            None => Json::Array(cases.to_vec()),
        };
        let text = batch.to_compact();
        let output = if served {
            self.serve_line(&text)?
        } else {
            self.run_container(&text)?
        };
        let json = Json::parse(output.trim()).map_err(|e| {
            format!(
                "the oracle printed something that is not JSON ({e}): {}",
                head(&output)
            )
        })?;
        let results = match json {
            Json::Array(items) => items,
            other => return Err(format!("the oracle printed {}", head(&other.to_compact()))),
        };
        if results.len() != cases.len() {
            return Err(format!(
                "the oracle returned {} results for {} cases: {}",
                results.len(),
                cases.len(),
                head(&output)
            ));
        }
        Ok(results)
    }

    fn spawn(&mut self, extra: &[&str]) -> Result<Child, String> {
        self.containers += 1;
        Command::new(&self.command[0])
            .args(&self.command[1..])
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start the oracle ({}): {e}", self.command.join(" ")))
    }

    fn run_container(&mut self, batch: &str) -> Result<String, String> {
        let mut child = self.spawn(&[])?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let input = batch.to_owned();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
        let output = child
            .wait_with_output()
            .map_err(|e| format!("the oracle failed: {e}"))?;
        let _ = writer.join();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if stdout.trim().is_empty() {
            return Err(format!(
                "the oracle printed nothing (exit {:?}): {}",
                output.status.code(),
                head(&String::from_utf8_lossy(&output.stderr))
            ));
        }
        Ok(stdout)
    }

    fn start_server(&mut self) -> Result<(), String> {
        let mut child = self.spawn(&["--serve"])?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        if let Some(mut stderr) = child.stderr.take() {
            // drain PHP's display_errors output so the pipe never fills up
            std::thread::spawn(move || {
                let mut sink = Vec::new();
                let _ = stderr.read_to_end(&mut sink);
            });
        }
        self.server = Some(Server {
            child,
            stdin,
            stdout,
        });
        self.served = 0;
        Ok(())
    }

    fn serve_line(&mut self, batch: &str) -> Result<String, String> {
        for attempt in 0..2 {
            if self.server.is_none() || self.served >= 400 {
                self.server = None;
                self.start_server()?;
            }
            self.served += 1;
            let server = self.server.as_mut().expect("started above");
            let sent = server
                .stdin
                .write_all(batch.as_bytes())
                .and_then(|()| server.stdin.write_all(b"\n"))
                .and_then(|()| server.stdin.flush());
            if sent.is_err() {
                // the server died after its previous batch
                self.server = None;
                continue;
            }
            let mut line = String::new();
            let read = server.stdout.read_line(&mut line);
            match read {
                Ok(0) | Err(_) => {
                    self.server = None;
                    if attempt == 1 {
                        return Err("the oracle server closed its output".to_owned());
                    }
                }
                Ok(_) => {
                    if line.trim_start().starts_with('[') && phase_list_has_fatal(&line) {
                        // the server printed its shutdown report and exits
                        self.server = None;
                    }
                    return Ok(line);
                }
            }
        }
        Err("the oracle server could not be started".to_owned())
    }
}

fn phase_of(result: &Json) -> Option<&str> {
    result.get("phase").and_then(Json::as_str)
}

fn phase_list_has_fatal(line: &str) -> bool {
    line.contains("\"phase\":\"fatal\"") || line.contains("\"phase\":\"lost\"")
}

fn head(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() > 600 {
        text.chars().take(600).collect::<String>() + "..."
    } else {
        text.to_owned()
    }
}
