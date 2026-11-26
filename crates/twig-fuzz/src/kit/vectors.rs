//! Regression vector files: oracle cases with the results the oracle recorded, replayed without
//! Docker (tests/fuzz_regressions.rs, `twig-fuzz replay`).
//!
//! ```text
//! {
//!   "description": "what these vectors cover",
//!   "vectors": [
//!     {"id": "...",
//!      "case": {...an oracle "twig" case, "env": "plain"...},
//!      "oracle": {...the oracle's result for it (with "data"/"state" for data_php cases)...},
//!      "known_mismatch": {"reason": "...", "signature": "..."}}
//!   ]
//! }
//! ```
//!
//! A vector without `known_mismatch` must replay identically (deviation rules allowed); a vector
//! with it must still mismatch, so that a fix fails the replay until the marker is removed (a
//! ratchet, like `tests/twig/known-failures.txt`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::case::Case;
use super::diff::{Verdict, compare};
use super::json::Json;
use super::result::CaseResult;
use super::run::{Outcome, run_guarded};

/// One vector.
#[derive(Clone, Debug, PartialEq)]
pub struct Vector {
    /// The id (defaults to the case's id).
    pub id: String,
    /// The oracle case.
    pub case: Json,
    /// The oracle's result.
    pub oracle: Json,
    /// Why the vector is expected to mismatch, if it is.
    pub known_mismatch: Option<KnownMismatch>,
    /// Free text.
    pub note: Option<String>,
}

/// The marker of a vector that is known to mismatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownMismatch {
    /// Why (the bug, where it lives).
    pub reason: String,
    /// The mismatch signature when the vector was recorded (informational).
    pub signature: Option<String>,
}

/// A vector file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorFile {
    /// What the file covers.
    pub description: String,
    /// The vectors.
    pub vectors: Vec<Vector>,
}

