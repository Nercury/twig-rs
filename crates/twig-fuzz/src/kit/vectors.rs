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
//! ratchet, like `tests/twig/known-failures.txt`). [`update_file`] (`twig-fuzz replay --update`)
//! removes the markers of fixed vectors and [`refresh_file`] (`twig-fuzz refresh`) records oracle
//! results again; both edit the file's text in place and change nothing else.

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

/// Runs one vector's case through twig-rs and compares the result with the recorded one; `Err` when
/// the case is invalid or the runner cannot run it.
pub fn replay_verdict(vector: &Vector, timeout: Duration) -> Result<Verdict, String> {
    let case = Case::from_json(&vector.case).map_err(|error| format!("invalid case: {error}"))?;
    let oracle = CaseResult::from_json(&vector.oracle);
    match run_guarded(&case, Some(&oracle), timeout) {
        Outcome::Ran(actual) => Ok(compare(&oracle, &actual)),
        Outcome::Unsupported(reason) => Err(format!("the twig-rs runner cannot run it: {reason}")),
    }
}

/// Replays one vector.
pub fn replay(vector: &Vector, timeout: Duration) -> ReplayStatus {
    match replay_verdict(vector, timeout) {
        Ok(verdict) => status(vector, verdict),
        Err(error) => ReplayStatus::Fail(error),
    }
}

