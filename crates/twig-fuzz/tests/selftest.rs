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
