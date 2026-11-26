//! The parts of Twig 3.31.1's `tests/ExpressionParserTest.php` that go through tags, compilation or rendering (the
//! expression-only parts are in tests/parser/expression_parser_test.rs). Templates load through
//! `Environment::load()`/`compile_source()`, which parse like the upstream `Parser::parse()` calls.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use twig::extension::runtime::CallableRuntime;
use twig::extension::{
    CallableParameter, Extension, FilterOptions, FunctionOptions, PhpCallable, TestOptions,
    TwigFilter, TwigFunction, TwigTest,
};
use twig::value::{Array, Value};
use twig::{ErrorKind, Source};

use super::support::{Mismatches, Oracle, Outcome, ctx, defaults, env, no_escape, obj, s, strict};

fn oracle() -> Oracle {
    Oracle::load("expression_parser")
}

/// Loads `index` from `new Environment(new ArrayLoader(['index' => $template]), ['autoescape' => false])`.
fn load_index(template: &str) -> Result<String, twig::Error> {
    env(&[("index", template)], no_escape())
        .load("index")
        .map(|_| String::new())
}

/// `getFailingTestsForAssignment()`.
const FAILING_ASSIGNMENTS: [&str; 12] = [
    r#"{% set false = "foo" %}"#,
    r#"{% set FALSE = "foo" %}"#,
    r#"{% set true = "foo" %}"#,
    r#"{% set TRUE = "foo" %}"#,
    r#"{% set none = "foo" %}"#,
    r#"{% set NONE = "foo" %}"#,
    r#"{% set null = "foo" %}"#,
    r#"{% set NULL = "foo" %}"#,
    r#"{% set 3 = "foo" %}"#,
    r#"{% set 1 + 2 = "foo" %}"#,
    r#"{% set "bar" = "foo" %}"#,
    "{% set %}{% endset %}",
];

/// `testCanOnlyAssignToNames()`: a `SyntaxError` (the message from the oracle).
#[test]
fn can_only_assign_to_names() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, template) in FAILING_ASSIGNMENTS.iter().enumerate() {
        let actual = load_index(template);
        if let Err(error) = &actual {
            mismatches.check(template, &ErrorKind::Syntax, &error.kind());
        }
        mismatches.check(
            template,
            &oracle.outcome(&format!("can_only_assign_to_names/{i}")),
            &Outcome::of(actual),
        );
    }
    mismatches.assert_none();
}

/// `getMisplacedSpreadTests()`.
const MISPLACED_SPREADS: [&str; 15] = [
    "{{ ...foo }}",
    "{{ (...foo) }}",
    "{{ -...foo }}",
    "{{ 1 + ...foo }}",
    "{% set bar = ...foo %}",
    "{% do ...foo %}",
    "{% with ...foo %}{% endwith %}",
    "{{ (bar => ...foo)(1) }}",
    "{{ [(...foo)] }}",
    "{{ [1, -...foo] }}",
    "{{ {a: ...foo} }}",
    "{{ {(...foo): 1} }}",
    "{{ bar(-...foo) }}",
    "{{ bar(a: ...foo) }}",
    "{{ bar(a = ...foo) }}",
];

/// `testMisplacedSpreadIsASyntaxError()`.
#[test]
fn misplaced_spread_is_a_syntax_error() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, template) in MISPLACED_SPREADS.iter().enumerate() {
        let actual = Outcome::of(load_index(template));
        mismatches.check(
            template,
            &Outcome::Error {
                class: "Twig\\Error\\SyntaxError".to_owned(),
                message: "The spread operator can only be used on sequence elements, mapping elements, and call arguments in \"index\" at line 1.".to_owned(),
            },
            &actual,
        );
        mismatches.check(
            template,
            &oracle.outcome(&format!("misplaced_spread_is_a_syntax_error/{i}")),
            &actual,
        );
    }
    mismatches.assert_none();
}

/// `testSequenceCompilationError()`: `compileSource()` fails with a message containing `Empty array elements are only
/// allowed in destructuring assignments`.
#[test]
fn sequence_compilation_error() {
    let env = env(&[("index", "{{ [1,,2] }}")], no_escape());
    let error = env
        .compile_source(&Source::new("{{ [1,,2] }}", "index"))
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Syntax);
    assert!(
        error
            .to_string()
            .contains("Empty array elements are only allowed in destructuring assignments"),
        "{error}"
    );
    assert_eq!(
        oracle().outcome("sequence_compilation_error"),
        Outcome::of(Err(error))
    );
}

