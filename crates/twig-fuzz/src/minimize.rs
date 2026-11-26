//! Delta debugging (ddmin, Zeller & Hildebrandt) of oracle cases.
//!
//! The minimiser shrinks a case while a [`Predicate`] says the candidate still reproduces the
//! mismatch; [`SameSignature`] is the usual predicate: the oracle and twig-rs still disagree with the
//! same [signature](crate::kit::diff::Mismatch::signature). Candidates of one ddmin step are tested
//! in one batch (one oracle round trip), and every tested candidate is cached.
//!
//! Passes, repeated until nothing shrinks:
//!
//! 1. templates other than the main one are dropped one at a time;
//! 2. each template is reduced by ddmin over its lines, then its tokens (Twig delimiters, words,
//!    numbers, quoted strings, whitespace runs, single punctuation bytes), then its bytes (for
//!    templates of at most [`Options::byte_limit`] bytes);
//! 3. `data_json` is reduced by ddmin over the entries of each array/object level, deepest last;
//!    `data_php` loses top-level keys (the code is wrapped and `unset()`s them; the keys come from
//!    the oracle's typed tree of the data);
//! 4. the `config`, `globals`, `extensions`, `sandbox`, `lexer` and `preload` entries are dropped
//!    one at a time (and `test_classes`, when [`Options::drop_test_classes`] says candidates run in
//!    fresh processes).
//!
//! Candidates must not see each other: PHP compiles a template class once per process (with `cache
//! => false` a later candidate with the same source and options reuses it, without its
//! compile-time deprecations), classes declared by one candidate's `data_php` or `test_classes`
//! stay declared for the next, and a filesystem cache directory holds the classes an earlier
//! candidate compiled. [`SameSignature`] therefore runs every candidate in a PHP process of its own
//! (`"isolation": "process"`, see [`ISOLATION`]) and gives each one a cache directory of its own
//! when the case has a filesystem cache.

use std::collections::HashMap;
use std::time::Duration;

use crate::kit::case::{Case, CaseData};
use crate::kit::diff::{Verdict, compare};
use crate::kit::json::Json;
use crate::kit::result::CaseResult;
use crate::kit::run::Outcome;
use crate::oracle::Oracle;
use crate::worker::Executor;

/// Decides which candidates still reproduce.
pub trait Predicate {
    /// One answer per candidate case object.
    fn test(&mut self, candidates: &[Json]) -> Vec<bool>;
}

/// Minimiser settings.
#[derive(Clone, Debug)]
pub struct Options {
    /// Templates longer than this are not reduced byte by byte.
    pub byte_limit: usize,
    /// Upper bound of predicate evaluations (candidates), to bound the cost of one case.
    pub max_tests: usize,
    /// Print each batch of candidates to stderr.
    pub verbose: bool,
    /// Try dropping `test_classes`. Only sound when every candidate runs in a fresh PHP process:
    /// in a shared one the classes an earlier candidate declared stay declared, so PHP would not
    /// notice that a candidate needs them while twig-rs would.
    pub drop_test_classes: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            byte_limit: 400,
            max_tests: 5_000,
            verbose: false,
            drop_test_classes: false,
        }
    }
}

/// The result of a minimisation.
#[derive(Clone, Debug)]
pub struct Minimized {
    /// The smallest case found.
    pub case: Json,
    /// Candidates tested (cache hits excluded).
    pub tests: usize,
}

struct Session<'p> {
    predicate: &'p mut dyn Predicate,
    cache: HashMap<String, bool>,
    tests: usize,
    options: Options,
}

