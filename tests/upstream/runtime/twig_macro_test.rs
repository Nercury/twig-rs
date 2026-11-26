//! Port of TWIG/tests/TwigMacroTest.php.
//!
//! The PHPUnit test builds `TwigMacro` objects by hand and calls `callLegacy($arguments, new Source('',
//! 'index.twig'), 7)`; the ports do the same with [`TwigMacro::new`] and [`TwigMacro::call_legacy`], the body
//! closures capturing what they get (`$captured = [$name, ..., $varargs]`).
//!
//! The same data sets also run through templates (`compiled_macro*`): a macro declared in a template named
//! `index.twig` and called on line 7, where `MacroNode` compiles the declaration into exactly the `TwigMacro` of the
//! data set (every argument a defaulted closure parameter, reserved names prefixed, the variadic bucket last), and
//! `_self.test(...)` hands the arguments to `callLegacy()` with the template's source and the call line; the macro
//! body passes its arguments to `capture()`. VERIFIED with the oracle (same templates).

use std::sync::{Arc, Mutex};

use twig::runtime::TwigMacro;
use twig::value::{Array, Value};
use twig::{ErrorKind, Source};

use crate::support::*;

/// `new Source('', 'index.twig')`.
fn index_twig() -> Source {
    Source::new("", "index.twig")
}

/// `new TwigMacro('input', static fn () => '', ['name' => false])`.
fn input() -> TwigMacro {
    TwigMacro::new("input", |_, _| Ok(Value::from("")), [("name", None)], false)
}

/// `['name' => 'a', 0 => 'b']` and the like: positional arguments under their position, named ones under their name.
fn arguments(pairs: &[(Option<&str>, Value)]) -> Array {
    let mut array = Array::new();
    for (name, value) in pairs {
        match name {
            Some(name) => {
                array.insert(*name, value.clone());
            }
            None => {
                array.push(value.clone()).unwrap();
            }
        }
    }
    array
}

#[test]
fn test_legacy_call_enriches_errors_with_the_call_site_source_and_line() {
    let error = expect_error(
        input().call_legacy(
            arguments(&[(Some("name"), s("a")), (None, s("b"))]),
            &index_twig(),
            7,
            &mut Vec::new(),
        ),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Positional arguments cannot be used after named arguments for macro \"input\" in \"index.twig\" at line 7."
    );
}

#[test]
fn test_legacy_call_does_not_report_deprecations_for_a_call_that_throws() {
    let mut deprecations = Vec::new();
    let error = expect_error(
        input().call_legacy(
            arguments(&[(Some("unknown"), s("a")), (None, s("b"))]),
            &index_twig(),
            7,
            &mut deprecations,
        ),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Positional arguments cannot be used after named arguments for macro \"input\" in \"index.twig\" at line 7."
    );
    assert!(deprecations.is_empty(), "{deprecations:?}");
}

#[test]
fn test_legacy_call_rejects_an_argument_defined_twice() {
    let error = expect_error(
        input().call_legacy(
            arguments(&[(None, s("a")), (Some("name"), s("b"))]),
            &index_twig(),
            7,
            &mut Vec::new(),
        ),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Argument \"name\" is defined twice for macro \"input\" in \"index.twig\" at line 7."
    );
}

/// `index.twig` declaring `definition` with `body`, calling `call` on line 7.
fn macro_template(definition: &str, body: &str, call: &str) -> String {
    format!("{{% macro {definition} %}}{body}{{% endmacro %}}\n\n\n\n\n\n{{{{ _self.{call} }}}}")
}

#[test]
fn compiled_macro_enriches_errors_with_the_call_site_source_and_line() {
    let template = macro_template("input(name)", "", "input(name='a', 'b')");
    let twig = env(&[("index.twig", template.as_str())]);
    let error = expect_error(twig.render("index.twig", Array::new()), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "Positional arguments cannot be used after named arguments for macro \"input\" in \"index.twig\" at line 7."
    );
}

#[test]
fn compiled_macro_does_not_report_deprecations_for_a_call_that_throws() {
    let template = macro_template("input(name)", "", "input(unknown='a', 'b')");
    let twig = env(&[("index.twig", template.as_str())]);
    let (result, deprecations) =
        collect_deprecations(&twig, || twig.render("index.twig", Array::new()));
    let error = expect_error(result, ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "Positional arguments cannot be used after named arguments for macro \"input\" in \"index.twig\" at line 7."
    );
    assert!(deprecations.is_empty(), "{deprecations:?}");
}

#[test]
fn compiled_macro_rejects_an_argument_defined_twice() {
    let template = macro_template("input(name)", "", "input('a', name='b')");
    let twig = env(&[("index.twig", template.as_str())]);
    let error = expect_error(twig.render("index.twig", Array::new()), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "Argument \"name\" is defined twice for macro \"input\" in \"index.twig\" at line 7."
    );
}

