//! Port of `TWIG/tests/Extension/CoreTest.php`.
//!
//! The upstream tests call the static helpers of `CoreExtension` (`cycle()`, `random()`, `first()`, ...) directly.
//! twig-rs does not expose these helpers; the ports call the filters, functions and operators that templates compile
//! to those helpers and hand the result back with a `capture()` function, so `assertSame()` still compares the PHP
//! values (`CoreExtension::compare()` is public: `twig::runtime::compare_values`).
//!
//! Where the upstream test only checks an exception class, the ports also compare the message and the deprecations
//! raised, as the oracle reported them (`tests/vectors/upstream/extensions/messages.gen.json`, results in
//! `messages.json` next to it: `./oracle.sh --pretty < messages.gen.json > messages.json`).
//!
//! Not ported: `testLastModified` (`getLastModified()` returns the mtime of the PHP extension class file; twig-rs
//! has no such API).

use std::sync::Arc;

use twig::ErrorKind;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, Markup, SandboxExtension, SecurityPolicy,
    compare_values,
};
use twig::value::object::{ArrayObject, StdClass};
use twig::value::{Array, Ignore, Value};
use twig_test_support::classes::simplexml::{SimpleXmlElement, parse_document};

use super::classes::{CoreTestAggregate, CoreTestIterator, NonCountableSequence, array_iterator};
use super::support::{
    Capture, array, check_cases, ctx, deprecations, env_with, expect_deprecation, ints, key, list,
    options, same, strings,
};

/// Evaluates `expr` in a template with `context` and returns its value (`{% do capture(expr) %}`).
fn eval_with(expr: &str, context: Array, options: EnvironmentOptions) -> Result<Value, String> {
    let template = format!("{{% do capture({expr}) %}}");
    let env = env_with(&[("index", &template)], options);
    let capture = Capture::new();
    capture.register(&env);
    env.render("index", context)
        .map_err(|e| format!("{}: {e}", e.class_name()))?;
    let mut values = capture.take();
    assert_eq!(values.len(), 1, "one captured value");
    Ok(values.remove(0))
}

fn eval(expr: &str, context: Array) -> Result<Value, String> {
    eval_with(expr, context, options())
}

/// Evaluates `expr` `times` times in one render (`{% for %}` around `capture()`).
fn eval_times(expr: &str, times: usize, context: Array, options: EnvironmentOptions) -> Vec<Value> {
    let template = format!("{{% for i in 1..{times} %}}{{% do capture({expr}) %}}{{% endfor %}}");
    let env = env_with(&[("index", &template)], options);
    let capture = Capture::new();
    capture.register(&env);
    env.render("index", context).expect("renders");
    capture.take()
}

/// `\SimpleXMLElement($xml)`.
fn simple_xml(xml: &str) -> Value {
    Value::Object(SimpleXmlElement::root(Arc::new(parse_document(xml).expect("XML"))).into_ref())
}

// ---- cycle -----------------------------------------------------------------------------------------------------

fn provide_cycle_cases() -> Vec<(String, (Value, i64, Value))> {
    let one_two_three = ints([1, 2, 3]);
    let falsy = list([Value::Bool(false), Value::Int(0), Value::Null]);
    vec![
        (one_two_three.clone(), 0, Value::Int(1)),
        (one_two_three.clone(), 1, Value::Int(2)),
        (one_two_three.clone(), 2, Value::Int(3)),
        (one_two_three, 3, Value::Int(1)),
        (falsy.clone(), 0, Value::Bool(false)),
        (falsy.clone(), 1, Value::Int(0)),
        (falsy, 2, Value::Null),
        (
            list([strings(&["a", "b"]), strings(&["c", "d"])]),
            3,
            strings(&["c", "d"]),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, case)| (format!("#{i}"), case))
    .collect()
}