/// `getEmptyDestructuringTests()`.
const EMPTY_DESTRUCTURING: [&str; 4] = [
    "{% do [] = values %}",
    "{% do {} = values %}",
    "{% do [,] = values %}",
    "{% do [,,] = values %}",
];

/// `testEmptyDestructuringThrows()`.
#[test]
fn empty_destructuring_throws() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, template) in EMPTY_DESTRUCTURING.iter().enumerate() {
        let env = env(&[], no_escape());
        let actual = env
            .compile_source(&Source::new(*template, "index"))
            .map(|_| String::new());
        if let Err(error) = &actual {
            mismatches.check(template, &ErrorKind::Syntax, &error.kind());
            mismatches.check(
                template,
                &true,
                &error.to_string().contains(
                    "Cannot destructure to an empty list of variables in \"index\" at line 1.",
                ),
            );
        }
        mismatches.check(
            template,
            &oracle.outcome(&format!("empty_destructuring_throws/{i}")),
            &Outcome::of(actual),
        );
    }
    mismatches.assert_none();
}

/// `testObjectDestructuringEvaluatesRightHandExpressionOnce()`.
#[test]
fn object_destructuring_evaluates_right_hand_expression_once() {
    let env = env(
        &[(
            "template",
            "{% do [result_first, result_second] = ({first, second} = next_value()) %}{{ first }} {{ second }} {{ result_first }} {{ result_second }}",
        )],
        defaults(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let callable = PhpCallable::closure(Vec::new());
    let counter = Arc::clone(&calls);
    CallableRuntime::new(move |_| {
        let calls = i64::try_from(counter.fetch_add(1, Ordering::SeqCst) + 1).unwrap();
        Ok(obj(vec![
            ("first", Value::Int(calls)),
            ("second", Value::Int(calls)),
        ]))
    })
    .attach(&callable);
    env.add_function(TwigFunction::new(
        "next_value",
        Some(callable),
        FunctionOptions::default(),
    ))
    .unwrap();

    assert_eq!(env.render("template", Array::new()).unwrap(), "1 1 1 1");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let expected = oracle().value("object_destructuring_evaluates_right_hand_expression_once");
    assert_eq!(expected.field("output").as_str(), Some("1 1 1 1"));
    assert_eq!(expected.field("calls").as_i64(), Some(1));
}

/// `getTestsForNullSafeOperator()`: template, context, expected output.
fn null_safe_tests() -> Vec<(&'static str, Array, &'static str)> {
    vec![
        (
            "{{ foo?.bar }}",
            ctx(vec![("foo", obj(vec![("bar", s("baz"))]))]),
            "baz",
        ),
        ("{{ foo?.bar }}", ctx(vec![("foo", Value::Null)]), ""),
        (
            "{{ foo?.bar?.baz }}",
            ctx(vec![(
                "foo",
                obj(vec![("bar", obj(vec![("baz", s("qux"))]))]),
            )]),
            "qux",
        ),
        (
            "{{ foo?.bar?.baz }}",
            ctx(vec![("foo", obj(vec![("bar", Value::Null)]))]),
            "",
        ),
        ("{{ foo?.bar?.baz }}", ctx(vec![("foo", Value::Null)]), ""),
        (
            r#"{{ foo?.bar?.baz ?? "qux" }}"#,
            ctx(vec![("foo", Value::Null)]),
            "qux",
        ),
        (
            r#"{{ foo?.bar ?? "qux" }}"#,
            ctx(vec![("foo", obj(vec![("bar", Value::Int(0))]))]),
            "0",
        ),
        (
            r#"{{ foo?.bar ?? "qux" }}"#,
            ctx(vec![("foo", obj(vec![("bar", Value::Bool(false))]))]),
            "",
        ),
        // short-circuiting
        ("{{ foo?.bar.baz }}", ctx(vec![("foo", Value::Null)]), ""),
        (
            "{{ foo?.bar.baz?.qux.corge }}",
            ctx(vec![("foo", Value::Null)]),
            "",
        ),
        (
            "{{ foo?.bar.baz?.qux.corge }}",
            ctx(vec![(
                "foo",
                obj(vec![("bar", obj(vec![("baz", Value::Null)]))]),
            )]),
            "",
        ),
        // default filter fallback
        (
            "{{ foo?.bar|default(foo?.baz) }}",
            ctx(vec![("foo", Value::Null)]),
            "",
        ),
        (
            "{{ foo?.bar|default(foo?.baz.qux) }}",
            ctx(vec![("foo", Value::Null)]),
            "",
        ),
        (
            "{% for foo in foos %}[{{ foo?.bar|default(foo?.baz) }}]{% endfor %}",
            ctx(vec![(
                "foos",
                Value::Array(Array::from_values([
                    obj(vec![("bar", s("corge")), ("baz", s("grault"))]),
                    obj(vec![("baz", s("qux"))]),
                ])),
            )]),
            "[corge][qux]",
        ),
    ]
}

/// `testNullSafeOperator()`: `new Environment(new ArrayLoader(['template' => $template]), ['strict_variables' =>
/// true])`.
#[test]
fn null_safe_operator() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, (template, context, expected)) in null_safe_tests().into_iter().enumerate() {
        let actual =
            Outcome::of(env(&[("template", template)], strict()).render("template", context));
        mismatches.check(template, &Outcome::Output(expected.to_owned()), &actual);
        mismatches.check(
            template,
            &oracle.outcome(&format!("null_safe_operator/{i}")),
            &actual,
        );
    }
    mismatches.assert_none();
}

