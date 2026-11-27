//! The known-failures ratchet: `tests/twig/known-failures.txt` lists the fixture trials expected to fail, one name per
//! line, sorted. A listed trial that fails passes (it is counted as a known failure); a listed trial that passes
//! fails the run until it is removed from the list; an unlisted trial that fails fails the run. With
//! `TWIG_FIXTURES_UPDATE=1` every fixture trial passes and the list is rewritten from the trials that ran.
//!
//! [`BUILD_KNOWN_FAILURES`] adds the trials that fail only in some builds (the default build's iconv charsets): they
//! are listed for those builds in the same way, and never written to the file, which stays the same for every build.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::runner::Outcome;

/// The environment variable that switches the run to update mode.
pub const UPDATE_VARIABLE: &str = "TWIG_FIXTURES_UPDATE";

/// The fixture trials that fail in this build only, by build: without the `glibc-iconv` feature, the conversions
/// through charsets beyond the default iconv backend (reference/deviations.md: the iconv charsets of the default
/// build; none of the fixtures needs one now). They count as known failures of this build, and the ratchet applies:
/// once one passes, the run fails until it is removed here.
#[cfg(not(feature = "glibc-iconv"))]
pub const BUILD_KNOWN_FAILURES: &[&str] = &[];
/// The fixture trials that fail in this build only (none with the `glibc-iconv` feature).
#[cfg(feature = "glibc-iconv")]
pub const BUILD_KNOWN_FAILURES: &[&str] = &[];

/// Where [`BUILD_KNOWN_FAILURES`] is, for messages.
const BUILD_LIST: &str = "BUILD_KNOWN_FAILURES (tests/fixtures/ratchet.rs)";

/// Whether this run rewrites the list: `TWIG_FIXTURES_UPDATE=1`.
pub fn update_mode() -> bool {
    std::env::var_os(UPDATE_VARIABLE).is_some_and(|v| v == "1")
}

/// The list as read from disk.
#[derive(Debug)]
pub struct KnownFailures {
    path: PathBuf,
    /// The path shown in messages (relative to the repository root).
    shown: String,
    /// The file content.
    text: String,
    /// The names in file order (`str::lines()`, so a checkout with CRLF line ends reads the same).
    lines: Vec<String>,
    names: BTreeSet<String>,
    /// [`BUILD_KNOWN_FAILURES`].
    build: BTreeSet<String>,
}

impl KnownFailures {
    /// Reads the list; a missing file is an empty list. `shown` is the path messages use.
    pub fn load(path: &Path, shown: &str) -> Result<KnownFailures, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("cannot read {shown}: {e}")),
        };
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let names = lines.iter().cloned().collect();
        Ok(KnownFailures {
            path: path.to_owned(),
            shown: shown.to_owned(),
            text,
            lines,
            names,
            build: BUILD_KNOWN_FAILURES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        })
    }

    /// Whether `name` is listed, in the file or in [`BUILD_KNOWN_FAILURES`].
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name) || self.build.contains(name)
    }

    /// Checks the file: one name per line (no blank lines, no surrounding whitespace), sorted, unique, and every
    /// name a fixture trial that runs (not skipped by its `--CONDITION--`).
    pub fn validate(&self, runnable: &BTreeSet<String>) -> Result<(), String> {
        let mut problems = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            let number = index + 1;
            if line.trim() != line || line.is_empty() {
                problems.push(format!("line {number}: {line:?} is not a bare trial name"));
            } else if !runnable.contains(line) {
                problems.push(format!(
                    "line {number}: {line:?} is not a fixture trial that runs (unknown, or skipped by its --CONDITION--)"
                ));
            }
            if let Some(previous) = index.checked_sub(1).map(|i| &self.lines[i])
                && previous.as_str() >= line.as_str()
            {
                problems.push(format!(
                    "line {number}: {line:?} is not sorted after {previous:?} (or is a duplicate)"
                ));
            }
            if self.build.contains(line) {
                problems.push(format!(
                    "line {number}: {line:?} is in {BUILD_LIST} already"
                ));
            }
        }
        for name in &self.build {
            if !runnable.contains(name) {
                problems.push(format!(
                    "{name:?} of {BUILD_LIST} is not a fixture trial that runs"
                ));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{} is invalid (run with {UPDATE_VARIABLE}=1 to rewrite it):\n{}",
                self.shown,
                problems.join("\n")
            ))
        }
    }

    /// The verdict of a fixture trial with the list applied.
    pub fn judge(&self, name: &str, outcome: &Outcome, update: bool) -> Result<(), String> {
        let listed = self.contains(name);
        let shown = &self.shown;
        // update mode rewrites the file, never the build list
        let remove = if self.build.contains(name) {
            format!("remove it from {BUILD_LIST}")
        } else {
            format!("remove it from {shown} (or run with {UPDATE_VARIABLE}=1)")
        };
        match outcome {
            _ if update => Ok(()),
            Outcome::Pass if listed => Err(format!("{name} passes now: {remove}")),
            Outcome::Skipped(_) if listed => {
                Err(format!("{name} is skipped by its --CONDITION--: {remove}"))
            }
            Outcome::Pass | Outcome::Skipped(_) => Ok(()),
            _ if listed => Ok(()),
            Outcome::Fail(failure) => Err(format!(
                "{failure}\n(an expected failure belongs in {shown}; run with {UPDATE_VARIABLE}=1 to update it)"
            )),
        }
    }

    /// Rewrites the list from a run: entries of trials that ran are replaced by their outcome, entries that are not
    /// runnable fixture trials are dropped, the others are kept (they were filtered out). The trials of
    /// [`BUILD_KNOWN_FAILURES`] are never written. Returns the number of names added and removed.
    pub fn rewrite(
        &self,
        recorded: &BTreeMap<String, Outcome>,
        runnable: &BTreeSet<String>,
    ) -> Result<(usize, usize), String> {
        let mut names: BTreeSet<String> = self
            .names
            .iter()
            .filter(|name| runnable.contains(*name) && !recorded.contains_key(*name))
            .filter(|name| !self.build.contains(*name))
            .cloned()
            .collect();
        names.extend(
            recorded
                .iter()
                .filter(|(name, outcome)| outcome.is_failure() && !self.build.contains(*name))
                .map(|(name, _)| name.clone()),
        );
        let added = names.difference(&self.names).count();
        let removed = self.names.difference(&names).count();
        let mut text = String::new();
        for name in &names {
            text.push_str(name);
            text.push('\n');
        }
        if text != self.text {
            std::fs::write(&self.path, text)
                .map_err(|e| format!("cannot write {}: {e}", self.shown))?;
        }
        Ok((added, removed))
    }
}

/// The outcomes of the fixture trials that ran (trials run on several threads).
#[derive(Debug, Default)]
pub struct Recorder {
    outcomes: Mutex<BTreeMap<String, Outcome>>,
}

impl Recorder {
    /// Records the outcome of a trial.
    pub fn record(&self, name: &str, outcome: Outcome) {
        self.outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_owned(), outcome);
    }

    /// The recorded outcomes.
    pub fn take(&self) -> BTreeMap<String, Outcome> {
        std::mem::take(&mut *self.outcomes.lock().unwrap_or_else(PoisonError::into_inner))
    }
}
