//! Twig 3.31.1's `tests/Node/Expression/CallTest.php` (`@group legacy`): the deprecated
//! `CallExpression::getArguments()`, called through reflection on a function node at line 0 without a source. twig-rs
//! has only the binding templates use, `CallableArgumentsExtractor`, with the same messages; where the two differ (the
//! arbitrary-arguments check throws a `LogicException` in the legacy method and a `SyntaxError` in the extractor),
//! the port expects the extractor's class, which the oracle shows for templates (`node.json`, `call_test/*`).
//!
//! Not ported: `testResolveArgumentsWithMissingValueForOptionalArgument` (skipped by Twig on PHP 8).

use twig::ErrorKind;
use twig::ast::{Arguments, ConstantValue, Expr, ExprKind};
use twig::deprecation::Deprecation;
use twig::extension::arguments::CallableArgumentsExtractor;
use twig::extension::{
    CallableParameter, FunctionOptions, PhpCallable, TwigCallable, TwigFunction,
};

/// An argument of the upstream calls: a position or a name, and its value.
enum Arg {
    Positional(ConstantValue),
    Named(&'static str, ConstantValue),
}

fn arguments(args: Vec<Arg>) -> Arguments {
    let mut arguments = Arguments::new();
    for arg in args {
        match arg {
            Arg::Positional(value) => arguments.push(Expr::constant(value, 0)),
            Arg::Named(name, value) => {
                arguments.set_named(name.as_bytes().to_vec(), Expr::constant(value, 0));
            }
        }
    }
    arguments
}

/// `$this->getArguments($node, [$callable, $arguments])` for `new Node_Expression_Call(new TwigFunction($name,
/// $callable, ['is_variadic' => $isVariadic]), new EmptyNode(), 0)`: the extracted values, or the error.
fn get_arguments(
    function: TwigFunction,
    args: Vec<Arg>,
) -> Result<Vec<ConstantValue>, twig::Error> {
    let callable = TwigCallable::Function(function);
    let extractor = CallableArgumentsExtractor::new(&callable, 0, None, false)?;
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let extracted = extractor.extract_arguments(&arguments(args), &mut deprecations)?;
    Ok(extracted
        .iter()
        .map(|expr| match &expr.kind {
            ExprKind::Constant(constant) => constant.value.clone(),
            other => panic!("not a constant: {other:?}"),
        })
        .collect())
}

/// `createFunctionExpression('date', 'date')`: a function named `date` calling the PHP function `date(string
/// $format, ?int $timestamp = null)`, whose parameters Twig reflects.
fn date() -> TwigFunction {
    TwigFunction::new(
        "date",
        Some(PhpCallable::function(
            "date",
            vec![
                CallableParameter::required("format"),
                CallableParameter::optional("timestamp", ConstantValue::Null),
            ],
        )),
        FunctionOptions::default(),
    )
}

fn message(result: Result<Vec<ConstantValue>, twig::Error>) -> (ErrorKind, String) {
    let error = result.expect_err("an error");
    (error.kind(), error.to_string())
}

fn s(value: &str) -> ConstantValue {
    ConstantValue::string(value)
}

/// `testGetArguments()`.
#[test]
fn get_arguments_binds_names() {
    use Arg::*;
    assert_eq!(
        get_arguments(
            date(),
            vec![
                Named("format", s("U")),
                Named("timestamp", ConstantValue::Null)
            ]
        )
        .unwrap(),
        [s("U"), ConstantValue::Null]
    );
}

/// `testGetArgumentsWhenPositionalArgumentsAfterNamedArguments()`.
#[test]
fn get_arguments_when_positional_arguments_after_named_arguments() {
    use Arg::*;
    assert_eq!(
        message(get_arguments(
            date(),
            vec![
                Named("timestamp", ConstantValue::Int(123456)),
                Positional(s("Y-m-d"))
            ]
        )),
        (
            ErrorKind::Syntax,
            "Positional arguments cannot be used after named arguments for function \"date\"."
                .to_owned()
        )
    );
}

/// `testGetArgumentsWhenArgumentIsDefinedTwice()`.
#[test]
fn get_arguments_when_argument_is_defined_twice() {
    use Arg::*;
    assert_eq!(
        message(get_arguments(
            date(),
            vec![Positional(s("Y-m-d")), Named("format", s("U"))]
        )),
        (
            ErrorKind::Syntax,
            "Argument \"format\" is defined twice for function \"date\".".to_owned()
        )
    );
}

/// `testGetArgumentsWithWrongNamedArgumentName()`.
#[test]
fn get_arguments_with_wrong_named_argument_name() {
    use Arg::*;
    assert_eq!(
        message(get_arguments(
            date(),
            vec![
                Positional(s("Y-m-d")),
                Named("timestamp", ConstantValue::Null),
                Named("unknown", s(""))
            ]
        )),
        (
            ErrorKind::Syntax,
            "Unknown argument \"unknown\" for function \"date(format, timestamp)\".".to_owned()
        )
    );
}

/// `testGetArgumentsWithWrongNamedArgumentNames()`.
#[test]
fn get_arguments_with_wrong_named_argument_names() {
    use Arg::*;
    assert_eq!(
        message(get_arguments(
            date(),
            vec![
                Positional(s("Y-m-d")),
                Named("timestamp", ConstantValue::Null),
                Named("unknown1", s("")),
                Named("unknown2", s(""))
            ]
        )),
        (
            ErrorKind::Syntax,
            "Unknown arguments \"unknown1\", \"unknown2\" for function \"date(format, timestamp)\"."
                .to_owned()
        )
    );
}

/// `customFunction($arg1, $arg2 = 'default', $arg3 = [])` (an instance or a static method of the test class).
fn custom_function(name: &str, method: &str) -> TwigFunction {
    TwigFunction::new(
        name,
        Some(PhpCallable::method(
            "Twig\\Tests\\Node\\Expression\\CallTest",
            method,
            vec![
                CallableParameter::required("arg1"),
                CallableParameter::optional("arg2", s("default")),
                CallableParameter::optional("arg3", ConstantValue::Array(Vec::new())),
            ],
        )),
        FunctionOptions::default(),
    )
}

/// `testResolveArgumentsOnlyNecessaryArgumentsForCustomFunction()`.
#[test]
fn resolve_arguments_only_necessary_arguments_for_custom_function() {
    assert_eq!(
        get_arguments(
            custom_function("custom_function", "customFunction"),
            vec![Arg::Named("arg1", s("arg1"))]
        )
        .unwrap(),
        [s("arg1")]
    );
}

/// `testGetArgumentsForStaticMethod()`.
#[test]
fn get_arguments_for_static_method() {
    assert_eq!(
        get_arguments(
            custom_function("custom_static_function", "customStaticFunction"),
            vec![Arg::Named("arg1", s("arg1"))]
        )
        .unwrap(),
        [s("arg1")]
    );
}

/// A variadic function `foo` whose PHP callable has the given name and parameters.
fn variadic(callable: PhpCallable) -> TwigFunction {
    TwigFunction::new(
        "foo",
        Some(callable),
        FunctionOptions {
            is_variadic: true,
            ..Default::default()
        },
    )
}

/// `testResolveArgumentsWithMissingParameterForArbitraryArguments()`, `...OnFunction()` and `...OnObject()`: the
/// last parameter of a variadic callable must be an array with a default value (a `LogicException` in the legacy
/// method, a `SyntaxError` from the extractor).
#[test]
fn resolve_arguments_with_missing_parameter_for_arbitrary_arguments() {
    for (callable, name) in [
        (
            PhpCallable::method(
                "Twig\\Tests\\Node\\Expression\\CallTest",
                "customFunctionWithArbitraryArguments",
                Vec::new(),
            ),
            "Twig\\Tests\\Node\\Expression\\CallTest::customFunctionWithArbitraryArguments",
        ),
        (
            PhpCallable::function(
                "Twig\\Tests\\Node\\Expression\\custom_call_test_function",
                vec![CallableParameter::required("required")],
            ),
            "Twig\\Tests\\Node\\Expression\\custom_call_test_function",
        ),
        (
            PhpCallable::method(
                "Twig\\Tests\\Node\\Expression\\CallableTestClass",
                "__invoke",
                vec![CallableParameter::required("required")],
            ),
            "Twig\\Tests\\Node\\Expression\\CallableTestClass::__invoke",
        ),
    ] {
        assert_eq!(
            message(get_arguments(variadic(callable), Vec::new())),
            (
                ErrorKind::Syntax,
                format!(
                    "The last parameter of \"{name}\" for function \"foo\" must be an array with default value, eg. \"array $arg = []\"."
                )
            ),
            "{name}"
        );
    }
}

/// `testGetArgumentsWithInvalidCallable()`: a function without a PHP callable (`'<not-a-callable>'` in Twig).
#[test]
fn get_arguments_with_invalid_callable() {
    let function = TwigFunction::new(
        "foo",
        None,
        FunctionOptions {
            is_variadic: true,
            ..Default::default()
        },
    );
    assert_eq!(
        message(get_arguments(function, Vec::new())),
        (
            ErrorKind::Logic,
            "Callback for function \"foo\" is not callable in the current scope.".to_owned()
        )
    );
}

/// The arbitrary-arguments check through templates (the oracle's `call_test/arbitrary_arguments_in_templates`): a
/// `SyntaxError` naming the PHP callable, with the template name.
#[test]
fn arbitrary_arguments_check_in_templates() {
    use std::sync::Arc;

    use twig::escaper::DefaultStrategy;
    use twig::loader::ArrayLoader;
    use twig::runtime::{Environment, EnvironmentOptions};
    use twig::value::Array;

    use super::support::{Oracle, Outcome};

    let expected = Oracle::load("node").value("call_test/arbitrary_arguments_in_templates");
    for (i, (template, callable)) in [
        ("{{ foo() }}", PhpCallable::closure(Vec::new())),
        (
            "{{ bar() }}",
            PhpCallable::function(
                "twig_rs_custom_call_test_function",
                vec![CallableParameter::required("required")],
            ),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let env = Environment::with_shared_loader(
            Arc::new(ArrayLoader::new(Vec::<(String, String)>::new())),
            EnvironmentOptions {
                autoescape: DefaultStrategy::Disabled,
                ..Default::default()
            },
        );
        let name = if i == 0 { "foo" } else { "bar" };
        env.add_function(variadic_named(name, callable)).unwrap();
        let actual = Outcome::of(
            env.create_template(template.as_bytes(), Some("index"))
                .and_then(|template| template.render(Array::new())),
        );
        let error = expected.as_array()[i].field("error");
        assert_eq!(
            actual,
            Outcome::Error {
                class: error.field("class").as_str().unwrap().to_owned(),
                message: error.field("message").as_str().unwrap().to_owned(),
            },
            "{template}"
        );
    }
}

/// A variadic function named `name`.
fn variadic_named(name: &str, callable: PhpCallable) -> TwigFunction {
    TwigFunction::new(
        name,
        Some(callable),
        FunctionOptions {
            is_variadic: true,
            ..Default::default()
        },
    )
}
