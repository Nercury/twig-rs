//! Support for the engine vector tests: the oracle batches `tests/vectors/engine/<topic>.gen.json` (the cases) and
//! `<topic>.json` (what Twig 3.31.1 on PHP 8.5.13 produced), run through [`twig::runtime::Environment`] with an
//! `ArrayLoader`.

pub mod api;
pub mod objects;

use std::sync::Arc;

use twig::escaper::DefaultStrategy;
use twig::extension::StringLoaderExtension;
use twig::loader::ArrayLoader;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, Notice, SandboxExtension, SecurityError,
    SecurityPolicy,
};
use twig::value::{Array, ArrayKey, Level, PhpString, Value};

use crate::common::Json;

/// A JSON value as `json_decode($json, true)` decodes it: objects become arrays (keys normalised), integers that fit
/// stay ints, other numbers are floats.
pub fn json_to_value(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(text) => {
            if !text.contains(['.', 'e', 'E'])
                && let Ok(i) = text.parse::<i64>()
            {
                return Value::Int(i);
            }
            Value::Float(text.parse::<f64>().expect("a JSON number"))
        }
        Json::String(s) => Value::String(PhpString::from(s.as_str())),
        Json::Array(items) => Value::Array(Array::from_values(items.iter().map(json_to_value))),
        Json::Object(fields) => {
            let mut array = Array::new();
            for (key, value) in fields {
                array.insert(
                    ArrayKey::from_string(PhpString::from(key.as_str())),
                    json_to_value(value),
                );
            }
            Value::Array(array)
        }
    }
}

/// The environment a case asks for (`"env": "plain"`): Twig's defaults plus the case's config, and the security
/// policy of its sandbox (whose `strict` mode and `tests` the case's `data_php` sets once the templates are loaded).
fn environment(case: &Json) -> (Environment, Option<Arc<SecurityPolicy>>) {
    let errors = case
        .get("errors")
        .and_then(Json::as_str)
        .unwrap_or("collect");
    let options = match case.get("config") {
        Some(config) => options_from(config, errors),
        None => options_from(&Json::Object(Vec::new()), errors),
    };
    environment_with(case, options)
}

/// The options of an environment from the oracle's `config` and `errors` fields.
fn options_from(config: &Json, errors: &str) -> EnvironmentOptions {
    let mut options = EnvironmentOptions::default();
    for (key, value) in config.as_object() {
        match key.as_str() {
            "autoescape" => {
                options.autoescape = match value {
                    Json::Bool(false) => DefaultStrategy::Disabled,
                    Json::String(s) => DefaultStrategy::Strategy(s.clone()),
                    other => panic!("unsupported autoescape {other:?}"),
                }
            }
            "strict_variables" => options.strict_variables = matches!(value, Json::Bool(true)),
            "optimizations" => options.optimizations = value.as_i64().expect("optimizations"),
            "charset" => options.charset = value.as_str().expect("charset").to_owned(),
            "debug" => options.debug = matches!(value, Json::Bool(true)),
            // twig-rs keeps compiled programs in memory; a filesystem cache only keeps the oracle's template files
            // apart (see `write()` in engine.gen.py)
            "cache" => {}
            other => panic!("unsupported config option {other}"),
        }
    }
    options.diagnostics = match errors {
        "throw" => DiagnosticsPolicy::Error,
        _ => DiagnosticsPolicy::Collect,
    };
    options
}

fn environment_with(
    case: &Json,
    options: EnvironmentOptions,
) -> (Environment, Option<Arc<SecurityPolicy>>) {
    let templates: Vec<(String, Vec<u8>)> = case
        .field("templates")
        .as_object()
        .iter()
        .map(|(name, code)| (name.clone(), code.as_bytes()))
        .collect();
    let env = Environment::with_options(ArrayLoader::new(templates), options);
    if let Some(globals) = case.get("globals") {
        for (name, value) in globals.as_object() {
            env.add_global(name, json_to_value(value))
                .expect("globals are added first");
        }
    }
    if let Some(extensions) = case.get("extensions") {
        for name in extensions.as_array() {
            match name.as_str() {
                Some("string_loader") => env
                    .add_extension(Arc::new(StringLoaderExtension::new()))
                    .expect("a fresh environment"),
                Some("sandbox") => {}
                other => panic!("unsupported extension {other:?}"),
            }
        }
    }
    let mut policy = None;
    if let Some(sandbox) = case.get("sandbox") {
        let (extension, sandbox_policy) = sandbox_extension(sandbox);
        env.add_extension(Arc::new(extension))
            .expect("a fresh environment");
        policy = Some(sandbox_policy);
    }
    (env, policy)
}