impl Session<'_> {
    /// Tests candidates through the cache; the answers follow the candidates' order.
    fn test(&mut self, candidates: &[Json]) -> Vec<bool> {
        let keys: Vec<String> = candidates.iter().map(Json::to_compact).collect();
        let mut fresh: Vec<Json> = Vec::new();
        let mut fresh_keys: Vec<String> = Vec::new();
        for (key, candidate) in keys.iter().zip(candidates) {
            if !self.cache.contains_key(key) && !fresh_keys.contains(key) {
                fresh_keys.push(key.clone());
                fresh.push(candidate.clone());
            }
        }
        if !fresh.is_empty() {
            if self.tests >= self.options.max_tests {
                for key in &fresh_keys {
                    self.cache.insert(key.clone(), false);
                }
            } else {
                self.tests += fresh.len();
                let started = std::time::Instant::now();
                let answers = self.predicate.test(&fresh);
                if self.options.verbose {
                    eprintln!(
                        "twig-fuzz: minimise: {} candidates, {} reproduce ({} tested, {} ms)",
                        fresh.len(),
                        answers.iter().filter(|&&a| a).count(),
                        self.tests,
                        started.elapsed().as_millis()
                    );
                }
                for (key, answer) in fresh_keys.into_iter().zip(answers) {
                    self.cache.insert(key, answer);
                }
            }
        }
        keys.iter()
            .map(|k| self.cache.get(k).copied().unwrap_or(false))
            .collect()
    }

    /// ddmin over `items`: the smallest sub-sequence for which `build` makes a reproducing case.
    fn ddmin<T: Clone>(&mut self, items: Vec<T>, build: &dyn Fn(&[T]) -> Json) -> Vec<T> {
        let mut items = items;
        if items.is_empty() {
            return items;
        }
        // the empty sequence first: it often reproduces (an unused template, data, ...)
        if self.test(&[build(&[])])[0] {
            return Vec::new();
        }
        let mut n = 2usize;
        while items.len() >= 2 {
            let n_now = n.min(items.len());
            let ranges = split(items.len(), n_now);
            let mut candidates: Vec<Vec<T>> = Vec::new();
            for r in &ranges {
                candidates.push(items[r.clone()].to_vec());
            }
            if n_now > 2 {
                for r in &ranges {
                    let mut complement = items[..r.start].to_vec();
                    complement.extend_from_slice(&items[r.end..]);
                    candidates.push(complement);
                }
            }
            let jsons: Vec<Json> = candidates.iter().map(|c| build(c)).collect();
            let answers = self.test(&jsons);
            if let Some(i) = (0..ranges.len()).find(|&i| answers[i]) {
                items = candidates[i].clone();
                n = 2;
                continue;
            }
            if n_now > 2
                && let Some(i) = (0..ranges.len()).find(|&i| answers[ranges.len() + i])
            {
                items = candidates[ranges.len() + i].clone();
                n = (n_now - 1).max(2);
                continue;
            }
            if n_now >= items.len() {
                break;
            }
            n = (n_now * 2).min(items.len());
        }
        items
    }
}

fn split(len: usize, n: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::with_capacity(n);
    let mut start = 0;
    for i in 0..n {
        let end = len * (i + 1) / n;
        if end > start {
            out.push(start..end);
        }
        start = end;
    }
    out
}

/// Minimises `case` (which must reproduce) under `predicate`.
pub fn minimize(case: &Json, predicate: &mut dyn Predicate, options: Options) -> Minimized {
    let mut session = Session {
        predicate,
        cache: HashMap::new(),
        tests: 0,
        options,
    };
    session.cache.insert(case.to_compact(), true);
    let mut current = case.clone();
    loop {
        let before = current.to_compact().len();
        current = drop_templates(&mut session, current);
        current = reduce_templates(&mut session, current);
        current = reduce_data(&mut session, current);
        current = drop_settings(&mut session, current);
        if current.to_compact().len() >= before || session.tests >= session.options.max_tests {
            break;
        }
    }
    Minimized {
        case: current,
        tests: session.tests,
    }
}

fn parsed(case: &Json) -> Option<Case> {
    Case::from_json(case).ok()
}

fn drop_templates(session: &mut Session<'_>, mut current: Json) -> Json {
    loop {
        let Some(case) = parsed(&current) else {
            return current;
        };
        let others: Vec<usize> = (0..case.templates.len())
            .filter(|&i| case.templates[i].0 != case.main)
            .collect();
        if others.is_empty() {
            return current;
        }
        let candidates: Vec<Json> = others
            .iter()
            .map(|&skip| {
                let kept: Vec<(String, Vec<u8>)> = case
                    .templates
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != skip)
                    .map(|(_, t)| t.clone())
                    .collect();
                with_main(&case, &kept)
            })
            .collect();
        let answers = session.test(&candidates);
        match answers.iter().position(|&a| a) {
            Some(i) => current = candidates[i].clone(),
            None => return current,
        }
    }
}

/// The case with these templates, keeping its main template explicit.
fn with_main(case: &Case, templates: &[(String, Vec<u8>)]) -> Json {
    let mut json = case.with_templates(templates);
    if case.json.get("main").is_none() && case.main != "index.twig" {
        json.set("main", Json::str(&case.main));
    }
    json
}

