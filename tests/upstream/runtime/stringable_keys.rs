//! A sweep of the Stringable-key handling of `getAttribute()` and of the optimized array access (the
//! `testStringableKey*` and `testArrayAccess*` tests of TWIG/tests/TemplateTest.php) against the oracle
//! (`tests/vectors/upstream/runtime/stringable_keys.json`, generator next to it): a fresh `TemplateStringableKey` as the
//! item on 15 containers × 3 call types × 6 modes, comparing the result or the error, the number of `__toString()`
//! calls and the offsets a tracking `ArrayAccess` saw.

use std::sync::Arc;

use twig::escaper::DefaultStrategy;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, SandboxExtension, SecurityPolicy,
};
use twig::value::object::{ArrayIterator, ArrayObject, ObjectRef};
use twig::value::{Array, Value};
use twig_test_support::classes::spl::SplObjectStorage;

use crate::classes::*;
use crate::support::*;

fn string_value() -> Array {
    let mut array = Array::new();
    array.insert("string", "value");
    array
}

/// The key of a case and, for `stringable`, the object (to count its `__toString()` calls).
fn key(kind: &str) -> (Value, Option<ObjectRef>) {
    match kind {
        "stringable" => {
            let key = template_stringable_key();
            (Value::Object(key.clone()), Some(key))
        }
        "object" => (std_class(), None),
        "array" => (Value::Array(Array::from_values([Value::from("x")])), None),
        "null" => (Value::Null, None),
        "int" => (Value::Int(1), None),
        "float" => (Value::Float(1.5), None),
        "true" => (Value::Bool(true), None),
        other => panic!("unknown key {other}"),
    }
}

fn container(name: &str, key: &Value) -> Value {
    // SplObjectStorage and TemplateTrackingArrayAccess need an object key
    let object_key = match key {
        Value::Object(object) => Some(object.clone()),
        _ => None,
    };
    let tracked_key = || {
        object_key
            .clone()
            .unwrap_or_else(|| std_class().as_object().unwrap().clone())
    };
    match name {
        "array" => Value::Array(string_value()),
        "ArrayObject" => Value::Object(ArrayObject::new(string_value()).into_ref()),
        "ArrayIterator" => Value::Object(ArrayIterator::new(string_value()).into_ref()),
        "RecursiveArrayIterator" => {
            named_array_iterator_subclass("RecursiveArrayIterator", string_value())
        }
        "TemplateArrayObjectSubclass" => template_array_object_subclass(string_value()),
        "TemplateArrayIteratorSubclass" => template_array_iterator_subclass(string_value()),
        "SplObjectStorage" => {
            let storage = SplObjectStorage::new();
            if let Some(object) = &object_key {
                storage.attach(object.clone(), Value::from("value"));
            }
            Value::Object(storage.into_ref())
        }
        "TemplateTrackingArrayAccess" => {
            Value::Object(template_tracking_array_access(&tracked_key(), false))
        }
        "TemplateTrackingArrayAccessRejecting" => {
            Value::Object(template_tracking_array_access(&tracked_key(), true))
        }
        "TemplateArrayAccessObject" => template_array_access_object(),
        "stdClass" => std_object(&string_value()),
        "TemplateMethodObject" => template_method_object(),
        "TemplateMagicMethodObject" => template_magic_method_object(),
        "null" => Value::Null,
        "string" => Value::from("str"),
        other => panic!("unknown container {other}"),
    }
}

fn environment(call_type: &str, mode: &str) -> (Environment, Captured) {
    let expression = match call_type {
        "any" => "o.(k)",
        "method" => "o.(k)()",
        _ => "o[k]",
    };
    let template = if mode.starts_with("defined") {
        format!("{{{{ capture({expression} is defined) }}}}")
    } else {
        format!("{{{{ capture({expression}) }}}}")
    };
    let env = env_with(
        &[("index.twig", template.as_str())],
        EnvironmentOptions {
            strict_variables: mode.ends_with("strict"),
            autoescape: DefaultStrategy::Disabled,
            ..Default::default()
        },
    );
    let captured = add_capture(&env);
    let policy = match mode {
        "sandbox_allow" => {
            let policy = SecurityPolicy::new();
            policy.set_allowed_methods([("Twig\\Tests\\TemplateStringableKey", ["__toString"])]);
            policy.set_allowed_properties([
                ("Twig\\Tests\\TemplateArrayObjectSubclass", ["string"]),
                ("Twig\\Tests\\TemplateArrayIteratorSubclass", ["string"]),
                ("Twig\\Tests\\TemplateTrackingArrayAccess", ["string"]),
                ("Twig\\Tests\\TemplateArrayAccessObject", ["string"]),
                ("stdClass", ["string"]),
            ]);
            Some(policy)
        }
        "sandbox_deny" => Some(SecurityPolicy::new()),
        _ => None,
    };
    if let Some(policy) = policy {
        policy.set_allowed_functions(["capture"]);
        env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), true)))
            .unwrap();
    }
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    (env, captured)
}

