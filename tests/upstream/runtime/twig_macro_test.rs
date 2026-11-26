//! Port of TWIG/tests/TwigMacroTest.php.
//!
//! The PHPUnit test builds `TwigMacro` objects by hand and calls `callLegacy($arguments, new Source('',
//! 'index.twig'), 7)`. `TwigMacro` is internal to twig-rs, so the ports declare the macro in a template named
//! `index.twig` and call it on line 7: `MacroNode` compiles the declaration into exactly the `TwigMacro` of the data
//! set (every argument a defaulted closure parameter, reserved names prefixed, the variadic bucket last), and
//! `_self.test(...)` hands the arguments to `callLegacy()` with the template's source and the call line. The macro body
//! passes its arguments to `capture()`, which stands for the `$captured` of the PHPUnit closures. VERIFIED with the
//! oracle (same templates).

use twig::ErrorKind;
use twig::value::{Array, Value};

use crate::support::*;

/// `index.twig` declaring `definition` with `body`, calling `call` on line 7.
fn macro_template(definition: &str, body: &str, call: &str) -> String {
    format!("{{% macro {definition} %}}{body}{{% endmacro %}}\n\n\n\n\n\n{{{{ _self.{call} }}}}")
}

#[test]
fn test_legacy_call_enriches_errors_with_the_call_site_source_and_line() {
    let template = macro_template("input(name)", "", "input(name='a', 'b')");
    let twig = env(&[("index.twig", template.as_str())]);
    let error = expect_error(twig.render("index.twig", Array::new()), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "Positional arguments cannot be used after named arguments for macro \"input\" in \"index.twig\" at line 7."
    );
}

#[test]
fn test_legacy_call_does_not_report_deprecations_for_a_call_that_throws() {
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
fn test_legacy_call_rejects_an_argument_defined_twice() {
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

/// One data set of `provideLegacyCalls()`: the macro definition (the signature and the body closure of the PHPUnit
/// test), the captured arguments, the call, the expected captured arguments and deprecations.
struct LegacyCall {
    name: &'static str,
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
    vec![
        LegacyCall {
            name: "missing argument without a default is lenient and deprecated",
            definition: "test(name, value = null)",
            captured: "[name, value, varargs]",
            call: "test()",
            expected: list(vec![Value::Null, Value::Null, list(vec![])]),
            deprecations: vec![NOT_PASSING_NAME],
        },
        LegacyCall {
            name: "extra positional argument is lenient and deprecated",
            definition: "test(name)",
            captured: "[name, varargs]",
            call: "test('a', 'b')",
            expected: list(vec![s("a"), list(vec![s("b")])]),
            deprecations: vec![MORE_ARGUMENTS],
        },
        LegacyCall {
            name: "unknown named argument is lenient and deprecated",
            definition: "test(name)",
            captured: "[name, varargs]",
            call: "test(name='a', extra='b')",
            expected: list(vec![s("a"), map(&[("extra", s("b"))])]),
            deprecations: vec![UNKNOWN_EXTRA],
        },
        LegacyCall {
            name: "unknown named argument is still reported when the argument count matches the signature",
            definition: "test(name, value)",
            captured: "[name, value, varargs]",
            call: "test(name='a', extra='b')",
            expected: list(vec![s("a"), Value::Null, map(&[("extra", s("b"))])]),
            deprecations: vec![NOT_PASSING_VALUE, UNKNOWN_EXTRA],
        },
        LegacyCall {
            name: "named arguments covering the whole signature bind to their parameter, reserved names included",
            definition: "test(context, value)",
            captured: "[context, value, varargs]",
            call: "test(value='b', context='a')",
            expected: list(vec![s("a"), s("b"), list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "null named argument value satisfies a required argument",
            definition: "test(name)",
            captured: "[name, varargs]",
            call: "test(name=null)",
            expected: list(vec![Value::Null, list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "a reserved argument name maps onto its prefixed parameter",
            definition: "test(name = null, blocks = 'default')",
            captured: "[name, blocks, varargs]",
            call: "test(blocks='value')",
            expected: list(vec![Value::Null, s("value"), list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "skipped optional arguments before a named argument get their defaults",
            definition: "test(name, value = 'v', type = 't')",
            captured: "[name, value, type, varargs]",
            call: "test(name='a', type='x')",
            expected: list(vec![s("a"), s("v"), s("x"), list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "positional and named arguments mix under the legacy call path",
            definition: "test(name, value = 'v', type = 't')",
            captured: "[name, value, type, varargs]",
            call: "test('a', type='x')",
            expected: list(vec![s("a"), s("v"), s("x"), list(vec![])]),
            deprecations: vec![],
        },
        LegacyCall {
            name: "known named arguments bind to their parameter and unknown ones fall into the variadic bucket",
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
