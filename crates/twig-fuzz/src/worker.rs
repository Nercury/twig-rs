//! Running twig-rs cases in a child process, so that a case that hangs, eats memory or crashes
//! the process (stack overflow, abort) cannot take the driver down.
//!
//! The child is the `twig-fuzz` binary itself (`twig-fuzz worker`): it reads one request per line
//! (`{"case": {...}, "oracle": {...}|null, "timeout_ms": N}`), runs it with
//! [`run_guarded`] and prints one line (a result in the oracle's
//! schema, or `{"unsupported": "why"}`). After a timeout it prints the timeout result and exits, so
//! the stuck thread dies with it; the parent starts a new child for the next case. A child that dies
//! without answering gives a result in phase `crash` ([`CRASH`]).
//!
//! A case that times out although the oracle finished it runs once more with as long as PHP was
//! allowed ([`Executor::run`]): the time limit is meant to catch hangs, and a case that is only
//! slow on a loaded machine (round 6's `ctl-15014-925-sbx` took twig-rs 14 s of the drive's 15)
//! would otherwise be reported as a mismatch.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use crate::kit::case::Case;
use crate::kit::json::Json;
use crate::kit::result::{CaseResult, ErrorDesc};
use crate::kit::run::{Outcome, run_guarded};
use crate::minimize::DEFAULT_TIME_LIMIT;

/// The class of the error reported when the worker process died while running a case.
pub const CRASH: &str = "TwigFuzz\\Crash";

/// The field of a twig-rs result that comes from the second run of a case that timed out
/// ([`Executor::run`]): that run's time limit in milliseconds. Results keep it in `twig-rs.json`;
/// the diff does not compare it.
pub const SECOND_RUN: &str = "second_run_timeout_ms";

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

    /// Runs one case, within `timeout`. A run that times out while the oracle finished the case
    /// (`oracle` is a result that is not an oracle failure) is run once more with the case's PHP
    /// time limit ([`second_chance`]), and its result carries [`SECOND_RUN`].
    pub fn run(&mut self, case: &Case, oracle: Option<&CaseResult>, timeout: Duration) -> Outcome {
        let outcome = self.run_once(case, oracle, timeout);
        let Some(longer) = second_chance(case, oracle, timeout, &outcome) else {
            return outcome;
        };
        let mut again = self.run_once(case, oracle, longer);
        if let Outcome::Ran(result) = &mut again {
            let ms = i64::try_from(longer.as_millis()).unwrap_or(i64::MAX);
            result.extra.push((SECOND_RUN.to_owned(), Json::int(ms)));
        }
        again
    }

    fn run_once(&mut self, case: &Case, oracle: Option<&CaseResult>, timeout: Duration) -> Outcome {
        match self {
            Executor::InProcess => run_guarded(case, oracle, timeout),
            Executor::Worker(worker) => worker.run(case, oracle, timeout),
        }
    }
}

/// The time limit of the second run of a case whose run gave `outcome` within `timeout`: when it
/// timed out while the oracle finished the case, as long as PHP was allowed to compute (the case's
/// `time_limit` in seconds, which the tools set to [`DEFAULT_TIME_LIMIT`]; for 0, PHP's "no limit",
/// the runner's default of 120), if that is longer than `timeout`.
pub fn second_chance(
    case: &Case,
    oracle: Option<&CaseResult>,
    timeout: Duration,
    outcome: &Outcome,
) -> Option<Duration> {
    let timed_out =
        matches!(outcome, Outcome::Ran(result) if result.phase.as_deref() == Some("timeout"));
    if !timed_out || !oracle.is_some_and(|o| !o.is_oracle_failure()) {
        return None;
    }
    let seconds = match case.field("time_limit") {
        None => DEFAULT_TIME_LIMIT,
        Some(value) => value
            .as_i64()
            .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
            .unwrap_or(DEFAULT_TIME_LIMIT),
    };
    let seconds = if seconds > 0 { seconds } else { 120 };
    let limit = Duration::from_secs(u64::try_from(seconds).unwrap_or(120));
    (limit > timeout).then_some(limit)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn case(json: &str) -> Case {
        Case::from_json(&Json::parse(json).expect("JSON")).expect("a case")
    }

    /// A case that times out where PHP finished it runs once more with the PHP time limit; where
    /// the oracle failed, or gave nothing, the timeout stands.
    #[test]
    fn a_timeout_where_php_finished_runs_again_with_the_php_time_limit() {
        let case = case(
            r#"{"id":"t","env":"plain","templates":{"index.twig":"{{ 1 + 1 }}"},"time_limit":3}"#,
        );
        let php = CaseResult::success(Json::str("t"), b"2".to_vec());
        let mut executor = Executor::InProcess;
        // (no run finishes in no time)
        let Outcome::Ran(result) = executor.run(&case, Some(&php), Duration::ZERO) else {
            panic!("unsupported");
        };
        assert!(result.ok, "{result:?}");
        assert_eq!(result.output.as_deref(), Some(&b"2"[..]));
        assert_eq!(result.extra(SECOND_RUN), Some(&Json::int(3000)));
        let fatal = CaseResult::failure(
            Json::str("t"),
            "fatal",
            ErrorDesc {
                class: "FatalError".to_owned(),
                message: b"Maximum execution time of 3 seconds exceeded".to_vec(),
                twig: None,
                code: None,
                previous: None,
            },
        );
        for oracle in [Some(&fatal), None] {
            let Outcome::Ran(result) = executor.run(&case, oracle, Duration::ZERO) else {
                panic!("unsupported");
            };
            assert_eq!(result.phase.as_deref(), Some("timeout"));
            assert!(result.extra(SECOND_RUN).is_none());
        }
        // the PHP time limit (30 s unless the case sets one; 120 s for PHP's "no limit") only when
        // it is longer than the timeout
        let timeout = run_guarded(&case, None, Duration::ZERO);
        let limit = |json: &str, after: u64| {
            second_chance(
                &self::case(json),
                Some(&php),
                Duration::from_secs(after),
                &timeout,
            )
        };
        let plain = r#"{"env":"plain","templates":{"index.twig":"x"}}"#;
        let zero = r#"{"env":"plain","templates":{"index.twig":"x"},"time_limit":"0"}"#;
        assert_eq!(limit(plain, 15), Some(Duration::from_secs(30)));
        assert_eq!(limit(plain, 30), None);
        assert_eq!(limit(zero, 15), Some(Duration::from_secs(120)));
        let ok = Outcome::Ran(Box::new(php.clone()));
        assert_eq!(second_chance(&case, Some(&php), Duration::ZERO, &ok), None);
    }
}