/// The `SandboxExtension` of a case: `{"global": bool, "policy": {"tags", "filters", "methods", "properties",
/// "functions"}}` as the oracle builds it, and its policy.
fn sandbox_extension(sandbox: &Json) -> (SandboxExtension, Arc<SecurityPolicy>) {
    let policy = SecurityPolicy::new();
    let names = |json: Option<&Json>| -> Vec<String> {
        json.map(|list| {
            list.as_array()
                .iter()
                .map(|name| name.as_lossy_string())
                .collect()
        })
        .unwrap_or_default()
    };
    let per_class = |json: Option<&Json>| -> Vec<(String, Vec<String>)> {
        json.map(|map| {
            map.as_object()
                .iter()
                .map(|(class, members)| {
                    let members = match members {
                        Json::Array(items) => items.iter().map(Json::as_lossy_string).collect(),
                        other => vec![other.as_lossy_string()],
                    };
                    (class.clone(), members)
                })
                .collect()
        })
        .unwrap_or_default()
    };
    if let Some(lists) = sandbox.get("policy") {
        policy.set_allowed_tags(names(lists.get("tags")));
        policy.set_allowed_filters(names(lists.get("filters")));
        policy.set_allowed_functions(names(lists.get("functions")));
        policy.set_allowed_methods(per_class(lists.get("methods")));
        policy.set_allowed_properties(per_class(lists.get("properties")));
    }
    let policy = Arc::new(policy);
    let global = matches!(sandbox.get("global"), Some(Json::Bool(true)));
    (SandboxExtension::new(policy.clone(), global), policy)
}

/// What the case's `data_php` does to the security policy (after the templates are loaded): `setStrict(true)`,
/// `setAllowedTests()`.
fn configure_policy(case: &Json, policy: &SecurityPolicy) {
    let Some(sandbox) = case.get("sandbox") else {
        return;
    };
    if matches!(sandbox.get("strict"), Some(Json::Bool(true))) {
        policy.set_strict(true);
    }
    if let Some(tests) = sandbox.get("tests") {
        policy.set_allowed_tests(tests.as_array().iter().map(Json::as_lossy_string));
    }
}

/// An expected message without what PHP appends to the `TypeError` of a userland function called from compiled code
/// (`, called in <compiled file> on line N`), which twig-rs omits (reference/deviations.md).
fn without_called_in(message: &str) -> String {
    let mut out = message.to_owned();
    while let Some(start) = out.find(", called in ") {
        let Some(on_line) = out[start..].find(" on line ") else {
            break;
        };
        let digits_start = start + on_line + " on line ".len();
        let digits = out[digits_start..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        out.replace_range(start..digits_start + digits, "");
    }
    out
}

/// The context of a case: `data_json`, or for object cases `data_json_rust` plus the named objects and template
/// wrappers (`wrappers`: variable => template of the environment; `foreign_wrappers`: variable => [name, source] of
/// another environment).
fn context(case: &Json, env: &Environment) -> Array {
    let mut context = match case.get("data_json").or_else(|| case.get("data_json_rust")) {
        Some(data) => match json_to_value(data) {
            Value::Array(array) => array,
            _ => Array::new(),
        },
        None => Array::new(),
    };
    if let Some(objects) = case.get("objects") {
        for name in objects.as_array() {
            let name = name.as_str().expect("object name");
            context.insert(name, objects::build(name));
        }
    }
    if let Some(wrappers) = case.get("wrappers") {
        for (variable, template) in wrappers.as_object() {
            let template = template.as_str().expect("template name");
            let wrapper = env.load(template).expect("a loadable wrapped template");
            context.insert(variable.as_str(), wrapper.to_value());
        }
    }
    if let Some(wrappers) = case.get("foreign_wrappers") {
        for (variable, template) in wrappers.as_object() {
            let parts = template.as_array();
            let name = parts[0].as_lossy_string();
            let source = parts[1].as_bytes();
            let other = Environment::new(ArrayLoader::new([(name.clone(), source)]));
            let wrapper = other.load(&name).expect("a loadable foreign template");
            context.insert(variable.as_str(), wrapper.to_value());
        }
    }
    context
}

/// The PHP class of an error: its sandbox class for a sandbox violation (twig-rs reports those as runtime errors
/// until the error kinds have the sandbox classes), else its kind's class.
fn error_class(error: &twig::Error) -> &'static str {
    SecurityError::of(error)
        .map(SecurityError::class_name)
        .unwrap_or_else(|| error.kind().class_name())
}