#[test]
fn test_cycle_function() {
    check_cases(
        "testCycleFunction",
        provide_cycle_cases(),
        |(values, position, expected)| {
            let actual = eval(
                "cycle(values, position)",
                ctx(&[
                    ("values", values.clone()),
                    ("position", Value::Int(*position)),
                ]),
            )?;
            same(expected, &actual)
        },
    );
}

#[test]
fn test_cycle_function_throw_runtime_error() {
    // 'non-countable' is `new class extends \ArrayObject {}` (countable, but empty): a plain empty ArrayObject behaves
    // the same for `cycle()`.
    let cases = vec![
        ("empty".to_owned(), Value::Array(Array::new())),
        (
            "non-countable".to_owned(),
            Value::Object(ArrayObject::new(Array::new()).into_ref()),
        ),
    ];
    check_cases("testCycleFunctionThrowRuntimeError", cases, |values| {
        let env = env_with(&[("index", "{{ cycle(values, 0) }}")], options());
        // the message as the oracle printed it (the upstream test only checks the class)
        expect_runtime_error(
            env.render("index", ctx(&[("values", values.clone())])),
            "The \"cycle\" function expects a non-empty sequence in \"index\" at line 1.",
        )
    });
}

/// A `RuntimeError` with the given message.
fn expect_runtime_error(result: twig::Result<String>, message: &str) -> Result<(), String> {
    match result {
        Err(e) if e.kind() == ErrorKind::Runtime && e.to_string() == message => Ok(()),
        Err(e) => Err(format!(
            "expected a RuntimeError {message:?}, got {}: {e}",
            e.class_name()
        )),
        Ok(output) => Err(format!(
            "expected a RuntimeError, got the output {output:?}"
        )),
    }
}

// ---- random ----------------------------------------------------------------------------------------------------

/// `[$expectedInArray, $value1, $value2]`.
type RandomCase = (Vec<Value>, Value, Value);

fn get_random_function_test_data() -> Vec<(String, RandomCase)> {
    let fruits = ["apple", "orange", "citrus"];
    let range = |from: i64, to: i64| (from..=to).map(Value::Int).collect::<Vec<_>>();
    let fruit_list = fruits.iter().map(|s| Value::from(*s)).collect::<Vec<_>>();
    vec![
        ("array", (fruit_list.clone(), strings(&fruits), Value::Null)),
        (
            "Traversable",
            (
                fruit_list,
                Value::Object(
                    ArrayObject::new(Array::from_values(fruits.iter().map(|s| Value::from(*s))))
                        .into_ref(),
                ),
                Value::Null,
            ),
        ),
        (
            "unicode string",
            (
                vec![Value::from("Ä"), Value::from("€"), Value::from("é")],
                Value::from("Ä€é"),
                Value::Null,
            ),
        ),
        (
            "numeric but string",
            (
                vec![Value::from("1"), Value::from("2"), Value::from("3")],
                Value::from("123"),
                Value::Null,
            ),
        ),
        ("integer", (range(0, 5), Value::Int(5), Value::Null)),
        ("float", (range(0, 5), Value::Float(5.9), Value::Null)),
        (
            "negative",
            (
                vec![Value::Int(0), Value::Int(-1), Value::Int(-2)],
                Value::Int(-2),
                Value::Null,
            ),
        ),
        (
            "min max int",
            (range(50, 100), Value::Int(50), Value::Int(100)),
        ),
        (
            "min max float",
            (range(-10, 10), Value::Float(-9.5), Value::Float(9.5)),
        ),
        ("min null", (range(0, 100), Value::Null, Value::Int(100))),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case))
    .collect()
}

#[test]
fn test_random_function() {
    check_cases(
        "testRandomFunction",
        get_random_function_test_data(),
        |(expected_in_array, value1, value2)| {
            let values = eval_times(
                "random(value1, value2)",
                100,
                ctx(&[("value1", value1.clone()), ("value2", value2.clone())]),
                options(),
            );
            for value in &values {
                // in_array(..., true)
                if !expected_in_array
                    .iter()
                    .any(|e| twig::value::compare::identical(e, value))
                {
                    return Err(format!(
                        "{} is not an expected value",
                        super::support::export(value)
                    ));
                }
            }
            Ok(())
        },
    );
}