/// `testInvalidNullSafeOperatorShortCircuiting()` with `getTestForInvalidNullSafeOperatorShortCircuiting()`: a
/// `RuntimeError` whose message contains the expected one.
#[test]
fn invalid_null_safe_operator_short_circuiting() {
    let tests = [
        (
            "{{ foo?.bar.baz }}",
            ctx(vec![("foo", obj(vec![("bar", Value::Null)]))]),
            "Impossible to access an attribute (\"baz\") on a null variable in \"template\" at line 1.",
        ),
        (
            "{{ foo?.bar.baz?.qux.corge }}",
            ctx(vec![(
                "foo",
                obj(vec![(
                    "bar",
                    obj(vec![("baz", obj(vec![("qux", Value::Null)]))]),
                )]),
            )]),
            "Impossible to access an attribute (\"corge\") on a null variable in \"template\" at line 1.",
        ),
    ];
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, (template, context, message)) in tests.into_iter().enumerate() {
        let actual = env(&[("template", template)], strict()).render("template", context);
        match &actual {
            Ok(output) => mismatches.push(format!("{template}: rendered {output:?}")),
            Err(error) => {
                mismatches.check(template, &ErrorKind::Runtime, &error.kind());
                mismatches.check(template, &true, &error.to_string().contains(message));
            }
        }
        mismatches.check(
            template,
            &oracle.outcome(&format!("invalid_null_safe_operator_short_circuiting/{i}")),
            &Outcome::of(actual),
        );
    }
    mismatches.assert_none();
}

/// `testMacroDefinitionDoesNotSupportNonNameVariableName()`.
#[test]
fn macro_definition_does_not_support_non_name_variable_name() {
    let actual = load_index(r#"{% macro foo("a") %}{% endmacro %}"#);
    let error = actual.as_ref().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Syntax);
    assert!(error.to_string().contains("An argument must be a name. Unexpected token \"string\" of value \"a\" (\"name\" expected) in \"index\" at line 1 column 14."), "{error}");
    assert_eq!(
        oracle().outcome("macro_definition_does_not_support_non_name_variable_name"),
        Outcome::of(actual)
    );
}

/// `testMacroDefinitionDoesNotSupportNonConstantDefaultValues()` with its data provider.
#[test]
fn macro_definition_does_not_support_non_constant_default_values() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, template) in [
        r#"{% macro foo(name = "a #{foo} a") %}{% endmacro %}"#,
        r#"{% macro foo(name = [["b", "a #{foo} a"]]) %}{% endmacro %}"#,
    ]
    .into_iter()
    .enumerate()
    {
        let actual = load_index(template);
        match &actual {
            Ok(_) => mismatches.push(format!("{template}: no error")),
            Err(error) => {
                mismatches.check(template, &ErrorKind::Syntax, &error.kind());
                mismatches.check(template, &true, &error.to_string().contains("A default value for an argument must be a constant (a boolean, a string, a number, a sequence, or a mapping) in \"index\" at line 1"));
            }
        }
        mismatches.check(
            template,
            &oracle.outcome(&format!(
                "macro_definition_does_not_support_non_constant_default_values/{i}"
            )),
            &Outcome::of(actual),
        );
    }
    mismatches.assert_none();
}