/// The PHP error type name of a diagnostic level, as the oracle reports it.
fn level_name(level: Level) -> &'static str {
    match level {
        Level::Warning => "E_WARNING",
        Level::Notice => "E_NOTICE",
        Level::Deprecated => "E_DEPRECATED",
        Level::UserDeprecated => "E_USER_DEPRECATED",
    }
}

/// The diagnostics and deprecations an environment recorded, as the oracle reports them (type, message).
fn notices_of(env: &Environment) -> Vec<(String, String)> {
    env.take_notices()
        .into_iter()
        .map(|notice| match notice {
            Notice::Php(diagnostic) => (
                level_name(diagnostic.level).to_owned(),
                diagnostic.message.to_string_lossy().into_owned(),
            ),
            Notice::Deprecation(deprecation) => {
                ("E_USER_DEPRECATED".to_owned(), deprecation.to_string())
            }
        })
        .collect()
}

/// Runs one case; returns a description of the mismatch, if any.
fn run_case(case: &Json, expected: &Json) -> Option<String> {
    let (env, policy) = environment(case);
    let main = case
        .get("main")
        .and_then(Json::as_str)
        .unwrap_or("index.twig");
    // like the oracle, every template is loaded first, in order, unless `load_all` is false
    let load_all = !matches!(case.get("load_all"), Some(Json::Bool(false)));
    let loaded: Result<(), twig::Error> = if load_all {
        case.field("templates")
            .as_object()
            .iter()
            .try_for_each(|(name, _)| env.load(name).map(|_| ()))
    } else {
        Ok(())
    };
    let result = loaded.and_then(|()| env.load(main)).and_then(|template| {
        if let Some(policy) = &policy {
            configure_policy(case, policy);
        }
        template.render_bytes(context(case, &env))
    });
    let notices = notices_of(&env);
    let mut problems = Vec::new();
    match (&result, expected.field("ok")) {
        (Ok(output), Json::Bool(true)) => {
            let want = match expected.get("output_b64") {
                Some(_) => expected.field("output").as_bytes(),
                None => expected.field("output").as_bytes(),
            };
            if *output != want {
                problems.push(format!(
                    "output {:?}, expected {:?}",
                    String::from_utf8_lossy(output),
                    String::from_utf8_lossy(&want)
                ));
            }
        }
        (Err(error), Json::Bool(false)) => {
            let want = expected.field("error");
            let class = want.field("class").as_str().unwrap_or_default();
            let message = without_called_in(&want.field("message").as_lossy_string());
            let got = format!("{}: {}", error_class(error), error);
            let wanted = format!("{class}: {message}");
            if got != wanted {
                problems.push(format!("error {got:?}, expected {wanted:?}"));
            }
        }
        (Ok(output), _) => problems.push(format!(
            "rendered {:?}, expected error {}",
            String::from_utf8_lossy(output),
            expected
                .get("error")
                .and_then(|e| e.get("formatted"))
                .and_then(Json::as_str)
                .unwrap_or("?")
        )),
        (Err(error), _) => problems.push(format!(
            "error {}: {error}, expected output {:?}",
            error_class(error),
            expected.get("output").and_then(Json::as_str).unwrap_or("?")
        )),
    }
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
        problems.push(format!(
            "diagnostics {notices:?}, expected {want_notices:?}"
        ));
    }
    if problems.is_empty() {
        None
    } else {
        Some(problems.join("; "))
    }
}

/// Runs every case of a topic and fails with the list of mismatches.
pub fn run_topic(topic: &str) {
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
        if let Some(problem) = run_case(case, expected) {
            let templates: Vec<String> = case
                .field("templates")
                .as_object()
                .iter()
                .map(|(name, code)| format!("{name}: {:?}", code.as_lossy_string()))
                .collect();
            failures.push(format!("{id} [{}]\n    {problem}", templates.join(", ")));
        }
    }
    assert!(
        failures.is_empty(),
        "{topic}: {} of {} cases differ from the oracle:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