/// The known mismatch of the sweep.
///
/// - `property_error`: in a sandbox, PHP builds `new SecurityNotAllowedPropertyError($message, $class, $property)` with
///   the raw key of an `ArrayAccess` object that is not one of `CoreExtension::ARRAY_LIKE_CLASSES` (userland classes,
///   subclasses of `ArrayObject`/`ArrayIterator`); its `string $propertyName` parameter rejects `null` and arrays with
///   a `TypeError` the template wraps, and converts a Stringable key once more (one more `__toString()` call).
///   twig-rs reports the property violation for `""`/`"Array"` and calls `__toString()` once less.
fn known_mismatch(key: &str, container: &str, call_type: &str, mode: &str) -> Option<&'static str> {
    let sandboxed = mode.starts_with("sandbox") && call_type != "method";
    let user_array_access = matches!(
        container,
        "TemplateTrackingArrayAccess"
            | "TemplateTrackingArrayAccessRejecting"
            | "TemplateArrayAccessObject"
    );
    let spl_subclass = matches!(
        container,
        "TemplateArrayObjectSubclass" | "TemplateArrayIteratorSubclass"
    );
    if sandboxed && (user_array_access || spl_subclass) {
        match key {
            "null" | "array" => return Some("property_error"),
            // the policy allows "string", but compares the raw object key
            "stringable" if user_array_access && mode == "sandbox_allow" => {
                return Some("property_error");
            }
            _ => {}
        }
    }
    None
}

/// Runs the cases selected by `select` (given the known-mismatch cluster of the case, `None` for the others); returns
/// how many ran and the differences.
fn run_sweep(select: impl Fn(Option<&'static str>) -> bool) -> (usize, Failures) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/upstream/runtime/stringable_keys.json"
    );
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the vectors")).unwrap();
    assert_eq!(cases.len(), 1890);
    let mut envs = Vec::new();
    for call_type in ["any", "method", "array"] {
        for mode in [
            "lax",
            "strict",
            "defined_lax",
            "defined_strict",
            "sandbox_allow",
            "sandbox_deny",
        ] {
            let (env, captured) = environment(call_type, mode);
            envs.push(((call_type, mode), env, captured));
        }
    }
    let mut failures = Failures::default();
    let mut count = 0;
    for case in &cases {
        let key_kind = case[0].as_str().unwrap();
        let name = case[1].as_str().unwrap();
        let call_type = case[2].as_str().unwrap();
        let mode = case[3].as_str().unwrap();
        if !select(known_mismatch(key_kind, name, call_type, mode)) {
            continue;
        }
        count += 1;
        let (_, env, captured) = envs
            .iter()
            .find(|(key, _, _)| *key == (call_type, mode))
            .unwrap();
        let (key, stringable) = key(key_kind);
        let data = container(name, &key);
        captured.take();
        let _ = env.take_notices();
        let result = env.render("index.twig", ctx(&[("o", data.clone()), ("k", key)]));
        let (kind, text) = match result {
            Ok(_) => (
                "value".to_owned(),
                captured
                    .take()
                    .iter()
                    .map(describe_value)
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
            Err(error) => ("error".to_owned(), describe(&error)),
        };
        let tracked = |property: &str| match &data {
            Value::Object(object)
                if object.class_name() == "Twig\\Tests\\TemplateTrackingArrayAccess" =>
            {
                tracked_calls(object, property).len() as i64
            }
            _ => -1,
        };
        failures.check(
            &format!("{key_kind} {name} {call_type} {mode}"),
            (
                kind,
                text,
                stringable.as_ref().map_or(-1, to_string_calls),
                tracked("offsetExistsCalls"),
                tracked("offsetGetCalls"),
                notices(env),
            ),
            (
                case[4].as_str().unwrap().to_owned(),
                strip_php_location(case[5].as_str().unwrap()),
                case[6].as_i64().unwrap(),
                case[7].as_i64().unwrap(),
                case[8].as_i64().unwrap(),
                case[9]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect::<Vec<_>>(),
            ),
        );
    }
    (count, failures)
}

#[test]
fn stringable_keys_match_the_oracle() {
    let (count, failures) = run_sweep(|known| known.is_none());
    assert!(count > 1800, "only {count} cases");
    failures.assert_none();
}

#[test]
fn stringable_keys_sandboxed_property_errors_of_array_access_objects() {
    let (count, failures) = run_sweep(|known| known == Some("property_error"));
    assert_eq!(count, 46);
    failures.assert_none();
}