/// `testMacroDefinitionSupportsConstantDefaultValues()` with its data provider.
#[test]
fn macro_definition_supports_constant_default_values() {
    let mut mismatches = Mismatches::default();
    for template in [
        r#"{% macro foo(name = "aa") %}{% endmacro %}"#,
        "{% macro foo(name = 12) %}{% endmacro %}",
        "{% macro foo(name = true) %}{% endmacro %}",
        r#"{% macro foo(name = ["a"]) %}{% endmacro %}"#,
        r#"{% macro foo(name = [["a"]]) %}{% endmacro %}"#,
        r#"{% macro foo(name = {a: "a"}) %}{% endmacro %}"#,
        r#"{% macro foo(name = {a: {b: "a"}}) %}{% endmacro %}"#,
    ] {
        mismatches.check(
            template,
            &Outcome::Output(String::new()),
            &Outcome::of(load_index(template)),
        );
    }
    mismatches.assert_none();
}

/// `getBindingPowerTests()`: name, expression, expected expression, whether the test passes the context
/// `['a' => ['b' => 1, 'c' => ['d' => 2]]]`.
fn binding_power_tests() -> Vec<(String, String, String, bool)> {
    let mut tests = Vec::new();
    // * / // % stronger than + -
    for op1 in ["*", "/", "//", "%"] {
        for op2 in ["+", "-"] {
            tests.push((
                format!("{op1} vs {op2}"),
                format!("{{{{ 12 {op1} 6 {op2} 3 }}}}"),
                format!("{{{{ (12 {op1} 6) {op2} 3 }}}}"),
                false,
            ));
            tests.push((
                format!("{op2} vs {op1}"),
                format!("{{{{ 12 {op2} 6 {op1} 3 }}}}"),
                format!("{{{{ 12 {op2} (6 {op1} 3) }}}}"),
                false,
            ));
        }
    }
    // + - * / // % stronger than == != <=> < > >= <=
    for op1 in ["+", "-", "*", "/", "//", "%"] {
        for op2 in ["==", "!=", "<=>", "<", ">", ">=", "<="] {
            tests.push((
                format!("{op1} vs {op2}"),
                format!("{{{{ 12 {op1} 6 {op2} 3 }}}}"),
                format!("{{{{ (12 {op1} 6) {op2} 3 }}}}"),
                false,
            ));
        }
    }
    let rest: [(&str, &str, &str, bool); 18] = [
        (
            "+ vs not in",
            "{{ 1 + 2 not in [3, 4] }}",
            "{{ (1 + 2) not in [3, 4] }}",
            false,
        ),
        (
            "+ vs in",
            "{{ 1 + 2 in [3, 4] }}",
            "{{ (1 + 2) in [3, 4] }}",
            false,
        ),
        (
            "+ vs matches",
            r#"{{ 1 + 2 matches "/^3$/" }}"#,
            r#"{{ (1 + 2) matches "/^3$/" }}"#,
            false,
        ),
        // ~ stronger than `starts with` `ends with`
        (
            "~ vs starts with",
            r#"{{ "a" ~ "b" starts with "a" }}"#,
            r#"{{ ("a" ~ "b") starts with "a" }}"#,
            false,
        ),
        (
            "~ vs ends with",
            r#"{{ "a" ~ "b" ends with "b" }}"#,
            r#"{{ ("a" ~ "b") ends with "b" }}"#,
            false,
        ),
        // [] . stronger than anything else
        (
            "[] vs unary -",
            r#"{{ -a["b"] + 3 }}"#,
            r#"{{ -(a["b"]) + 3 }}"#,
            true,
        ),
        (
            "[] vs unary - (multiple levels)",
            r#"{{ -a["c"]["d"] }}"#,
            r#"{{ -((a["c"])["d"]) }}"#,
            true,
        ),
        (". vs unary -", "{{ -a.b }}", "{{ -(a.b) }}", true),
        (
            ". vs unary - (multiple levels)",
            "{{ -a.c.d }}",
            "{{ -((a.c).d) }}",
            true,
        ),
        (
            ". [] vs unary -",
            r#"{{ -a.c["d"] }}"#,
            r#"{{ -((a.c)["d"]) }}"#,
            true,
        ),
        (
            "[] . vs unary -",
            r#"{{ -a["c"].d }}"#,
            r#"{{ -((a["c"]).d) }}"#,
            true,
        ),
        // () stronger than anything else
        (
            "() vs unary -",
            "{{ -random(1, 1) + 3 }}",
            "{{ -(random(1, 1)) + 3 }}",
            false,
        ),
        // + - stronger than |
        (
            "+ vs |",
            "{{ 10 + 2|length }}",
            "{{ 10 + (2|length) }}",
            true,
        ),
        // = stronger than anything else
        (
            "= same as literal",
            r#"{% do c = "a" %}{{ c }}"#,
            r#"{% do c = ("a") %}{{ c }}"#,
            false,
        ),
        (
            "= stronger than .",
            "{% do c = a.b %}{{ c }}",
            "{% do c = (a.b) %}{{ c }}",
            true,
        ),
        (
            "= stronger than math",
            "{% do a = 1 + 3 %}{{ a }}",
            "{% do a = (1 + 3) %}{{ a }}",
            false,
        ),
        (
            "= stronger than logical",
            "{% do a = false or true %}{{ a }}",
            "{% do a = (false or true) %}{{ a }}",
            false,
        ),
        (
            "= stronger than ternary",
            "{% do c = 4 ? 0 : -1 %}{{ c }}",
            "{% do c = (4 ? 0 : -1) %}{{ c }}",
            false,
        ),
    ];
    tests.extend(rest.iter().map(|(name, expression, expected, context)| {
        (
            (*name).to_owned(),
            (*expression).to_owned(),
            (*expected).to_owned(),
            *context,
        )
    }));
    tests
}

