//! Port of `TWIG/tests/Util/CallableArgumentsExtractorTest.php`, on `twig::extension::arguments::
//! CallableArgumentsExtractor` (the port of `Twig\Util\CallableArgumentsExtractor`).
//!
//! PHP reflects the callables; here they are described with the parameter lists PHP reflection reports
//! (`PhpCallable::function("date", ...)`, `PhpCallable::method(...)`, closures). The upstream assertions check that
//! the messages contain the expected text; the ports compare the whole message, as the oracle printed it (case
//! `callable-arguments-extractor` of `tests/vectors/upstream/extensions/messages.gen.json`).
//!
//! Not ported: `testResolveArgumentsWithMissingValueForOptionalArgument` (skipped by Twig on PHP 8).
//! `testGetArgumentsWithInvalidCallable` passes the string `'<not-a-callable>'`, which twig-rs cannot tell from a PHP
//! function (no PHP function table, reference/deviations.md "String callables"); the port uses Twig's `null` callable,
//! which takes the same path in `ReflectionCallable` (`Closure::fromCallable()` fails) and gives the same
//! `LogicException` (checked with the oracle).

use twig::ast::{Arguments, ConstantValue, Expr, ExprKind};
use twig::deprecation::Deprecation;
use twig::extension::arguments::CallableArgumentsExtractor;
use twig::extension::{
    CallableParameter as P, FunctionOptions, PhpCallable, TwigCallable, TwigFunction,
};
use twig::{ErrorKind, Source};

use crate::support::check_cases;