/// How a verdict counts for a vector (marked or not).
fn status(vector: &Vector, verdict: Verdict) -> ReplayStatus {
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

/// What [`update_file`] did with one vector.
#[derive(Clone, Debug)]
pub struct Updated {
    /// The vector id.
    pub id: String,
    /// How the vector replayed before the update.
    pub status: ReplayStatus,
    /// Whether its `known_mismatch` marker was removed (it replays as [`ReplayStatus::Pass`] or
    /// [`ReplayStatus::PassWithDeviation`] now).
    pub dropped: bool,
}

/// `twig-fuzz replay --update`: replays every vector of the file at `path` and removes the
/// `known_mismatch` marker of each marked vector that now matches the oracle (identical, or
/// identical up to the deviation rules), so that a fixed vector flips into a plain regression test
/// mechanically. Nothing else changes: a marked vector that still mismatches keeps its marker, also
/// when the mismatch has another signature than recorded (it is reported as
/// [`ReplayStatus::PassSignatureChanged`] and left for a person to look at), and so does a vector
/// that cannot be compared. `only` limits the update to these vector ids.
///
/// The file is edited in place as text: only the removed `"known_mismatch": {...}` members (with
/// their separating comma) disappear, so its layout stays as it was. The edited text is parsed again
/// and must equal the old file without those markers, or nothing is written.
///
/// # Errors
///
/// The file cannot be read, parsed or written.
pub fn update_file(
    path: &Path,
    timeout: Duration,
    only: Option<&[String]>,
) -> Result<Vec<Updated>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let file = VectorFile::from_json(
        &Json::parse(text.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut updated = Vec::with_capacity(file.vectors.len());
    for vector in &file.vectors {
        let verdict = replay_verdict(vector, timeout);
        let wanted = only.is_none_or(|ids| ids.contains(&vector.id));
        let dropped = wanted
            && vector.known_mismatch.is_some()
            && matches!(verdict, Ok(Verdict::Same | Verdict::Deviation(_)));
        updated.push(Updated {
            id: vector.id.clone(),
            status: match verdict {
                Ok(verdict) => status(vector, verdict),
                Err(error) => ReplayStatus::Fail(error),
            },
            dropped,
        });
    }
    let drop: Vec<usize> = (0..updated.len()).filter(|&i| updated[i].dropped).collect();
    if drop.is_empty() {
        return Ok(updated);
    }
    let edited = without_markers(&text, &drop)
        .map_err(|e| format!("cannot edit {}: {e}", path.display()))?;
    let mut expected = file.clone();
    for &i in &drop {
        expected.vectors[i].known_mismatch = None;
    }
    let reparsed = Json::parse(edited.trim_start_matches('\u{feff}'))
        .map_err(|e| e.to_string())
        .and_then(|json| VectorFile::from_json(&json));
    if reparsed.as_ref() != Ok(&expected) {
        return Err(format!(
            "editing {} would change more than the markers; nothing written",
            path.display()
        ));
    }
    std::fs::write(path, edited).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(updated)
}

/// Runs case objects through the oracle and returns its result objects, in order.
pub type OracleFn<'a> = dyn FnMut(&[Json]) -> Result<Vec<Json>, String> + 'a;

/// `twig-fuzz refresh`: records again the oracle results of the vectors of the file at `path` whose
/// ids are in `only` (all vectors when `only` is `None`), with `oracle` (a function from case
/// objects to result objects, normally the Docker oracle), and writes them in place of the old
/// ones. Only the `oracle` values change, edited as text like [`update_file`] does; the markers stay
/// (replay, then `replay --update`). This is how a vector gets what the oracle records today, for
/// example the `props` of date objects in the typed data trees. Returns the ids refreshed.
///
/// # Errors
///
/// The file cannot be read, parsed or written, or the oracle failed.
pub fn refresh_file(
    path: &Path,
    only: Option<&[String]>,
    oracle: &mut OracleFn<'_>,
) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let file = VectorFile::from_json(
        &Json::parse(text.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    let chosen: Vec<usize> = (0..file.vectors.len())
        .filter(|&i| only.is_none_or(|ids| ids.contains(&file.vectors[i].id)))
        .collect();
    if chosen.is_empty() {
        return Ok(Vec::new());
    }
    let cases: Vec<Json> = chosen
        .iter()
        .map(|&i| file.vectors[i].case.clone())
        .collect();
    let results = oracle(&cases)?;
    if results.len() != cases.len() {
        return Err(format!(
            "{} oracle results for {} cases",
            results.len(),
            cases.len()
        ));
    }
    let mut expected = file.clone();
    let mut replacements: Vec<(usize, String)> = Vec::new();
    for (&i, result) in chosen.iter().zip(&results) {
        if CaseResult::from_json(result).is_oracle_failure() {
            return Err(format!(
                "the oracle failed on {}: {}",
                file.vectors[i].id,
                result.to_compact()
            ));
        }
        expected.vectors[i].oracle = result.clone();
        replacements.push((i, result.to_compact()));
    }
    let edited = with_members_replaced(&text, "oracle", &replacements)
        .map_err(|e| format!("cannot edit {}: {e}", path.display()))?;
    let reparsed = Json::parse(edited.trim_start_matches('\u{feff}'))
        .map_err(|e| e.to_string())
        .and_then(|json| VectorFile::from_json(&json));
    if reparsed.as_ref() != Ok(&expected) {
        return Err(format!(
            "editing {} would change more than the oracle results; nothing written",
            path.display()
        ));
    }
    std::fs::write(path, edited).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(chosen.iter().map(|&i| file.vectors[i].id.clone()).collect())
}

/// The members of the vector objects of a vector file's text, per vector (in order).
fn vector_members(text: &str) -> Result<Vec<Vec<Member>>, String> {
    let bytes = text.as_bytes();
    let start = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let mut scan = Scan {
        bytes,
        position: start,
    };
    let mut out = Vec::new();
    for top in scan.members()? {
        if top.key != "vectors" {
            continue;
        }
        let mut inner = Scan {
            bytes,
            position: top.value_start,
        };
        out.clear();
        for element in inner.elements()? {
            let mut object = Scan {
                bytes,
                position: element,
            };
            out.push(object.members()?);
        }
    }
    Ok(out)
}

/// `text` (a vector file) with the value of member `key` of the vectors at the given indexes
/// replaced by the given JSON texts.
fn with_members_replaced(
    text: &str,
    key: &str,
    replacements: &[(usize, String)],
) -> Result<String, String> {
    let vectors = vector_members(text)?;
    let mut cuts: Vec<(usize, usize, &str)> = Vec::new();
    for (index, value) in replacements {
        let member = vectors
            .get(*index)
            .and_then(|members| members.iter().find(|m| m.key == key))
            .ok_or_else(|| format!("vector {index} has no {key:?}"))?;
        cuts.push((member.value_start, member.end, value));
    }
    cuts.sort_unstable_by_key(|c| c.0);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (from, to, value) in cuts {
        out.push_str(&text[last..from]);
        out.push_str(value);
        last = to;
    }
    out.push_str(&text[last..]);
    Ok(out)
}

/// `text` (a vector file) without the `known_mismatch` members of the vectors at the indexes
/// `drop`, each removed with the comma that separates it from its neighbour.
fn without_markers(text: &str, drop: &[usize]) -> Result<String, String> {
    let vectors = vector_members(text)?;
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    for &index in drop {
        let members = vectors
            .get(index)
            .ok_or_else(|| format!("no vector {index}"))?;
        for (k, member) in members.iter().enumerate() {
            if member.key != "known_mismatch" {
                continue;
            }
            cuts.push(if k > 0 {
                (members[k - 1].end, member.end)
            } else if let Some(next) = members.get(1) {
                (member.start, next.start)
            } else {
                return Err(format!("vector {index} has nothing but its marker"));
            });
        }
    }
    cuts.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (from, to) in cuts {
        if from < last {
            return Err("overlapping members".to_owned());
        }
        out.push_str(&text[last..from]);
        last = to;
    }
    out.push_str(&text[last..]);
    Ok(out)
}

/// A member of a JSON object in a text: its key and byte offsets.
struct Member {
    key: String,
    /// Where the key's opening quote is.
    start: usize,
    /// Where the value starts.
    value_start: usize,
    /// Just after the value.
    end: usize,
}

/// A scanner over JSON text that records where members and elements are (the text is valid JSON:
/// it was parsed before).
struct Scan<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Scan<'_> {
    fn whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.bytes.get(self.position) {
            self.position += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        self.whitespace();
        if self.bytes.get(self.position) == Some(&byte) {
            self.position += 1;
            Ok(())
        } else {
            Err(format!(
                "expected {:?} at byte {}",
                char::from(byte),
                self.position
            ))
        }
    }

    /// Skips a string literal and returns its value.
    fn string(&mut self) -> Result<String, String> {
        self.whitespace();
        let start = self.position;
        self.skip_string()?;
        let raw =
            std::str::from_utf8(&self.bytes[start..self.position]).map_err(|e| e.to_string())?;
        match Json::parse(raw)? {
            Json::String(s) => Ok(s),
            _ => Err(format!("expected a string at byte {start}")),
        }
    }

    fn skip_string(&mut self) -> Result<(), String> {
        if self.bytes.get(self.position) != Some(&b'"') {
            return Err(format!("expected a string at byte {}", self.position));
        }
        self.position += 1;
        loop {
            match self.bytes.get(self.position) {
                Some(b'"') => {
                    self.position += 1;
                    return Ok(());
                }
                Some(b'\\') => self.position += 2,
                Some(_) => self.position += 1,
                None => return Err("unterminated string".to_owned()),
            }
        }
    }

    fn skip_value(&mut self) -> Result<(), String> {
        self.whitespace();
        match self.bytes.get(self.position) {
            Some(b'{') => self.members().map(|_| ()),
            Some(b'[') => self.elements().map(|_| ()),
            Some(b'"') => self.skip_string(),
            Some(_) => {
                while let Some(&b) = self.bytes.get(self.position) {
                    if matches!(b, b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r') {
                        break;
                    }
                    self.position += 1;
                }
                Ok(())
            }
            None => Err("unexpected end".to_owned()),
        }
    }

    /// The members of the object at the position (which is left after it).
    fn members(&mut self) -> Result<Vec<Member>, String> {
        self.eat(b'{')?;
        let mut out = Vec::new();
        self.whitespace();
        if self.bytes.get(self.position) == Some(&b'}') {
            self.position += 1;
            return Ok(out);
        }
        loop {
            self.whitespace();
            let start = self.position;
            let key = self.string()?;
            self.eat(b':')?;
            self.whitespace();
            let value_start = self.position;
            self.skip_value()?;
            out.push(Member {
                key,
                start,
                value_start,
                end: self.position,
            });
            self.whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    return Ok(out);
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.position)),
            }
        }
    }

    /// Where the elements of the array at the position start (the position is left after it).
    fn elements(&mut self) -> Result<Vec<usize>, String> {
        self.eat(b'[')?;
        let mut out = Vec::new();
        self.whitespace();
        if self.bytes.get(self.position) == Some(&b']') {
            self.position += 1;
            return Ok(out);
        }
        loop {
            self.whitespace();
            out.push(self.position);
            self.skip_value()?;
            self.whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(out);
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.position)),
            }
        }
    }
}