/// `testBindingPower()`: `{{ expression }}` renders like its explicitly parenthesised twin, and like PHP evaluates
/// the expression (the PHP value of the provider is compared loosely by PHPUnit; the oracle has the exact output).
#[test]
fn binding_power() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    let context = || {
        ctx(vec![(
            "a",
            Value::Array({
                let mut a = Array::new();
                a.insert("b", Value::Int(1));
                let mut c = Array::new();
                c.insert("d", Value::Int(2));
                a.insert("c", Value::Array(c));
                a
            }),
        )])
    };
    let tests = binding_power_tests();
    assert_eq!(tests.len(), 76);
    for (i, (name, expression, expected, with_context)) in tests.iter().enumerate() {
        let env = env(
            &[("expression", expression), ("expected", expected)],
            defaults(),
        );
        let data = || {
            if *with_context {
                context()
            } else {
                Array::new()
            }
        };
        let actual = Outcome::of(env.render("expression", data()));
        let twin = Outcome::of(env.render("expected", data()));
        let case = format!("{name}: {expression}");
        mismatches.check(&case, &twin, &actual);
        mismatches.check(
            &case,
            &oracle.outcome(&format!("binding_power/{i}/expression")),
            &actual,
        );
        mismatches.check(
            &format!("{name}: {expected}"),
            &oracle.outcome(&format!("binding_power/{i}/expected")),
            &twin,
        );
    }
    mismatches.assert_none();
}

/// `static function ($foo, $bar, $a) {}` that returns its arguments joined with commas: the dynamic parts of the
/// name come first (Twig compiles `getCallable()("foo", "bar", "a")`).
fn three_arguments(seen: &Arc<Mutex<Vec<String>>>) -> PhpCallable {
    let callable = PhpCallable::closure(vec![
        CallableParameter::required("foo"),
        CallableParameter::required("bar"),
        CallableParameter::required("a"),
    ]);
    let seen = Arc::clone(seen);
    CallableRuntime::new(move |call| {
        let mut arguments: Vec<String> = call
            .dynamic_arguments()
            .iter()
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();
        arguments.extend(call.args().iter().map(|value| {
            value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{value:?}"))
        }));
        let joined = arguments.join(",");
        seen.lock().unwrap().push(joined.clone());
        Ok(Value::from(joined))
    })
    .attach(&callable);
    callable
}

