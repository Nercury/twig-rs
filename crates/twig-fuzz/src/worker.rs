//! Running twig-rs cases in a child process, so that a case that hangs, eats memory or crashes
//! the process (stack overflow, abort) cannot take the driver down.
//!
//! The child is the `twig-fuzz` binary itself (`twig-fuzz worker`): it reads one request per line
//! (`{"case": {...}, "oracle": {...}|null, "timeout_ms": N}`), runs it with
//! [`run_guarded`] and prints one line (a result in the oracle's
//! schema, or `{"unsupported": "why"}`). After a timeout it prints the timeout result and exits, so
//! the stuck thread dies with it; the parent starts a new child for the next case. A child that dies
//! without answering gives a result in phase `crash` ([`CRASH`]).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use crate::kit::case::Case;
use crate::kit::json::Json;
use crate::kit::result::{CaseResult, ErrorDesc};
use crate::kit::run::{Outcome, run_guarded};

/// The class of the error reported when the worker process died while running a case.
pub const CRASH: &str = "TwigFuzz\\Crash";

/// Runs cases in process (threads with a timeout) or in a worker process.
pub enum Executor {
    /// [`run_guarded`] on a thread of this process.
    InProcess,
    /// A `twig-fuzz worker` child process.
    Worker(WorkerProcess),
}

impl Executor {
    /// A worker process running `program` (normally `std::env::current_exe()`).
    pub fn worker(program: PathBuf) -> Executor {
        Executor::Worker(WorkerProcess {
            program,
            child: None,
            restarts: 0,
        })
    }

    /// Runs one case.
    pub fn run(&mut self, case: &Case, oracle: Option<&CaseResult>, timeout: Duration) -> Outcome {
        match self {
            Executor::InProcess => run_guarded(case, oracle, timeout),
            Executor::Worker(worker) => worker.run(case, oracle, timeout),
        }
    }
}

/// A `twig-fuzz worker` child process.
pub struct WorkerProcess {
    program: PathBuf,
    child: Option<Running>,
    /// Children started after the first one (timeouts and crashes).
    pub restarts: usize,
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl WorkerProcess {
    fn start(&mut self) -> Result<(), String> {
        let mut child = Command::new(&self.program)
            .arg("worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start {} worker: {e}", self.program.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        self.child = Some(Running {
            child,
            stdin,
            lines,
        });
        Ok(())
    }

    fn run(&mut self, case: &Case, oracle: Option<&CaseResult>, timeout: Duration) -> Outcome {
        let id = case.json.get("id").cloned().unwrap_or(Json::Null);
        if self.child.is_none()
            && let Err(error) = self.start()
        {
            return Outcome::Unsupported(error);
        }
        let request = Json::Object(vec![
            ("case".to_owned(), case.json.clone()),
            (
                "oracle".to_owned(),
                oracle.map_or(Json::Null, CaseResult::to_json),
            ),
            (
                "timeout_ms".to_owned(),
                Json::int(i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX)),
            ),
        ]);
        let running = self.child.as_mut().expect("started above");
        let sent = running
            .stdin
            .write_all(request.to_compact().as_bytes())
            .and_then(|()| running.stdin.write_all(b"\n"))
            .and_then(|()| running.stdin.flush());
        let answer = if sent.is_ok() {
            // the worker enforces the timeout itself; this margin covers a worker that hangs
            running
                .lines
                .recv_timeout(timeout + Duration::from_secs(5))
                .ok()
        } else {
            None
        };
        let outcome = match answer.as_deref().map(Json::parse) {
            Some(Ok(json)) => match json.get("unsupported").and_then(Json::as_str) {
                Some(reason) => Outcome::Unsupported(reason.to_owned()),
                None => Outcome::Ran(Box::new(CaseResult::from_json(&json))),
            },
            Some(Err(error)) => crash(
                id,
                format!("the worker printed something unexpected: {error}"),
            ),
            None => crash(
                id,
                "the twig-rs worker process died or hung while running the case".to_owned(),
            ),
        };
        let restart = match &outcome {
            Outcome::Ran(result) => {
                matches!(result.phase.as_deref(), Some("timeout" | "crash"))
            }
            Outcome::Unsupported(_) => false,
        };
        if restart {
            self.child = None;
            self.restarts += 1;
        }
        outcome
    }
}

fn crash(id: Json, message: String) -> Outcome {
    Outcome::Ran(Box::new(CaseResult::failure(
        id,
        "crash",
        ErrorDesc {
            class: CRASH.to_owned(),
            message: message.into_bytes(),
            twig: None,
            code: None,
            previous: None,
        },
    )))
}

/// The worker loop (`twig-fuzz worker`): runs requests from stdin until it closes, or until a case
/// times out (the stuck thread must not keep running).
pub fn serve_worker() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let (answer, stop) = match Json::parse(&line) {
            Err(error) => (
                Json::Object(vec![(
                    "unsupported".to_owned(),
                    Json::str(format!("invalid request: {error}")),
                )]),
                false,
            ),
            Ok(request) => {
                let timeout = Duration::from_millis(
                    request
                        .get("timeout_ms")
                        .and_then(Json::as_i64)
                        .and_then(|t| u64::try_from(t).ok())
                        .unwrap_or(10_000),
                );
                let oracle = request.get_non_null("oracle").map(CaseResult::from_json);
                match request.get("case").map(Case::from_json) {
                    Some(Ok(case)) => match run_guarded(&case, oracle.as_ref(), timeout) {
                        Outcome::Ran(result) => {
                            let stop = result.phase.as_deref() == Some("timeout");
                            (result.to_json(), stop)
                        }
                        Outcome::Unsupported(reason) => (
                            Json::Object(vec![("unsupported".to_owned(), Json::str(reason))]),
                            false,
                        ),
                    },
                    Some(Err(error)) => (
                        Json::Object(vec![("unsupported".to_owned(), Json::str(error))]),
                        false,
                    ),
                    None => (
                        Json::Object(vec![(
                            "unsupported".to_owned(),
                            Json::str("a request needs a case"),
                        )]),
                        false,
                    ),
                }
            }
        };
        let printed = writeln!(stdout, "{}", answer.to_compact()).and_then(|()| stdout.flush());
        if printed.is_err() || stop {
            // a timed-out case leaves a thread running: end the process with it
            std::process::exit(0);
        }
    }
}