#[test]
fn test_random_function_without_parameter() {
    let max = 2_147_483_647; // mt_getrandmax()
    for value in eval_times("random()", 100, Array::new(), options()) {
        assert!(
            matches!(value, Value::Int(v) if (0..=max).contains(&v)),
            "{value:?}"
        );
    }
}

#[test]
fn test_random_function_returns_as_is() {
    assert_eq!(
        same(&Value::from(""), &eval("random('')", Array::new()).unwrap()),
        Ok(())
    );
    let instance = Value::Object(StdClass::new().into_ref());
    let actual = eval("random(instance)", ctx(&[("instance", instance.clone())])).unwrap();
    assert_eq!(same(&instance, &actual), Ok(()));
}

#[test]
fn test_random_function_of_empty_array_throws_exception() {
    let env = env_with(&[("index", "{{ random([]) }}")], options());
    expect_runtime_error(
        env.render("index", Array::new()),
        "The \"random\" function cannot pick from an empty sequence or mapping in \"index\" at line 1.",
    )
    .unwrap();
}

/// The charset of `CoreExtension::random()`/`reverse()` is the environment's (`needs_charset`).
fn iso_8859_1() -> EnvironmentOptions {
    EnvironmentOptions {
        charset: "ISO-8859-1".to_owned(),
        ..options()
    }
}

#[test]
fn test_random_function_on_non_utf8_string() {
    // iconv('UTF-8', 'ISO-8859-1', 'Äé')
    let text = Value::from(&b"\xC4\xE9"[..]);
    for value in eval_times(
        "random(text)",
        30,
        ctx(&[("text", text.clone())]),
        iso_8859_1(),
    ) {
        assert!(
            value == Value::from(&b"\xC4"[..]) || value == Value::from(&b"\xE9"[..]),
            "{value:?}"
        );
    }
}

#[test]
fn test_reverse_filter_on_non_utf8_string() {
    let input = Value::from(&b"\xC4\xE9"[..]);
    let output = eval_with("input|reverse", ctx(&[("input", input)]), iso_8859_1()).unwrap();
    // iconv('ISO-8859-1', 'UTF-8', ...) === 'éÄ'
    assert_eq!(same(&Value::from(&b"\xE9\xC4"[..]), &output), Ok(()));
}

// ---- first, last -----------------------------------------------------------------------------------------------

/// `[1 => 'a', 2 => 'b', 3 => 'c']` and its keys.
fn one_based_abc() -> (Array, Vec<Value>) {
    let mut i = Array::new();
    i.insert(1, "a");
    i.insert(2, "b");
    i.insert(3, "c");
    (i, vec![Value::Int(1), Value::Int(2), Value::Int(3)])
}

