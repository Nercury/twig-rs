//! Port of TWIG/tests/TemplateTest.php.
//!
//! twig-rs has no public `CoreExtension::getAttribute()` and no `Template` subclassing, so the tests that call
//! `getAttribute()` directly run it through templates instead: `o.(i)` (`Template::ANY_CALL`), `o.(i)()`
//! (`METHOD_CALL`) and `o[i]` (`ARRAY_CALL`), plus `is defined` for the defined tests; `GetAttrExpression::compile()`
//! turns them into the same `getAttribute()` call (with the template's line, so strict-mode messages end with `at
//! line 1`). The oracle confirms that the template path gives every expectation of the data providers
//! (`tests/vectors/upstream/runtime/get_attribute.json`, generator next to it).
//!
//! twig-value has no `RecursiveArrayIterator` and its `ArrayObject`/`ArrayIterator` cannot take another class, so
//! `RecursiveArrayIterator`, `TemplateArrayObjectSubclass` and `TemplateArrayIteratorSubclass` are subclasses that
//! forward every handler to a parent instance (what a PHP subclass without overrides does).
//!
//! Not ported:
//! - `testDisplayBlocksAcceptTemplateOnlyAsBlocks`: passes a hand-made `$blocks` array to `Template::displayBlock()`;
//!   twig-rs's block tables are internal.
//! - `testRenderParentBlockRestoresOutputBuffersOnError`: PHP output buffers (`ob_get_level()`); twig-rs renders into
//!   Rust buffers.
//! - `testHasFixedParentOnlyReportsLineagesThatCanNoLongerMove`: `Template::hasFixedParent()`/`getParent()` are
//!   internal PHP API without a twig-rs equivalent.
//! - `testGetAttributeOnLazyGhostWithPrivatePropertyCallsTheGetterWithoutInitializingIt`: PHP 8.4 lazy objects.
//! - The first half of `testGetAttributeOnArrayWithConfusableKey` (plain PHP `$array[false]` lookups) tests PHP, not
//!   Twig; its `getAttribute()` half is ported.

use std::sync::Arc;

use twig::ErrorKind;
use twig::runtime::{Environment, SandboxExtension, SecurityError, SecurityPolicy};
use twig::value::object::ObjectRef;
use twig::value::{Array, Value};
use twig_test_support::classes::spl::SplObjectStorage;

use crate::classes::*;
use crate::support::*;

/// Builds a test object.
type ObjectFactory = fn() -> Value;

/// Builds an `ArrayAccess` container over an array.
type ContainerFactory = fn(Array) -> Value;

fn sandbox(env: &Environment, policy: SecurityPolicy) {
    env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), true)))
        .unwrap();
}

fn stringable_key_policy() -> SecurityPolicy {
    let policy = SecurityPolicy::new();
    policy.set_allowed_methods([("Twig\\Tests\\TemplateStringableKey", ["__toString"])]);
    policy
}

fn string_value_array() -> Array {
    let mut array = Array::new();
    array.insert("string", "value");
    array
}

// ---- testGetAttributeExceptions ----------------------------------------------------------------------------------

fn get_attribute_exceptions() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "{{ string[\"a\"] }}",
            "Impossible to access a key (\"a\") on a string variable (\"foo\") in \"%s\" at line 1.",
        ),
        (
            "{{ null[\"a\"] }}",
            "Impossible to access a key (\"a\") on a null variable in \"%s\" at line 1.",
        ),
        (
            "{{ empty_array[\"a\"] }}",
            "Key \"a\" does not exist as the sequence/mapping is empty in \"%s\" at line 1.",
        ),
        (
            "{{ array[\"a\"] }}",
            "Key \"a\" for sequence/mapping with keys \"foo\" does not exist in \"%s\" at line 1.",
        ),
        (
            "{{ array_access[\"a\"] }}",
            "Key \"a\" does not exist in ArrayAccess-able object of class \"Twig\\Tests\\TemplateArrayAccessObject\" in \"%s\" at line 1.",
        ),
        (
            "{{ string.a }}",
            "Impossible to access an attribute (\"a\") on a string variable (\"foo\") in \"%s\" at line 1.",
        ),
        (
            "{{ string.a() }}",
            "Impossible to invoke a method (\"a\") on a string variable (\"foo\") in \"%s\" at line 1.",
        ),
        (
            "{{ null.a }}",
            "Impossible to access an attribute (\"a\") on a null variable in \"%s\" at line 1.",
        ),
        (
            "{{ null.a() }}",
            "Impossible to invoke a method (\"a\") on a null variable in \"%s\" at line 1.",
        ),
        (
            "{{ array.a() }}",
            "Impossible to invoke a method (\"a\") on a sequence/mapping in \"%s\" at line 1.",
        ),
        (
            "{{ empty_array.a }}",
            "Key \"a\" does not exist as the sequence/mapping is empty in \"%s\" at line 1.",
        ),
        (
            "{{ array.a }}",
            "Key \"a\" for sequence/mapping with keys \"foo\" does not exist in \"%s\" at line 1.",
        ),
        (
            "{{ array.(-10) }}",
            "Key \"-10\" for sequence/mapping with keys \"foo\" does not exist in \"%s\" at line 1.",
        ),
        (
            "{{ array_access.a }}",
            "Neither the property \"a\" nor one of the methods \"a()\", \"geta()\", \"isa()\", \"hasa()\" or \"__call()\" exist and have public access in class \"Twig\\Tests\\TemplateArrayAccessObject\" in \"%s\" at line 1.",
        ),
        (
            "{% from _self import foo %}{% macro foo(obj) %}{{ obj.missing_method() }}{% endmacro %}{{ foo(array_access) }}",
            "Neither the property \"missing_method\" nor one of the methods \"missing_method()\", \"getmissing_method()\", \"ismissing_method()\", \"hasmissing_method()\" or \"__call()\" exist and have public access in class \"Twig\\Tests\\TemplateArrayAccessObject\" in \"%s\" at line 1.",
        ),
        (
            "{{ magic_exception.test }}",
            "An exception has been thrown during the rendering of a template (\"Hey! Don't try to isset me!\") in \"%s\" at line 1.",
        ),
        (
            "{{ object[\"a\"] }}",
            "Impossible to access a key \"a\" on an object of class \"stdClass\" that does not implement ArrayAccess interface in \"%s\" at line 1.",
        ),
    ]
}