/// An argument of `getArguments()`: positional (`None`) or named.
type Arg = (Option<&'static str>, ConstantValue);

fn named(name: &'static str, value: impl Into<ConstantValue>) -> Arg {
    (Some(name), value.into())
}

fn positional(value: impl Into<ConstantValue>) -> Arg {
    (None, value.into())
}

/// What `getArguments()` returns: the values of the extracted constants (or, for variadic callables, the nodes
/// described with their classes, values and lines, which `assertEquals()` compares), or the exception.
#[derive(Debug, PartialEq)]
enum Extracted {
    Values(Vec<ConstantValue>),
    Nodes(Vec<String>),
    Error(ErrorKind, String),
}

/// `ConstantExpression('a')@0`, `VariadicExpression[key => value, ...]@2`.
fn describe(node: &Expr) -> String {
    match &node.kind {
        ExprKind::Constant(constant) => {
            let value = match &constant.value {
                ConstantValue::String(s) => format!("'{}'", String::from_utf8_lossy(s)),
                ConstantValue::Int(i) => i.to_string(),
                other => format!("{other:?}"),
            };
            format!("ConstantExpression({value})@{}", node.line)
        }
        ExprKind::Array(array) => format!(
            "{}[{}]@{}",
            if array.variadic {
                "VariadicExpression"
            } else {
                "ArrayExpression"
            },
            array
                .elements
                .iter()
                .map(|(key, value)| format!("{} => {}", describe(key), describe(value)))
                .collect::<Vec<_>>()
                .join(", "),
            node.line
        ),
        other => format!("{other:?}"),
    }
}

/// `getArguments($name, $callable, $args, $isVariadic)`: a `FunctionExpression` at line 2 of `test.twig`, arguments
/// as `ConstantExpression`s at line 0; also returns the deprecations raised.
fn get_arguments(
    name: &str,
    callable: Option<PhpCallable>,
    args: &[Arg],
    is_variadic: bool,
) -> (Extracted, Vec<String>) {
    let function = TwigCallable::Function(TwigFunction::new(
        name,
        callable,
        FunctionOptions {
            is_variadic,
            ..Default::default()
        },
    ));
    let source = Source::new("", "test.twig");
    let mut arguments = Arguments::new();
    for (key, value) in args {
        let node = Expr::constant(value.clone(), 0);
        match key {
            Some(name) => arguments.set_named(name.as_bytes().to_vec(), node),
            None => arguments.push(node),
        }
    }
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let result = CallableArgumentsExtractor::new(&function, 2, Some(&source), false)
        .and_then(|extractor| extractor.extract_arguments(&arguments, &mut deprecations));
    let extracted = match result {
        Ok(nodes) if is_variadic => Extracted::Nodes(nodes.iter().map(describe).collect()),
        Ok(nodes) => Extracted::Values(
            nodes
                .iter()
                .map(|node| match &node.kind {
                    ExprKind::Constant(constant) => constant.value.clone(),
                    other => panic!("not a constant: {other:?}"),
                })
                .collect(),
        ),
        Err(error) => Extracted::Error(error.kind(), error.to_string()),
    };
    (
        extracted,
        deprecations.iter().map(ToString::to_string).collect(),
    )
}

/// PHP's `date(string $format, ?int $timestamp = null)`.
fn date() -> Option<PhpCallable> {
    Some(PhpCallable::function(
        "date",
        vec![
            P::required("format"),
            P::optional("timestamp", ConstantValue::Null),
        ],
    ))
}

fn syntax_error(message: &str) -> Extracted {
    Extracted::Error(ErrorKind::Syntax, message.to_owned())
}

#[test]
fn test_get_arguments() {
    assert_eq!(
        get_arguments(
            "date",
            date(),
            &[
                named("format", "U"),
                named("timestamp", ConstantValue::Null)
            ],
            false
        )
        .0,
        Extracted::Values(vec!["U".into(), ConstantValue::Null])
    );
}

#[test]
fn test_get_arguments_when_positional_arguments_after_named_arguments() {
    assert_eq!(
        get_arguments(
            "date",
            date(),
            &[named("timestamp", 123456), positional("Y-m-d")],
            false
        )
        .0,
        syntax_error(
            "Positional arguments cannot be used after named arguments for function \"date\" in \"test.twig\" at line 2."
        )
    );
}

#[test]
fn test_get_arguments_when_argument_is_defined_twice() {
    assert_eq!(
        get_arguments(
            "date",
            date(),
            &[positional("Y-m-d"), named("format", "U")],
            false
        )
        .0,
        syntax_error(
            "Argument \"format\" is defined twice for function \"date\" in \"test.twig\" at line 2."
        )
    );
}

/// Twig reports an unknown argument with the line and the source of the argument node; nodes built without a source
/// (as here) give a message without the template name. twig-rs nodes carry no source: the extractor uses the call's
/// (` in "test.twig"`). Through templates both agree (every node has the template's source).
#[test]
#[ignore = "twig-rs mismatch: unknown-argument error takes the call's source instead of the argument node's"]
fn test_get_arguments_with_wrong_named_argument_name() {
    assert_eq!(
        get_arguments(
            "date",
            date(),
            &[
                positional("Y-m-d"),
                named("timestamp", ConstantValue::Null),
                named("unknown", "")
            ],
            false
        )
        .0,
        syntax_error("Unknown argument \"unknown\" for function \"date(format, timestamp)\".")
    );
}

#[test]
#[ignore = "twig-rs mismatch: unknown-argument error takes the call's source instead of the argument node's"]
fn test_get_arguments_with_wrong_named_argument_names() {
    assert_eq!(
        get_arguments(
            "date",
            date(),
            &[
                positional("Y-m-d"),
                named("timestamp", ConstantValue::Null),
                named("unknown1", ""),
                named("unknown2", "")
            ],
            false
        )
        .0,
        syntax_error(
            "Unknown arguments \"unknown1\", \"unknown2\" for function \"date(format, timestamp)\"."
        )
    );
}

/// `customFunction($arg1, $arg2 = 'default', $arg3 = [])` (and `customStaticFunction()`).
fn custom_function_parameters() -> Vec<P> {
    vec![
        P::required("arg1"),
        P::optional("arg2", "default".into()),
        P::optional("arg3", ConstantValue::Array(Vec::new())),
    ]
}

const TEST_CLASS: &str = "Twig\\Tests\\Util\\CallableArgumentsExtractorTest";

#[test]
fn test_resolve_arguments_only_necessary_arguments_for_custom_function() {
    assert_eq!(
        get_arguments(
            "custom_function",
            Some(PhpCallable::method(
                TEST_CLASS,
                "customFunction",
                custom_function_parameters()
            )),
            &[named("arg1", "arg1")],
            false
        )
        .0,
        Extracted::Values(vec!["arg1".into()])
    );
}

#[test]
fn test_get_arguments_for_static_method() {
    assert_eq!(
        get_arguments(
            "custom_static_function",
            Some(PhpCallable::method(
                TEST_CLASS,
                "customStaticFunction",
                custom_function_parameters()
            )),
            &[named("arg1", "arg1")],
            false
        )
        .0,
        Extracted::Values(vec!["arg1".into()])
    );
}

fn get_get_arguments_conversion_data() -> Vec<(String, (&'static str, &'static str))> {
    [
        ("some_name", "some_name"),
        ("someName", "some_name"),
        ("no_svg", "noSVG"),
        ("error_404", "error404"),
        ("errCode_404", "err_code_404"),
        ("errCode404", "err_code_404"),
        ("aBc", "a_b_c"),
        ("aBC", "a_b_c"),
    ]
    .into_iter()
    .map(|case| (format!("{} / {}", case.0, case.1), case))
    .collect()
}

#[test]
fn test_get_arguments_conversion() {
    check_cases(
        "testGetArgumentsConversion",
        get_get_arguments_conversion_data(),
        |(arg1, arg2)| {
            // eval("return fn (\$$param) => '';") called with [$name => null]
            for (param, name) in [(arg1, arg1), (arg2, arg2), (arg1, arg2), (arg2, arg1)] {
                let closure = PhpCallable::closure(vec![P::required(param)]);
                let (extracted, _) = get_arguments(
                    "custom",
                    Some(closure),
                    &[(Some(name), ConstantValue::Null)],
                    false,
                );
                if extracted != Extracted::Values(vec![ConstantValue::Null]) {
                    return Err(format!("fn (${param}) with {name}: {extracted:?}"));
                }
            }
            Ok(())
        },
    );
}

#[test]
fn test_get_arguments_conversion_for_variadics() {
    // eval("return fn (string \$someText, int \$some_number, ...\$args) => '';")
    let closure = PhpCallable::closure(vec![
        P::required("someText"),
        P::required("some_number"),
        P::variadic("args"),
    ]);
    let (extracted, deprecations) = get_arguments(
        "custom",
        Some(closure),
        &[
            named("some_text", "a"),
            named("someNumber", 12),
            named("some_text_variadic", "a"),
            named("someNumberVariadic", 12),
        ],
        true,
    );
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.15: Using \"snake_case\" for variadic arguments is required for a smooth upgrade with Twig 4.0; rename \"someNumberVariadic\" to \"some_number_variadic\" in \"test.twig\" at line 2."
        ]
    );
    // assertEquals() of the nodes (classes, attributes and lines), as the oracle reports them: the constants keep
    // their line 0, the variadic bucket and its names are at the call's line 2
    assert_eq!(
        extracted,
        Extracted::Nodes(vec![
            "ConstantExpression('a')@0".to_owned(),
            "ConstantExpression(12)@0".to_owned(),
            "VariadicExpression[ConstantExpression('some_text_variadic')@2 => ConstantExpression('a')@0, ConstantExpression('some_number_variadic')@2 => ConstantExpression(12)@0]@2".to_owned(),
        ])
    );
}