struct DynamicExtension {
    seen: Arc<Mutex<Vec<String>>>,
}

impl Extension for DynamicExtension {
    fn class_name(&self) -> &str {
        "DynamicExtension"
    }

    fn tests(&self) -> Vec<TwigTest> {
        vec![TwigTest::new(
            "*_foo_*_bar",
            Some(three_arguments(&self.seen)),
            TestOptions::default(),
        )]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        vec![TwigFunction::new(
            "*_foo_*_bar",
            Some(three_arguments(&self.seen)),
            FunctionOptions::default(),
        )]
    }

    fn filters(&self) -> Vec<TwigFilter> {
        vec![TwigFilter::new(
            "*_foo_*_bar",
            Some(three_arguments(&self.seen)),
            FilterOptions::default(),
        )]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `testCompiledCodeForDynamicTest()`, `testCompiledCodeForDynamicFunction()` and
/// `testCompiledCodeForDynamicFilter()`, on what the compiled call does: the callable receives the parts of the name
/// matched by `*`, then the argument (and a test prints what its callable returns).
#[test]
fn compiled_code_for_dynamic_callables() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (kind, template) in [
        ("test", r#"{{ "a" is foo_foo_bar_bar }}"#),
        ("function", r#"{{ foo_foo_bar_bar("a") }}"#),
        ("filter", r#"{{ "a"|foo_foo_bar_bar }}"#),
    ] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let env = env(&[("index", template)], no_escape());
        env.add_extension(Arc::new(DynamicExtension {
            seen: Arc::clone(&seen),
        }))
        .unwrap();
        let output = Outcome::of(env.render("index", Array::new()));
        let expected = oracle.value(&format!("compiled_code_for_dynamic_callables/{kind}"));
        mismatches.check(
            template,
            &Outcome::Output(expected.field("output").as_str().unwrap().to_owned()),
            &output,
        );
        let expected_seen: Vec<String> = expected
            .field("seen")
            .as_array()
            .iter()
            .map(|call| call.as_str().unwrap().to_owned())
            .collect();
        mismatches.check(template, &expected_seen, &seen.lock().unwrap().clone());
    }
    mismatches.assert_none();
}

// ---- mismatches found around the ported tests (oracle cases `<topic>/<i>` of expression_parser.json) ---------------

/// The template list of a group of oracle cases, rendered and compared one by one.
fn check_group(group: &str, templates: &[&str], render: impl Fn(&str) -> Outcome) {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, template) in templates.iter().enumerate() {
        mismatches.check(
            template,
            &oracle.outcome(&format!("{group}/{i}")),
            &render(template),
        );
    }
    mismatches.assert_none();
}

/// Around `testNullSafeOperator()` (which runs with `strict_variables`): when a null-safe access is followed by a
/// plain one (`u?.c.z`), Twig's `GetAttrExpression::compile()` hoists the null check of the null-safe node and
/// compiles its object (`u`) before the null-safe node's own `compile()` gives that object `ignore_strict_check`, so
/// an undefined `u` fails in strict mode even under `??`, `default` or `is defined`. twig-rs passes the null-safe
/// node's `ignore_strict_check` down at once and renders the fallback.
#[test]
#[ignore = "twig-rs mismatch: null-safe short circuit drops the strict check of the base (u?.c.z ?? 'd')"]
fn null_safe_short_circuit_keeps_the_strict_check_of_its_object() {
    check_group(
        "null_safe_short_circuit_strict",
        &[
            "{{ u?.c.z ?? 'd' }}",
            "{{ u?.c.z is defined ? 'y' : 'n' }}",
            "{{ u?.c.z|default('d') }}",
            "{{ u?.c['z'] ?? 'd' }}",
            "{{ u?.c.z.w ?? 'd' }}",
            "{{ u?.c ?? 'd' }}",
            "{{ u?.c?.z.w ?? 'd' }}",
            "{{ n?.c.z ?? 'd' }}",
            "{{ u.b?.c.z ?? 'd' }}",
        ],
        |template| {
            Outcome::of(
                env(&[("template", template)], strict())
                    .render("template", ctx(vec![("n", Value::Null)])),
            )
        },
    );
}

/// Around `testMacroDefinitionSupportsConstantDefaultValues()`: a default value is compiled into the PHP signature of
/// the macro closure (`$a = -"a"`), which PHP evaluates when the macro is called without that argument, so `-`/`+`
/// of a non-numeric constant throws a `TypeError` then (wrapped in a `RuntimeError`, at the line `Error::guess()`
/// finds: the last marker of the template body) and a leading-numeric string warns `A non-numeric value
/// encountered` (once: PHP caches the evaluated default). twig-rs evaluates defaults at compile time
/// (`const_eval()` in src/lltl/codegen.rs), drops the error (the value becomes `null`) and the warning.
#[test]
#[ignore = "twig-rs mismatch: macro defaults like -'a' become null instead of a TypeError at call time"]
fn macro_definition_unary_default_values_are_evaluated_when_called() {
    use twig::runtime::{DiagnosticsPolicy, EnvironmentOptions, Notice};
    let templates = [
        "{% macro m(a = -'a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
        "{% macro m(a = +'a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
        "{% macro m(a = -[1]) %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
        "{% macro m(a = [-'a']) %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
        "{% macro m(a = {a: -''}) %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
        "{% macro m(a = - -'a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
        "{% macro m(a = -'a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m(7) }}",
        "{% macro m(a = -'a') %}\n[{{ a }}]\n{% endmacro %}\nx\n{{ _self.m() }}\ny\n{{ 'z' }}\n",
        "{% macro m(a = -'1a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}{{ _self.m() }}",
    ];
    check_group("macro_unary_defaults", &templates, |template| {
        Outcome::of(env(&[("index", template)], no_escape()).render("index", Array::new()))
    });
    // the warning of the leading-numeric default, raised once
    let options = EnvironmentOptions {
        diagnostics: DiagnosticsPolicy::Collect,
        ..no_escape()
    };
    let env = env(&[("index", templates[8])], options);
    env.render("index", Array::new()).unwrap();
    let warnings: Vec<String> = env
        .take_notices()
        .iter()
        .filter_map(|notice| match notice {
            Notice::Php(diagnostic) => Some(diagnostic.message.to_string_lossy().into_owned()),
            Notice::Deprecation(_) => None,
        })
        .collect();
    let expected: Vec<String> = oracle()
        .result("macro_unary_defaults/8")
        .field("diagnostics")
        .as_array()
        .iter()
        .map(|diagnostic| diagnostic.field("message").as_str().unwrap().to_owned())
        .collect();
    assert_eq!(warnings, expected);
}

/// Around `testBindingPower()`: PHP swaps the operands of its commutative opcodes (`*`, `b-and`, `b-or`, `b-xor`:
/// `ZEND_MUL`, `ZEND_BW_AND`, `ZEND_BW_OR`, `ZEND_BW_XOR`) when the left one has the lower operand type (CONST < TMP
/// < VAR < CV, `zend_vm_set_opcode_handler()` in PHPSRC/Zend/zend_vm_execute.h), so the `TypeError` names the types
/// in the swapped order: `7 * arr` fails with `Unsupported operand types: array * int` (a literal, or a constant PHP
/// folds, against a variable; a variable against a function call). twig-rs keeps the template order.
#[test]
#[ignore = "twig-rs mismatch: TypeError of 7 * arr names the types in template order, PHP swaps them"]
fn binding_power_type_errors_follow_php_operand_order() {
    check_group(
        "operand_order",
        &[
            "{{ 7 * arr }}",
            "{{ 7 b-and arr }}",
            "{{ 'a' b-or arr }}",
            "{{ null b-xor arr }}",
            "{{ (1 + 2) * arr }}",
            "{{ i * range(1, 2) }}",
            "{{ [1] * i }}",
            "{{ arr * 7 }}",
            "{{ i * arr }}",
            "{{ 7 + arr }}",
            "{{ range(1, 2) * i }}",
        ],
        |template| {
            Outcome::of(env(&[("index", template)], no_escape()).render(
                "index",
                ctx(vec![
                    ("arr", Value::Array(Array::from_values([Value::Int(1)]))),
                    ("i", Value::Int(7)),
                ]),
            ))
        },
    );
}