#[test]
fn test_get_attribute_exceptions() {
    let mut failures = Failures::default();
    for (template, message) in get_attribute_exceptions() {
        let env = env_with(&[("index", template)], strict(true));
        let mut array = Array::new();
        array.insert("foo", "foo");
        let context = ctx(&[
            ("string", Value::from("foo")),
            ("null", Value::Null),
            ("empty_array", Value::Array(Array::new())),
            ("array", Value::Array(array)),
            ("array_access", template_array_access_object()),
            (
                "magic_exception",
                template_magic_property_object_with_exception(),
            ),
            ("object", std_class()),
        ]);
        match env.render("index", context) {
            Ok(output) => failures.fail(format!("{template}: no exception, rendered {output:?}")),
            Err(error) => failures.check(
                template,
                (error.kind(), error.to_string()),
                (ErrorKind::Runtime, message.replace("%s", "index")),
            ),
        }
    }
    failures.assert_none();
}

// ---- testGetAttributeWithSandbox ---------------------------------------------------------------------------------

#[test]
fn test_get_attribute_with_sandbox() {
    // `CoreExtension::getAttribute($twig, $source, $object, 'defined', [], 'any', false, false, true)` is what
    // `{{ object.defined }}` compiles to once the SandboxExtension is registered; autoescaping is off so that no
    // `escape` filter is checked first. VERIFIED with the oracle (messages).
    let objects: [(&str, ObjectFactory); 2] = [
        ("TemplatePropertyObject", template_property_object),
        ("TemplateMethodObject", template_method_object),
    ];
    let mut failures = Failures::default();
    for (class, object) in objects {
        for allowed in [false, true] {
            let env = env_with(&[("index", "{{ object.defined }}")], strict_raw(false));
            sandbox_with(&env, SecurityPolicy::new(), !allowed);
            let result = env.render("index", ctx(&[("object", object())]));
            let label = format!("{class} allowed={allowed}");
            let expected = if allowed {
                Ok("defined".to_owned())
            } else {
                Err(format!(
                    "Twig\\Sandbox\\SecurityNotAllowedPropertyError: Calling \"defined\" property on a \"Twig\\Tests\\{class}\" object is not allowed in \"index\" at line 1."
                ))
            };
            failures.check(&label, result.map_err(|e| describe(&e)), expected);
        }
    }
    failures.assert_none();
}

fn sandbox_with(env: &Environment, policy: SecurityPolicy, global: bool) {
    env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), global)))
        .unwrap();
}

// ---- testRenderTemplateWithoutOutput, testNullCoalesceWithImportedMacro -------------------------------------------

#[test]
fn test_render_template_without_output() {
    for template in [
        "",
        "{% for var in [] %}{% endfor %}",
        "{% if false %}{% endif %}",
    ] {
        let env = env(&[("index", template)]);
        assert_eq!(env.render("index", Array::new()).unwrap(), "", "{template}");
    }
}

#[test]
fn test_null_coalesce_with_imported_macro() {
    let data: [(&str, &str, &str); 3] = [
        (
            "from import",
            "{% from \"helper.twig\" import foo %}{{ foo(\"bar\") ?? \"\" }}",
            "bar",
        ),
        (
            "from import with undefined macro falls back",
            "{% from \"helper.twig\" import foo, nonexistent %}{{ nonexistent(\"bar\") ?? \"fallback\" }}",
            "fallback",
        ),
        (
            "from import used multiple times",
            "{% from \"helper.twig\" import foo %}{{ foo(\"a\") ?? \"\" }}-{{ foo(\"b\") ?? \"\" }}",
            "a-b",
        ),
    ];
    let mut failures = Failures::default();
    for (name, index, expected) in data {
        let env = env(&[
            ("index.twig", index),
            (
                "helper.twig",
                "{% macro foo(param) %}{{ param }}{% endmacro %}",
            ),
        ]);
        let output = env.render("index.twig", Array::new());
        failures.check(
            name,
            output
                .map(|o| o.trim().to_owned())
                .map_err(|e| e.to_string()),
            Ok(expected.to_owned()),
        );
    }
    failures.assert_none();
}

