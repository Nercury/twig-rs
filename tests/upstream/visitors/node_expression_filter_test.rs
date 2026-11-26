//! Twig 3.31.1's `tests/Node/Expression/FilterTest.php`. `provideTests()` compares generated PHP: its cases are
//! ported as renders, with filters that return what they receive (the arguments the compiled call passes), against
//! the oracle (`node.json`, `filter_test/*`).
//!
//! Not ported from `provideTests()`: the filters calling PHP callables of other kinds (`first_class_callable_static`,
//! `first_class_callable_object`, `bar_closure`, the extension methods `foo`/`foobar`, `magic_static` through
//! `__callStatic()`): twig-rs callables are Rust closures, whatever PHP callable they stand for.

use std::sync::Arc;

use twig::Source;
use twig::ast::{Arguments, ConstantValue, Expr, ExprKind, FilterKind};
use twig::escaper::DefaultStrategy;
use twig::extension::runtime::CallableRuntime;
use twig::extension::{CallableParameter, FilterOptions, PhpCallable, TwigFilter};
use twig::loader::ArrayLoader;
use twig::runtime::{Environment, EnvironmentOptions};
use twig::value::builtins::format::json_encode;
use twig::value::{Array, Ignore, Value};

use super::support::{Mismatches, Oracle, Outcome};

fn oracle() -> Oracle {
    Oracle::load("node")
}

/// `testConstructor()`.
#[test]
fn constructor() {
    let expr = Expr::constant("foo", 1);
    let filter = TwigFilter::new("upper", None, FilterOptions::default());
    let args = Arguments::empty_node();
    let node = Expr::filter_node(expr.clone(), filter, args.clone(), 1, FilterKind::Filter);
    let ExprKind::Filter(filter) = &node.kind else {
        panic!("expected a FilterExpression, got {node:?}");
    };
    assert_eq!(filter.node, expr);
    assert_eq!(filter.filter.name(), b"upper");
    assert_eq!(
        filter.filter_name.as_constant(),
        Some(&ConstantValue::string("upper"))
    );
    assert_eq!(filter.arguments, args);
}

/// `json_encode()` of a list.
fn json(values: Vec<Value>) -> Value {
    json_encode(
        &Value::Array(Array::from_values(values)),
        None,
        None,
        &mut Ignore,
    )
    .unwrap()
}