fn reduce_templates(session: &mut Session<'_>, mut current: Json) -> Json {
    let Some(case) = parsed(&current) else {
        return current;
    };
    let mut order: Vec<String> = vec![case.main.clone()];
    order.extend(
        case.templates
            .iter()
            .map(|(n, _)| n.clone())
            .filter(|n| *n != case.main),
    );
    for name in order {
        for level in [Level::Lines, Level::Tokens, Level::Bytes] {
            let Some(case) = parsed(&current) else {
                return current;
            };
            let Some(source) = case.template(&name).map(<[u8]>::to_vec) else {
                break;
            };
            if level == Level::Bytes && source.len() > session.options.byte_limit {
                continue;
            }
            let chunks = level.split(&source);
            if chunks.len() < 2 && level != Level::Bytes {
                continue;
            }
            let base = case.clone();
            let template_name = name.clone();
            let build = move |kept: &[Vec<u8>]| -> Json {
                let source: Vec<u8> = kept.concat();
                let templates: Vec<(String, Vec<u8>)> = base
                    .templates
                    .iter()
                    .map(|(n, s)| {
                        if *n == template_name {
                            (n.clone(), source.clone())
                        } else {
                            (n.clone(), s.clone())
                        }
                    })
                    .collect();
                with_main(&base, &templates)
            };
            let kept = session.ddmin(chunks, &build);
            current = build(&kept);
        }
    }
    current
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Lines,
    Tokens,
    Bytes,
}

impl Level {
    fn split(self, source: &[u8]) -> Vec<Vec<u8>> {
        match self {
            Level::Lines => source
                .split_inclusive(|&b| b == b'\n')
                .map(<[u8]>::to_vec)
                .collect(),
            Level::Tokens => tokens(source),
            Level::Bytes => source.iter().map(|&b| vec![b]).collect(),
        }
    }
}

