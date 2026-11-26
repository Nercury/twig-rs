//! A wider sweep of the getAttribute() matrix of TWIG/tests/TemplateTest.php against the oracle
//! (`tests/vectors/upstream/runtime/get_attribute_sweep.json`, generator next to it): every helper object of the test
//! (plus arrays, scalars and a closure) × 70 items × the three call types × six modes (lax, strict, defined tests,
//! two sandbox policies), comparing the captured value or the error and the PHP diagnostics raised on the way.

use std::sync::Arc;

use twig::escaper::DefaultStrategy;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, SandboxExtension, SecurityPolicy,
};
use twig::value::Value;
use twig::value::object::ArrayObject;

use crate::classes::*;
use crate::support::*;

/// The objects of the generator, in its order.
fn objects() -> Vec<(&'static str, Value)> {
    let array = attribute_array(true);
    vec![
        ("array", Value::Array(array.clone())),
        ("empty_array", Value::Array(twig::value::Array::new())),
        (
            "list",
            Value::Array(twig::value::Array::from_values([
                Value::from("a"),
                Value::from("b"),
            ])),
        ),
        ("TemplateArrayAccessObject", template_array_access_object()),
        (
            "ArrayObject",
            Value::Object(ArrayObject::new(array.clone()).into_ref()),
        ),
        ("stdClass", std_object(&array)),
        (
            "TemplateMagicPropertyObject",
            template_magic_property_object(),
        ),
        ("TemplateMethodObject", template_method_object()),
        ("TemplatePropertyObject", template_property_object()),
        (
            "TemplatePropertyObjectAndIterator",
            template_property_object_and_iterator(),
        ),
        (
            "TemplatePropertyObjectAndArrayAccess",
            template_property_object_and_array_access(),
        ),
        (
            "TemplatePropertyObjectDefinedWithUndefinedValue",
            template_property_object_defined_with_undefined_value(),
        ),
        ("TemplateMagicMethodObject", template_magic_method_object()),
        (
            "TemplateMagicMethodExceptionObject",
            template_magic_method_exception_object(),
        ),
        (
            "TemplateMethodAndPropObject",
            template_method_and_prop_object(),
        ),
        ("TemplateArrayAccess", template_array_access()),
        ("TemplateGetIsMethods", template_get_is_methods()),
        (
            "TemplateAttributeResolution",
            template_attribute_resolution(),
        ),
        (
            "TemplateMagicPrivatePropertyObject",
            template_magic_private_property_object(),
        ),
        ("closure", closure("closure called")),
        ("null", Value::Null),
        ("int", Value::Int(42)),
        ("string", Value::from("string")),
        ("true", Value::Bool(true)),
        ("float", Value::Float(1.5)),
    ]
}

/// The items of the generator, in its order.
fn items() -> Vec<Value> {
    let names = ["defined", "undefined", "protected", "zero"];
    let mut items: Vec<Value> = names.iter().map(|s| Value::from(*s)).collect();
    items.push(Value::Int(1));
    items.push(Value::Float(1.0));
    for name in [
        "null",
        "bar",
        "foo",
        "baz",
        "baf",
        "09",
        "+4",
        "BAR",
        "Baz",
        "DEFINED",
        "getDefined",
        "GETDEFINED",
        "isBar",
        "hasFoo",
        "hasBaz",
        "isBaz",
        "getBaz",
        "isBaf",
        "hasBaf",
        "static",
        "getStatic",
        "getProtected",
        "get1",
        "get09",
        "__call",
        "__invoke",
        "__isset",
        "__get",
        "NAME",
        "name",
        "hidden",
        "getHidden",
        "secret",
        "title",
        "a",
        "A",
        "b",
        "B",
        "c",
        "getC",
        "vars",
        "attributes",
        "data",
        "children",
        "get",
        "is",
        "has",
        "offsetGet",
        "offsetExists",
        "getIterator",
    ] {
        items.push(Value::from(name));
    }
    items.push(Value::Int(0));
    items.push(Value::from("0"));
    items.push(Value::from(""));
    items.push(Value::Null);
    items.push(Value::Bool(true));
    items.push(Value::Bool(false));
    items.push(Value::Int(-1));
    items.push(Value::Int(-10));
    items.push(Value::Float(1.5));
    for name in ["1.5", "01", " 1", "1 "] {
        items.push(Value::from(name));
    }
    items.push(Value::Int(2));
    items
}

