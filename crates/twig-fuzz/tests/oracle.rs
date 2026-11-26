//! The oracle client and runner against the real oracle (Docker, image `twig-oracle:3.31.1`, built
//! from `research/php-oracle` when missing). Ignored by default:
//!
//! ```text
//! cargo test -p twig-fuzz --test oracle -- --ignored
//! ```

use std::time::{Duration, Instant};

use twig_fuzz::kit::case::Case;
use twig_fuzz::kit::diff::{Verdict, compare};
use twig_fuzz::kit::json::Json;
use twig_fuzz::kit::result::CaseResult;
use twig_fuzz::minimize::{self, Options, SameSignature, minimize, oracle_request};
use twig_fuzz::oracle::Oracle;
use twig_fuzz::worker::Executor;

fn oracle() -> Oracle {
    let oracle = Oracle::from_env();
    oracle.ensure_image().expect("the oracle image");
    oracle
}

fn cases(json: &str) -> Vec<Json> {
    Json::parse(json)
        .expect("valid JSON")
        .items()
        .iter()
        .map(oracle_request)
        .collect()
}

fn phase(result: &Json) -> Option<&str> {
    result.get("phase").and_then(Json::as_str)
}

/// PHP's hard memory limit ends the runner with status 137 and no JSON: the chunk runs again with
/// process isolation, and only the case that died is lost (as an oracle failure).
#[test]
#[ignore = "needs Docker"]
fn a_container_killed_by_the_hard_memory_limit_costs_only_its_case() {
    let mut oracle = oracle();
    let results = oracle
        .run(
            &cases(
                r#"[{"id":"a","env":"plain","templates":{"index.twig":"a"}},
                    {"id":"big","env":"plain","templates":{"index.twig":"{{ 1.5|number_format(2147483647) }}"}},
                    {"id":"c","env":"plain","templates":{"index.twig":"c"}}]"#,
            ),
            None,
        )
        .unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].get("output"), Some(&Json::str("a")));
    assert_eq!(phase(&results[1]), Some("fatal"));
    assert!(CaseResult::from_json(&results[1]).is_oracle_failure());
    assert_eq!(results[2].get("output"), Some(&Json::str("c")));
    assert_eq!(oracle.incidents.len(), 1, "{:?}", oracle.incidents);
}

/// INF in `data_json` reaches a child process as written (the runner does not re-encode cases),
/// isolated cases run in parallel and keep their order, and a child stuck past its `wall_limit` is
/// killed.
#[test]
#[ignore = "needs Docker"]
fn isolated_children_get_their_text_run_in_parallel_and_are_killed_when_stuck() {
    let mut oracle = oracle();
    oracle.jobs = 4;
    let mut batch = String::from("[");
    for i in 0..12 {
        batch.push_str(&format!(
            r#"{{"id":"c{i}","env":"plain","templates":{{"index.twig":"{{{{ x }}}}|{i}"}},"data_json":{{"x":1e400}}}},"#
        ));
    }
    batch.push_str(r#"{"id":"stuck","php_code":"sleep(1000); return 1;","wall_limit":2}]"#);
    let started = Instant::now();
    let results = oracle.run(&cases(&batch), Some("process")).unwrap();
    assert!(started.elapsed() < Duration::from_secs(60));
    for (i, result) in results.iter().take(12).enumerate() {
        assert_eq!(
            result.get("output"),
            Some(&Json::str(format!("INF|{i}"))),
            "{}",
            result.to_compact()
        );
    }
    assert_eq!(phase(&results[12]), Some("fatal"));
}

/// A runner that stops reporting progress is killed and its chunk runs again with process
/// isolation, where the runner's own wall-clock limit ends the stuck case.
#[test]
#[ignore = "needs Docker (about a minute)"]
fn a_silent_runner_is_killed_and_its_cases_run_again() {
    let mut oracle = oracle();
    let results = oracle
        .run(
            &cases(
                r#"[{"id":"a","env":"plain","templates":{"index.twig":"a"},"time_limit":1,"wall_limit":1},
                    {"id":"stuck","php_code":"sleep(1000); return 1;","time_limit":1,"wall_limit":1}]"#,
            ),
            None,
        )
        .unwrap();
    assert_eq!(results[0].get("output"), Some(&Json::str("a")));
    assert_eq!(phase(&results[1]), Some("fatal"));
    assert!(
        oracle.incidents[0].contains("no progress"),
        "{:?}",
        oracle.incidents
    );
}

/// The runner empties a case's filesystem cache directory first: a second run of the same case in
/// the same server compiles again, with its compile-time deprecation.
#[test]
#[ignore = "needs Docker"]
fn every_run_of_a_case_compiles_its_templates() {
    let mut oracle = oracle();
    let case = cases(
        r#"[{"id":"dep","env":"plain","templates":{"index.twig":"{{ 'a'|spaceless }}"},
             "config":{"cache":"/tmp/twig-fuzz-test/dep"}}]"#,
    );
    for _ in 0..2 {
        let results = oracle.run_served(&case, Some("process")).unwrap();
        assert_eq!(
            results[0]
                .get("deprecations")
                .map(Json::items)
                .map(<[Json]>::len),
            Some(1),
            "{}",
            results[0].to_compact()
        );
    }
}

/// The minimiser (a PHP process and a cache directory per candidate) shrinks a known mismatch of
/// twig-rs: the operand order of `*` (crates/twig-fuzz/vectors/round0-mismatches.json).
#[test]
#[ignore = "needs Docker"]
fn the_minimiser_shrinks_a_known_mismatch() {
    let mut oracle = oracle();
    let mut executor = Executor::InProcess;
    let start = Json::parse(
        r#"{"id":"m","env":"plain","templates":{"index.twig":"x{{ 1 + 2 }}\n{{ '' * b }}\ny"},
            "data_json":{"b":[1],"unused":1},"config":{"cache":"/tmp/twig-fuzz-test/m","autoescape":false}}"#,
    )
    .unwrap();
    let request = oracle_request(&start);
    let expected = CaseResult::from_json(
        &oracle
            .run_served(std::slice::from_ref(&request), Some(minimize::ISOLATION))
            .unwrap()[0],
    );
    let actual = twig_fuzz::kit::run::run(&Case::from_json(&request).unwrap(), Some(&expected));
    let Verdict::Mismatch(mismatch) = compare(&expected, actual.result().unwrap()) else {
        // twig-rs no longer swaps the operands wrongly: nothing to minimise
        return;
    };
    let mut predicate = SameSignature::new(
        &mut oracle,
        &mut executor,
        mismatch.signature(),
        Duration::from_secs(10),
        Some(minimize::ISOLATION.to_owned()),
    );
    let minimized = minimize(
        &start,
        &mut predicate,
        Options {
            drop_test_classes: true,
            ..Options::default()
        },
    );
    let case = Case::from_json(&minimized.case).unwrap();
    let source = String::from_utf8_lossy(case.template("index.twig").unwrap()).into_owned();
    assert!(source.contains('*') && source.len() < 20, "{source:?}");
    assert!(predicate.oracle_errors.is_empty());
}
