//! Oracle vectors: the node trees (or errors) and deprecations of Twig 3.31.1's parser and `CorrectnessNodeVisitor`:
//! `tests/vectors/parser/expressions.json` (templates without tags, every expression parser),
//! `expressions_random.json` (random valid and corrupted expressions) and `statements.json` (every tag, the module
//! assembly and the correctness checks).
//!
//! Regenerate (from the repository root; Git Bash on Windows needs MSYS_NO_PATHCONV=1):
//!
//!     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/parser/expressions.gen.php > tests/vectors/parser/expressions.json
//!     python -I tests/vectors/parser/statements.gen.py > tests/vectors/parser/statements.gen.json
//!     MSYS_NO_PATHCONV=1 docker run --rm -i -v "$PWD/tests/vectors/parser:/v" -e CASES_FILE=/v/statements.gen.json twig-oracle:3.31.1 php < tests/vectors/parser/expressions.gen.php > tests/vectors/parser/statements.json
//!
//! (`expressions_random.json` and `statements_random.json` the same way from their `.gen.json` batches, see
//! `expressions_random.gen.py` and `statements_random.gen.py`.)

use twig::Source;
use twig::ast::dump::dump_node_bytes;

use super::support::{extension_set, parse_with};
use crate::common::Json;

/// Cases whose expected result depends on PHP's PCRE (`MatchesBinary` validates constant patterns at parse time):
/// they need the regular-expression validator of the PCRE port, which is wired in by a later stage.
const NEEDS_PCRE: &[&str] = &[
    "bin-matches-invalid",
    "bin-matches-number",
    "bin-matches-compile",
];

#[test]
fn expression_vectors() {
    let vectors = Json::load_vectors("parser/expressions.json");
    let (checked, failures) = check_cases(&vectors, NEEDS_PCRE);
    assert!(checked > 200, "only {checked} cases");
    report(checked, &failures);
}

/// A committed random batch (`expressions_random.gen.py`): valid and corrupted expressions.
#[test]
fn random_expression_vectors() {
    let vectors = Json::load_vectors("parser/expressions_random.json");
    let (checked, failures) = check_cases(&vectors, &[]);
    assert!(checked > 800, "only {checked} cases");
    report(checked, &failures);
}

/// Every tag, the module assembly and the correctness checks (`statements.gen.py`).
#[test]
fn statement_vectors() {
    let vectors = Json::load_vectors("parser/statements.json");
    let (checked, failures) = check_cases(&vectors, &[]);
    assert!(checked > 350, "only {checked} cases");
    report(checked, &failures);
}

/// A committed random batch of templates with tags, valid and corrupted (`statements_random.gen.py`).
#[test]
fn random_statement_vectors() {
    let vectors = Json::load_vectors("parser/statements_random.json");
    let (checked, failures) = check_cases(&vectors, &[]);
    assert!(checked > 950, "only {checked} cases");
    report(checked, &failures);
}

/// Runs an external vector file produced by `expressions.gen.php` (e.g. a large random batch), given in the
/// `TWIG_PARSER_VECTORS` environment variable: `cargo test --test parser_tests -- --ignored external`.
#[test]
#[ignore = "needs an external vector file in TWIG_PARSER_VECTORS"]
fn external_vectors() {
    let path = std::env::var("TWIG_PARSER_VECTORS").expect("TWIG_PARSER_VECTORS is not set");
    let vectors = Json::load_path(&path);
    let (checked, failures) = check_cases(&vectors, &[]);
    report(checked, &failures);
}

fn report(checked: usize, failures: &[String]) {
    let ids: Vec<&str> = failures
        .iter()
        .map(|f| f.split(':').next().unwrap_or(""))
        .collect();
    if !ids.is_empty() {
        eprintln!("failed: {ids:?}");
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} cases failed:

{}",
        failures.len(),
        failures.join(
            "

"
        )
    );
}

/// Compares every case of a vector file with the Rust parser; returns the number of checked cases and the
/// failures.
fn check_cases(vectors: &Json, skip: &[&str]) -> (usize, Vec<String>) {
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in vectors.as_array() {
        let id = case.field("id").as_str().unwrap();
        if skip.contains(&id) {
            continue;
        }
        let code = match case.get("source") {
            Some(source) => source.as_bytes(),
            None => crate::common::base64_decode(case.field("source_b64").as_str().unwrap()),
        };
        let name = case.get("name").and_then(Json::as_str).unwrap_or("index");
        let path = case.get("path").and_then(Json::as_str).unwrap_or("");
        let source = Source::with_path(code.clone(), name, path);
        let set = extension_set(case);
        let (result, deprecations) = parse_with(&set, &source);
        // Constant patterns of `matches` are validated with PHP's PCRE, which a later stage wires in.
        if case.get("error").is_some_and(|e| {
            e.field("message")
                .as_lossy_string()
                .starts_with("Regexp \"")
        }) && skip.is_empty()
        {
            continue;
        }
        checked += 1;
        let template = String::from_utf8_lossy(&code).into_owned();

        let expected_deprecations: Vec<String> = case
            .field("deprecations")
            .as_array()
            .iter()
            .map(Json::as_lossy_string)
            .collect();
        let actual_deprecations: Vec<String> =
            deprecations.iter().map(ToString::to_string).collect();
        if expected_deprecations != actual_deprecations {
            failures.push(format!(
                "{id}: deprecations for {template:?}
  expected {expected_deprecations:?}
  actual   {actual_deprecations:?}"
            ));
        }

        match (case.get("dump"), case.get("error"), result) {
            (Some(dump), None, Ok(module)) => {
                let expected = dump.as_bytes();
                let actual = dump_node_bytes(&module);
                if expected != actual {
                    failures.push(format!(
                        "{id}: dump for {template:?}
{}",
                        first_difference(&expected, &actual)
                    ));
                }
            }
            (None, Some(error), Err(actual)) => {
                let expected_message = error.field("message").as_lossy_string();
                let expected_class = error.field("class").as_str().unwrap();
                if expected_message != actual.to_string()
                    || expected_class != actual.kind().class_name()
                {
                    failures.push(format!(
                        "{id}: error for {template:?}
  expected {expected_class}: {expected_message}
  actual   {}: {actual}",
                        actual.kind().class_name()
                    ));
                }
            }
            (Some(_), _, Err(actual)) => {
                failures.push(format!("{id}: unexpected error for {template:?}: {actual}"));
            }
            (_, Some(error), Ok(_)) => failures.push(format!(
                "{id}: expected error for {template:?}: {}",
                error.field("message").as_lossy_string()
            )),
            _ => failures.push(format!("{id}: malformed case")),
        }
    }
    (checked, failures)
}

/// The first differing line of two dumps, with context.
fn first_difference(expected: &[u8], actual: &[u8]) -> String {
    let expected = String::from_utf8_lossy(expected);
    let actual = String::from_utf8_lossy(actual);
    let e: Vec<&str> = expected.lines().collect();
    let a: Vec<&str> = actual.lines().collect();
    let index = e
        .iter()
        .zip(a.iter())
        .position(|(x, y)| x != y)
        .unwrap_or(e.len().min(a.len()));
    let from = index.saturating_sub(4);
    let to_e = (index + 3).min(e.len());
    let to_a = (index + 3).min(a.len());
    format!(
        "--- expected (line {})
{}
--- actual
{}",
        index + 1,
        e[from..to_e].join(
            "
"
        ),
        a[from..to_a].join(
            "
"
        )
    )
}