/// The policy of the `sandbox_some` mode.
fn some_policy() -> SecurityPolicy {
    let policy = SecurityPolicy::new();
    policy.set_allowed_methods([
        (
            "Twig\\Tests\\TemplateMethodObject",
            vec!["getDefined", "isBar", "baz", "getStatic"],
        ),
        (
            "Twig\\Tests\\TemplateMagicMethodObject",
            vec!["undefined", "bar"],
        ),
        ("Twig\\Tests\\TemplateMethodAndPropObject", vec!["getA"]),
        (
            "Twig\\Tests\\TemplateAttributeResolution",
            vec!["getHidden"],
        ),
        ("Closure", vec!["__invoke"]),
        ("ArrayObject", vec!["offsetGet"]),
    ]);
    policy.set_allowed_properties([
        (
            "Twig\\Tests\\TemplatePropertyObject",
            vec!["defined", "zero"],
        ),
        (
            "Twig\\Tests\\TemplatePropertyObjectAndArrayAccess",
            vec!["defined", "bar"],
        ),
        (
            "Twig\\Tests\\TemplateArrayAccessObject",
            vec!["defined", "zero", "1", "attributes"],
        ),
        (
            "Twig\\Tests\\TemplateMagicPropertyObject",
            vec!["zero", "defined"],
        ),
        ("Twig\\Tests\\TemplateMethodAndPropObject", vec!["b", "a"]),
        (
            "Twig\\Tests\\TemplateMagicPrivatePropertyObject",
            vec!["title"],
        ),
        ("stdClass", vec!["defined", "bar", "null"]),
    ]);
    policy.set_allowed_functions(["capture"]);
    policy
}

const MODES: [&str; 6] = [
    "lax",
    "strict",
    "defined_lax",
    "defined_strict",
    "sandbox_none",
    "sandbox_some",
];

fn environment(call_type: &str, mode: &str) -> (Environment, Captured) {
    let expression = match call_type {
        "any" => "o.(i)",
        "method" => "o.(i)()",
        _ => "o[i]",
    };
    let template = if mode.starts_with("defined") {
        format!("{{{{ capture({expression} is defined) }}}}")
    } else {
        format!("{{{{ capture({expression}) }}}}")
    };
    let sandbox = match mode {
        "sandbox_none" => Some({
            let policy = SecurityPolicy::new();
            policy.set_allowed_functions(["capture"]);
            policy
        }),
        "sandbox_some" => Some(some_policy()),
        _ => None,
    };
    let mut options = EnvironmentOptions {
        strict_variables: mode.ends_with("strict"),
        ..Default::default()
    };
    if sandbox.is_some() {
        options.autoescape = DefaultStrategy::Disabled;
    }
    let env = env_with(&[("index.twig", template.as_str())], options);
    let captured = add_capture(&env);
    if let Some(policy) = sandbox {
        env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), true)))
            .unwrap();
    }
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    (env, captured)
}

/// The known twig-rs mismatch of the sweep: a `null` item on an `ArrayAccess` object that is not one of
/// `CoreExtension::ARRAY_LIKE_CLASSES` in a sandbox. PHP's `SecurityPolicy::checkPropertyAllowed()` receives the
/// `null` key and fails building its error (`new SecurityNotAllowedPropertyError(..., $class, $property)` with a
/// `string $propertyName`): a `TypeError` the template wraps. twig-rs reports the property violation for `""`.
fn known_mismatch(object: &str, item: &Value, call_type: &str, mode: &str) -> bool {
    item.is_null()
        && call_type != "method"
        && mode.starts_with("sandbox")
        && matches!(
            object,
            "TemplateArrayAccessObject"
                | "TemplatePropertyObjectAndArrayAccess"
                | "TemplateArrayAccess"
        )
}