// ---- blocks ------------------------------------------------------------------------------------------------------

#[test]
fn test_render_block_with_undefined_block() {
    // `new TemplateForTest($twig, 'index.twig')`: a template without blocks; renderBlock() through the wrapper
    let env = env(&[("index.twig", "")]);
    let error = expect_error(
        env.load("index.twig")
            .unwrap()
            .render_block("unknown", Array::new()),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Block \"unknown\" on template \"index.twig\" does not exist in \"index.twig\"."
    );
}

#[test]
fn test_display_block_with_undefined_block() {
    let env = env(&[("index.twig", "")]);
    let mut out: Vec<u8> = Vec::new();
    let error = expect_error(
        env.load("index.twig")
            .unwrap()
            .display_block("unknown", Array::new(), &mut out),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Block \"unknown\" on template \"index.twig\" does not exist in \"index.twig\"."
    );
}

#[test]
fn test_display_block_with_undefined_parent_block() {
    // The PHPUnit test calls `parent.twig`'s displayBlock('foo', [], ['foo' => [index.twig, 'block_foo']], false)
    // by hand; `{{ parent() }}` in a child block makes the same call (`yieldParentBlock()` → `yieldBlock(..., false)`)
    // when a parent without that block renders it with `block()`. The error is then guessed at the `parent()` call.
    // VERIFIED with the oracle.
    let env = env(&[
        (
            "index.twig",
            "{% extends \"parent.twig\" %}{% block foo %}{{ parent() }}{% endblock %}",
        ),
        ("parent.twig", "{{ block(\"foo\") }}"),
    ]);
    let error = expect_error(env.render("index.twig", Array::new()), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "Block \"foo\" should not call parent() in \"index.twig\" as the block does not exist in the parent template \"parent.twig\" in \"index.twig\" at line 1."
    );
}

// ---- array keys --------------------------------------------------------------------------------------------------

#[test]
fn test_get_attribute_on_array_with_confusable_key() {
    let mut array = Array::new();
    array.insert(0, "Zero");
    array.insert(1, "One");
    array.insert(-1, "MinusOne");
    array.insert("", "EmptyString");
    array.insert("1.5", "FloatButString");
    array.insert("01", "IntegerButStringWithLeadingZeros");
    let env = env(&[("index.twig", "{{ capture(attribute(array, key)) }}")]);
    let captured = add_capture(&env);
    let cases = [
        (Value::Bool(false), "Zero"),
        (Value::Bool(true), "One"),
        (Value::Float(1.5), "One"),
        (Value::from("1"), "One"),
        (Value::Float(-1.5), "MinusOne"),
        (Value::from("1.5"), "FloatButString"),
        (Value::from("01"), "IntegerButStringWithLeadingZeros"),
        (Value::Null, "EmptyString"),
    ];
    let mut failures = Failures::default();
    for (key, expected) in cases {
        let label = var_export(&key);
        match env.render(
            "index.twig",
            ctx(&[("array", Value::Array(array.clone())), ("key", key)]),
        ) {
            Ok(_) => failures.check(
                &label,
                var_export(&captured.one()),
                var_export(&Value::from(expected)),
            ),
            Err(error) => failures.fail(format!("{label}: {}", describe(&error))),
        }
    }
    failures.assert_none();
}

#[test]
fn test_array_with_stringable_key_is_consistent_across_strict_modes() {
    for strict in [false, true] {
        let env = env_with(&[("index", "{{ array[object] }}")], strict_raw(strict));
        let key = template_stringable_key();
        let output = env.render(
            "index",
            ctx(&[
                ("array", Value::Array(string_value_array())),
                ("object", Value::Object(key)),
            ]),
        );
        assert_eq!(output.unwrap(), "value", "strict={strict}");
    }
}

fn internal_array_access(kind: &str) -> Value {
    match kind {
        "ArrayObject" => {
            Value::Object(twig::value::object::ArrayObject::new(string_value_array()).into_ref())
        }
        "ArrayIterator" => {
            Value::Object(twig::value::object::ArrayIterator::new(string_value_array()).into_ref())
        }
        "RecursiveArrayIterator" => recursive_array_iterator(string_value_array()),
        other => panic!("unknown container {other}"),
    }
}

/// `new \RecursiveArrayIterator($array)`. twig-value has no `RecursiveArrayIterator`; it extends `ArrayIterator`
/// without changing its dimension handlers, so a subclass of that name stands in for it (the getAttribute() logic
/// under test only looks at the class name and at `instanceof \ArrayIterator`).
fn recursive_array_iterator(array: Array) -> Value {
    crate::classes::named_array_iterator_subclass("RecursiveArrayIterator", array)
}

#[test]
fn test_stringable_key_is_coerced_for_internal_array_access() {
    let mut failures = Failures::default();
    for (mode, strict) in [("lax", false), ("strict", true)] {
        for (sandbox_mode, sandboxed) in [("unsandboxed", false), ("sandboxed", true)] {
            for container in ["ArrayObject", "ArrayIterator", "RecursiveArrayIterator"] {
                let label = format!("{mode} {sandbox_mode} {container}");
                let env = env_with(&[("index", "{{ data[key] }}")], strict_raw(strict));
                let key = template_stringable_key();
                if sandboxed {
                    sandbox(&env, stringable_key_policy());
                }
                let output = env.render(
                    "index",
                    ctx(&[
                        ("data", internal_array_access(container)),
                        ("key", Value::Object(key.clone())),
                    ]),
                );
                failures.check(
                    &label,
                    (output.map_err(|e| describe(&e)), to_string_calls(&key)),
                    (Ok("value".to_owned()), 1),
                );
            }
        }
    }
    failures.assert_none();
}

