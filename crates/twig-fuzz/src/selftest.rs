//! Batches for checking the toolkit against what the repository's tests already prove.
//!
//! * [`fixture_batch`]: the plain-environment equivalent of every runnable case of Twig's
//!   integration fixtures (the oracle corpus `tests/twig/oracle-corpus.json`): the same templates
//!   (with the harness's cumulative padding), CONFIG (from the corpus's typed tree), DATA code
//!   (`data_php` with `typed_data`), the harness's extensions (debug, string_loader, a non-global
//!   sandbox allowing `dump`), its `global` global, its helper classes (`test_classes`) and its
//!   error mode (warnings become exceptions). The test extension of `IntegrationTest` is not
//!   there, so fixtures using its filters fail the same way on both sides.
//! * [`engine_batch`]: the oracle batches of `tests/vectors/engine/` with their recorded results
//!   (the `api` topic, which is not made of "twig" cases, is left out).

use std::path::Path;

use crate::kit::json::Json;

/// The repository root (two levels above this crate).
pub fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A typed tree of plain data as JSON (`None` for objects, closures and resources, and for keyed
/// arrays whose keys JSON objects cannot keep apart).
fn typed_to_json(node: &Json) -> Option<Json> {
    match node.get("t")?.as_str()? {
        "null" => Some(Json::Null),
        "bool" => node.get("v").cloned(),
        "int" => node.get("v").cloned(),
        "float" => {
            let text = node.get("v")?.as_str()?;
            let d: f64 = text.parse().ok()?;
            if !d.is_finite() {
                return None;
            }
            // keep it a float for json_decode(): a fraction or exponent
            Some(Json::Number(if text.contains(['.', 'e', 'E']) {
                text.to_owned()
            } else {
                format!("{text}.0")
            }))
        }
        "string" => {
            if node.get("v_b64").is_some() {
                return None;
            }
            node.get("v").cloned()
        }
        "array" => {
            let items = node.get("v")?.items();
            let is_list = items
                .iter()
                .enumerate()
                .all(|(i, item)| item.items().first().and_then(Json::as_i64) == Some(i as i64));
            if is_list {
                items
                    .iter()
                    .map(|item| typed_to_json(item.items().get(1)?))
                    .collect::<Option<Vec<_>>>()
                    .map(Json::Array)
            } else {
                items
                    .iter()
                    .map(|item| {
                        let key = match item.items().first()? {
                            Json::String(s) => s.clone(),
                            Json::Number(n) => n.clone(),
                            _ => return None,
                        };
                        Some((key, typed_to_json(item.items().get(1)?)?))
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(Json::Object)
            }
        }
        _ => None,
    }
}

/// The plain-environment cases of the fixture corpus, split into those that can share one PHP
/// process and those whose DATA declares classes (to run with `"isolation": "process"`). Also
/// returns the number of fixture cases left out (skipped fixtures, CONFIG that JSON cannot carry).
///
/// # Errors
///
/// The corpus cannot be read.
pub fn fixture_batch(corpus: &Path) -> Result<(Vec<Json>, Vec<Json>, usize), String> {
    let corpus = Json::load(corpus)?;
    let mut shared = Vec::new();
    let mut isolated = Vec::new();
    let mut left_out = 0;
    for fixture in corpus.get("fixtures").map(Json::items).unwrap_or_default() {
        let file = fixture
            .get("file")
            .and_then(Json::as_str)
            .unwrap_or_default();
        let actual = fixture.get("actual").cloned().unwrap_or(Json::Null);
        if actual.get_non_null("skipped").is_some() {
            left_out += fixture
                .get("outputs")
                .map(Json::items)
                .unwrap_or_default()
                .len();
            continue;
        }
        let defines_classes = fixture
            .get("flags")
            .and_then(|f| f.get("data_defines_classes"))
            .is_some_and(Json::truthy);
        let templates = fixture
            .get("templates")
            .map(Json::entries)
            .unwrap_or_default();
        let outputs = fixture.get("outputs").map(Json::items).unwrap_or_default();
        let actual_cases = actual.get("cases").map(Json::items).unwrap_or_default();
        let mut padded: Vec<(String, String)> = templates
            .iter()
            .map(|(name, source)| (name.clone(), source.as_str().unwrap_or_default().to_owned()))
            .collect();
        for (i, output) in outputs.iter().enumerate() {
            // the harness appends i spaces to every template, cumulatively
            for (_, source) in &mut padded {
                source.push_str(&" ".repeat(i));
            }
            let Some(actual_case) = actual_cases.get(i) else {
                // the harness stopped at an earlier load error
                left_out += 1;
                continue;
            };
            let Some(config) = actual_case.get("config").and_then(typed_to_json) else {
                left_out += 1;
                continue;
            };
            let mut case = vec![
                ("id".to_owned(), Json::str(format!("{file}#{i}"))),
                ("env".to_owned(), Json::str("plain")),
                (
                    "templates".to_owned(),
                    Json::Object(
                        padded
                            .iter()
                            .map(|(n, s)| (n.clone(), Json::str(s)))
                            .collect(),
                    ),
                ),
                ("config".to_owned(), config),
                (
                    "extensions".to_owned(),
                    Json::Array(vec![
                        Json::str("debug"),
                        Json::str("sandbox"),
                        Json::str("string_loader"),
                    ]),
                ),
                (
                    "sandbox".to_owned(),
                    Json::parse(r#"{"global": false, "policy": {"functions": ["dump"]}}"#)
                        .expect("valid JSON"),
                ),
                (
                    "globals".to_owned(),
                    Json::Object(vec![("global".to_owned(), Json::str("global"))]),
                ),
                ("test_classes".to_owned(), Json::Bool(true)),
                ("errors".to_owned(), Json::str("throw")),
            ];
            if let Some(code) = output.get("data_php").and_then(Json::as_str)
                && !code.trim().is_empty()
            {
                case.push(("data_php".to_owned(), Json::str(code)));
                case.push(("typed_data".to_owned(), Json::Bool(true)));
            }
            let case = Json::Object(case);
            if defines_classes {
                isolated.push(case);
            } else {
                shared.push(case);
            }
        }
    }
    Ok((shared, isolated, left_out))
}

/// The "twig" cases of the engine vector batches and their recorded results, in order.
///
/// # Errors
///
/// A batch or result file cannot be read, or they differ in length.
pub fn engine_batch(dir: &Path) -> Result<(Vec<Json>, Vec<Json>), String> {
    let mut cases = Vec::new();
    let mut results = Vec::new();
    let mut topics: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".gen.json"))
        })
        .collect();
    topics.sort();
    for batch in topics {
        let name = batch
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let topic = name.trim_end_matches(".gen.json");
        if topic == "api" {
            continue;
        }
        let batch_json = Json::load(&batch)?;
        let result_json = Json::load(&dir.join(format!("{topic}.json")))?;
        let batch_cases = batch_json
            .get("cases")
            .map(Json::items)
            .unwrap_or_else(|| batch_json.items());
        let batch_results = result_json.items();
        if batch_cases.len() != batch_results.len() {
            return Err(format!("{topic}: cases and results differ in number"));
        }
        cases.extend(batch_cases.iter().cloned());
        results.extend(batch_results.iter().cloned());
    }
    Ok((cases, results))
}