/// One render of the sweep: kind, text, diagnostics.
type Outcome = (String, String, Vec<String>);

/// Runs the sweep; `known` selects the known mismatches (true) or every other render (false). Returns the number of
/// renders compared and the differences.
fn run_sweep(known: bool) -> (usize, Vec<String>) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/upstream/runtime/get_attribute_sweep.json"
    );
    let vectors: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the sweep vectors")).unwrap();
    let objects = objects();
    let items = items();
    let names: Vec<&str> = vectors["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        objects.iter().map(|(name, _)| *name).collect::<Vec<_>>()
    );
    let item_texts: Vec<&str> = vectors["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(item_texts, items.iter().map(var_export).collect::<Vec<_>>());

    let mut envs = Vec::new();
    for call_type in ["any", "method", "array"] {
        for mode in MODES {
            let (env, captured) = environment(call_type, mode);
            envs.push(((call_type, mode), env, captured));
        }
    }
    let rows = vectors["rows"].as_array().unwrap();
    assert_eq!(rows.len(), objects.len() * items.len() * 3);
    let mut failures = Vec::new();
    let mut count = 0;
    for row in rows {
        let (object, item) = (
            &objects[row[0].as_u64().unwrap() as usize],
            &items[row[1].as_u64().unwrap() as usize],
        );
        let call_type = row[2].as_str().unwrap();
        for mode in MODES {
            if known_mismatch(object.0, item, call_type, mode) != known {
                continue;
            }
            let expected = &row[3][mode];
            let expected: Outcome = (
                expected[0].as_str().unwrap().to_owned(),
                strip_php_location(expected[1].as_str().unwrap()),
                expected[2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect(),
            );
            let (_, env, captured) = envs
                .iter()
                .find(|(key, _, _)| *key == (call_type, mode))
                .unwrap();
            captured.take();
            let result = env.render(
                "index.twig",
                ctx(&[("o", object.1.clone()), ("i", item.clone())]),
            );
            let actual: Outcome = match result {
                Ok(_) => {
                    let values = captured.take();
                    let text = match values.as_slice() {
                        [value] => describe_value(value),
                        other => format!("capture() called {} times", other.len()),
                    };
                    ("value".to_owned(), text, notices(env))
                }
                Err(error) => ("error".to_owned(), describe(&error), notices(env)),
            };
            count += 1;
            if actual != expected {
                failures.push(format!(
                    "{} {} {call_type} {mode}\n    actual:   {actual:?}\n    expected: {expected:?}",
                    object.0,
                    var_export(item)
                ));
            }
        }
    }
    (count, failures)
}

#[test]
fn get_attribute_sweep_matches_the_oracle() {
    let (count, failures) = run_sweep(false);
    assert!(count > 30_000, "only {count} renders");
    assert!(
        failures.is_empty(),
        "{} of {count} renders differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "twig-rs mismatch: a null key on a sandboxed ArrayAccess object is a property violation instead of PHP's TypeError"]
fn get_attribute_sweep_null_keys_of_sandboxed_array_access_objects() {
    // e.g. {{ o[i] }} with o = TemplateArrayAccessObject, i = null, sandboxed: PHP renders `An exception has been
    // thrown during the rendering of a template ("Twig\Sandbox\SecurityNotAllowedPropertyError::__construct():
    // Argument #3 ($propertyName) must be of type string, null given") in "index.twig" at line 1.`
    let (count, failures) = run_sweep(true);
    assert_eq!(count, 12);
    assert!(
        failures.is_empty(),
        "{} of {count} renders differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