#[test]
fn test_stringable_key_is_coerced_for_subclasses_of_internal_array_access() {
    let containers: [(&str, &str, ContainerFactory); 2] = [
        (
            "ArrayObject subclass",
            "Twig\\Tests\\TemplateArrayObjectSubclass",
            template_array_object_subclass,
        ),
        (
            "ArrayIterator subclass",
            "Twig\\Tests\\TemplateArrayIteratorSubclass",
            template_array_iterator_subclass,
        ),
    ];
    let mut failures = Failures::default();
    for (mode, strict) in [("lax", false), ("strict", true)] {
        for (sandbox_mode, sandboxed) in [("unsandboxed", false), ("sandboxed", true)] {
            for (name, class, container) in containers {
                let label = format!("{mode} {sandbox_mode} {name}");
                let env = env_with(&[("index", "{{ data[key] }}")], strict_raw(strict));
                let key = template_stringable_key();
                if sandboxed {
                    // subclasses are not part of CoreExtension::ARRAY_LIKE_CLASSES, so the sandbox checks the key as a
                    // property
                    let policy = stringable_key_policy();
                    policy.set_allowed_properties([(class, ["string"])]);
                    sandbox(&env, policy);
                }
                let output = env.render(
                    "index",
                    ctx(&[
                        ("data", container(string_value_array())),
                        ("key", Value::Object(key.clone())),
                    ]),
                );
                failures.check(
                    &label,
                    (output.map_err(|e| describe(&e)), to_string_calls(&key)),
                    (Ok("value".to_owned()), 1),
                );
            }
        }
    }
    failures.assert_none();
}

fn spl_object_storage(key: &ObjectRef) -> Value {
    // `$data = new \SplObjectStorage(); $data[$key] = 'value';`
    let storage = SplObjectStorage::new();
    storage.attach(key.clone(), Value::from("value"));
    Value::Object(storage.into_ref())
}

#[test]
fn test_array_access_with_object_key_keeps_the_object_key() {
    for strict in [false, true] {
        let env = env_with(&[("index", "{{ data[key] }}")], strict_raw(strict));
        let key = template_stringable_key();
        let output = env.render(
            "index",
            ctx(&[
                ("data", spl_object_storage(&key)),
                ("key", Value::Object(key.clone())),
            ]),
        );
        assert_eq!(output.unwrap(), "value", "strict={strict}");
        assert_eq!(to_string_calls(&key), 0, "strict={strict}");
    }
}

fn is_key(values: &[Value], key: &ObjectRef) -> bool {
    matches!(values, [Value::Object(o)] if ObjectRef::ptr_eq(o, key))
}

#[test]
fn test_array_access_lookup_does_not_repeat_offset_checks() {
    for strict in [false, true] {
        let env = env_with(&[("index", "{{ data[key] }}")], strict_raw(strict));
        let key = template_stringable_key();
        let data = template_tracking_array_access(&key, false);
        let output = env.render(
            "index",
            ctx(&[
                ("data", Value::Object(data.clone())),
                ("key", Value::Object(key.clone())),
            ]),
        );
        assert_eq!(output.unwrap(), "value", "strict={strict}");
        assert!(
            is_key(&tracked_calls(&data, "offsetExistsCalls"), &key),
            "strict={strict}: offsetExists calls {:?}",
            tracked_calls(&data, "offsetExistsCalls")
        );
        assert!(
            is_key(&tracked_calls(&data, "offsetGetCalls"), &key),
            "strict={strict}: offsetGet calls {:?}",
            tracked_calls(&data, "offsetGetCalls")
        );
        assert_eq!(to_string_calls(&key), 0, "strict={strict}");
    }
}

#[test]
fn test_array_access_defined_test_does_not_read_the_offset() {
    let env = env(&[("index", "{{ data[key] is defined ? \"yes\" : \"no\" }}")]);
    let key = template_stringable_key();
    let data = template_tracking_array_access(&key, false);
    let output = env.render(
        "index",
        ctx(&[
            ("data", Value::Object(data.clone())),
            ("key", Value::Object(key.clone())),
        ]),
    );
    assert_eq!(output.unwrap(), "yes");
    assert!(is_key(&tracked_calls(&data, "offsetExistsCalls"), &key));
    assert!(tracked_calls(&data, "offsetGetCalls").is_empty());
}

