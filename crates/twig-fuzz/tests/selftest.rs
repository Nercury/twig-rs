//! Offline checks of the toolkit (no Docker).
//!
//! `tests/vectors/features.json` holds one case per runner feature (`data_json` decoding, typed
//! `data_php` trees with objects, PHP state and data diagnostics, `templates_b64`, error modes,
//! setup/load/data/render failures, the sandbox policy, globals, timezones, `load_all`, `dump()`,
//! `template_from_string()`, composition) with the oracle's results; every case must replay
//! identically. Regenerate it from its batch with
//!
//! ```text
//! twig-fuzz record crates/twig-fuzz/tests/vectors/features.gen.json \
//!     --out crates/twig-fuzz/tests/vectors/features.json \
//!     --description "twig-fuzz self-test: one case per runner feature"
//! ```
//!
//! `deviations.json` and `typed-data.json` are recorded the same way from their `.gen.json` batches
//! (keep the descriptions they have).

use std::path::{Path, PathBuf};
use std::time::Duration;

use twig_fuzz::kit::case::Case;
use twig_fuzz::kit::diff::{Verdict, compare};
use twig_fuzz::kit::json::Json;
use twig_fuzz::kit::result::CaseResult;
use twig_fuzz::kit::run::{Outcome, run};
use twig_fuzz::kit::vectors::{KnownMismatch, ReplayStatus, VectorFile, replay, replay_files};
use twig_fuzz::minimize::{Options, Predicate, minimize};

fn vectors_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

const TIMEOUT: Duration = Duration::from_secs(60);

