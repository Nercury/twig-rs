//! A sweep of the subject of TWIG/tests/ErrorTest.php against the oracle
//! (`tests/vectors/upstream/runtime/error_lines.json`, generator next to it): `foo.bar` failing (a missing variable in
//! strict mode, or `ErrorTest_Foo::bar()` throwing) inside 40 composition constructs, with debug on and off, comparing
//! the message, the template line, the source name and the previous exception.

use twig::runtime::EnvironmentOptions;
use twig::value::Array;

use crate::classes::error_test_foo;
use crate::support::*;

#[test]
fn error_lines_match_the_oracle() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/upstream/runtime/error_lines.json"
    );
    let vectors: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the vectors")).unwrap();
    let cases = vectors["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 240);
    let mut failures = Failures::default();
    for case in cases {
        let construct = case[0].as_str().unwrap();
        let mode = case[1].as_str().unwrap();
        let debug = case[2].as_bool().unwrap();
        let templates: Vec<(String, String)> = vectors["constructs"][construct]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, code)| (name.clone(), code.as_str().unwrap().to_owned()))
            .collect();
        let templates: Vec<(&str, &str)> = templates
            .iter()
            .map(|(n, c)| (n.as_str(), c.as_str()))
            .collect();
        let twig = env_with(
            &templates,
            EnvironmentOptions {
                strict_variables: mode != "lax_exception",
                debug,
                ..Default::default()
            },
        );
        let context = if mode == "missing" {
            Array::new()
        } else {
            ctx(&[("foo", error_test_foo())])
        };
        let actual = match twig.render("index", context) {
            Ok(output) => ("value".to_owned(), output, None, None, None),
            Err(error) => {
                let twig_error = error.kind().is_twig_error();
                (
                    "error".to_owned(),
                    describe(&error),
                    twig_error.then(|| error.template_line()),
                    twig_error
                        .then(|| error.source_context().map(|s| s.name().to_owned()))
                        .flatten(),
                    previous(&error),
                )
            }
        };
        let expected = (
            case[3].as_str().unwrap().to_owned(),
            strip_php_location(case[4].as_str().unwrap()),
            case[5].as_i64(),
            case[6].as_str().map(str::to_owned),
            case[7].as_str().map(strip_php_location),
        );
        failures.check(
            &format!("{construct} {mode} debug={debug}"),
            actual,
            expected,
        );
    }
    failures.assert_none();
}
