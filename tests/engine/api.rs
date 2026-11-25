//! The `api` topic: sequences of API calls on one environment (the block methods of `TemplateWrapper`,
//! `Environment::resolve_template()`, the `Sandbox` facade, `render_sandboxed()`), compared with what the oracle's
//! PHP script returned for the same calls (`spec` of each case of `tests/vectors/engine/api.gen.json`).

use std::sync::Arc;

use twig::extension::SandboxBridgeExtension;
use twig::loader::ArrayLoader;
use twig::runtime::{
    Environment, Sandbox, SandboxBridgeRuntime, SecurityPolicy, SecurityPolicyInterface,
};
use twig::value::{Array, Value};

use super::{error_class, json_to_value, notices_of, options_from};
use crate::common::Json;

/// What one call returned: a value as JSON, or the class and message of the exception.
#[derive(Debug, PartialEq)]
enum Outcome {
    Value(Json),
    Error(String, String),
}

fn context(op: &Json) -> Array {
    match op.get("context").map(json_to_value) {
        Some(Value::Array(array)) => array,
        _ => Array::new(),
    }
}

fn names(json: Option<&Json>) -> Vec<String> {
    json.map(|list| list.as_array().iter().map(Json::as_lossy_string).collect())
        .unwrap_or_default()
}

/// The `SecurityPolicy` of an op: `{"tags", "filters", "methods", "properties", "functions", "tests"}`.
fn policy(json: &Json, strict: bool) -> Arc<dyn SecurityPolicyInterface> {
    let policy = SecurityPolicy::new();
    policy.set_allowed_tags(names(json.get("tags")));
    policy.set_allowed_filters(names(json.get("filters")));
    policy.set_allowed_functions(names(json.get("functions")));
    policy.set_allowed_tests(names(json.get("tests")));
    policy.set_strict(strict);
    Arc::new(policy)
}

fn templates_env(templates: &Json) -> Environment {
    let templates: Vec<(String, Vec<u8>)> = templates
        .as_object()
        .iter()
        .map(|(name, code)| (name.clone(), code.as_bytes()))
        .collect();
    let options = twig::runtime::EnvironmentOptions {
        autoescape: twig::escaper::DefaultStrategy::Disabled,
        ..Default::default()
    };
    Environment::with_options(ArrayLoader::new(templates), options)
}

fn string(bytes: Vec<u8>) -> Json {
    Json::String(String::from_utf8_lossy(&bytes).into_owned())
}

fn run_op(env: &Environment, sandbox: &mut Option<Sandbox>, op: &Json) -> Outcome {
    let template = op
        .get("template")
        .map(Json::as_lossy_string)
        .unwrap_or_default();
    let block = op
        .get("block")
        .map(Json::as_lossy_string)
        .unwrap_or_default();
    let result: Result<Json, twig::Error> = match op.field("op").as_str().unwrap_or_default() {
        "render" => env
            .load(&template)
            .and_then(|t| t.render_bytes(context(op)))
            .map(string),
        "renderBlock" => env
            .load(&template)
            .and_then(|t| t.render_block_bytes(&block, context(op)))
            .map(string),
        "displayBlock" | "streamBlock" => env.load(&template).and_then(|t| {
            let mut out: Vec<u8> = Vec::new();
            t.stream_block(&block, context(op), &mut out)?;
            Ok(string(out))
        }),
        "stream" => env.load(&template).and_then(|t| {
            let mut out: Vec<u8> = Vec::new();
            t.stream(context(op), &mut out)?;
            Ok(string(out))
        }),
        "hasBlock" => env
            .load(&template)
            .and_then(|t| t.has_block(&block, context(op)))
            .map(Json::Bool),
        "getBlockNames" => env
            .load(&template)
            .and_then(|t| t.block_names(context(op)))
            .map(|names| Json::Array(names.into_iter().map(Json::String).collect())),
        "resolveTemplate" => {
            let names = names(op.get("names"));
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            env.resolve_template(&names)
                .map(|t| Json::String(t.template_name().to_owned()))
        }
        "sandbox" => {
            let other = templates_env(op.field("templates"));
            let used = matches!(op.get("used"), Some(Json::Bool(true)));
            let first = op.field("templates").as_object()[0].0.clone();
            let loaded = if used {
                other.load(&first).map(|_| ())
            } else {
                Ok(())
            };
            loaded.and_then(|()| {
                let strict = matches!(op.get("strict"), Some(Json::Bool(true)));
                *sandbox = Some(Sandbox::new(other, policy(op.field("policy"), strict))?);
                Ok(Json::Null)
            })
        }
        "sandboxRender" => sandbox
            .as_ref()
            .expect("a sandbox op first")
            .render_bytes(&template, context(op))
            .map(string),
        "sandboxRenderBlock" => sandbox
            .as_ref()
            .expect("a sandbox op first")
            .render_block(&template, &block, context(op))
            .map(Json::String),
        "bridge" => {
            let other = templates_env(op.field("templates"));
            Sandbox::new(other, policy(op.field("policy"), true)).and_then(|bridged| {
                env.add_extension(Arc::new(SandboxBridgeExtension::new()))?;
                if !matches!(op.get("runtime"), Some(Json::Bool(false))) {
                    env.add_runtime(
                        SandboxBridgeRuntime::CLASS,
                        Arc::new(SandboxBridgeRuntime::new(bridged)),
                    );
                }
                Ok(Json::Null)
            })
        }
        other => panic!("unknown op {other}"),
    };
    match result {
        Ok(value) => Outcome::Value(value),
        Err(error) => Outcome::Error(error_class(&error).to_owned(), error.to_string()),
    }
}