#[test]
fn every_feature_vector_replays_identically() {
    let replayed = replay_files(&[vectors_dir().join("features.json")], TIMEOUT);
    assert!(replayed.len() >= 20, "{} vectors", replayed.len());
    let failures: Vec<String> = replayed
        .iter()
        .filter(|r| {
            !matches!(
                r.status,
                ReplayStatus::Pass | ReplayStatus::PassWithDeviation(_)
            )
        })
        .map(|r| format!("{}: {:?}", r.id, r.status))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `tests/vectors/deviations.json` (from `deviations.gen.json`): oracle results whose only
/// differences are listed deviations: closure names of compiled templates (scoped, nested, in
/// macros, blocks and embedded templates) and `var_dump()` object handles through every escaping
/// strategy, case filters and output that is not valid UTF-8.
#[test]
fn deviation_rule_vectors_need_exactly_their_rule() {
    let replayed = replay_files(&[vectors_dir().join("deviations.json")], TIMEOUT);
    assert_eq!(replayed.len(), 17);
    for r in &replayed {
        let ReplayStatus::PassWithDeviation(rules) = &r.status else {
            panic!("{}: {:?}", r.id, r.status);
        };
        let rule = if r.id.starts_with("closure-") {
            "closure-name"
        } else {
            "object-handle"
        };
        assert!(rules.contains(&rule), "{}: {rules:?}", r.id);
    }
}

/// `tests/vectors/typed-data.json` (from `typed-data.gen.json`, a process-isolated batch): data the
/// runner rebuilds from the oracle's typed trees (the `props` of date objects included) and INF in
/// `data_json`, which the oracle must pass to its child process as written.
#[test]
fn typed_data_vectors_replay_identically() {
    let replayed = replay_files(&[vectors_dir().join("typed-data.json")], TIMEOUT);
    assert_eq!(replayed.len(), 12);
    for r in &replayed {
        assert_eq!(r.status, ReplayStatus::Pass, "{}", r.id);
    }
}

#[test]
fn the_known_mismatch_marker_is_a_ratchet() {
    let file = VectorFile::load(&vectors_dir().join("features.json")).unwrap();
    let vector = file
        .vectors
        .iter()
        .find(|v| v.id == "collect-warning")
        .expect("the collect-warning vector")
        .clone();

    // identical and unmarked: passes; marked: fails (the marker must go)
    assert_eq!(replay(&vector, TIMEOUT), ReplayStatus::Pass);
    let mut marked = vector.clone();
    marked.known_mismatch = Some(KnownMismatch {
        reason: "test".to_owned(),
        signature: None,
    });
    assert!(matches!(replay(&marked, TIMEOUT), ReplayStatus::Fail(_)));

    // a different recorded output: unmarked fails, marked passes
    let mut tampered = vector.clone();
    tampered.oracle.set("output", Json::str("something else"));
    assert!(matches!(replay(&tampered, TIMEOUT), ReplayStatus::Fail(_)));
    tampered.known_mismatch = Some(KnownMismatch {
        reason: "test".to_owned(),
        signature: Some("output:changed".to_owned()),
    });
    assert!(matches!(
        replay(&tampered, TIMEOUT),
        ReplayStatus::PassSignatureChanged { .. }
    ));
}

#[test]
fn results_round_trip_through_the_oracle_schema() {
    let file = VectorFile::load(&vectors_dir().join("features.json")).unwrap();
    for vector in &file.vectors {
        let case = Case::from_json(&vector.case).unwrap();
        let oracle = CaseResult::from_json(&vector.oracle);
        let Outcome::Ran(actual) = run(&case, Some(&oracle)) else {
            panic!("{}: unsupported", vector.id);
        };
        let again = CaseResult::from_json(&actual.to_json());
        assert_eq!(*actual, again, "{}", vector.id);
        assert_eq!(compare(&oracle, &again), compare(&oracle, &actual));
    }
}

#[test]
fn replay_update_removes_only_the_markers_of_vectors_that_match_now() {
    let file = VectorFile::load(&vectors_dir().join("features.json")).unwrap();
    let base = file
        .vectors
        .iter()
        .find(|v| v.id == "collect-warning")
        .expect("the collect-warning vector")
        .clone();
    let marker = |signature: &str| {
        Some(KnownMismatch {
            reason: "test".to_owned(),
            signature: Some(signature.to_owned()),
        })
    };
    // fixed: matches, still marked -> the marker goes
    let mut fixed = base.clone();
    fixed.id = "fixed".to_owned();
    fixed.known_mismatch = marker("output:changed");
    // still mismatching with the recorded signature -> kept
    let mut open = base.clone();
    open.id = "open".to_owned();
    open.oracle.set("output", Json::str("something else"));
    let Verdict::Mismatch(mismatch) = compare(
        &CaseResult::from_json(&open.oracle),
        run(&Case::from_json(&open.case).unwrap(), None)
            .result()
            .unwrap(),
    ) else {
        panic!("the tampered vector must mismatch");
    };
    open.known_mismatch = marker(&mismatch.signature());
    // still mismatching, another signature -> left alone
    let mut changed = open.clone();
    changed.id = "changed".to_owned();
    changed.known_mismatch = marker("output:missing");
    // fixed, but not asked for -> kept (and reported as a failure)
    let mut other = fixed.clone();
    other.id = "other".to_owned();
    let marked = VectorFile {
        description: "update test".to_owned(),
        vectors: vec![fixed.clone(), open.clone(), changed.clone(), other.clone()],
    };
    let dir = std::env::temp_dir().join(format!("twig-fuzz-update-{}", std::process::id()));
    let path = dir.join("v.json");
    marked.save(&path).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();

    let only = ["fixed".to_owned(), "open".to_owned(), "changed".to_owned()];
    let updated = twig_fuzz::kit::vectors::update_file(&path, TIMEOUT, Some(&only)).unwrap();
    let dropped: Vec<&str> = updated
        .iter()
        .filter(|u| u.dropped)
        .map(|u| u.id.as_str())
        .collect();
    assert_eq!(dropped, ["fixed"]);
    assert!(matches!(
        updated[2].status,
        ReplayStatus::PassSignatureChanged { .. }
    ));
    assert!(matches!(updated[3].status, ReplayStatus::Fail(_)));

    // the text lost exactly the first marker
    let after = std::fs::read_to_string(&path).unwrap();
    let mut expected = marked.clone();
    expected.vectors[0].known_mismatch = None;
    assert_eq!(VectorFile::load(&path).unwrap(), expected);
    let removed = before.len() - after.len();
    let marker_text =
        ",\n      \"known_mismatch\": {\"reason\":\"test\",\"signature\":\"output:changed\"}";
    assert_eq!(removed, marker_text.len(), "{after}");
    assert!(before.contains(marker_text));

    // a second run changes nothing
    let again = twig_fuzz::kit::vectors::update_file(&path, TIMEOUT, Some(&only)).unwrap();
    assert!(again.iter().all(|u| !u.dropped));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refresh_replaces_only_the_recorded_results_it_was_asked_for() {
    let file = VectorFile::load(&vectors_dir().join("features.json")).unwrap();
    let mut first = file
        .vectors
        .iter()
        .find(|v| v.id == "collect-warning")
        .expect("the collect-warning vector")
        .clone();
    let good = first.oracle.clone();
    first.oracle.set("output", Json::str("stale"));
    let mut second = first.clone();
    second.id = "other".to_owned();
    let vectors = VectorFile {
        description: "refresh test".to_owned(),
        vectors: vec![first, second.clone()],
    };
    let dir = std::env::temp_dir().join(format!("twig-fuzz-refresh-{}", std::process::id()));
    let path = dir.join("v.json");
    vectors.save(&path).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    let mut asked = Vec::new();
    let mut oracle = |cases: &[Json]| -> Result<Vec<Json>, String> {
        asked.extend(cases.iter().cloned());
        Ok(vec![good.clone(); cases.len()])
    };
    let only = ["collect-warning".to_owned()];
    let refreshed = twig_fuzz::kit::vectors::refresh_file(&path, Some(&only), &mut oracle).unwrap();
    assert_eq!(refreshed, ["collect-warning"]);
    assert_eq!(asked.len(), 1);
    let after = VectorFile::load(&path).unwrap();
    assert_eq!(after.vectors[0].oracle, good);
    assert_eq!(after.vectors[1], second);
    // one line changed: the first vector's "oracle"
    let text = std::fs::read_to_string(&path).unwrap();
    let changed = before
        .lines()
        .zip(text.lines())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(changed, 1);
    assert_eq!(before.lines().count(), text.lines().count());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A predicate that replays recorded results: a candidate reproduces when twig-rs, given the
/// recorded oracle result of the original case, differs from a deliberately wrong expectation in
/// the same way (an offline stand-in for the oracle).
struct WrongExpectation {
    oracle: CaseResult,
}

impl Predicate for WrongExpectation {
    fn test(&mut self, candidates: &[Json]) -> Vec<bool> {
        candidates
            .iter()
            .map(|candidate| {
                let Ok(case) = Case::from_json(candidate) else {
                    return false;
                };
                let Outcome::Ran(actual) = run(&case, None) else {
                    return false;
                };
                // reproduces while twig-rs still prints "Array" (the warning's source) at all
                matches!(compare(&self.oracle, &actual), Verdict::Mismatch(_))
                    && actual
                        .output
                        .as_deref()
                        .is_some_and(|o| o.windows(5).any(|w| w == b"Array"))
            })
            .collect()
    }
}

#[test]
fn the_minimiser_shrinks_a_case_offline() {
    let case = Json::parse(
        r#"{"id":"m","env":"plain","templates":{"index.twig":"a{{ x }}\n{% for i in [1, 2] %}{{ i }}{% endfor %}{{ list }}b","unused.twig":"u"},"data_json":{"x":1,"list":[1],"y":2}}"#,
    )
    .unwrap();
    let oracle = CaseResult::from_json(
        &Json::parse(r#"{"ok":true,"output":"nothing","deprecations":[],"diagnostics":[]}"#)
            .unwrap(),
    );
    let minimized = minimize(&case, &mut WrongExpectation { oracle }, Options::default());
    let result = Case::from_json(&minimized.case).unwrap();
    assert_eq!(result.templates.len(), 1);
    assert_eq!(result.template("index.twig"), Some(&b"{{list}}"[..]));
}