#[test]
fn test_rejected_stringable_array_access_key_rethrows_original_type_error() {
    for strict in [false, true] {
        let env = env_with(&[("index", "{{ data[key] }}")], strict_raw(strict));
        let key = template_stringable_key();
        let data = template_tracking_array_access(&key, true);
        let error = expect_error(
            env.render(
                "index",
                ctx(&[
                    ("data", Value::Object(data.clone())),
                    ("key", Value::Object(key.clone())),
                ]),
            ),
            ErrorKind::Runtime,
        );
        // `assertSame($data->offsetExistsError, $e->getPrevious())`: the TypeError thrown by offsetExists()
        assert_eq!(
            previous(&error).as_deref(),
            Some("TypeError: Object offsets are not supported."),
            "strict={strict}"
        );
        assert!(is_key(&tracked_calls(&data, "offsetExistsCalls"), &key));
        assert!(tracked_calls(&data, "offsetGetCalls").is_empty());
        assert_eq!(to_string_calls(&key), 0);
    }
}

#[test]
fn test_sandbox_does_not_authorize_string_property_for_array_access_object_key() {
    for strict in [false, true] {
        let env = env_with(&[("index", "{{ data[key] }}")], strict_raw(strict));
        let key = template_stringable_key();
        let data = template_tracking_array_access(&key, false);
        let policy = stringable_key_policy();
        policy.set_allowed_properties([("Twig\\Tests\\TemplateTrackingArrayAccess", ["string"])]);
        sandbox(&env, policy);
        let result = env.render(
            "index",
            ctx(&[
                ("data", Value::Object(data.clone())),
                ("key", Value::Object(key.clone())),
            ]),
        );
        let error = expect_error(result, ErrorKind::SecurityNotAllowedProperty);
        assert!(SecurityError::of(&error).is_some(), "strict={strict}");
        assert!(tracked_calls(&data, "offsetExistsCalls").is_empty());
        assert!(tracked_calls(&data, "offsetGetCalls").is_empty());
    }
}

#[test]
fn test_array_with_stringable_key_is_checked_by_sandbox() {
    for strict in [false, true] {
        let key = template_stringable_key();
        let context = ctx(&[
            ("array", Value::Array(string_value_array())),
            ("key", Value::Object(key.clone())),
        ]);

        let env = env_with(&[("index", "{{ array[key] }}")], strict_raw(strict));
        sandbox(&env, SecurityPolicy::new());
        let error = env.render("index", context.clone()).unwrap_err();
        assert!(error.kind().is_security_error(), "{}", describe(&error));
        assert!(
            error.to_string().to_lowercase().contains("__tostring"),
            "{error}"
        );
        assert_eq!(to_string_calls(&key), 0);

        let env = env_with(&[("index", "{{ array[key] }}")], strict_raw(strict));
        sandbox(&env, stringable_key_policy());
        assert_eq!(env.render("index", context).unwrap(), "value");
        assert_eq!(to_string_calls(&key), 1);
    }
}

#[test]
fn test_internal_array_access_with_stringable_key_is_checked_by_sandbox() {
    for strict in [false, true] {
        let key = template_stringable_key();
        let env = env_with(&[("index", "{{ data[key] }}")], strict_raw(strict));
        sandbox(&env, SecurityPolicy::new());
        let error = env
            .render(
                "index",
                ctx(&[
                    ("data", internal_array_access("ArrayObject")),
                    ("key", Value::Object(key.clone())),
                ]),
            )
            .unwrap_err();
        assert!(error.kind().is_security_error(), "{}", describe(&error));
        assert!(
            error.to_string().to_lowercase().contains("__tostring"),
            "{error}"
        );
        assert_eq!(to_string_calls(&key), 0);
    }
}

#[test]
fn test_sandboxed_array_access_with_object_key_keeps_the_object_key() {
    let key = template_stringable_key();
    let env = env_with(&[("index", "{{ data[key] }}")], strict_raw(false));
    sandbox(&env, stringable_key_policy());
    let output = env.render(
        "index",
        ctx(&[
            ("data", spl_object_storage(&key)),
            ("key", Value::Object(key.clone())),
        ]),
    );
    assert_eq!(output.unwrap(), "value");
    assert_eq!(to_string_calls(&key), 0);
}

// ---- the getAttribute() matrix -----------------------------------------------------------------------------------

/// A data set of `getGetAttributeTests()`: defined, value, object, item, type, exception message.
struct AttributeCase {
    defined: bool,
    value: Value,
    object: Value,
    item: Value,
    call_type: &'static str,
    message: Option<&'static str>,
}

