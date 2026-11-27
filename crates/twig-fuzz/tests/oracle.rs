//! The oracle client and runner against the real oracle (Docker, image `twig-oracle:3.31.1`, built
//! from `research/php-oracle` when missing). Ignored by default:
//!
//! ```text
//! cargo test -p twig-fuzz --test oracle -- --ignored
//! ```

use std::process::Command;
use std::time::{Duration, Instant};

use twig_fuzz::kit::case::Case;
use twig_fuzz::kit::diff::{Verdict, compare};
use twig_fuzz::kit::json::Json;
use twig_fuzz::kit::result::CaseResult;
use twig_fuzz::minimize::{self, Options, SameSignature, minimize, oracle_request};
use twig_fuzz::oracle::{DEFAULT_IMAGE, Oracle};
use twig_fuzz::worker::Executor;

fn oracle() -> Oracle {
    let oracle = Oracle::from_env();
    oracle.ensure_image().expect("the oracle image");
    oracle
}

/// The containers (in any state) whose names start with the client's prefix.
fn containers(oracle_prefix: &str) -> Vec<String> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name={oracle_prefix}-"),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .expect("docker ps");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
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
    // the silent container was removed, the others removed themselves (`--rm`)
    assert_eq!(containers(&oracle.container_prefix), Vec::<String>::new());
}

/// A container under the name the client is about to use, left in state `Created` (which `--rm`
/// never removes), is removed, and the batch goes to the next name (T20).
#[test]
#[ignore = "needs Docker"]
fn a_stale_container_with_the_next_name_is_removed_and_the_name_skipped() {
    let mut oracle = oracle();
    let prefix = oracle.container_prefix.clone();
    let image = std::env::var("TWIG_ORACLE_IMAGE").unwrap_or_else(|_| DEFAULT_IMAGE.to_owned());
    let stale = format!("{prefix}-1");
    let created = Command::new("docker")
        .args(["create", "--name", &stale, &image, "php", "-v"])
        .output()
        .expect("docker create");
    assert!(created.status.success(), "{created:?}");
    assert_eq!(containers(&prefix), [stale.as_str()]);
    let results = oracle
        .run(
            &cases(r#"[{"id":"a","env":"plain","templates":{"index.twig":"a"}}]"#),
            None,
        )
        .unwrap();
    assert_eq!(results[0].get("output"), Some(&Json::str("a")));
    assert!(
        oracle.incidents.len() == 1 && oracle.incidents[0].contains(&stale),
        "{:?}",
        oracle.incidents
    );
    assert_eq!(containers(&prefix), Vec::<String>::new());
}

/// A client that is dropped while its server runs removes the server's container.
#[test]
#[ignore = "needs Docker"]
fn a_dropped_client_leaves_no_container() {
    let mut oracle = oracle();
    let prefix = oracle.container_prefix.clone();
    let results = oracle
        .run_served(
            &cases(r#"[{"id":"a","env":"plain","templates":{"index.twig":"a"}}]"#),
            Some("process"),
        )
        .unwrap();
    assert_eq!(results[0].get("output"), Some(&Json::str("a")));
    assert_eq!(containers(&prefix).len(), 1, "the server runs");
    drop(oracle);
    assert_eq!(containers(&prefix), Vec::<String>::new());
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

/// The typed data marks an interval made by `new DateInterval()` as one with wall-clock arithmetic also when its
/// date part or a long time part keeps adding it from crossing a change of the UTC offset, or when it has only
/// microseconds, and marks no civil interval and no interval without a time part (T23: `interval_is_wall()` in
/// research/php-oracle/lib.php). Fails on an image built before T23.
#[test]
#[ignore = "needs Docker"]
fn the_typed_data_marks_every_wall_clock_interval() {
    let mut oracle = oracle();
    let data_php = "return [\
        'marked' => [new \\DateInterval('PT1H'), new \\DateInterval('P9MT62825H6M7S'), \
            new \\DateInterval('P1Y2M3DT4H5M6S'), clone new \\DateInterval('P2MT1H'), \
            (function () { $i = new \\DateInterval('PT0S'); $i->f = 0.5; return $i; })()], \
        'unmarked' => [unserialize(serialize(new \\DateInterval('P9MT62825H6M7S'))), \
            \\DateInterval::createFromDateString('9 months 62825 hours'), \
            (new \\DateTime('2020-01-01'))->diff(new \\DateTime('2021-03-05 10:20:30')), \
            new \\DateInterval('P9M')]]";
    let case = Json::Object(vec![
        ("id".to_owned(), Json::str("wall")),
        ("env".to_owned(), Json::str("plain")),
        (
            "templates".to_owned(),
            Json::Object(vec![("index.twig".to_owned(), Json::str(""))]),
        ),
        ("data_php".to_owned(), Json::str(data_php)),
    ]);
    let results = oracle
        .run(&[oracle_request(&case)], Some("process"))
        .unwrap();
    let data = results[0]
        .get("data")
        .unwrap_or_else(|| panic!("no typed data: {}", results[0].to_compact()));
    // an array node's items are `[key, node]` pairs
    let list = |key: &str| -> Vec<bool> {
        let node = data
            .get("v")
            .map(Json::items)
            .unwrap_or_default()
            .iter()
            .find(|item| item.items().first().and_then(Json::as_str) == Some(key))
            .and_then(|item| item.items().get(1))
            .unwrap_or_else(|| panic!("no {key}: {}", data.to_compact()));
        node.get("v")
            .map(Json::items)
            .unwrap_or_default()
            .iter()
            .map(|item| {
                let interval = &item.items()[1];
                assert_eq!(
                    interval.get("class").and_then(Json::as_str),
                    Some("DateInterval")
                );
                interval.get("wall").is_some_and(Json::truthy)
            })
            .collect()
    };
    assert_eq!(list("marked"), [true; 5]);
    assert_eq!(list("unmarked"), [false; 4]);
}
