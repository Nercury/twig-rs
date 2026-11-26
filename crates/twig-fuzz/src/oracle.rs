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
//!
//! Batches with `"isolation": "process"` get `"jobs"` ([`Oracle::jobs`], default 8,
//! `TWIG_FUZZ_ORACLE_JOBS`): the runner then runs that many cases at a time, each in a PHP process
//! of its own.
//!
//! # Containers that die or hang
//!
//! PHP can take the whole runner down with a case: its hard memory limit (`Hard memory limit of
//! 1073741824 bytes exhausted`, exit status 137: `number_format(x, PHP_INT_MAX)`, a `*` sprintf
//! width near `INT_MAX`) and crashes after a memory fatal error (status 139) end the process
//! without the JSON of the batch. A chunk whose container printed no usable result is run again
//! with process isolation (the dying case is then one child process, reported as `"phase":
//! "fatal"`, which [`compare`](crate::kit::diff::compare) treats as not comparable); a chunk that
//! dies even so is split in halves down to single cases, and a single case whose container still
//! dies gets a `fatal` result saying so. A container that cannot be started at all (Docker's own
//! exit statuses 125 to 127) is an error.
//!
//! Containers are named (`twig-fuzz-<pid>-<n>`, with the default command) so that they can be
//! killed: the runner is started with `--heartbeat` and reports each finished case on stderr, and a
//! container that reports nothing for longer than the slowest case of its batch may take (its
//! `wall_limit`, see runner.php, plus a minute) is killed and treated like one that died. The
//! runner itself kills isolated children that exceed their `wall_limit`, so this only catches
//! runners stuck outside a child.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::kit::json::Json;

/// The image `oracle.sh` uses by default.
pub const DEFAULT_IMAGE: &str = "twig-oracle:3.31.1";

/// Parallel children of an isolated batch unless `TWIG_FUZZ_ORACLE_JOBS` says otherwise.
pub const DEFAULT_JOBS: usize = 8;

/// The repository's `research/php-oracle` directory (build context of the image).
pub fn oracle_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../research/php-oracle")
}

/// The oracle client.
pub struct Oracle {
    command: Vec<String>,
    /// Cases per container in [`Oracle::run`].
    pub chunk: usize,
    /// Cases an isolated batch runs at a time (the batch's `"jobs"`).
    pub jobs: usize,
    server: Option<Server>,
    /// Batches sent to the current server (it is restarted every few hundred batches).
    served: usize,
    /// Containers started so far.
    pub containers: usize,
    /// Containers that died or hung and how the cases were run instead, in order.
    pub incidents: Vec<String>,
    /// Print each incident to stderr as it happens.
    pub verbose: bool,
}

/// What a container gave for a batch.
enum Exchange {
    /// Its standard output.
    Output(String),
    /// It ended (or was killed) without an answer: why.
    Died(String),
}

/// A batch's results, or why the container gave none.
enum Answer {
    Results(Vec<Json>),
    Died(String),
}

/// What the runner reports on stderr: its heartbeats and anything else it printed.
#[derive(Default)]
struct Activity {
    /// The runner announced heartbeats (`--heartbeat`): silence means it is stuck.
    armed: AtomicBool,
    last: Mutex<Option<Instant>>,
    text: Mutex<String>,
}

impl Activity {
    fn seen(&self) {
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }

    /// Whether the runner announced heartbeats and has been silent for longer than `idle`.
    fn stuck(&self, idle: Duration) -> bool {
        self.armed.load(Ordering::Acquire)
            && self
                .last
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some_and(|last| last.elapsed() > idle)
    }