/// Twig: `TemplateTest::getGetAttributeTests()`, in the same order.
fn get_get_attribute_tests() -> Vec<AttributeCase> {
    let array = attribute_array(true);
    let object_array = template_array_access_object();
    let array_object =
        Value::Object(twig::value::object::ArrayObject::new(array.clone()).into_ref());
    let std_object = std_object(&array);
    let magic_property_object = template_magic_property_object();
    let property_object = template_property_object();
    let property_object1 = template_property_object_and_iterator();
    let property_object2 = template_property_object_and_array_access();
    let property_object3 = template_property_object_defined_with_undefined_value();
    let method_object = template_method_object();
    let magic_method_object = template_magic_method_object();

    let basic_tests: Vec<(bool, Value, Value)> = vec![
        (true, Value::from("defined"), Value::from("defined")),
        (false, Value::Null, Value::from("undefined")),
        (false, Value::Null, Value::from("protected")),
        (true, Value::Int(0), Value::from("zero")),
        (true, Value::Int(1), Value::Int(1)),
        (true, Value::Int(1), Value::Float(1.0)),
        (true, Value::Null, Value::from("null")),
        (true, Value::Bool(true), Value::from("bar")),
        (true, Value::Bool(true), Value::from("foo")),
        (true, Value::from("baz"), Value::from("baz")),
        (true, Value::from("baf"), Value::from("baf")),
        (true, Value::from("09"), Value::from("09")),
        (true, Value::from("+4"), Value::from("+4")),
    ];
    // (object, type, properties cannot be numbers, is the method object)
    let test_objects: Vec<(Value, &'static str, bool, bool)> = vec![
        (Value::Array(array.clone()), "array", false, false),
        (object_array, "array", false, false),
        (array_object, "any", false, false),
        (std_object, "any", true, false),
        (magic_property_object, "any", false, false),
        (method_object.clone(), "method", false, true),
        (method_object.clone(), "any", false, true),
        (property_object, "any", true, false),
        (property_object1, "any", true, false),
        (property_object2, "any", true, false),
    ];
    let is_numeric = |item: &Value| match item {
        Value::Int(_) | Value::Float(_) => true,
        Value::String(s) => matches!(s.as_bytes(), b"09" | b"+4"),
        _ => false,
    };

    let mut tests = Vec::new();
    for (object, call_type, no_numbers, is_method_object) in &test_objects {
        for (defined, value, item) in &basic_tests {
            // properties cannot be numbers
            if *no_numbers && is_numeric(item) {
                continue;
            }
            if item.as_str().map(|s| s.as_bytes() == b"+4") == Some(true) && *is_method_object {
                continue;
            }
            tests.push(AttributeCase {
                defined: *defined,
                value: value.clone(),
                object: object.clone(),
                item: item.clone(),
                call_type,
                message: None,
            });
        }
    }

    let case = |defined, value: Value, object: &Value, item: &str, call_type| AttributeCase {
        defined,
        value,
        object: object.clone(),
        item: Value::from(item),
        call_type,
        message: None,
    };
    // additional properties tests
    tests.push(case(true, Value::Null, &property_object3, "foo", "any"));
    // additional method tests
    tests.push(case(
        true,
        Value::from("defined"),
        &method_object,
        "defined",
        "method",
    ));
    tests.push(case(
        true,
        Value::from("defined"),
        &method_object,
        "DEFINED",
        "method",
    ));
    tests.push(case(
        true,
        Value::from("defined"),
        &method_object,
        "getDefined",
        "method",
    ));
    tests.push(case(
        true,
        Value::from("defined"),
        &method_object,
        "GETDEFINED",
        "method",
    ));
    tests.push(case(
        true,
        Value::from("static"),
        &method_object,
        "static",
        "method",
    ));
    tests.push(case(
        true,
        Value::from("static"),
        &method_object,
        "getStatic",
        "method",
    ));
    tests.push(case(
        true,
        Value::from("__call_undefined"),
        &magic_method_object,
        "undefined",
        "method",
    ));
    tests.push(case(
        true,
        Value::from("__call_UNDEFINED"),
        &magic_method_object,
        "UNDEFINED",
        "method",
    ));

    // add the same tests for the any type
    let count = tests.len();
    for i in 0..count {
        if tests[i].call_type != "any" {
            let test = &tests[i];
            let copy = AttributeCase {
                defined: test.defined,
                value: test.value.clone(),
                object: test.object.clone(),
                item: test.item.clone(),
                call_type: "any",
                message: test.message,
            };
            tests.push(copy);
        }
    }

    let method_and_prop_object = template_method_and_prop_object();
    // additional method tests
    tests.push(case(
        true,
        Value::from("a"),
        &method_and_prop_object,
        "a",
        "any",
    ));
    tests.push(case(
        true,
        Value::from("a"),
        &method_and_prop_object,
        "a",
        "method",
    ));
    tests.push(case(
        false,
        Value::Null,
        &method_and_prop_object,
        "a",
        "array",
    ));
    tests.push(case(
        true,
        Value::from("b_prop"),
        &method_and_prop_object,
        "b",
        "any",
    ));
    tests.push(case(
        true,
        Value::from("b"),
        &method_and_prop_object,
        "B",
        "any",
    ));
    tests.push(case(
        true,
        Value::from("b"),
        &method_and_prop_object,
        "b",
        "method",
    ));
    tests.push(case(
        true,
        Value::from("b"),
        &method_and_prop_object,
        "B",
        "method",
    ));
    tests.push(case(
        false,
        Value::Null,
        &method_and_prop_object,
        "b",
        "array",
    ));
    tests.push(case(
        false,
        Value::Null,
        &method_and_prop_object,
        "c",
        "any",
    ));
    tests.push(case(
        false,
        Value::Null,
        &method_and_prop_object,
        "c",
        "method",
    ));
    tests.push(case(
        false,
        Value::Null,
        &method_and_prop_object,
        "c",
        "array",
    ));

    let mut vars = Array::new();
    vars.insert("foo", "bar");
    tests.push(case(
        true,
        Value::Array(vars),
        &template_array_access(),
        "vars",
        "any",
    ));

    // test for Closure::__invoke()
    let closure = closure("closure called");
    tests.push(case(
        true,
        Value::from("closure called"),
        &closure,
        "__invoke",
        "any",
    ));
    tests.push(case(
        true,
        Value::from("closure called"),
        &closure,
        "__invoke",
        "method",
    ));

    // tests when input is not an array or object
    let mut not_object = |object: Value, message| {
        tests.push(AttributeCase {
            defined: false,
            value: Value::Null,
            object,
            item: Value::from("a"),
            call_type: "any",
            message: Some(message),
        })
    };
    not_object(
        Value::Int(42),
        "Impossible to access an attribute (\"a\") on a int variable (\"42\") in \"index.twig\".",
    );
    not_object(
        Value::from("string"),
        "Impossible to access an attribute (\"a\") on a string variable (\"string\") in \"index.twig\".",
    );
    not_object(
        Value::Array(Array::new()),
        "Key \"a\" does not exist as the sequence/mapping is empty in \"index.twig\".",
    );
    tests
}