/// The oracle's result of one call.
fn expected_outcome(json: &Json) -> Outcome {
    if matches!(json.get("ok"), Some(Json::Bool(true))) {
        let value = match json.field("value") {
            // getBlockNames() keeps the keys of array_unique(), which the script drops with array_values()
            Json::Object(fields) => Json::Array(fields.iter().map(|(_, v)| v.clone()).collect()),
            other => other.clone(),
        };
        Outcome::Value(value)
    } else {
        Outcome::Error(
            json.field("class").as_lossy_string(),
            super::without_called_in(&json.field("message").as_lossy_string()),
        )
    }
}

/// Runs every case of an API topic and fails with the list of mismatches.
pub fn run_api_topic(topic: &str) {
    let cases = Json::load_vectors(&format!("engine/{topic}.gen.json"));
    let results = Json::load_vectors(&format!("engine/{topic}.json"));
    let cases = cases.field("cases").as_array();
    let results = results.as_array();
    assert_eq!(
        cases.len(),
        results.len(),
        "{topic}: cases and results differ in number"
    );
    let mut failures = Vec::new();
    for (case, expected) in cases.iter().zip(results) {
        let id = case.field("id").as_str().unwrap_or_default();
        assert_eq!(expected.field("id").as_str(), Some(id), "result order");
        assert!(
            matches!(expected.get("ok"), Some(Json::Bool(true))),
            "{id}: the oracle script failed: {expected:?}"
        );
        let spec = case.field("spec");
        let templates: Vec<(String, Vec<u8>)> = spec
            .field("templates")
            .as_object()
            .iter()
            .map(|(name, code)| (name.clone(), code.as_bytes()))
            .collect();
        let env = Environment::with_options(
            ArrayLoader::new(templates),
            options_from(spec.field("config"), "collect"),
        );
        for (name, value) in spec.field("globals").as_object() {
            env.add_global(name, json_to_value(value))
                .expect("globals are added first");
        }
        let want: Vec<Outcome> = Json::parse(&expected.field("json").as_lossy_string())
            .expect("the script returns JSON")
            .as_array()
            .iter()
            .map(expected_outcome)
            .collect();
        let mut sandbox = None;
        for (i, (op, want)) in spec.field("ops").as_array().iter().zip(&want).enumerate() {
            let got = run_op(&env, &mut sandbox, op);
            if got != *want {
                failures.push(format!(
                    "{id} op {i} {op:?}\n    got {got:?}\n    expected {want:?}"
                ));
            }
        }
        let notices = notices_of(&env);
        let want_notices: Vec<(String, String)> = expected
            .get("diagnostics")
            .map(|d| {
                d.as_array()
                    .iter()
                    .map(|n| {
                        (
                            n.field("type").as_str().unwrap_or_default().to_owned(),
                            n.field("message").as_lossy_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        if notices != want_notices {
            failures.push(format!(
                "{id}: diagnostics {notices:?}, expected {want_notices:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{topic}: {} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