fn list(values: Vec<Value>) -> Value {
    Value::Array(Array::from_values(values))
}

fn map(pairs: &[(&str, Value)]) -> Value {
    Value::Array(ctx(pairs))
}

fn s(value: &str) -> Value {
    Value::from(value)
}

/// One data set of `provideLegacyCalls()`: the signature (each argument with the default of its closure parameter,
/// `None` for an argument without a default), whether the macro is variadic, the arguments, the expected captured
/// arguments and deprecations; and the same macro as a template: its definition, the captured arguments and the call.
struct LegacyCall {
    name: &'static str,
    signature: Vec<(&'static str, Option<Value>)>,
    variadic: bool,
    arguments: Array,
    definition: &'static str,
    captured: &'static str,
    call: &'static str,
    expected: Value,
    deprecations: Vec<&'static str>,
}

const NOT_PASSING_NAME: &str = "Since twig/twig 3.29: Not passing a value for the \"name\" argument of macro \"test\" is deprecated and the argument will be required in Twig 4.0; give it a default value in the macro definition or pass a value when calling it (in \"index.twig\" at line 7).";
const NOT_PASSING_VALUE: &str = "Since twig/twig 3.29: Not passing a value for the \"value\" argument of macro \"test\" is deprecated and the argument will be required in Twig 4.0; give it a default value in the macro definition or pass a value when calling it (in \"index.twig\" at line 7).";
const MORE_ARGUMENTS: &str = "Since twig/twig 3.29: Passing more arguments than the macro \"test\" accepts is deprecated and will throw in Twig 4.0; declare a variadic argument (\"...name\") in the macro definition to accept extra arguments (in \"index.twig\" at line 7).";
const UNKNOWN_EXTRA: &str = "Since twig/twig 3.29: Passing the unknown named argument \"extra\" to the macro \"test\" is deprecated and will throw in Twig 4.0; declare a variadic argument (\"...name\") in the macro definition to accept it (in \"index.twig\" at line 7).";

/// Twig: `TwigMacroTest::provideLegacyCalls()`.
fn provide_legacy_calls() -> Vec<LegacyCall> {
    // $nameOnly: function ($name = null, ...$varargs); $nameValueType: function ($name = null, $value = 'v',
    // $type = 't', ...$varargs)
    let name_only = || vec![("name", None)];
    let name_value_type = || {
        vec![
            ("name", None),
            ("value", Some(s("v"))),
            ("type", Some(s("t"))),
        ]
    };
    vec![
        LegacyCall {
            name: "missing argument without a default is lenient and deprecated",
            signature: vec![("name", None), ("value", Some(Value::Null))],
            variadic: false,
            arguments: Array::new(),
            definition: "test(name, value = null)",
            captured: "[name, value, varargs]",
            call: "test()",
            expected: list(vec![Value::Null, Value::Null, list(vec![])]),
            deprecations: vec![NOT_PASSING_NAME],
        },
        LegacyCall {
            name: "extra positional argument is lenient and deprecated",
            signature: name_only(),
            variadic: false,
            arguments: arguments(&[(None, s("a")), (None, s("b"))]),
            definition: "test(name)",
            captured: "[name, varargs]",
            call: "test('a', 'b')",
            expected: list(vec![s("a"), list(vec![s("b")])]),
            deprecations: vec![MORE_ARGUMENTS],
        },
        LegacyCall {
            name: "unknown named argument is lenient and deprecated",
            signature: name_only(),
            variadic: false,
            arguments: arguments(&[(Some("name"), s("a")), (Some("extra"), s("b"))]),
            definition: "test(name)",
            captured: "[name, varargs]",
            call: "test(name='a', extra='b')",
            expected: list(vec![s("a"), map(&[("extra", s("b"))])]),
            deprecations: vec![UNKNOWN_EXTRA],
        },
        LegacyCall {
            name: "unknown named argument is still reported when the argument count matches the signature",
            signature: vec![("name", None), ("value", None)],
            variadic: false,
            arguments: arguments(&[(Some("name"), s("a")), (Some("extra"), s("b"))]),
            definition: "test(name, value)",
            captured: "[name, value, varargs]",
            call: "test(name='a', extra='b')",
            expected: list(vec![s("a"), Value::Null, map(&[("extra", s("b"))])]),
            deprecations: vec![NOT_PASSING_VALUE, UNKNOWN_EXTRA],
        },
        LegacyCall {
            name: "named arguments covering the whole signature bind to their parameter, reserved names included",
            signature: vec![("context", None), ("value", None)],
            variadic: false,
            arguments: arguments(&[(Some("value"), s("b")), (Some("context"), s("a"))]),
            definition: "test(context, value)",
            captured: "[context, value, varargs]",
            call: "test(value='b', context='a')",
            expected: list(vec![s("a"), s("b"), list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "null named argument value satisfies a required argument",
            signature: name_only(),
            variadic: false,
            arguments: arguments(&[(Some("name"), Value::Null)]),
            definition: "test(name)",
            captured: "[name, varargs]",
            call: "test(name=null)",
            expected: list(vec![Value::Null, list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "a reserved argument name maps onto its prefixed parameter",
            signature: vec![("name", Some(Value::Null)), ("blocks", Some(s("default")))],
            variadic: false,
            arguments: arguments(&[(Some("blocks"), s("value"))]),
            definition: "test(name = null, blocks = 'default')",
            captured: "[name, blocks, varargs]",
            call: "test(blocks='value')",
            expected: list(vec![Value::Null, s("value"), list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "skipped optional arguments before a named argument get their defaults",
            signature: name_value_type(),
            variadic: false,
            arguments: arguments(&[(Some("name"), s("a")), (Some("type"), s("x"))]),
            definition: "test(name, value = 'v', type = 't')",
            captured: "[name, value, type, varargs]",
            call: "test(name='a', type='x')",
            expected: list(vec![s("a"), s("v"), s("x"), list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "positional and named arguments mix under the legacy call path",
            signature: name_value_type(),
            variadic: false,
            arguments: arguments(&[(None, s("a")), (Some("type"), s("x"))]),
            definition: "test(name, value = 'v', type = 't')",
            captured: "[name, value, type, varargs]",
            call: "test('a', type='x')",
            expected: list(vec![s("a"), s("v"), s("x"), list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "known named arguments bind to their parameter and unknown ones fall into the variadic bucket",
            signature: name_value_type(),
            variadic: true,
            arguments: arguments(&[
                (None, s("a")),
                (Some("type"), s("submit")),
                (Some("extra"), s("value")),
            ]),
            definition: "test(name, value = 'v', type = 't', ...varargs)",
            captured: "[name, value, type, varargs]",
            call: "test('a', type='submit', extra='value')",
            expected: list(vec![
                s("a"),
                s("v"),
                s("submit"),
                map(&[("extra", s("value"))]),
            ]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "a variadic macro reports nothing",
            signature: name_only(),
            variadic: true,
            arguments: arguments(&[(None, s("a")), (None, s("b")), (Some("extra"), s("c"))]),
            definition: "test(name, ...varargs)",
            captured: "[name, varargs]",
            call: "test('a', 'b', extra='c')",
            expected: {
                let mut bucket = Array::new();
                bucket.push(s("b")).unwrap();
                bucket.insert("extra", s("c"));
                list(vec![s("a"), Value::Array(bucket)])
            },
            deprecations: vec![],
        },
    ]
}

#[test]
fn test_legacy_call_is_lenient_but_reports_future_errors() {
    let mut failures = Failures::default();
    for case in provide_legacy_calls() {
        let captured: Arc<Mutex<Option<Value>>> = Arc::default();
        let slot = Arc::clone(&captured);
        let body = move |mut values: Vec<Value>, bucket: Array| {
            values.push(Value::Array(bucket));
            *slot.lock().unwrap() = Some(list(values));
            Ok(Value::from(""))
        };
        let test = TwigMacro::new("test", body, case.signature, case.variadic);
        let mut deprecations = Vec::new();
        if let Err(error) = test.call_legacy(case.arguments, &index_twig(), 7, &mut deprecations) {
            failures.fail(format!("{}: {}", case.name, describe(&error)));
            continue;
        }
        let captured = captured.lock().unwrap().take().unwrap_or_default();
        failures.check(
            case.name,
            (
                var_export(&captured),
                deprecations
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            ),
            (
                var_export(&case.expected),
                case.deprecations.iter().map(|d| d.to_string()).collect(),
            ),
        );
    }
    failures.assert_none();
}

#[test]
fn compiled_macros_are_lenient_but_report_future_errors() {
    let mut failures = Failures::default();
    for case in provide_legacy_calls() {
        let template = macro_template(
            case.definition,
            &format!("{{{{ capture({}) }}}}", case.captured),
            case.call,
        );
        let twig = env(&[("index.twig", template.as_str())]);
        let captured = add_capture(&twig);
        let (result, deprecations) =
            collect_deprecations(&twig, || twig.render("index.twig", Array::new()));
        if let Err(error) = result {
            failures.fail(format!("{}: {}", case.name, describe(&error)));
            continue;
        }
        let values = captured.take();
        failures.check(
            case.name,
            (
                values.iter().map(var_export).collect::<Vec<_>>(),
                deprecations,
            ),
            (
                vec![var_export(&case.expected)],
                case.deprecations.iter().map(|d| d.to_string()).collect(),
            ),
        );
    }
    failures.assert_none();
}