fn provide_twig_first_cases() -> Vec<(String, (Value, Value))> {
    let (i, keys) = one_based_abc();
    vec![
        (Value::from("a"), Value::from("abc")),
        (Value::Int(1), ints([1, 2, 3])),
        (Value::from(""), Value::Null),
        (Value::from(""), Value::from("")),
        (
            Value::from("a"),
            CoreTestIterator::value(&i, &keys, true, Some(3)),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

#[test]
fn test_twig_first() {
    check_cases(
        "testTwigFirst",
        provide_twig_first_cases(),
        |(expected, input)| {
            same(
                expected,
                &eval("input|first", ctx(&[("input", input.clone())]))?,
            )
        },
    );
}

fn provide_twig_last_cases() -> Vec<(String, (Value, Value))> {
    let (i, keys) = one_based_abc();
    vec![
        (Value::from("c"), Value::from("abc")),
        (Value::Int(3), ints([1, 2, 3])),
        (Value::from(""), Value::Null),
        (Value::from(""), Value::from("")),
        (
            Value::from("c"),
            CoreTestIterator::value(&i, &keys, true, None),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

#[test]
fn test_twig_last() {
    check_cases(
        "testTwigLast",
        provide_twig_last_cases(),
        |(expected, input)| {
            same(
                expected,
                &eval("input|last", ctx(&[("input", input.clone())]))?,
            )
        },
    );
}

// ---- keys --------------------------------------------------------------------------------------------------------

fn provide_array_key_cases() -> Vec<(String, (Value, Value))> {
    let array = array(vec![
        (key("a"), Value::from("a1")),
        (key("b"), Value::from("b1")),
        (key("c"), Value::from("c1")),
    ]);
    let keys = vec![Value::from("a"), Value::from("b"), Value::from("c")];
    let keys_value = list(keys.clone());
    vec![
        (keys_value.clone(), Value::Array(array.clone())),
        (
            keys_value.clone(),
            CoreTestIterator::value(&array, &keys, false, None),
        ),
        (
            keys_value.clone(),
            Value::Object(CoreTestAggregate::aggregate(&array, &keys, false, None)),
        ),
        (
            keys_value,
            Value::Object(CoreTestAggregate::aggregate_aggregate(
                &array, &keys, false, None,
            )),
        ),
        (list([]), Value::Null),
        (strings(&["a"]), simple_xml("<xml><a></a></xml>")),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

#[test]
fn test_array_keys_filter() {
    check_cases(
        "testArrayKeysFilter",
        provide_array_key_cases(),
        |(expected, input)| {
            same(
                expected,
                &eval("input|keys", ctx(&[("input", input.clone())]))?,
            )
        },
    );
}

// ---- in ----------------------------------------------------------------------------------------------------------

fn provide_in_filter_cases() -> Vec<(String, (bool, Value, Value))> {
    // [1, 2, 'a' => 3, 5, 6, 7]
    let array = array(vec![
        (None, Value::Int(1)),
        (None, Value::Int(2)),
        (key("a"), Value::Int(3)),
        (None, Value::Int(5)),
        (None, Value::Int(6)),
        (None, Value::Int(7)),
    ]);
    let keys: Vec<Value> = vec![
        Value::Int(0),
        Value::Int(1),
        Value::from("a"),
        Value::Int(2),
        Value::Int(3),
        Value::Int(4),
    ];
    let a = Value::Array(array.clone());
    vec![
        (true, Value::Int(1), a.clone()),
        (true, Value::from("3"), a.clone()),
        (true, Value::from("3"), Value::from("abc3def")),
        (
            true,
            Value::Int(1),
            CoreTestIterator::value(&array, &keys, true, Some(1)),
        ),
        (
            true,
            Value::from("3"),
            CoreTestIterator::value(&array, &keys, true, Some(3)),
        ),
        (
            true,
            Value::from("3"),
            Value::Object(CoreTestAggregate::aggregate_aggregate(
                &array,
                &keys,
                true,
                Some(3),
            )),
        ),
        (false, Value::Int(4), a),
        (
            false,
            Value::Int(4),
            CoreTestIterator::value(&array, &keys, true, None),
        ),
        (
            false,
            Value::Int(4),
            Value::Object(CoreTestAggregate::aggregate_aggregate(
                &array, &keys, true, None,
            )),
        ),
        (false, Value::Int(1), Value::Int(1)),
        (true, Value::from("b"), simple_xml("<xml><a>b</a></xml>")),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

#[test]
fn test_in_filter() {
    check_cases(
        "testInFilter",
        provide_in_filter_cases(),
        |(expected, value, compare)| {
            same(
                &Value::Bool(*expected),
                &eval(
                    "value in compare",
                    ctx(&[("value", value.clone()), ("compare", compare.clone())]),
                )?,
            )
        },
    );
}

// ---- slice -------------------------------------------------------------------------------------------------------

/// `[$expected, $input, $start, $length, $preserveKeys]`.
type SliceCase = (Value, Value, i64, Value, bool);

fn provide_slice_filter_cases() -> Vec<(String, SliceCase)> {
    // ['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4]
    let i = array(vec![
        (key("a"), Value::Int(1)),
        (key("b"), Value::Int(2)),
        (key("c"), Value::Int(3)),
        (key("d"), Value::Int(4)),
    ]);
    let keys = vec![
        Value::from("a"),
        Value::from("b"),
        Value::from("c"),
        Value::from("d"),
    ];
    let one_to_four = ints([1, 2, 3, 4]);
    let a1 = Value::Array(array(vec![(key("a"), Value::Int(1))]));
    vec![
        (a1.clone(), Value::Array(i.clone()), 0, Value::Int(1), true),
        (a1, Value::Array(i.clone()), 0, Value::Int(1), false),
        (
            Value::Array(array(vec![
                (key("b"), Value::Int(2)),
                (key("c"), Value::Int(3)),
            ])),
            Value::Array(i.clone()),
            1,
            Value::Int(2),
            false,
        ),
        (ints([1]), one_to_four.clone(), 0, Value::Int(1), false),
        (ints([2, 3]), one_to_four, 1, Value::Int(2), false),
        (
            ints([2, 3]),
            CoreTestIterator::value(&i, &keys, true, None),
            1,
            Value::Int(2),
            false,
        ),
        (
            Value::Array(array(vec![
                (key("c"), Value::Int(3)),
                (key("d"), Value::Int(4)),
            ])),
            Value::Object(CoreTestAggregate::aggregate(&i, &keys, true, None)),
            2,
            Value::Null,
            true,
        ),
        (
            Value::Array(i.clone()),
            CoreTestIterator::value(&i, &keys, true, None),
            0,
            Value::Int(keys.len() as i64 + 10),
            true,
        ),
        (
            list([]),
            CoreTestIterator::value(&i, &keys, true, None),
            keys.len() as i64 + 10,
            Value::Null,
            false,
        ),
        (
            Value::from("de"),
            Value::from("abcdef"),
            3,
            Value::Int(2),
            false,
        ),
        (
            list([]),
            simple_xml("<items><item>1</item><item>2</item></items>"),
            3,
            Value::Null,
            false,
        ),
        (
            list([]),
            array_iterator(Array::from_values([Value::Int(1), Value::Int(2)])),
            3,
            Value::Null,
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

#[test]
fn test_slice_filter() {
    check_cases(
        "testSliceFilter",
        provide_slice_filter_cases(),
        |(expected, input, start, length, preserve_keys)| {
            let actual = eval(
                "input|slice(start, length, preserve_keys)",
                ctx(&[
                    ("input", input.clone()),
                    ("start", Value::Int(*start)),
                    ("length", length.clone()),
                    ("preserve_keys", Value::Bool(*preserve_keys)),
                ]),
            )?;
            same(expected, &actual)
        },
    );
}

// ---- compare -----------------------------------------------------------------------------------------------------

fn provide_compare_cases() -> Vec<(String, (i64, Value, Value))> {
    let s = |v: &str| Value::from(v);
    let b = |v: &[u8]| Value::from(v);
    let i = Value::Int;
    let f = Value::Float;
    vec![
        (0, s("a"), s("a")),
        // from https://wiki.php.net/rfc/string_to_number_comparison
        (0, i(0), s("0")),
        (0, i(0), s("0.0")),
        (-1, i(0), s("foo")),
        (1, i(0), s("")),
        (0, i(42), s("   42")),
        (-1, i(42), s("42foo")),
        (0, s("0"), s("0")),
        (0, s("0"), s("0.0")),
        (-1, s("0"), s("foo")),
        (1, s("0"), s("")),
        (0, s("42"), s("   42")),
        (-1, s("42"), s("42foo")),
        (0, i(42), s("000042")),
        (0, i(42), s("42.0")),
        (0, f(42.0), s("+42.0E0")),
        (0, i(0), s("0e214987142012")),
        (0, s("42"), s("000042")),
        (0, s("42"), s("42.0")),
        (0, s("42.0"), s("+42.0E0")),
        (0, s("0"), s("0e214987142012")),
        (0, i(42), s("   42")),
        (0, i(42), s("42   ")),
        (-1, i(42), s("42abc")),
        (-1, i(42), s("abc42")),
        (-1, i(0), s("abc42")),
        (0, f(42.0), s("   42.0")),
        (0, f(42.0), s("42.0   ")),
        (-1, f(42.0), s("42.0abc")),
        (-1, f(42.0), s("abc42.0")),
        (-1, f(0.0), s("abc42.0")),
        (0, f(f64::INFINITY), s("INF")),
        (0, f(f64::NEG_INFINITY), s("-INF")),
        (0, f(f64::INFINITY), s("1e1000")),
        (0, f(f64::NEG_INFINITY), s("-1e1000")),
        (-1, i(10), i(20)),
        (-1, s("10"), i(20)),
        (-1, i(10), s("20")),
        (1, i(42), s(" foo")),
        (0, i(42), s("42\x0C")),
        (1, i(42), b(b"\x00\x34\x32")),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

#[test]
fn test_compare() {
    check_cases(
        "testCompare",
        provide_compare_cases(),
        |(expected, a, b)| {
            let ab = compare_values(a, b, &mut Ignore).map_err(|e| format!("{e:?}"))?;
            let ba = compare_values(b, a, &mut Ignore).map_err(|e| format!("{e:?}"))?;
            if ab != *expected {
                return Err(format!("compare(a, b) = {ab}, expected {expected}"));
            }
            if -ba != *expected {
                return Err(format!("-compare(b, a) = {}, expected {expected}", -ba));
            }
            Ok(())
        },
    );
}

#[test]
fn test_compare_nan() {
    let nan = Value::Float(f64::NAN);
    let cmp = |a: &Value, b: &Value| compare_values(a, b, &mut Ignore).unwrap();
    assert_eq!(cmp(&nan, &Value::from("NAN")), 1);
    assert_eq!(cmp(&Value::from("NAN"), &nan), 1);
    assert_eq!(cmp(&nan, &Value::from("foo")), 1);
    assert_eq!(cmp(&Value::from("foo"), &nan), 1);
}

// ---- include ----------------------------------------------------------------------------------------------------

/// `new SandboxExtension($policy, false)` with `new SecurityPolicy($tags, $filters, [], [], $functions)`.
fn add_sandbox(env: &Environment, filters: &[&str], functions: &[&str]) {
    let policy = SecurityPolicy::new();
    policy.set_allowed_filters(filters.iter().copied());
    policy.set_allowed_functions(functions.iter().copied());
    env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), false)))
        .unwrap();
}

/// `new Environment(new ArrayLoader([...]))`: Twig's default options (`autoescape` is `html`).
fn default_env(templates: &[(&str, &str)]) -> Environment {
    env_with(templates, EnvironmentOptions::default())
}

#[test]
fn test_sandboxed_include() {
    let twig = default_env(&[
        ("index", "{{ include(\"included\", sandboxed: true) }}"),
        ("included", "{{ \"included\"|e }}"),
    ]);
    twig.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    add_sandbox(&twig, &[], &["include"]);
    // We expect a compile error
    let error = twig.render("index", Array::new()).unwrap_err();
    assert!(error.kind().is_security_error(), "{error}");
    // the error and the deprecations as the oracle reports them
    assert_eq!(
        error.to_string(),
        "Filter \"e\" is not allowed in \"included\" at line 1."
    );
    assert_eq!(deprecations(&twig), [SANDBOXED_INCLUDE_DEPRECATION]);
}

/// The deprecation of the `sandboxed` argument of `include()` (Twig 3.29).
const SANDBOXED_INCLUDE_DEPRECATION: &str = "Since twig/twig 3.29: The \"sandboxed\" argument of the \"include\" function is deprecated, use the \"render_sandboxed\" function to render untrusted templates instead.";

#[test]
fn test_sandboxed_include_with_preloaded_template() {
    let twig = default_env(&[
        ("index", "{{ include(\"included\", sandboxed: true) }}"),
        ("included", "{{ \"included\"|e }}"),
    ]);
    twig.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    add_sandbox(&twig, &[], &["include"]);
    // The template is loaded without the sandbox enabled, so, no compile error
    twig.load("included").unwrap();
    // We expect a runtime error
    let error = twig.render("index", Array::new()).unwrap_err();
    assert!(error.kind().is_security_error(), "{error}");
    assert_eq!(
        error.to_string(),
        "Filter \"e\" is not allowed in \"included\" at line 1."
    );
    assert_eq!(deprecations(&twig), [SANDBOXED_INCLUDE_DEPRECATION]);
}

#[test]
fn test_sandboxed_include_result_stays_escaped_when_assigned() {
    let twig = default_env(&[
        (
            "index",
            "{% set body = include('included', sandboxed: true) %}[{{ body }}]",
        ),
        ("included", "{{ evil }}"),
    ]);
    twig.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    add_sandbox(&twig, &["escape"], &["include"]);
    assert_eq!(
        twig.render("index", ctx(&[("evil", Value::from("<script>"))]))
            .unwrap(),
        "[&lt;script&gt;]"
    );
    assert_eq!(deprecations(&twig), [SANDBOXED_INCLUDE_DEPRECATION]);
}

#[test]
fn test_include_returns_markup_for_rendered_content() {
    let twig = default_env(&[
        ("index", "{% do capture(include('included')) %}"),
        ("included", "content"),
    ]);
    let capture = Capture::new();
    capture.register(&twig);
    twig.render("index", Array::new()).unwrap();
    let result = capture.take().remove(0);
    let Value::Object(object) = &result else {
        panic!("expected a Markup, got {result:?}");
    };
    assert!(Markup::is_markup(object), "{result:?}");
    assert_eq!(object.to_php_string(&mut Ignore).unwrap(), "content");
}

#[test]
fn test_include_returns_an_empty_string_for_empty_content() {
    let twig = default_env(&[
        ("index", "{% do capture(include('included')) %}"),
        ("included", ""),
    ]);
    let capture = Capture::new();
    capture.register(&twig);
    twig.render("index", Array::new()).unwrap();
    assert_eq!(same(&Value::from(""), &capture.take()[0]), Ok(()));
}

#[test]
fn test_include_returns_an_empty_string_for_ignored_missing_template() {
    let twig = default_env(&[(
        "index",
        "{% do capture(include('missing', ignore_missing: true)) %}",
    )]);
    let capture = Capture::new();
    capture.register(&twig);
    twig.render("index", Array::new()).unwrap();
    assert_eq!(same(&Value::from(""), &capture.take()[0]), Ok(()));
}

#[test]
fn test_cycle_with_array_access_and_traversable_but_not_countable() {
    let env = env_with(&[("index", "{% do capture(cycle(seq, 0)) %}")], options());
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    let capture = Capture::new();
    capture.register(&env);
    env.render("index", ctx(&[("seq", NonCountableSequence::value())]))
        .unwrap();
    let deprecations = deprecations(&env);
    expect_deprecation(
        &deprecations,
        "Since twig/twig 3.12: Passing a non-countable sequence of values to \"Twig\\Extension\\CoreExtension::cycle()\" is deprecated.",
    );
    // and nothing else (oracle)
    assert_eq!(deprecations.len(), 1, "{deprecations:?}");
    // cycle should return the first item from the traversable sequence, not the sequence itself.
    assert_eq!(same(&Value::from("odd"), &capture.take()[0]), Ok(()));
}