    fn text(&self) -> String {
        self.text.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// A started oracle command.
struct Process {
    child: Child,
    /// The container's name (default command only), for `docker kill`.
    name: Option<String>,
    activity: Arc<Activity>,
    /// The thread reading stderr (it ends when the process has closed it).
    stderr_reader: Option<std::thread::JoinHandle<()>>,
}

impl Process {
    /// Waits until everything the process wrote to stderr is read (call it once the process has
    /// ended); then `activity` is complete.
    fn drain_stderr(&mut self) {
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }

    /// Whether the runner got as far as announcing its heartbeats (a process that ends without it
    /// was not a runner that died on a case but a command that did not start: no Docker daemon, no
    /// image, ...). Complete once [`Process::drain_stderr`] returned.
    fn runner_started(&self) -> bool {
        self.activity.armed.load(Ordering::Acquire)
    }

    fn kill(&mut self) {
        if let Some(name) = &self.name {
            let _ = Command::new("docker")
                .args(["kill", name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Server {
    process: Process,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.process.kill();
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
        let mut oracle = Oracle::with_command(command);
        if let Some(jobs) = std::env::var("TWIG_FUZZ_ORACLE_JOBS")
            .ok()
            .and_then(|j| j.trim().parse::<usize>().ok())
        {
            oracle.jobs = jobs.max(1);
        }
        oracle
    }

    /// An oracle running `command` (which must read a batch on stdin and print the results).
    pub fn with_command(command: Vec<String>) -> Oracle {
        Oracle {
            command,
            chunk: 2000,
            jobs: DEFAULT_JOBS,
            server: None,
            served: 0,
            containers: 0,
            incidents: Vec::new(),
            verbose: false,
        }
    }

    fn incident(&mut self, message: String) {
        if self.verbose {
            eprintln!("twig-fuzz: oracle: {message}");
        }
        self.incidents.push(message);
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
    /// The oracle could not be started at all.
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
    /// The server could not be started at all.
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
        let mut results = match self.send(cases, isolation, served)? {
            Answer::Results(results) => results,
            Answer::Died(why) => return self.rescue(cases, isolation, served, &why),
        };
        // cases that never ran because an earlier case died: run them again (a few rounds in one
        // process, then each in a process of its own)
        let mut rounds = 0;
        loop {
            let lost: Vec<usize> = (0..results.len())
                .filter(|&i| phase_of(&results[i]) == Some("lost"))
                .collect();
            if lost.is_empty() {
                break;
            }
            rounds += 1;
            if rounds > 5 {
                // the runner itself keeps dying at the same point
                for &i in &lost {
                    results[i] = died(&cases[i], "lost again and again");
                }
                break;
            }
            let retry: Vec<Json> = lost.iter().map(|&i| cases[i].clone()).collect();
            let mode = if rounds > 2 {
                Some("process")
            } else {
                isolation
            };
            let again = match self.send(&retry, mode, served)? {
                Answer::Results(again) => again,
                Answer::Died(why) => self.rescue(&retry, mode, served, &why)?,
            };
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
            let again = self.run_chunk(&retry, Some("process"), served)?;
            for (&i, result) in fatal.iter().zip(again) {
                results[i] = result;
            }
        }
        Ok(results)
    }

    /// Runs the cases of a container that died again: with process isolation, then in halves,
    /// then alone (see the module documentation).
    fn rescue(
        &mut self,
        cases: &[Json],
        isolation: Option<&str>,
        served: bool,
        why: &str,
    ) -> Result<Vec<Json>, String> {
        if isolation != Some("process") {
            self.incident(format!(
                "{} cases: the oracle died ({}); running them again with process isolation",
                cases.len(),
                head(why, 300)
            ));
            return self.run_chunk(cases, Some("process"), served);
        }
        if let [case] = cases {
            self.incident(format!(
                "case {}: the oracle died even with process isolation ({})",
                case.get("id").map(Json::to_compact).unwrap_or_default(),
                head(why, 300)
            ));
            return Ok(vec![died(case, why)]);
        }
        let (first, second) = cases.split_at(cases.len() / 2);
        let mut results = self.run_chunk(first, isolation, served)?;
        results.extend(self.run_chunk(second, isolation, served)?);
        Ok(results)
    }

    fn send(
        &mut self,
        cases: &[Json],
        isolation: Option<&str>,
        served: bool,
    ) -> Result<Answer, String> {
        let batch = match isolation {
            Some(mode) => {
                let mut fields = vec![
                    ("cases".to_owned(), Json::Array(cases.to_vec())),
                    ("isolation".to_owned(), Json::str(mode)),
                ];
                if mode == "process" && self.jobs > 1 {
                    fields.push(("jobs".to_owned(), Json::int(self.jobs as i64)));
                }
                Json::Object(fields)
            }
            None => Json::Array(cases.to_vec()),
        };
        let text = batch.to_compact();
        let idle = idle_timeout(cases);
        let exchange = if served {
            self.serve_line(&text, idle)?
        } else {
            self.run_container(&text, idle)?
        };
        let output = match exchange {
            Exchange::Output(output) => output,
            Exchange::Died(why) => return Ok(Answer::Died(why)),
        };
        let results = match Json::parse(output.trim()) {
            Ok(Json::Array(items)) => items,
            Ok(other) => {
                return Ok(Answer::Died(format!(
                    "it printed {}",
                    head(&other.to_compact(), 600)
                )));
            }
            Err(e) => {
                return Ok(Answer::Died(format!(
                    "it printed something that is not JSON ({e}): {}",
                    head(&output, 600)
                )));
            }
        };
        if results.len() != cases.len() {
            return Ok(Answer::Died(format!(
                "it returned {} results for {} cases: {}",
                results.len(),
                cases.len(),
                head(&output, 600)
            )));
        }
        Ok(Answer::Results(results))
    }

    fn spawn(&mut self, extra: &[&str]) -> Result<Process, String> {
        self.containers += 1;
        let mut args: Vec<String> = self.command[1..].to_vec();
        // name the container so that a stuck one can be killed (the docker client dying does not
        // stop it)
        let name = if self.command[0] == "docker" && args.first().map(String::as_str) == Some("run")
        {
            static NEXT: AtomicUsize = AtomicUsize::new(1);
            let name = format!(
                "twig-fuzz-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            args.splice(1..1, ["--name".to_owned(), name.clone()]);
            Some(name)
        } else {
            None
        };
        let mut child = Command::new(&self.command[0])
            .args(&args)
            .args(extra)
            .arg("--heartbeat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start the oracle ({}): {e}", self.command.join(" ")))?;
        let activity = Arc::new(Activity::default());
        activity.seen();
        let stderr_reader = child.stderr.take().map(|stderr| {
            // heartbeats; everything else is kept for messages (and the pipe never fills up)
            let activity = Arc::clone(&activity);
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).split(b'\n') {
                    let Ok(line) = line else { break };
                    activity.seen();
                    let line = String::from_utf8_lossy(&line);
                    let line = line.trim_end_matches('\r');
                    if line == "twig-oracle heartbeat" {
                        activity.armed.store(true, Ordering::Release);
                    } else if line != "." {
                        let mut text = activity.text.lock().unwrap_or_else(|e| e.into_inner());
                        if text.len() < 1 << 16 {
                            text.push_str(line);
                            text.push('\n');
                        }
                    }
                }
            })
        });
        Ok(Process {
            child,
            name,
            activity,
            stderr_reader,
        })
    }

    fn run_container(&mut self, batch: &str, idle: Duration) -> Result<Exchange, String> {
        let mut process = self.spawn(&[])?;
        let mut stdin = process.child.stdin.take().ok_or("no stdin")?;
        let input = batch.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
        let mut stdout = process.child.stdout.take().ok_or("no stdout")?;
        let reader = std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = stdout.read_to_end(&mut out);
            out
        });
        let status = loop {
            match process.child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(e) => return Err(format!("the oracle failed: {e}")),
            }
            if process.activity.stuck(idle) {
                process.kill();
                let _ = reader.join();
                return Ok(Exchange::Died(format!(
                    "no progress for {} s, container killed",
                    idle.as_secs()
                )));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let stdout = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
        process.drain_stderr();
        let stderr = process.activity.text();
        let code = status.code();
        if stdout.trim().is_empty() {
            if !process.runner_started() || matches!(code, Some(125..=127)) {
                // the runner never ran: Docker could not start the container (no daemon, no
                // image, bad command)
                return Err(format!(
                    "the oracle could not be started (exit {code:?}): {}",
                    head(&stderr, 600)
                ));
            }
            return Ok(Exchange::Died(format!(
                "exit {code:?} without output: {}",
                head(&stderr, 600)
            )));
        }
        if !status.success() && Json::parse(stdout.trim()).is_err() {
            return Ok(Exchange::Died(format!(
                "exit {code:?}: {}",
                head(&format!("{stderr}{stdout}"), 600)
            )));
        }
        Ok(Exchange::Output(stdout))
    }

    fn start_server(&mut self) -> Result<(), String> {
        let mut process = self.spawn(&["--serve"])?;
        let stdin = process.child.stdin.take().ok_or("no stdin")?;
        let stdout = process.child.stdout.take().ok_or("no stdout")?;
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        self.server = Some(Server {
            process,
            stdin,
            lines,
        });
        self.served = 0;
        Ok(())
    }

    fn serve_line(&mut self, batch: &str, idle: Duration) -> Result<Exchange, String> {
        for _ in 0..2 {
            if self.server.is_none() || self.served >= 400 {
                self.server = None;
                self.start_server()?;
            }
            self.served += 1;
            let server = self.server.as_mut().expect("started above");
            server.process.activity.seen();
            let sent = server
                .stdin
                .write_all(batch.as_bytes())
                .and_then(|()| server.stdin.write_all(b"\n"))
                .and_then(|()| server.stdin.flush());
            if sent.is_err() {
                // the server died after its previous batch: start another one
                self.server = None;
                continue;
            }
            loop {
                match server.lines.recv_timeout(Duration::from_millis(200)) {
                    Ok(line) => {
                        if line.trim_start().starts_with('[') && phase_list_has_fatal(&line) {
                            // the server printed its shutdown report and exits
                            self.server = None;
                        }
                        return Ok(Exchange::Output(line));
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if server.process.activity.stuck(idle) {
                            self.server = None;
                            return Ok(Exchange::Died(format!(
                                "no progress for {} s, server killed",
                                idle.as_secs()
                            )));
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        let mut server = self.server.take().expect("still there");
                        let status = server.process.child.wait().ok().and_then(|s| s.code());
                        server.process.drain_stderr();
                        let stderr = server.process.activity.text();
                        if !server.process.runner_started()
                            || (self.served == 1 && matches!(status, Some(125..=127)))
                        {
                            return Err(format!(
                                "the oracle server could not be started (exit {status:?}): {}",
                                head(&stderr, 600)
                            ));
                        }
                        return Ok(Exchange::Died(format!(
                            "the server closed its output (exit {status:?}): {}",
                            head(&stderr, 600)
                        )));
                    }
                }
            }
        }
        Err("the oracle server could not be started".to_owned())
    }
}

/// How long a runner may stay silent while it works on `cases`: the longest wall-clock limit of
/// one of them (runner.php's `wall_limit()`: the case's `wall_limit`, else twice its `time_limit`
/// (default 120 s) plus 30 s, at least 60 s), plus a minute.
fn idle_timeout(cases: &[Json]) -> Duration {
    let longest = cases
        .iter()
        .map(|case| {
            let number = |key: &str| {
                case.get(key).and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                })
            };
            number("wall_limit")
                .unwrap_or_else(|| (2 * number("time_limit").unwrap_or(120) + 30).max(60))
        })
        .max()
        .unwrap_or(60);
    Duration::from_secs(u64::try_from(longest.max(1)).unwrap_or(60) + 60)
}

/// The result of a case whose container died every time.
fn died(case: &Json, why: &str) -> Json {
    Json::Object(vec![
        (
            "id".to_owned(),
            case.get("id").cloned().unwrap_or(Json::Null),
        ),
        ("kind".to_owned(), Json::str("twig")),
        ("ok".to_owned(), Json::Bool(false)),
        ("phase".to_owned(), Json::str("fatal")),
        (
            "error".to_owned(),
            Json::Object(vec![
                ("class".to_owned(), Json::str("FatalError")),
                (
                    "message".to_owned(),
                    Json::str(format!(
                        "The oracle died while running the case alone: {why}"
                    )),
                ),
            ]),
        ),
    ])
}

fn phase_of(result: &Json) -> Option<&str> {
    result.get("phase").and_then(Json::as_str)
}

fn phase_list_has_fatal(line: &str) -> bool {
    line.contains("\"phase\":\"fatal\"") || line.contains("\"phase\":\"lost\"")
}

fn head(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() > limit {
        text.chars().take(limit).collect::<String>() + "..."
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timeouts_follow_the_wall_limits() {
        let cases = |json: &str| Json::parse(json).unwrap().items().to_vec();
        assert_eq!(idle_timeout(&cases("[{}]")), Duration::from_secs(270 + 60));
        assert_eq!(
            idle_timeout(&cases(r#"[{"time_limit":30},{"time_limit":"10"}]"#)),
            Duration::from_secs(90 + 60)
        );
        assert_eq!(
            idle_timeout(&cases(r#"[{"time_limit":1}]"#)),
            Duration::from_secs(60 + 60)
        );
        assert_eq!(
            idle_timeout(&cases(r#"[{"time_limit":30,"wall_limit":5}]"#)),
            Duration::from_secs(5 + 60)
        );
    }
}