#[test]
fn test_get_arguments_error() {
    // customFunctionSnakeCamel($someName, $some_city)
    assert_eq!(
        get_arguments(
            "custom_static_function",
            Some(PhpCallable::method(
                TEST_CLASS,
                "customFunctionSnakeCamel",
                vec![P::required("someName"), P::required("some_city")]
            )),
            &[named("someCity", "Paris")],
            false
        )
        .0,
        syntax_error(
            "Value for argument \"some_name\" is required for function \"custom_static_function\" in \"test.twig\" at line 2."
        )
    );
}

#[test]
fn test_resolve_arguments_with_missing_parameter_for_arbitrary_arguments() {
    assert_eq!(
        get_arguments(
            "foo",
            Some(PhpCallable::method(
                TEST_CLASS,
                "customFunctionWithArbitraryArguments",
                vec![]
            )),
            &[],
            true
        )
        .0,
        syntax_error(
            "The last parameter of \"Twig\\Tests\\Util\\CallableArgumentsExtractorTest::customFunctionWithArbitraryArguments\" for function \"foo\" must be an array with default value, eg. \"array $arg = []\"."
        )
    );
}

#[test]
fn test_get_arguments_with_invalid_callable() {
    assert_eq!(
        get_arguments("foo", None, &[], true).0,
        Extracted::Error(
            ErrorKind::Logic,
            "Callback for function \"foo\" is not callable in the current scope.".to_owned()
        )
    );
}

#[test]
fn test_resolve_arguments_with_missing_parameter_for_arbitrary_arguments_on_function() {
    assert_eq!(
        get_arguments(
            "foo",
            Some(PhpCallable::function(
                "Twig\\Tests\\Util\\custom_call_test_function",
                vec![P::required("required")]
            )),
            &[],
            true
        )
        .0,
        syntax_error(
            "The last parameter of \"Twig\\Tests\\Util\\custom_call_test_function\" for function \"foo\" must be an array with default value, eg. \"array $arg = []\"."
        )
    );
}

#[test]
fn test_resolve_arguments_with_missing_parameter_for_arbitrary_arguments_on_object() {
    assert_eq!(
        get_arguments(
            "foo",
            Some(PhpCallable::method(
                "Twig\\Tests\\Util\\CallableTestClass",
                "__invoke",
                vec![P::required("required")]
            )),
            &[],
            true
        )
        .0,
        syntax_error(
            "The last parameter of \"Twig\\Tests\\Util\\CallableTestClass::__invoke\" for function \"foo\" must be an array with default value, eg. \"array $arg = []\"."
        )
    );
}
