//! A sweep of the `TemplateWrapper` block API (the subject of TWIG/tests/TemplateWrapperTest.php) against the oracle
//! (`tests/vectors/upstream/runtime/block_api.json`, generator next to it): `render_block()`, `has_block()` and
//! `block_names()` on template families (inheritance, dynamic parents, traits, nested blocks, embeds, macros,
//! `parent()`/`block()` calls, strict variables) with several block names and contexts, comparing the result or the
//! error with its template line and source.

use twig::runtime::{DiagnosticsPolicy, Environment, EnvironmentOptions};
use twig::value::{Array, Value};

use crate::support::*;

fn context(json: &serde_json::Value) -> Array {
    let mut array = Array::new();
    // PHP encodes the empty context as `[]`
    if let Some(object) = json.as_object() {
        for (key, value) in object {
            array.insert(key.as_str(), Value::from(value.as_str().unwrap()));
        }
    }
    array
}

#[test]
fn block_api_matches_the_oracle() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/upstream/runtime/block_api.json"
    );
    let vectors: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the vectors")).unwrap();
    let cases = vectors["cases"].as_array().unwrap();
    assert!(cases.len() > 4000, "the vector file looks truncated");
    let mut failures = Failures::default();
    // one environment per family, mode and context, as in the generator
    let mut current: Option<((String, bool, String), Environment)> = None;
    for case in cases {
        let family = case["family"].as_str().unwrap();
        let strict = case["strict"].as_bool().unwrap();
        let context_name = case["context"].as_str().unwrap();
        let group = (family.to_owned(), strict, context_name.to_owned());
        if current.as_ref().map(|(key, _)| key) != Some(&group) {
            let templates: Vec<(String, String)> = vectors["families"][family]
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
                    strict_variables: strict,
                    ..Default::default()
                },
            );
            twig.add_global("g", "G").unwrap();
            twig.set_diagnostics_policy(DiagnosticsPolicy::Collect);
            current = Some((group, twig));
        }
        let twig = &current.as_ref().unwrap().1;
        let op = case["op"].as_str().unwrap();
        let block = case["block"].as_str().unwrap_or("");
        let context = context(&vectors["contexts"][context_name]);
        let result = twig.load("index").and_then(|wrapper| match op {
            "blocks" => wrapper.block_names(context).map(|names| names.join(",")),
            "has" => wrapper.has_block(block, context).map(|has| has.to_string()),
            _ => wrapper.render_block(block, context),
        });
        let actual = match result {
            Ok(text) => ("value".to_owned(), text, None, None, notices(twig)),
            Err(error) => (
                "error".to_owned(),
                describe(&error),
                error.kind().is_twig_error().then(|| error.template_line()),
                error
                    .kind()
                    .is_twig_error()
                    .then(|| error.source_context().map(|s| s.name().to_owned()))
                    .flatten(),
                notices(twig),
            ),
        };
        let expected = (
            case["kind"].as_str().unwrap().to_owned(),
            strip_php_location(case["text"].as_str().unwrap()),
            case["line"].as_i64(),
            case["source"].as_str().map(str::to_owned),
            case["deprecations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<Vec<_>>(),
        );
        failures.check(
            &format!("{family} strict={strict} {op} {block} {context_name}"),
            actual,
            expected,
        );
    }
    failures.assert_none();
}