/// The outcome of one template run: `var_export()` of the captured value, or the error as `Class: message`.
type Outcome = Result<String, String>;

/// Runs the matrix templates; environments are shared by the data sets (the templates only depend on the type).
struct Matrix {
    envs: Vec<((&'static str, bool, bool), Environment, Captured)>,
}

impl Matrix {
    fn new() -> Self {
        let mut envs = Vec::new();
        for call_type in ["any", "method", "array"] {
            for defined in [false, true] {
                for strict_mode in [false, true] {
                    let expression = match call_type {
                        "any" => "o.(i)",
                        "method" => "o.(i)()",
                        _ => "o[i]",
                    };
                    let template = if defined {
                        format!("{{{{ capture({expression} is defined) }}}}")
                    } else {
                        format!("{{{{ capture({expression}) }}}}")
                    };
                    let env = env_with(&[("index.twig", template.as_str())], strict(strict_mode));
                    let captured = add_capture(&env);
                    envs.push(((call_type, defined, strict_mode), env, captured));
                }
            }
        }
        Matrix { envs }
    }

    fn run(&self, case: &AttributeCase, defined: bool, strict: bool) -> Outcome {
        let (_, env, captured) = self
            .envs
            .iter()
            .find(|(key, _, _)| *key == (case.call_type, defined, strict))
            .expect("an environment");
        captured.take();
        match env.render(
            "index.twig",
            ctx(&[("o", case.object.clone()), ("i", case.item.clone())]),
        ) {
            Ok(_) => Ok(var_export(&captured.one())),
            Err(error) => Err(describe(&error)),
        }
    }
}

fn label(index: usize, case: &AttributeCase) -> String {
    let class = match &case.object {
        Value::Object(object) => object.class_name().to_owned(),
        other => other.type_tag().name().to_owned(),
    };
    format!(
        "#{index} {class} {} {}",
        var_export(&case.item),
        case.call_type
    )
}

/// The oracle results of the template path (`get_attribute.json`), checked against the Rust data provider.
fn oracle_rows() -> Vec<serde_json::Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/upstream/runtime/get_attribute.json"
    );
    let text = std::fs::read_to_string(path).expect("get_attribute.json");
    let rows: Vec<serde_json::Value> = serde_json::from_str(&text).expect("valid JSON");
    let cases = get_get_attribute_tests();
    assert_eq!(
        rows.len(),
        cases.len(),
        "the data provider has another size"
    );
    for (index, (row, case)) in rows.iter().zip(&cases).enumerate() {
        assert_eq!(
            (
                row["item"].as_str().unwrap(),
                row["type"].as_str().unwrap(),
                row["value"].as_str().unwrap(),
                row["defined"].as_bool().unwrap()
            ),
            (
                var_export(&case.item).as_str(),
                case.call_type,
                var_export(&case.value).as_str(),
                case.defined
            ),
            "data set #{index} differs from the oracle's"
        );
    }
    rows
}

fn oracle_outcome(row: &serde_json::Value, mode: &str) -> Outcome {
    let result = &row[mode];
    match result.get("value") {
        Some(value) => Ok(value.as_str().unwrap().to_owned()),
        None => Err(format!(
            "{}: {}",
            result["error"][0].as_str().unwrap(),
            result["error"][1].as_str().unwrap()
        )),
    }
}

#[test]
fn test_get_attribute() {
    let matrix = Matrix::new();
    let mut failures = Failures::default();
    for (index, case) in get_get_attribute_tests().iter().enumerate() {
        failures.check(
            &label(index, case),
            matrix.run(case, false, false),
            Ok(var_export(&case.value)),
        );
    }
    failures.assert_none();
}

#[test]
fn test_get_attribute_strict() {
    let matrix = Matrix::new();
    let rows = oracle_rows();
    let mut failures = Failures::default();
    for (index, case) in get_get_attribute_tests().iter().enumerate() {
        let outcome = matrix.run(case, false, true);
        let label = label(index, case);
        if case.defined {
            failures.check(&label, outcome, Ok(var_export(&case.value)));
        } else {
            // a RuntimeError; the PHPUnit test checks the message of the last data sets (without the template line
            // the template path adds), the oracle gives every message
            match &outcome {
                Err(error) if error.starts_with("Twig\\Error\\RuntimeError: ") => {
                    if let Some(message) = case.message {
                        let message = message.trim_end_matches('.');
                        if !error.contains(message) {
                            failures.fail(format!("{label}: {error} does not contain {message}"));
                        }
                    }
                }
                _ => failures.fail(format!("{label}: expected a RuntimeError, got {outcome:?}")),
            }
            failures.check(&label, outcome, oracle_outcome(&rows[index], "strict"));
        }
    }
    failures.assert_none();
}

