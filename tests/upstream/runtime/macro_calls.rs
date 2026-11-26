//! A sweep of `TwigMacro::callLegacy()` (the subject of TWIG/tests/TwigMacroTest.php) against the oracle
//! (`tests/vectors/upstream/runtime/macro_calls.json`, generator next to it): 15 macro definitions × 33 calls, each
//! a template declaring the macro and calling it on line 7, comparing the captured arguments or the error, and the
//! deprecations.

use twig::runtime::DiagnosticsPolicy;
use twig::value::Array;

use crate::support::*;

#[test]
fn macro_calls_match_the_oracle() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/upstream/runtime/macro_calls.json"
    );
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the vectors")).unwrap();
    assert!(cases.len() > 400, "the vector file looks truncated");
    let mut failures = Failures::default();
    for case in &cases {
        let template = case[0].as_str().unwrap();
        let expected = (
            case[1].as_str().unwrap().to_owned(),
            strip_php_location(case[2].as_str().unwrap()),
            case[3]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<Vec<_>>(),
        );
        let twig = env(&[("index.twig", template)]);
        let captured = add_capture(&twig);
        twig.set_diagnostics_policy(DiagnosticsPolicy::Collect);
        let actual = match twig.render("index.twig", Array::new()) {
            Ok(_) => (
                "value".to_owned(),
                captured
                    .take()
                    .iter()
                    .map(describe_value)
                    .collect::<Vec<_>>()
                    .join("|"),
                notices(&twig),
            ),
            Err(error) => ("error".to_owned(), describe(&error), notices(&twig)),
        };
        failures.check(template, actual, expected);
    }
    failures.assert_none();
}