/// A filter whose PHP callable has `parameters` and whose runtime returns `describe(call)`.
fn filter(
    name: &str,
    parameters: Vec<CallableParameter>,
    options: FilterOptions,
    describe: impl Fn(&twig::extension::runtime::Call<'_>) -> Value + Send + Sync + 'static,
) -> TwigFilter {
    let callable = PhpCallable::closure(parameters);
    CallableRuntime::new(move |call| Ok(describe(call))).attach(&callable);
    TwigFilter::new(name, Some(callable), options)
}

/// `FilterTest::createEnvironment()`, with filters that return what they receive (as JSON, the environment and the
/// context replaced by markers).
fn environment() -> Environment {
    let env = Environment::with_options(
        ArrayLoader::new(Vec::<(String, String)>::new()),
        EnvironmentOptions {
            autoescape: DefaultStrategy::Disabled,
            ..Default::default()
        },
    );
    let p = CallableParameter::required;
    let filters = vec![
        filter("anonymous", Vec::new(), FilterOptions::default(), |_| {
            Value::Null
        }),
        filter(
            "bar",
            vec![p("env"), CallableParameter::variadic("args")],
            FilterOptions {
                needs_environment: true,
                ..Default::default()
            },
            |call| {
                let mut values = vec![Value::from("env")];
                values.extend(call.args().iter().cloned());
                json(values)
            },
        ),
        filter(
            "barbar",
            vec![
                p("context"),
                p("string"),
                CallableParameter::optional("arg1", ConstantValue::Null),
                CallableParameter::optional("arg2", ConstantValue::Null),
                CallableParameter::optional("args", ConstantValue::Array(Vec::new())).array(),
            ],
            FilterOptions {
                needs_context: true,
                is_variadic: true,
                ..Default::default()
            },
            |call| {
                let mut values = vec![Value::from("context")];
                values.extend(call.args().iter().cloned());
                // PHP fills the parameters the call leaves out with their defaults
                while values.len() < 4 {
                    values.push(Value::Null);
                }
                if values.len() < 5 {
                    values.push(Value::Array(Array::new()));
                }
                json(values)
            },
        ),
        filter(
            "bar_sandbox",
            vec![p("isSandboxed"), p("string")],
            FilterOptions {
                needs_is_sandboxed: true,
                ..Default::default()
            },
            |call| {
                let mut values = vec![Value::Bool(call.is_sandboxed())];
                values.extend(call.args().iter().cloned());
                json(values)
            },
        ),
        filter(
            "bar_all",
            vec![
                p("charset"),
                p("env"),
                p("context"),
                p("isSandboxed"),
                p("string"),
            ],
            FilterOptions {
                needs_charset: true,
                needs_environment: true,
                needs_context: true,
                needs_is_sandboxed: true,
                ..Default::default()
            },
            |call| {
                let mut values = vec![
                    Value::String(call.charset()),
                    Value::from("env"),
                    Value::from("context"),
                    Value::Bool(call.is_sandboxed()),
                ];
                values.extend(call.args().iter().cloned());
                json(values)
            },
        ),
    ];
    for filter in filters {
        env.add_filter(filter).unwrap();
    }
    env
}

/// The cases of `provideTests()` as templates, in the order of `FILTER_TEMPLATES` in the generator.
const PROVIDE_TESTS: [&str; 14] = [
    r#"{{ "foo"|upper|number_format(2, ".", ",") }}"#,
    r#"{{ 0|date(timezone="America/Chicago", format="d/m/Y H:i:s P") }}"#,
    r#"{{ 0|date(timezone="America/Chicago") }}"#,
    r#"{{ "abc"|reverse(preserve_keys=true) }}"#,
    r#"{{ "abc"|reverse(preserveKeys=true) }}"#,
    r#"{{ "foo"|anonymous }}"#,
    r#"{{ "abc"|bar_sandbox }}"#,
    r#"{{ "abc"|bar_all }}"#,
    r#"{{ "abc"|bar }}"#,
    r#"{{ "abc"|bar("bar") }}"#,
    r#"{{ "abc"|barbar }}"#,
    r#"{{ "abc"|barbar(foo="bar") }}"#,
    r#"{{ "abc"|barbar(arg2="bar") }}"#,
    r#"{{ "abc"|barbar("1", "2", "3", foo="bar") }}"#,
];

/// `provideTests()` (the compile assertions of `NodeTestCase::testCompile()`), rendered: named arguments in any
/// order, a skipped optional argument, snake case and camel case names, the injected environment, context, charset
/// and sandbox state, and the arbitrary arguments of a variadic filter.
#[test]
fn provide_tests() {
    let expected = oracle().value("filter_test/provide_tests");
    let expected = expected.as_array();
    assert_eq!(expected.len(), PROVIDE_TESTS.len());
    let mut mismatches = Mismatches::default();
    for (template, expected) in PROVIDE_TESTS.iter().zip(expected) {
        let env = environment();
        let actual = Outcome::of(
            env.create_template(template.as_bytes(), Some("index"))
                .and_then(|template| template.render(Array::new())),
        );
        let expected = match expected.get("output") {
            Some(output) => Outcome::Output(output.as_str().unwrap().to_owned()),
            None => {
                let error = expected.field("error");
                Outcome::Error {
                    class: error.field("class").as_str().unwrap().to_owned(),
                    message: error.field("message").as_str().unwrap().to_owned(),
                }
            }
        };
        mismatches.check(template, &expected, &actual);
    }
    mismatches.assert_none();
}

/// `testCompileWithWrongNamedArgumentName()` and `testCompileWithMissingNamedArgument()`: compiling the filter node
/// (no template name in the message: the upstream node has no source) fails with a `SyntaxError`.
#[test]
fn compile_with_wrong_or_missing_named_argument() {
    let expected = oracle().value("filter_test/compile_errors");
    let mut mismatches = Mismatches::default();
    for (i, (template, message)) in [
        (
            r#"{{ 0|date(foobar="America/Chicago") }}"#,
            "Unknown argument \"foobar\" for filter \"date(format, timezone)\" at line 1.",
        ),
        (
            r#"{{ 0|replace(to="foo") }}"#,
            "Value for argument \"from\" is required for filter \"replace\" at line 1.",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let env = Environment::with_shared_loader(
            Arc::new(ArrayLoader::new([("", template)])),
            EnvironmentOptions::default(),
        );
        let actual = Outcome::of(
            env.compile_source(&Source::new(template, ""))
                .map(|_| String::new()),
        );
        mismatches.check(
            template,
            &Outcome::Error {
                class: "Twig\\Error\\SyntaxError".to_owned(),
                message: message.to_owned(),
            },
            &actual,
        );
        let oracle = &expected.as_array()[i];
        mismatches.check(
            template,
            &Outcome::Error {
                class: oracle.field("class").as_str().unwrap().to_owned(),
                message: oracle.field("message").as_str().unwrap().to_owned(),
            },
            &actual,
        );
    }
    mismatches.assert_none();
}