#[test]
fn test_get_attribute_defined() {
    let matrix = Matrix::new();
    let mut failures = Failures::default();
    for (index, case) in get_get_attribute_tests().iter().enumerate() {
        failures.check(
            &label(index, case),
            matrix.run(case, true, false),
            Ok(var_export(&Value::Bool(case.defined))),
        );
    }
    failures.assert_none();
}

#[test]
fn test_get_attribute_defined_strict() {
    let matrix = Matrix::new();
    let mut failures = Failures::default();
    for (index, case) in get_get_attribute_tests().iter().enumerate() {
        failures.check(
            &label(index, case),
            matrix.run(case, true, true),
            Ok(var_export(&Value::Bool(case.defined))),
        );
    }
    failures.assert_none();
}

#[test]
fn get_attribute_matrix_matches_the_oracle() {
    // every mode of every data set against the oracle's template runs (lax and strict, value and defined test)
    let matrix = Matrix::new();
    let rows = oracle_rows();
    let mut failures = Failures::default();
    for (index, case) in get_get_attribute_tests().iter().enumerate() {
        let label = label(index, case);
        for (mode, defined, strict) in [
            ("lax", false, false),
            ("strict", false, true),
            ("defined_lax", true, false),
            ("defined_strict", true, true),
        ] {
            failures.check(
                &format!("{label} {mode}"),
                matrix.run(case, defined, strict),
                oracle_outcome(&rows[index], mode),
            );
        }
    }
    failures.assert_none();
}

// ---- remaining getAttribute() tests ------------------------------------------------------------------------------

#[test]
fn test_get_attribute_call_exceptions() {
    // `__call()` throws a BadMethodCallException: null in lax mode
    let env = env(&[("index.twig", "{{ capture(object.foo) }}")]);
    let captured = add_capture(&env);
    env.render(
        "index.twig",
        ctx(&[("object", template_magic_method_exception_object())]),
    )
    .unwrap();
    assert!(captured.one().is_null());
}

#[test]
fn test_printing_a_non_string_reports_the_error_at_the_print_location() {
    // `set_error_handler()` turning E_WARNING into ErrorException: the Error diagnostics policy
    let values = [
        (
            "array",
            Value::Array(Array::from_values([Value::from("a"), Value::from("b")])),
            "Array to string conversion",
        ),
        (
            "non-Stringable object",
            std_class(),
            "Object of class stdClass could not be converted to string",
        ),
    ];
    let mut failures = Failures::default();
    for (name, value, expected) in values {
        let env = env(&[("index", "foo\n{{ value }}\nbar")]);
        env.set_diagnostics_policy(twig::runtime::DiagnosticsPolicy::Error);
        match env.render("index", ctx(&[("value", value)])) {
            Ok(output) => failures.fail(format!("{name}: rendered {output:?}")),
            Err(error) => failures.check(
                name,
                (
                    error.kind(),
                    error.source_context().map(|s| s.name().to_owned()),
                    error.template_line(),
                    previous(&error)
                        .map(|p| p.split_once(": ").map(|(_, m)| m.to_owned()).unwrap_or(p)),
                ),
                (
                    ErrorKind::Runtime,
                    Some("index".to_owned()),
                    2,
                    Some(expected.to_owned()),
                ),
            ),
        }
    }
    failures.assert_none();
}

#[test]
fn test_get_is_methods() {
    let env = env(&[(
        "index.twig",
        "{{ capture(object.get) }}{{ capture(attribute(object, 0)) }}",
    )]);
    let captured = add_capture(&env);
    env.render("index.twig", ctx(&[("object", template_get_is_methods())]))
        .unwrap();
    // first time should not create a cache for "get"; 0 should be in the method cache now, so this should fail
    let values = captured.take();
    assert_eq!(values.len(), 2);
    assert!(values.iter().all(Value::is_null), "{values:?}");
}

#[test]
fn test_repeated_object_attribute_lookups_resolve_consistently() {
    let env = env_with(
        &[(
            "index.twig",
            "{{ capture(o.NAME) }}{{ capture(o.NAME is defined) }}{{ capture(o.hidden) }}{{ capture(o.hidden is defined) }}{{ capture(o.secret is defined) }}",
        )],
        strict(true),
    );
    let captured = add_capture(&env);
    let object = template_attribute_resolution();
    for _ in 0..2 {
        env.render("index.twig", ctx(&[("o", object.clone())]))
            .unwrap();
        let values: Vec<String> = captured.take().iter().map(var_export).collect();
        assert_eq!(values, ["'constant'", "true", "'getter'", "true", "false"]);
    }
}

#[test]
fn test_get_attribute_on_private_property_still_uses_magic_isset() {
    let env = env_with(&[("index.twig", "{{ capture(o.title) }}")], strict(true));
    let captured = add_capture(&env);
    env.render(
        "index.twig",
        ctx(&[("o", template_magic_private_property_object())]),
    )
    .unwrap();
    assert_eq!(var_export(&captured.one()), "'magic'");
}