/// Splits a template into chunks for ddmin: Twig delimiters (with their `-`/`~` modifiers), quoted
/// strings, words and numbers, whitespace runs, and any other single byte.
pub fn tokens(source: &[u8]) -> Vec<Vec<u8>> {
    const DELIMITERS: &[&[u8]] = &[
        b"{{-", b"{{~", b"{%-", b"{%~", b"-}}", b"~}}", b"-%}", b"~%}", b"{{", b"}}", b"{%", b"%}",
        b"{#", b"#}",
    ];
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut i = 0;
    while i < source.len() {
        let rest = &source[i..];
        if let Some(d) = DELIMITERS.iter().find(|d| rest.starts_with(d)) {
            out.push(d.to_vec());
            i += d.len();
            continue;
        }
        let b = source[i];
        let start = i;
        if b == b'"' || b == b'\'' {
            i += 1;
            while i < source.len() && source[i] != b {
                if source[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(source.len());
        } else if b.is_ascii_whitespace() {
            while i < source.len() && source[i].is_ascii_whitespace() {
                i += 1;
            }
        } else if b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80 {
            while i < source.len()
                && (source[i].is_ascii_alphanumeric() || source[i] == b'_' || source[i] >= 0x80)
            {
                i += 1;
            }
        } else {
            i += 1;
        }
        out.push(source[start..i].to_vec());
    }
    out
}

fn reduce_data(session: &mut Session<'_>, current: Json) -> Json {
    let Some(case) = parsed(&current) else {
        return current;
    };
    match &case.data {
        CaseData::Json(data) => {
            let mut data = data.clone();
            // level by level: the entries of the data, then of each surviving container
            let mut depth = 0;
            loop {
                let paths = container_paths(&data, depth);
                if paths.is_empty() {
                    break;
                }
                for path in paths {
                    let entries = entries_at(&data, &path);
                    if entries < 1 {
                        continue;
                    }
                    let base = current_with_data(&current, &data);
                    let snapshot = data.clone();
                    let path_for_build = path.clone();
                    let build = move |kept: &[usize]| -> Json {
                        let reduced = keep_entries(&snapshot, &path_for_build, kept);
                        current_with_data(&base, &reduced)
                    };
                    let kept = session.ddmin((0..entries).collect(), &build);
                    data = keep_entries(&data, &path, &kept);
                }
                depth += 1;
                if depth > 16 {
                    break;
                }
            }
            current_with_data(&current, &data)
        }
        CaseData::Php(code) => {
            let keys = typed_top_keys(&current);
            if keys.is_empty() {
                return current;
            }
            let base = current.clone();
            let code = code.clone();
            let all = keys.clone();
            let build = move |kept: &[String]| -> Json {
                let removed: Vec<&String> = all.iter().filter(|k| !kept.contains(k)).collect();
                let mut json = base.clone();
                if !removed.is_empty() {
                    json.set("data_php", Json::str(unset_keys(&code, &removed)));
                }
                json
            };
            let kept = session.ddmin(keys.clone(), &build);
            if kept.len() == keys.len() {
                current
            } else {
                build(&kept)
            }
        }
        CaseData::None => current,
    }
}

/// The top-level keys of a `data_php` case, from the typed tree the driver attached
/// (`_fuzz_data_keys`).
fn typed_top_keys(case: &Json) -> Vec<String> {
    case.get("_fuzz_data_keys")
        .map(Json::items)
        .unwrap_or_default()
        .iter()
        .filter_map(|k| match k {
            Json::String(s) => Some(s.clone()),
            Json::Number(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

/// PHP code that evaluates `code` like the oracle's `eval_data_code()` and removes `keys`.
fn unset_keys(code: &str, keys: &[&String]) -> String {
    let has_return = code.as_bytes().windows(6).enumerate().any(|(i, w)| {
        w.eq_ignore_ascii_case(b"return")
            && (i == 0 || !is_word(code.as_bytes()[i - 1]))
            && code.as_bytes().get(i + 6).is_none_or(|&b| !is_word(b))
    });
    let value = if has_return {
        format!("(static function () use ($twig) {{ {code}; }})()")
    } else {
        format!(
            "({})",
            code.trim().trim_end_matches([';', ' ', '\t', '\n', '\r'])
        )
    };
    let unsets: Vec<String> = keys
        .iter()
        .map(|k| format!("$__fuzz_data[{}]", php_key(k)))
        .collect();
    format!(
        "$__fuzz_data = {value}; unset({}); return $__fuzz_data;",
        unsets.join(", ")
    )
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn php_key(key: &str) -> String {
    if key.parse::<i64>().is_ok_and(|i| i.to_string() == key) {
        key.to_owned()
    } else {
        format!("'{}'", key.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

fn current_with_data(case: &Json, data: &Json) -> Json {
    let mut json = case.clone();
    json.set("data_json", data.clone());
    json
}

/// The paths (sequences of entry indexes) of the containers `depth` levels below the root.
fn container_paths(data: &Json, depth: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    collect_paths(data, depth, &mut Vec::new(), &mut out);
    out
}

fn collect_paths(node: &Json, depth: usize, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    if !matches!(node, Json::Array(_) | Json::Object(_)) {
        return;
    }
    if depth == 0 {
        out.push(path.clone());
        return;
    }
    let children: Vec<&Json> = match node {
        Json::Array(items) => items.iter().collect(),
        Json::Object(fields) => fields.iter().map(|(_, v)| v).collect(),
        _ => Vec::new(),
    };
    for (i, child) in children.into_iter().enumerate() {
        path.push(i);
        collect_paths(child, depth - 1, path, out);
        path.pop();
    }
}

fn node_at<'a>(node: &'a Json, path: &[usize]) -> Option<&'a Json> {
    let mut node = node;
    for &i in path {
        node = match node {
            Json::Array(items) => items.get(i)?,
            Json::Object(fields) => &fields.get(i)?.1,
            _ => return None,
        };
    }
    Some(node)
}

fn entries_at(data: &Json, path: &[usize]) -> usize {
    match node_at(data, path) {
        Some(Json::Array(items)) => items.len(),
        Some(Json::Object(fields)) => fields.len(),
        _ => 0,
    }
}

fn keep_entries(data: &Json, path: &[usize], kept: &[usize]) -> Json {
    fn rebuild(node: &Json, path: &[usize], kept: &[usize]) -> Json {
        match (node, path.split_first()) {
            (Json::Array(items), None) => {
                Json::Array(kept.iter().filter_map(|&i| items.get(i).cloned()).collect())
            }
            (Json::Object(fields), None) => Json::Object(
                kept.iter()
                    .filter_map(|&i| fields.get(i).cloned())
                    .collect(),
            ),
            (Json::Array(items), Some((&first, rest))) => Json::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        if i == first {
                            rebuild(v, rest, kept)
                        } else {
                            v.clone()
                        }
                    })
                    .collect(),
            ),
            (Json::Object(fields), Some((&first, rest))) => Json::Object(
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, (k, v))| {
                        if i == first {
                            (k.clone(), rebuild(v, rest, kept))
                        } else {
                            (k.clone(), v.clone())
                        }
                    })
                    .collect(),
            ),
            (other, _) => other.clone(),
        }
    }
    rebuild(data, path, kept)
}

fn drop_settings(session: &mut Session<'_>, mut current: Json) -> Json {
    // whole fields first, then the entries of objects/lists
    for field in [
        "config",
        "globals",
        "extensions",
        "sandbox",
        "timezone",
        "lexer",
        "preload",
        "test_classes",
    ] {
        if current.get(field).is_none()
            || (field == "test_classes" && !session.options.drop_test_classes)
        {
            continue;
        }
        let mut candidate = current.clone();
        candidate.remove(field);
        if session.test(&[candidate.clone()])[0] {
            current = candidate;
            continue;
        }
        let entries = match current.get(field) {
            Some(Json::Object(fields)) => fields.len(),
            Some(Json::Array(items)) => items.len(),
            _ => 0,
        };
        if entries > 0 {
            let snapshot = current.clone();
            let field_name = field.to_owned();
            let build = move |kept: &[usize]| -> Json {
                let mut json = snapshot.clone();
                let reduced =
                    keep_entries(snapshot.get(&field_name).unwrap_or(&Json::Null), &[], kept);
                json.set(&field_name, reduced);
                json
            };
            let kept = session.ddmin((0..entries).collect(), &build);
            current = build(&kept);
        }
    }
    current
}

/// How candidates run in the oracle unless told otherwise: every one in a PHP process of its own
/// (see the module documentation).
pub const ISOLATION: &str = "process";

/// Where candidates with a filesystem cache get their own directories (in the oracle's container).
pub const CANDIDATE_CACHE_DIR: &str = "/tmp/twig-fuzz-candidates";

/// The predicate "the oracle and twig-rs still disagree with this signature", through a served
/// oracle. Candidates that need the oracle's typed data get `typed_data`; candidates of a case with a
/// filesystem cache (`config.cache`) each get a directory of their own under
/// [`CANDIDATE_CACHE_DIR`] (the runner empties a directory before a case uses it, but candidates that
/// share one could not run in parallel).
pub struct SameSignature<'o> {
    oracle: &'o mut Oracle,
    executor: &'o mut Executor,
    signature: String,
    timeout: Duration,
    isolation: Option<String>,
    candidates: usize,
    /// Oracle failures seen (a broken oracle answers "no" to everything).
    pub oracle_errors: Vec<String>,
}

impl<'o> SameSignature<'o> {
    /// The predicate for `signature`; `isolation` is the oracle batch's isolation mode (normally
    /// `Some(`[`ISOLATION`]`)`).
    pub fn new(
        oracle: &'o mut Oracle,
        executor: &'o mut Executor,
        signature: String,
        timeout: Duration,
        isolation: Option<String>,
    ) -> Self {
        SameSignature {
            oracle,
            executor,
            signature,
            timeout,
            isolation,
            candidates: 0,
            oracle_errors: Vec::new(),
        }
    }
}

/// The filesystem cache directory of a case (`config.cache` when it is a string).
pub fn cache_dir(case: &Json) -> Option<&str> {
    case.get("config")?.get("cache")?.as_str()
}

/// `case` with the filesystem cache directory `dir` instead of its own (cases without one are left
/// alone).
pub fn with_cache_dir(case: &Json, dir: &str) -> Json {
    let mut case = case.clone();
    if cache_dir(&case).is_some()
        && let Some(mut config) = case.get("config").cloned()
    {
        config.set("cache", Json::str(dir));
        case.set("config", config);
    }
    case
}

/// The PHP memory limit of fuzz cases that set none (the image's default is unlimited; a runaway
/// `range()` would take the machine's memory).
pub const DEFAULT_MEMORY_LIMIT: &str = "512M";
/// The PHP time limit (seconds) of fuzz cases that set none.
pub const DEFAULT_TIME_LIMIT: i64 = 30;

/// The case object to send to the oracle: driver-private fields removed, `typed_data` set for
/// `data_php` cases, `clock` (twig-rs then renders with the oracle's "now"), and the
/// [`DEFAULT_MEMORY_LIMIT`] / [`DEFAULT_TIME_LIMIT`] unless the case sets its own.
pub fn oracle_request(case: &Json) -> Json {
    let mut json = case.clone();
    json.remove("_fuzz_data_keys");
    if json.get_non_null("data_php").is_some() && json.get("typed_data").is_none() {
        json.set("typed_data", Json::Bool(true));
    }
    if matches!(json, Json::Object(_)) {
        if json.get("memory_limit").is_none() {
            json.set("memory_limit", Json::str(DEFAULT_MEMORY_LIMIT));
        }
        if json.get("time_limit").is_none() {
            json.set("time_limit", Json::int(DEFAULT_TIME_LIMIT));
        }
        if json.get("clock").is_none() {
            json.set("clock", Json::Bool(true));
        }
    }
    json
}

impl Predicate for SameSignature<'_> {
    fn test(&mut self, candidates: &[Json]) -> Vec<bool> {
        let requests: Vec<Json> = candidates
            .iter()
            .map(|candidate| {
                self.candidates += 1;
                let dir = format!(
                    "{CANDIDATE_CACHE_DIR}/{}-{}",
                    std::process::id(),
                    self.candidates
                );
                oracle_request(&with_cache_dir(candidate, &dir))
            })
            .collect();
        let results = match self.oracle.run_served(&requests, self.isolation.as_deref()) {
            Ok(results) => results,
            Err(error) => {
                self.oracle_errors.push(error);
                return vec![false; candidates.len()];
            }
        };
        requests
            .iter()
            .zip(results)
            .map(|(request, result)| {
                let Ok(case) = Case::from_json(request) else {
                    return false;
                };
                let expected = CaseResult::from_json(&result);
                match self.executor.run(&case, Some(&expected), self.timeout) {
                    Outcome::Ran(actual) => match compare(&expected, &actual) {
                        Verdict::Mismatch(m) => m.signature() == self.signature,
                        _ => false,
                    },
                    Outcome::Unsupported(_) => false,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A predicate that reproduces while the main template contains `needle` and the data has key
    /// `k`.
    struct Contains;

    impl Predicate for Contains {
        fn test(&mut self, candidates: &[Json]) -> Vec<bool> {
            candidates
                .iter()
                .map(|c| {
                    let case = Case::from_json(c).unwrap();
                    let source =
                        String::from_utf8_lossy(case.template(&case.main).unwrap_or_default())
                            .into_owned();
                    let data_ok = match &case.data {
                        CaseData::Json(d) => d.get("k").is_some(),
                        _ => false,
                    };
                    source.contains("|round") && data_ok
                })
                .collect()
        }
    }

    #[test]
    fn shrinks_templates_and_data() {
        let case = Json::parse(
            r#"{"env":"plain","templates":{"index.twig":"Hello {{ a }}\n{% for x in [1,2] %}{{ x|round }}{% endfor %}\nbye {{ b ~ 'c' }}","other.twig":"x"},"data_json":{"a":1,"k":[1,2,{"z":3}],"b":2}}"#,
        )
        .unwrap();
        let minimized = minimize(&case, &mut Contains, Options::default());
        let result = Case::from_json(&minimized.case).unwrap();
        assert_eq!(result.templates.len(), 1);
        assert_eq!(result.template("index.twig"), Some(&b"|round"[..]));
        match &result.data {
            CaseData::Json(d) => assert_eq!(d.to_compact(), r#"{"k":[]}"#),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tokenizes_twig() {
        let t = tokens(b"a {{- x|f('s\\'') }}{# c #}");
        let t: Vec<String> = t
            .iter()
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        assert_eq!(
            t,
            [
                "a", " ", "{{-", " ", "x", "|", "f", "(", "'s\\''", ")", " ", "}}", "{#", " ", "c",
                " ", "#}"
            ]
        );
    }

    #[test]
    fn unsets_php_keys() {
        assert_eq!(
            unset_keys(
                "return ['a' => 1, 3 => 2];",
                &[&"a".to_owned(), &"3".to_owned()]
            ),
            "$__fuzz_data = (static function () use ($twig) { return ['a' => 1, 3 => 2];; })(); unset($__fuzz_data['a'], $__fuzz_data[3]); return $__fuzz_data;"
        );
        assert_eq!(
            unset_keys("['x' => 1]", &[&"x".to_owned()]),
            "$__fuzz_data = (['x' => 1]); unset($__fuzz_data['x']); return $__fuzz_data;"
        );
    }
}