impl VectorFile {
    /// Reads a vector file.
    ///
    /// # Errors
    ///
    /// The file cannot be read or does not have the format above.
    pub fn load(path: &Path) -> Result<VectorFile, String> {
        let json = Json::load(path)?;
        VectorFile::from_json(&json).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Reads the format above.
    ///
    /// # Errors
    ///
    /// A missing `vectors` list or a vector without `case`/`oracle`.
    pub fn from_json(json: &Json) -> Result<VectorFile, String> {
        let vectors = json
            .get("vectors")
            .ok_or("no \"vectors\" list")?
            .items()
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let case = v.get("case").ok_or(format!("vector {i} has no case"))?;
                let oracle = v.get("oracle").ok_or(format!("vector {i} has no oracle"))?;
                let id = v
                    .get("id")
                    .and_then(Json::as_str)
                    .map(str::to_owned)
                    .or_else(|| case.get("id").and_then(Json::as_str).map(str::to_owned))
                    .unwrap_or_else(|| format!("#{i}"));
                let known_mismatch = v.get_non_null("known_mismatch").map(|m| KnownMismatch {
                    reason: m
                        .get("reason")
                        .and_then(Json::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    signature: m.get("signature").and_then(Json::as_str).map(str::to_owned),
                });
                Ok(Vector {
                    id,
                    case: case.clone(),
                    oracle: oracle.clone(),
                    known_mismatch,
                    note: v.get("note").and_then(Json::as_str).map(str::to_owned),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(VectorFile {
            description: json
                .get("description")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_owned(),
            vectors,
        })
    }

    /// The JSON form (one line per vector field, values compact).
    pub fn to_json(&self) -> Json {
        let vectors = self
            .vectors
            .iter()
            .map(|v| {
                let mut fields = vec![("id".to_owned(), Json::str(&v.id))];
                if let Some(note) = &v.note {
                    fields.push(("note".to_owned(), Json::str(note)));
                }
                fields.push(("case".to_owned(), v.case.clone()));
                fields.push(("oracle".to_owned(), v.oracle.clone()));
                if let Some(known) = &v.known_mismatch {
                    let mut marker = vec![("reason".to_owned(), Json::str(&known.reason))];
                    if let Some(signature) = &known.signature {
                        marker.push(("signature".to_owned(), Json::str(signature)));
                    }
                    fields.push(("known_mismatch".to_owned(), Json::Object(marker)));
                }
                Json::Object(fields)
            })
            .collect();
        Json::Object(vec![
            ("description".to_owned(), Json::str(&self.description)),
            ("vectors".to_owned(), Json::Array(vectors)),
        ])
    }

    /// Writes the file.
    ///
    /// # Errors
    ///
    /// The file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let mut text = self.to_json().to_layout(3);
        text.push('\n');
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

/// How a vector replayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayStatus {
    /// As expected (identical, or still mismatching for a `known_mismatch` vector).
    Pass,
    /// Identical once deviation rules were applied.
    PassWithDeviation(Vec<&'static str>),
    /// A `known_mismatch` vector that mismatches with another signature than recorded.
    PassSignatureChanged {
        /// The recorded signature.
        recorded: String,
        /// The signature now.
        now: String,
    },
    /// Not as expected (why).
    Fail(String),
}

/// The replay of one vector.
#[derive(Clone, Debug)]
pub struct Replayed {
    /// The file.
    pub file: PathBuf,
    /// The vector id.
    pub id: String,
    /// The status.
    pub status: ReplayStatus,
}

/// Replays one vector.
pub fn replay(vector: &Vector, timeout: Duration) -> ReplayStatus {
    let case = match Case::from_json(&vector.case) {
        Ok(case) => case,
        Err(error) => return ReplayStatus::Fail(format!("invalid case: {error}")),
    };
    let oracle = CaseResult::from_json(&vector.oracle);
    let actual = match run_guarded(&case, Some(&oracle), timeout) {
        Outcome::Ran(result) => *result,
        Outcome::Unsupported(reason) => {
            return ReplayStatus::Fail(format!("the twig-rs runner cannot run it: {reason}"));
        }
    };
    let verdict = compare(&oracle, &actual);
    match (&vector.known_mismatch, verdict) {
        (_, Verdict::Skip(reason)) => ReplayStatus::Fail(format!("not comparable: {reason}")),
        (None, Verdict::Same) => ReplayStatus::Pass,
        (None, Verdict::Deviation(rules)) => ReplayStatus::PassWithDeviation(rules),
        (None, Verdict::Mismatch(mismatch)) => ReplayStatus::Fail(format!(
            "twig-rs differs from the oracle ({}):\n{}",
            mismatch.signature(),
            mismatch.describe()
        )),
        (Some(known), Verdict::Same | Verdict::Deviation(_)) => ReplayStatus::Fail(format!(
            "marked known_mismatch ({}) but twig-rs now matches the oracle: remove the marker",
            known.reason
        )),
        (Some(known), Verdict::Mismatch(mismatch)) => {
            let now = mismatch.signature();
            match &known.signature {
                Some(recorded) if *recorded != now => ReplayStatus::PassSignatureChanged {
                    recorded: recorded.clone(),
                    now,
                },
                _ => ReplayStatus::Pass,
            }
        }
    }
}

/// Every `*.json` file under `dir` (recursively), sorted.
pub fn vector_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Replays every vector of every file; a file that cannot be read is one failed entry.
pub fn replay_files(files: &[PathBuf], timeout: Duration) -> Vec<Replayed> {
    let mut out = Vec::new();
    for file in files {
        match VectorFile::load(file) {
            Ok(vectors) => {
                for vector in &vectors.vectors {
                    out.push(Replayed {
                        file: file.clone(),
                        id: vector.id.clone(),
                        status: replay(vector, timeout),
                    });
                }
            }
            Err(error) => out.push(Replayed {
                file: file.clone(),
                id: String::new(),
                status: ReplayStatus::Fail(error),
            }),
        }
    }
    out
}
