//! Port of `TWIG/tests/Extension/SandboxTest.php` (the deprecated stateful sandbox of `SandboxExtension`).
//!
//! `getEnvironment()` becomes [`Setup`]; PHP's typed closures become host callables that declare the PHP parameter
//! types the sandbox reflects (`CallableParameter::string_coercion_safe()`) and coerce their arguments as PHP would
//! (`verify_user_arg()`). PHPUnit converts warnings and notices to exceptions, so the environments use
//! `DiagnosticsPolicy::Error` (deprecations are recorded for `expectDeprecation()`).
//!
//! Not ported:
//!
//! - the `recursion` case of `testSandboxUnallowedToString` and the `recursion` entry of `$params`: twig-rs arrays
//!   cannot contain references, so a recursive array cannot be built (reference/deviations.md, "Resources,
//!   references");
//! - `testLegacySecurityPolicyWithoutTestsParameterTriggersDeprecation`,
//!   `testLegacyCheckSecurityCallWithSourceAs4thArgumentTriggersDeprecation`,
//!   `testLegacySecurityPolicyCheckSecurityWithoutTestsArgTriggersDeprecation`: they reflect on the arity of PHP
//!   methods (`ReflectionMethod::getNumberOfParameters()`, `func_num_args()`) or pass a `Source` where an array is
//!   declared; `SecurityPolicyInterface::check_security()` always takes the four lists;
//! - `testLegacyCheckSecurityNodeWithoutUsedTestsTriggersDeprecation`: constructs a `CheckSecurityNode` with PHP
//!   arguments (node constructors are internal to the twig-rs sandbox visitor);
//! - `testNeedsIsSandboxedHelperTriggersDeprecationForCustomImplementation`,
//!   `testCustomTokenParserWithoutIsAlwaysAllowedInSandboxTriggersDeprecation`: deprecations for PHP classes that do
//!   not implement a method of an interface; the Rust traits have default methods;
//! - `testSandboxAllowsNonStringCoercingCustomTestNodeInput`: a test whose node class overrides `compile()`
//!   (reference/deviations.md, "Custom node classes in compiled code");
//! - `testNonSandboxedSourcePolicyAllowsNonClosureCallable`: sorts with the PHP function `strnatcasecmp` named by a
//!   string (reference/deviations.md, "String callables");
//! - the generated-PHP assertions of `testSelfMacroReferenceWithStringLiteralDoesNotInjectPhp` and
//!   `testImportedTemplateMacroReferenceWithBadIdentifierDoesNotInjectPhp` (`compileSource()` returns PHP code); their
//!   render part is ported.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use twig::ast::{Expr, Node};
use twig::deprecation::Deprecation;
use twig::escaper::DefaultStrategy;
use twig::extension::runtime::{Call, CallResult};
use twig::extension::{
    CallableParameter as P, FilterOptions, FunctionOptions, PhpCallable, StringLoaderExtension,
    TestOptions, TokenParser, TwigFilter, TwigFunction, TwigTest,
};
use twig::lexer::{Token, TokenType};
use twig::parser::Parser;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, Markup, SandboxExtension, SecurityError,
    SecurityErrorKind, SecurityPolicy, SecurityPolicyInterface, SourcePolicyInterface,
};
use twig::value::object::ArrayObject;
use twig::value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig::value::{Array, PhpError, PhpString, ThrowableClass, Value, convert};
use twig::{ErrorKind, Source};

use super::classes::{
    ArrayLikeObject, COLUMN_OBJECT, ChildClass, ColumnObject, Counters, CyclicTraversableObject,
    FOO_OBJECT, FooObject, MAGIC_CALL_OBJECT, MagicCallObject, MagicObject, NullArrayAccess,
    STRINGABLE_PLAIN_OBJECT, STRINGABLE_TRAVERSABLE_OBJECT, StringablePlainObject,
    StringableTraversableObject, array_iterator,
};
use super::support::{
    check_cases, ctx, deprecations, expect_deprecation, filter_with, function, function_with, list,
    loader, scenario_id, set_scenario, test_with, upstream_test_name,
};

// ---- fixtures ----------------------------------------------------------------------------------------------------

/// `self::$templates` of `setUp()`.
const TEMPLATES: &[(&str, &str)] = &[
    ("1_basic1", "{{ obj.foo }}"),
    ("1_basic2", "{{ name|upper }}"),
    ("1_basic3", "{% if name %}foo{% endif %}"),
    ("1_basic4", "{{ obj.bar }}"),
    ("1_basic5", "{{ obj }}"),
    ("1_basic7", "{{ cycle([\"foo\",\"bar\"], 1) }}"),
    ("1_basic8", "{{ obj.getfoobar }}{{ obj.getFooBar }}"),
    ("1_basic9", "{{ obj.foobar }}{{ obj.fooBar }}"),
    ("1_basic", "{% if obj.foo %}{{ obj.foo|upper }}{% endif %}"),
    ("1_layout", "{% block content %}{% endblock %}"),
    (
        "1_child",
        "{% extends \"1_layout\" %}\n{% block content %}\n{{ \"a\"|json_encode }}\n{% endblock %}",
    ),
    ("1_include", "{{ include(\"1_basic1\", sandboxed=true) }}"),
    (
        "1_basic2_include_template_from_string_sandboxed",
        "{{ include(template_from_string(\"{{ name|upper }}\"), sandboxed=true) }}",
    ),
    (
        "1_basic2_include_template_from_string",
        "{{ include(template_from_string(\"{{ name|upper }}\")) }}",
    ),
    ("1_range_operator", "{{ (1..2)[0] }}"),
    (
        "1_syntax_error_wrapper_legacy",
        "{% sandbox %}{% include \"1_syntax_error\" %}{% endsandbox %}",
    ),
    (
        "1_syntax_error_wrapper",
        "{{ include(\"1_syntax_error\", sandboxed: true) }}",
    ),
    ("1_syntax_error", "{% syntax error }}"),
    ("1_childobj_parentmethod", "{{ child_obj.ParentMethod() }}"),
    ("1_childobj_childmethod", "{{ child_obj.ChildMethod() }}"),
    ("1_empty", ""),
    ("1_array_like", "{{ array_like[\"foo\"] }}"),
];

/// `self::$params` of `setUp()` (without `recursion`, see the module documentation); the `FooObject`s count into
/// `counters`.
fn params_with(counters: &Arc<Counters>) -> Array {
    let foo = || FooObject::with_counters(counters);
    let mut obj_map = Array::new();
    obj_map.insert("obj", foo());
    let mut to_string_map = Array::new();
    to_string_map.insert("__toString", foo());
    ctx(&[
        ("name", Value::from("Fabien")),
        ("obj", foo()),
        ("arr", Value::Array(obj_map)),
        ("child_obj", ChildClass::value()),
        (
            "some_array",
            list([Value::Int(5), Value::Int(6), Value::Int(7), foo()]),
        ),
        ("array_like", ArrayLikeObject::value()),
        ("magic", MagicObject::value()),
        (
            "iterator",
            array_iterator(Array::from_values([Value::from("a"), foo()])),
        ),
        ("iterator_map", array_iterator(to_string_map.clone())),
        (
            "iterator_nested",
            array_iterator(Array::from_values([
                Value::from("a"),
                array_iterator(Array::from_values([Value::from("b"), foo()])),
            ])),
        ),
        (
            "stringable_iterator",
            StringableTraversableObject::value(Array::from_values([Value::from("a"), foo()])),
        ),
        (
            "stringable_iterator_map",
            StringableTraversableObject::value(to_string_map),
        ),
    ])
}

fn params() -> Array {
    params_with(&Counters::new())
}

/// The arguments of `getEnvironment($sandboxed, $options, $templates, $tags, $filters, $methods, $properties,
/// $functions, $sourcePolicy, $strict, $tests)`.
#[derive(Default, Clone)]
struct Setup {
    sandboxed: bool,
    strict_variables: bool,
    autoescape_html: bool,
    templates: Vec<(String, String)>,
    tags: Vec<String>,
    filters: Vec<String>,
    methods: Vec<(String, Vec<String>)>,
    properties: Vec<(String, Vec<String>)>,
    functions: Vec<String>,
    source_policy: Option<Arc<dyn SourcePolicyInterface>>,
    strict: bool,
    tests: Vec<String>,
}

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn per_class(lists: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    lists
        .iter()
        .map(|(class, names)| (class.to_string(), owned(names)))
        .collect()
}

/// `getEnvironment($sandboxed, [], $templates)`.
fn setup(sandboxed: bool, templates: &[(&str, &str)]) -> Setup {
    Setup {
        sandboxed,
        templates: templates
            .iter()
            .map(|(n, s)| (n.to_string(), s.to_string()))
            .collect(),
        ..Setup::default()
    }
}

impl Setup {
    fn tags(mut self, tags: &[&str]) -> Self {
        self.tags = owned(tags);
        self
    }

    fn filters(mut self, filters: &[&str]) -> Self {
        self.filters = owned(filters);
        self
    }

    fn methods(mut self, methods: &[(&str, &[&str])]) -> Self {
        self.methods = per_class(methods);
        self
    }

    fn properties(mut self, properties: &[(&str, &[&str])]) -> Self {
        self.properties = per_class(properties);
        self
    }

    fn functions(mut self, functions: &[&str]) -> Self {
        self.functions = owned(functions);
        self
    }

    fn tests(mut self, tests: &[&str]) -> Self {
        self.tests = owned(tests);
        self
    }

    fn strict(mut self) -> Self {
        self.strict = true;
        self
    }

    fn strict_variables(mut self) -> Self {
        self.strict_variables = true;
        self
    }

    fn autoescape_html(mut self) -> Self {
        self.autoescape_html = true;
        self
    }

    fn source_policy(mut self, policy: impl SourcePolicyInterface) -> Self {
        self.source_policy = Some(Arc::new(policy));
        self
    }

    /// `new Environment(new ArrayLoader($templates), array_merge(['debug' => true, 'cache' => false, 'autoescape' =>
    /// false], $options))`, `new SecurityPolicy(...)`, `setStrict()`, `addExtension(new SandboxExtension($policy,
    /// $sandboxed, $sourcePolicy))`.
    fn env(&self) -> Environment {
        let templates: Vec<(&str, &str)> = self
            .templates
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        let env = Environment::with_options(
            loader(&templates),
            EnvironmentOptions {
                debug: true,
                strict_variables: self.strict_variables,
                autoescape: if self.autoescape_html {
                    DefaultStrategy::html()
                } else {
                    DefaultStrategy::Disabled
                },
                diagnostics: DiagnosticsPolicy::Error,
                ..Default::default()
            },
        );
        let policy = SecurityPolicy::new();
        policy.set_allowed_tags(self.tags.clone());
        policy.set_allowed_filters(self.filters.clone());
        policy.set_allowed_methods(self.methods.clone());
        policy.set_allowed_properties(self.properties.clone());
        policy.set_allowed_functions(self.functions.clone());
        policy.set_allowed_tests(self.tests.clone());
        policy.set_strict(self.strict);
        let extension = match &self.source_policy {
            Some(source_policy) => {
                let mut constructor: Vec<Deprecation> = Vec::new();
                let extension = SandboxExtension::with_source_policy(
                    Arc::new(policy),
                    self.sandboxed,
                    Arc::clone(source_policy),
                    &mut constructor,
                );
                // expectDeprecation() of every test passing a source policy
                let messages: Vec<String> = constructor.iter().map(ToString::to_string).collect();
                expect_deprecation(&messages, SOURCE_POLICY_DEPRECATION);
                for deprecation in constructor {
                    env.report_deprecation(deprecation);
                }
                extension
            }
            None => SandboxExtension::new(Arc::new(policy), self.sandboxed),
        };
        env.add_extension(Arc::new(extension)).unwrap();
        env
    }
}

/// `$twig->getExtension(SandboxExtension::class)->getSecurityPolicy()` as the `SecurityPolicy` it is.
fn with_policy(env: &Environment, f: impl FnOnce(&SecurityPolicy)) {
    let policy = env.with_extensions(|set| {
        set.extension::<SandboxExtension>()
            .expect("the sandbox extension")
            .security_policy()
    });
    f(policy
        .as_any()
        .downcast_ref::<SecurityPolicy>()
        .expect("a SecurityPolicy"));
}

/// `$twig->getExtension(SandboxExtension::class)->isSandboxed()`.
fn is_sandboxed(env: &Environment) -> bool {
    env.with_extensions(|set| {
        set.extension::<SandboxExtension>()
            .expect("the sandbox extension")
            .is_sandboxed(None)
    })
}

/// A source policy (`SourcePolicyInterface` implemented by an anonymous class).
struct SourcePolicy<F: Fn(&Source) -> bool + Send + Sync + 'static>(F);

impl<F: Fn(&Source) -> bool + Send + Sync + 'static> SourcePolicyInterface for SourcePolicy<F> {
    fn enable_sandbox(&self, source: &Source) -> bool {
        (self.0)(source)
    }
}

fn always(enabled: bool) -> SourcePolicy<impl Fn(&Source) -> bool + Send + Sync + 'static> {
    SourcePolicy(move |_: &Source| enabled)
}

const SOURCE_POLICY_DEPRECATION: &str = "Since twig/twig 3.27.0: The \"Twig\\Sandbox\\SourcePolicyInterface\" interface is deprecated with no replacement, do not pass an instance to \"Twig\\Extension\\SandboxExtension\".";

type Rendered = twig::Result<String>;

/// `$twig->load($name)->render($context)`, compared with the oracle (see [`finish`]).
fn render(env: &Environment, name: &str, context: Array) -> Rendered {
    let result = env.load(name).and_then(|template| template.render(context));
    finish(env, result)
}

/// `$twig->createTemplate($template, 'index')->render([])`, compared with the oracle (see [`finish`]).
fn render_string(env: &Environment, template: &str) -> Rendered {
    let result = env
        .create_template(template.as_bytes(), Some("index"))
        .and_then(|template| template.render(Array::new()));
    finish(env, result)
}

/// `tests/vectors/upstream/extensions/sandbox.json`: the oracle results of the scenarios, by scenario id.
static VECTORS: std::sync::LazyLock<crate::common::Json> = std::sync::LazyLock::new(|| {
    crate::common::Json::load_vectors("upstream/extensions/sandbox.json")
});

thread_local! {
    /// The deprecations raised by the last scenario of this thread (everything since its environment was created).
    static LAST_DEPRECATIONS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The deprecations of the last rendered scenario (Symfony's `expectDeprecation()` checks them).
fn last_deprecations() -> Vec<String> {
    LAST_DEPRECATIONS.with(|d| d.borrow().clone())
}

/// Takes the deprecations the environment recorded and compares the scenario with the oracle: the output, or the
/// exception class, message and previous exception class, and the deprecations. Messages naming PHP closures
/// (`{closure:<file>:<line>}`, reference/deviations.md) are compared by class only.
fn finish(env: &Environment, result: Rendered) -> Rendered {
    let deprecations = deprecations(env);
    LAST_DEPRECATIONS.with(|d| *d.borrow_mut() = deprecations.clone());
    if let Some(id) = scenario_id()
        && let Err(mismatch) = compare_with_oracle(&id, &result, &deprecations)
    {
        panic!("oracle mismatch for \"{id}\": {mismatch}");
    }
    result
}

fn compare_with_oracle(id: &str, result: &Rendered, deprecations: &[String]) -> Result<(), String> {
    let expected = VECTORS
        .get(id)
        .ok_or_else(|| format!("no oracle vector for scenario \"{id}\""))?;
    let field = |name: &str| {
        expected
            .get(name)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
    };
    match (expected.field("ok"), result) {
        (crate::common::Json::Bool(true), Ok(output)) if output == field("output") => {}
        (crate::common::Json::Bool(false), Err(error)) => {
            if error.class_name() != field("class") {
                return Err(format!(
                    "expected {}: {}, got {}",
                    field("class"),
                    field("message"),
                    describe(result)
                ));
            }
            let message = field("message");
            if !message.contains("{closure") && error.to_string() != message {
                return Err(format!(
                    "expected the message {message:?}, got {:?}",
                    error.to_string()
                ));
            }
            let previous = expected.get("previous").and_then(|v| v.as_str());
            if previous.is_some() && previous_class(error).as_deref() != previous {
                return Err(format!(
                    "expected a previous {previous:?}, got {:?}",
                    previous_class(error)
                ));
            }
        }
        (crate::common::Json::Bool(true), _) => {
            return Err(format!(
                "expected the output {:?}, got {}",
                field("output"),
                describe(result)
            ));
        }
        _ => {
            return Err(format!(
                "expected {}: {}, got {}",
                field("class"),
                field("message"),
                describe(result)
            ));
        }
    }
    let expected_deprecations: Vec<String> = expected
        .field("deprecations")
        .as_array()
        .iter()
        .map(|d| d.as_lossy_string())
        .collect();
    if expected_deprecations != deprecations {
        return Err(format!(
            "expected the deprecations {expected_deprecations:#?}, got {deprecations:#?}"
        ));
    }
    Ok(())
}

/// Sets the id of the next scenario of the running test: `<upstream test>/<part>`.
fn sub_scenario(part: &str) {
    let base = upstream_test_name().expect("a test thread");
    set_scenario(Some(format!("{base}/{part}")));
}

/// Compares a policy check with the oracle (the scenarios that call `checkSecurity()` directly).
fn compare_policy_with_oracle(id: &str, result: &std::result::Result<(), SecurityError>) {
    let expected = VECTORS.field(id);
    let field = |name: &str| {
        expected
            .get(name)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
    };
    match result {
        Err(error) => {
            assert_eq!(error.class_name(), field("class"), "{id}");
            assert_eq!(error.message(), field("message"), "{id}");
        }
        Ok(()) => assert_eq!(
            expected.field("ok"),
            &crate::common::Json::Bool(true),
            "{id}"
        ),
    }
}

fn describe(result: &Rendered) -> String {
    match result {
        Ok(output) => format!("rendered {output:?}"),
        Err(e) => format!("{}: {e}", e.class_name()),
    }
}

/// The sandbox violation of a failed render.
fn violation(result: &Rendered) -> Result<SecurityErrorKind, String> {
    match result {
        Err(e) if e.kind().is_security_error() => SecurityError::of(e)
            .map(|v| v.kind().clone())
            .ok_or_else(|| format!("a security error without its names: {e}")),
        other => Err(format!("expected a SecurityError, got {}", describe(other))),
    }
}

/// `catch (SecurityNotAllowedMethodError $e)` with `getClassName()` and `getMethodName()`.
fn expect_method(result: &Rendered, class: &str, method: &str) -> Result<(), String> {
    match violation(result)? {
        SecurityErrorKind::Method {
            class: c,
            method: m,
        } if c == class && m == method => Ok(()),
        other => Err(format!(
            "expected a method error on {class}::{method}, got {other:?} ({})",
            describe(result)
        )),
    }
}

/// `SecurityNotAllowedMethodError` on `FooObject::__tostring`.
fn expect_to_string(result: &Rendered) -> Result<(), String> {
    expect_method(result, FOO_OBJECT, "__tostring")
}

fn expect_property(result: &Rendered, class: &str, property: &str) -> Result<(), String> {
    match violation(result)? {
        SecurityErrorKind::Property {
            class: c,
            property: p,
        } if c == class && p == property => Ok(()),
        other => Err(format!(
            "expected a property error on {class}::{property}, got {other:?} ({})",
            describe(result)
        )),
    }
}

fn expect_named(
    result: &Rendered,
    what: &str,
    name: &str,
    pick: fn(&SecurityErrorKind) -> Option<&String>,
) -> Result<(), String> {
    let kind = violation(result)?;
    match pick(&kind) {
        Some(n) if n == name => Ok(()),
        _ => Err(format!(
            "expected a {what} error on {name}, got {kind:?} ({})",
            describe(result)
        )),
    }
}

fn expect_tag(result: &Rendered, tag: &str) -> Result<(), String> {
    expect_named(result, "tag", tag, |k| match k {
        SecurityErrorKind::Tag(n) => Some(n),
        _ => None,
    })
}

fn expect_filter(result: &Rendered, filter: &str) -> Result<(), String> {
    expect_named(result, "filter", filter, |k| match k {
        SecurityErrorKind::Filter(n) => Some(n),
        _ => None,
    })
}

fn expect_function(result: &Rendered, function: &str) -> Result<(), String> {
    expect_named(result, "function", function, |k| match k {
        SecurityErrorKind::Function(n) => Some(n),
        _ => None,
    })
}

fn expect_test(result: &Rendered, test: &str) -> Result<(), String> {
    expect_named(result, "test", test, |k| match k {
        SecurityErrorKind::Test(n) => Some(n),
        _ => None,
    })
}

/// `expectExceptionMessage()`: the message contains the text.
fn expect_message(result: &Rendered, kind: ErrorKind, message: &str) -> Result<(), String> {
    match result {
        Err(e) if e.kind() == kind && e.to_string().contains(message) => Ok(()),
        other => Err(format!(
            "expected {} containing {message:?}, got {}",
            kind.class_name(),
            describe(other)
        )),
    }
}

fn expect_output(result: &Rendered, expected: &str) -> Result<(), String> {
    match result {
        Ok(output) if output == expected => Ok(()),
        other => Err(format!("expected {expected:?}, got {}", describe(other))),
    }
}

fn ok(result: Result<(), String>) {
    if let Err(message) = result {
        panic!("{message}");
    }
}

/// The class of the previous exception of a wrapped error (`$e->getPrevious()`).
fn previous_class(error: &twig::Error) -> Option<String> {
    let previous = error.previous()?;
    if let Some(e) = previous.downcast_ref::<twig::Error>() {
        return Some(e.class_name().to_owned());
    }
    previous
        .downcast_ref::<PhpError>()
        .map(|e| e.class().name().to_owned())
}

/// `catch (RuntimeError $e) { $this->assertInstanceOf(\TypeError::class, $e->getPrevious()); }`.
fn expect_wrapped_type_error(result: &Rendered) -> Result<(), String> {
    match result {
        Err(e) if e.kind() == ErrorKind::Runtime => match previous_class(e).as_deref() {
            Some("TypeError") => Ok(()),
            other => Err(format!("previous is {other:?}: {e}")),
        },
        other => Err(format!(
            "expected a RuntimeError wrapping a TypeError, got {}",
            describe(other)
        )),
    }
}

// ---- PHP closures ------------------------------------------------------------------------------------------------

/// PHP's coercion of an argument to a typed closure parameter (`{closure}(): Argument #n ($name) must be of type
/// ...` on failure).
fn typed(
    call: &mut Call<'_>,
    index: usize,
    name: &'static str,
    ty: &DeclaredType<'_>,
) -> CallResult {
    let value = call.arg(index).cloned().unwrap_or_default();
    Ok(verify_user_arg(
        value,
        ty,
        &UserArg::new("{closure}", index as u32 + 1, name),
        call.diagnostics(),
    )?)
}

const INT: DeclaredType<'static> = DeclaredType::builtin(TypeMask::INT);
const STRING: DeclaredType<'static> = DeclaredType::builtin(TypeMask::STRING);

/// `(string) $value`.
fn to_string(call: &mut Call<'_>, value: &Value) -> CallResult {
    Ok(Value::String(convert::to_string(
        value,
        call.diagnostics(),
    )?))
}

/// `'foo' === $value`.
fn is_foo(value: &Value) -> Value {
    Value::Bool(*value == Value::from("foo"))
}

/// `static fn ($v): bool => true` (untyped).
fn true_test(name: &str, options: TestOptions) -> TwigTest {
    test_with(name, vec![P::required("v")], options, |_| {
        Ok(Value::Bool(true))
    })
}

/// `PHP strtoupper()` as the callable of a filter or function.
fn strtoupper(call: &mut Call<'_>) -> CallResult {
    let value = call.arg(0).cloned().unwrap_or_default();
    let string = convert::to_string(&value, call.diagnostics())?;
    Ok(Value::String(PhpString::from(
        string.as_bytes().to_ascii_uppercase(),
    )))
}

fn strtoupper_callable() -> PhpCallable {
    let callable = PhpCallable::function("strtoupper", vec![P::required("string")]);
    twig::extension::runtime::CallableRuntime::new(strtoupper).attach(&callable);
    callable
}

// ---- core tags, extends and use ----------------------------------------------------------------------------------

fn get_sandboxed_for_core_tags_tests() -> Vec<(String, (&'static str, &'static str))> {
    [
        ("apply", "{% apply upper %}foo{% endapply %}"),
        ("autoescape", "{% autoescape %}foo{% endautoescape %}"),
        ("block", "{% block foo %}foo{% endblock %}"),
        ("deprecated", "{% deprecated \"message\" %}"),
        ("do", "{% do 1 + 2 %}"),
        ("embed", "{% embed \"base.twig\" %}{% endembed %}"),
        // To be uncommented in 4.0: ('extends', '{% extends "base.twig" %}')
        ("flush", "{% flush %}"),
        ("for", "{% for i in 1..2 %}{% endfor %}"),
        ("from", "{% from \"macros\" import foo %}"),
        ("guard", "{% guard function max %}{% endguard %}"),
        ("if", "{% if false %}{% endif %}"),
        ("import", "{% import \"macros\" as macros %}"),
        ("include", "{% include \"macros\" %}"),
        ("macro", "{% macro foo() %}{% endmacro %}"),
        ("set", "{% set foo = 1 %}"),
        // To be uncommented in 4.0: ('use', '{% use "1_empty" %}')
        ("with", "{% with foo %}{% endwith %}"),
    ]
    .into_iter()
    .map(|case| (case.0.to_owned(), case))
    .collect()
}

#[test]
fn test_sandbox_for_core_tags() {
    check_cases(
        "testSandboxForCoreTags",
        get_sandboxed_for_core_tags_tests(),
        |(tag, template)| {
            let twig = setup(true, TEMPLATES).env();
            let result = render_string(&twig, template);
            violation(&result)?;
            // '/Tag "%s" is not allowed in "index \(string template .+?\)" at line 1/'
            let message = describe(&result);
            let prefix = format!("Tag \"{tag}\" is not allowed in \"index (string template ");
            match message.find(&prefix) {
                Some(at) if message[at + prefix.len()..].contains(")\" at line 1") => Ok(()),
                _ => Err(format!("unexpected message: {message}")),
            }
        },
    );
}

#[test]
fn test_sandbox_for_extends_and_use_tags() {
    let cases = [
        ("extends", "{% extends \"1_empty\" %}"),
        ("use", "{% use \"1_empty\" %}"),
    ]
    .into_iter()
    .map(|case| (case.0.to_owned(), case));
    check_cases(
        "testSandboxForExtendsAndUseTags",
        cases,
        |(tag, template)| {
            let twig = setup(true, TEMPLATES).env();
            expect_output(&render_string(&twig, template), "")?;
            let expected = format!(
                "Since twig/twig 3.12: The \"{tag}\" tag is always allowed in sandboxes, but won't be in 4.0, please enable it explicitly in your sandbox policy if needed (or enable strict mode on the security policy to opt-in to the 4.0 behavior now)."
            );
            let deprecations = last_deprecations();
            if deprecations.contains(&expected) {
                Ok(())
            } else {
                Err(format!(
                    "missing deprecation {expected:?} in {deprecations:?}"
                ))
            }
        },
    );
}

/// A case of `getSandboxedForParserCallableFunctionsTests()` / `getAllowedParserCallableFunctionsTests()`.
struct ParserCallableCase {
    function: &'static str,
    template_name: &'static str,
    extra_templates: Vec<(&'static str, &'static str)>,
    allowed_tags: Vec<&'static str>,
    allowed_methods: Vec<(&'static str, Vec<&'static str>)>,
    allowed_properties: Vec<(&'static str, Vec<&'static str>)>,
    allowed_functions: Vec<&'static str>,
    context: Array,
    expected: &'static str,
}

fn parser_callable_setup(case: &ParserCallableCase, strict: bool) -> Environment {
    let methods: Vec<(&str, &[&str])> = case
        .allowed_methods
        .iter()
        .map(|(c, m)| (*c, m.as_slice()))
        .collect();
    let properties: Vec<(&str, &[&str])> = case
        .allowed_properties
        .iter()
        .map(|(c, p)| (*c, p.as_slice()))
        .collect();
    let mut setup = setup(true, &case.extra_templates)
        .tags(&case.allowed_tags)
        .methods(&methods)
        .properties(&properties)
        .functions(&case.allowed_functions);
    setup.strict = strict;
    setup.env()
}

fn base_and_child() -> Vec<(&'static str, &'static str)> {
    vec![
        ("base", "{% block content %}PARENT{% endblock %}"),
        (
            "child",
            "{% extends \"base\" %}{% block content %}{{ parent() }} CHILD{% endblock %}",
        ),
    ]
}

fn data(key_name: &str, value: &str) -> Array {
    let mut data = Array::new();
    data.insert(key_name, value);
    ctx(&[("data", Value::Array(data))])
}

fn get_sandboxed_for_parser_callable_functions_tests() -> Vec<(String, ParserCallableCase)> {
    let case = |function, template_name, extra_templates, allowed_tags: Vec<&'static str>| {
        ParserCallableCase {
            function,
            template_name,
            extra_templates,
            allowed_tags,
            allowed_methods: vec![],
            allowed_properties: vec![],
            allowed_functions: vec![],
            context: Array::new(),
            expected: "",
        }
    };
    vec![
        (
            "attribute on array",
            ParserCallableCase {
                context: data("secret", "LEAK"),
                expected: "LEAK",
                ..case(
                    "attribute",
                    "index",
                    vec![("index", "{{ attribute(data, \"secret\") }}")],
                    vec![],
                )
            },
        ),
        (
            "attribute on object property",
            ParserCallableCase {
                allowed_properties: vec![(FOO_OBJECT, vec!["bar"])],
                context: ctx(&[("obj", FooObject::value())]),
                expected: "bar",
                ..case(
                    "attribute",
                    "index",
                    vec![("index", "{{ attribute(obj, \"bar\") }}")],
                    vec![],
                )
            },
        ),
        (
            "attribute on object method",
            ParserCallableCase {
                allowed_methods: vec![(FOO_OBJECT, vec!["foo"])],
                context: ctx(&[("obj", FooObject::value())]),
                expected: "foo",
                ..case(
                    "attribute",
                    "index",
                    vec![("index", "{{ attribute(obj, \"foo\") }}")],
                    vec![],
                )
            },
        ),
        (
            "block from same template",
            ParserCallableCase {
                expected: "BB",
                ..case(
                    "block",
                    "index",
                    vec![(
                        "index",
                        "{% block content %}B{% endblock %}{{ block(\"content\") }}",
                    )],
                    vec!["block"],
                )
            },
        ),
        (
            "parent inside inherited block",
            ParserCallableCase {
                expected: "PARENT CHILD",
                ..case("parent", "child", base_and_child(), vec!["block"])
            },
        ),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case))
    .collect()
}

#[test]
fn test_sandbox_for_parser_callable_functions() {
    check_cases(
        "testSandboxForParserCallableFunctions",
        get_sandboxed_for_parser_callable_functions_tests(),
        |case| {
            let twig = parser_callable_setup(case, false);
            expect_output(
                &render(&twig, case.template_name, case.context.clone()),
                case.expected,
            )?;
            let expected = format!(
                "Since twig/twig 3.27: The \"{}\" function is always allowed in sandboxes, but won't be in 4.0, please enable it explicitly in your sandbox policy if needed (or enable strict mode on the security policy to opt-in to the 4.0 behavior now).",
                case.function
            );
            let deprecations = last_deprecations();
            if deprecations.contains(&expected) {
                Ok(())
            } else {
                Err(format!(
                    "missing deprecation {expected:?} in {deprecations:?}"
                ))
            }
        },
    );
}

fn get_allowed_parser_callable_functions_tests() -> Vec<(String, ParserCallableCase)> {
    vec![
        (
            "attribute allowed",
            ParserCallableCase {
                function: "attribute",
                template_name: "index",
                extra_templates: vec![("index", "{{ attribute(data, \"x\") }}")],
                allowed_tags: vec![],
                allowed_methods: vec![],
                allowed_properties: vec![],
                allowed_functions: vec!["attribute"],
                context: data("x", "OK"),
                expected: "OK",
            },
        ),
        (
            "block allowed",
            ParserCallableCase {
                function: "block",
                template_name: "index",
                extra_templates: vec![(
                    "index",
                    "{% block content %}B{% endblock %}{{ block(\"content\") }}",
                )],
                allowed_tags: vec!["block"],
                allowed_methods: vec![],
                allowed_properties: vec![],
                allowed_functions: vec!["block"],
                context: Array::new(),
                expected: "BB",
            },
        ),
        (
            "parent allowed",
            ParserCallableCase {
                function: "parent",
                template_name: "child",
                extra_templates: base_and_child(),
                allowed_tags: vec!["block", "extends"],
                allowed_methods: vec![],
                allowed_properties: vec![],
                allowed_functions: vec!["parent"],
                context: Array::new(),
                expected: "PARENT CHILD",
            },
        ),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case))
    .collect()
}

#[test]
fn test_sandbox_with_allowed_parser_callable_functions() {
    check_cases(
        "testSandboxWithAllowedParserCallableFunctions",
        get_allowed_parser_callable_functions_tests(),
        |case| {
            let twig = parser_callable_setup(case, false);
            expect_output(
                &render(&twig, case.template_name, case.context.clone()),
                case.expected,
            )
        },
    );
}

#[test]
fn test_strict_sandbox_rejects_grandfathered_tags() {
    let cases = [
        ("extends", ("extends", "{% extends \"1_empty\" %}")),
        ("use", ("use", "{% use \"1_empty\" %}")),
        (
            "use of a missing template is rejected before the loader is reached",
            ("use", "{% use \"does_not_exist\" %}"),
        ),
        (
            "use of a missing block is rejected before the trait is resolved",
            ("use", "{% use \"1_layout\" with does_not_exist as alias %}"),
        ),
        (
            "use of a non-traitable template is rejected before the trait is resolved",
            ("use", "{% use \"1_child\" %}"),
        ),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case));
    check_cases(
        "testStrictSandboxRejectsGrandfatheredTags",
        cases,
        |(tag, template)| {
            let twig = setup(true, TEMPLATES).strict().env();
            let result = render_string(&twig, template);
            expect_tag(&result, tag)?;
            expect_message(
                &result,
                ErrorKind::SecurityNotAllowedTag,
                &format!("Tag \"{tag}\" is not allowed"),
            )
        },
    );
}

#[test]
fn test_strict_sandbox_rejects_grandfathered_functions() {
    let cases = vec![
        (
            "attribute",
            (
                "attribute",
                "index",
                vec![("index", "{{ attribute(data, \"secret\") }}")],
                vec![],
                data("secret", "LEAK"),
            ),
        ),
        (
            "block",
            (
                "block",
                "index",
                vec![(
                    "index",
                    "{% block content %}B{% endblock %}{{ block(\"content\") }}",
                )],
                vec!["block"],
                Array::new(),
            ),
        ),
        (
            "parent",
            (
                "parent",
                "child",
                base_and_child(),
                vec!["extends", "block"],
                Array::new(),
            ),
        ),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case));
    check_cases(
        "testStrictSandboxRejectsGrandfatheredFunctions",
        cases,
        |(function, template_name, extra_templates, allowed_tags, context)| {
            let twig = setup(true, extra_templates)
                .tags(allowed_tags)
                .strict()
                .env();
            let result = render(&twig, template_name, context.clone());
            expect_function(&result, function)?;
            expect_message(
                &result,
                ErrorKind::SecurityNotAllowedFunction,
                &format!("Function \"{function}\" is not allowed"),
            )
        },
    );
}

#[test]
fn test_strict_sandbox_still_allows_explicitly_allowed_grandfathered_names() {
    let twig = setup(
        true,
        &[
            ("base", "{% block content %}PARENT{% endblock %}"),
            (
                "child",
                "{% extends \"base\" %}{% block content %}{{ parent() }} CHILD - {{ attribute(data, \"x\") }}{% endblock %}",
            ),
        ],
    )
    .tags(&["extends", "block"])
    .functions(&["parent", "attribute"])
    .strict()
    .env();
    ok(expect_output(
        &render(&twig, "child", data("x", "OK")),
        "PARENT CHILD - OK",
    ));
}

#[test]
fn test_strict_mode_can_be_enabled_via_setter_after_construction() {
    let policy = SecurityPolicy::new();
    policy.set_strict(true);
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let result = policy.check_security(&[b"extends"], &[], &[], &[], &mut deprecations);
    compare_policy_with_oracle(
        "testStrictModeCanBeEnabledViaSetterAfterConstruction",
        &result,
    );
    let error = result.unwrap_err();
    assert!(
        matches!(error.kind(), SecurityErrorKind::Tag(t) if t == "extends"),
        "{error}"
    );
}

fn get_always_allowed_core_tests() -> Vec<(String, &'static str)> {
    [
        "{{ \"\" is empty ? \"y\" }}",
        "{{ [] is iterable ? \"y\" }}",
        "{{ null is null ? \"y\" }}",
        "{{ null is none ? \"y\" }}",
        "{{ 1 is defined ? \"y\" }}",
        "{{ 2 is even ? \"y\" }}",
        "{{ 3 is odd ? \"y\" }}",
        "{{ true is true ? \"y\" }}",
        "{{ 1 is same as(1) ? \"y\" }}",
        "{{ 4 is divisible by(2) ? \"y\" }}",
        "{{ [] is sequence ? \"y\" }}",
        "{{ {\"a\": 1} is mapping ? \"y\" }}",
    ]
    .into_iter()
    .map(|template| (template.to_owned(), template))
    .collect()
}

#[test]
fn test_sandbox_allows_always_allowed_core_tests() {
    // the safe built-in tests are always allowed in a sandbox (they carry the `always_allowed_in_sandbox` flag), so
    // they need neither an allow-list entry nor strict mode to be opted out of
    check_cases(
        "testSandboxAllowsAlwaysAllowedCoreTests",
        get_always_allowed_core_tests(),
        |template| {
            let twig = setup(true, TEMPLATES).strict().env();
            expect_output(&render_string(&twig, template), "y")
        },
    );
}

#[test]
fn test_sandbox_for_constant_test() {
    // unlike the other built-in tests, "constant" reaches into the PHP runtime, so it is not always allowed and is
    // deprecated until 4.0
    let twig = setup(
        true,
        &[("index", "{{ 1 is constant(\"PHP_INT_MAX\") ? \"y\" }}")],
    )
    .env();
    render(&twig, "index", Array::new()).unwrap();
    expect_deprecation(
        &last_deprecations(),
        "Since twig/twig 3.28: The \"constant\" test is always allowed in sandboxes, but won't be in 4.0, please enable it explicitly in your sandbox policy if needed (or enable strict mode on the security policy to opt-in to the 4.0 behavior now).",
    );
}

#[test]
fn test_sandbox_for_user_defined_test() {
    let called = Arc::new(AtomicUsize::new(0));
    let twig = setup(true, &[("index", "{{ \"x\" is unsafe ? \"y\" }}")]).env();
    let counter = Arc::clone(&called);
    twig.add_test(test_with(
        "unsafe",
        vec![P::required("value")],
        TestOptions::default(),
        move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Bool(true))
        },
    ))
    .unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "y");
    assert_eq!(called.load(Ordering::SeqCst), 1);
    expect_deprecation(
        &last_deprecations(),
        "Since twig/twig 3.28: The \"unsafe\" test is always allowed in sandboxes, but won't be in 4.0, please enable it explicitly in your sandbox policy if needed (or enable strict mode on the security policy to opt-in to the 4.0 behavior now).",
    );
}

#[test]
fn test_sandbox_allows_allow_listed_test() {
    let twig = setup(true, &[("index", "{{ \"x\" is unsafe ? \"y\" }}")])
        .tests(&["unsafe"])
        .env();
    twig.add_test(true_test("unsafe", TestOptions::default()))
        .unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "y");
}

#[test]
fn test_strict_sandbox_rejects_constant_test() {
    // "constant" is the only built-in test that is not always allowed
    let twig = setup(
        true,
        &[("index", "{{ 1 is constant(\"PHP_INT_MAX\") ? \"y\" }}")],
    )
    .strict()
    .env();
    let result = render(&twig, "index", Array::new());
    ok(expect_test(&result, "constant"));
    ok(expect_message(
        &result,
        ErrorKind::SecurityNotAllowedTest,
        "Test \"constant\" is not allowed",
    ));
}

#[test]
fn test_strict_sandbox_still_allows_allow_listed_test() {
    let twig = setup(
        true,
        &[(
            "index",
            "{{ 1 is constant(\"PHP_INT_MAX\") ? \"y\" : \"n\" }}",
        )],
    )
    .strict()
    .tests(&["constant"])
    .env();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "n");
}

#[test]
fn test_strict_sandbox_rejects_user_defined_test() {
    let twig = setup(true, &[("index", "{{ \"x\" is unsafe ? \"y\" }}")])
        .strict()
        .env();
    twig.add_test(true_test("unsafe", TestOptions::default()))
        .unwrap();
    let result = render(&twig, "index", Array::new());
    ok(expect_test(&result, "unsafe"));
    ok(expect_message(
        &result,
        ErrorKind::SecurityNotAllowedTest,
        "Test \"unsafe\" is not allowed",
    ));
}

#[test]
fn test_implicit_boolean_coercion_does_not_require_allow_listing_tests() {
    // `{% if %}`, ternary, `?:`, `??`, and `|default` conditions are coerced to a boolean through
    // `true`/`defined`/`null` tests that the compiler injects; the template author never wrote them and they are
    // always allowed, so they must work even in strict mode with no test allow-listed
    let template =
        "{% if x %}a{% endif %}{{ y ? 'b' : 'c' }}{{ z ?: 'd' }}{{ w ?? 'e' }}{{ v|default('f') }}";
    let twig = setup(true, &[("index", template)])
        .tags(&["if"])
        .filters(&["default"])
        .strict()
        .env();
    assert_eq!(
        render(
            &twig,
            "index",
            ctx(&[("x", Value::Bool(true)), ("y", Value::Bool(false))])
        )
        .unwrap(),
        "acdef"
    );
}

#[test]
fn test_strict_sandbox_rejected_test_carries_source_and_line() {
    let twig = setup(
        true,
        &[("index", "{{ 1 }}\n{{ 1 is constant('PHP_INT_MAX') ? 'y' }}")],
    )
    .strict()
    .env();
    let error = render(&twig, "index", Array::new()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SecurityNotAllowedTest, "{error}");
    assert!(
        matches!(SecurityError::of(&error).map(SecurityError::kind), Some(SecurityErrorKind::Test(t)) if t == "constant")
    );
    assert_eq!(error.template_line(), 2);
    assert_eq!(error.source_context().map(Source::name), Some("index"));
}

#[test]
fn test_strict_mode_rejects_tests_via_setter() {
    let policy = SecurityPolicy::new();
    policy.set_strict(true);
    let result = policy.check_security(&[], &[], &[], &[b"empty"], &mut Vec::<Deprecation>::new());
    compare_policy_with_oracle("testStrictModeRejectsTestsViaSetter", &result);
    let error = result.unwrap_err();
    assert!(
        matches!(error.kind(), SecurityErrorKind::Test(_)),
        "{error}"
    );
}

#[test]
fn test_allowed_tests_can_be_updated_via_setter() {
    let policy = SecurityPolicy::new();
    policy.set_strict(true);
    policy.set_allowed_tests(["empty"]);
    let mut deprecations: Vec<Deprecation> = Vec::new();
    // does not throw
    policy
        .check_security(&[], &[], &[], &[b"empty"], &mut deprecations)
        .unwrap();
    let result = policy.check_security(&[], &[], &[], &[b"null"], &mut deprecations);
    compare_policy_with_oracle("testAllowedTestsCanBeUpdatedViaSetter", &result);
    let error = result.unwrap_err();
    assert!(
        matches!(error.kind(), SecurityErrorKind::Test(t) if t == "null"),
        "{error}"
    );
}

#[test]
fn test_sandbox_with_inheritance() {
    let twig = setup(true, TEMPLATES).tags(&["extends", "block"]).env();
    let result = render(&twig, "1_child", Array::new());
    ok(violation(&result).map(|_| ()));
    ok(expect_message(
        &result,
        ErrorKind::SecurityNotAllowedFilter,
        "Filter \"json_encode\" is not allowed in \"1_child\" at line 3.",
    ));
}

#[test]
fn test_sandbox_globally_set() {
    let twig = setup(false, TEMPLATES).env();
    // Sandbox does nothing if it is disabled globally
    assert_eq!(render(&twig, "1_basic", params()).unwrap(), "FOO");
}

#[test]
fn test_sandbox_unallowed_property_accessor() {
    let twig = setup(true, TEMPLATES).env();
    ok(expect_property(
        &render(&twig, "1_basic1", ctx(&[("obj", MagicObject::value())])),
        "Twig\\Tests\\Extension\\MagicObject",
        "foo",
    ));
}

#[test]
fn test_sandbox_unallowed_array_index_accessor() {
    let twig = setup(true, TEMPLATES).env();
    // ArrayObject and other internal array-like classes are exempted from sandbox restrictions
    let mut data = Array::new();
    data.insert("foo", "bar");
    sub_scenario("1");
    assert_eq!(
        render(
            &twig,
            "1_array_like",
            ctx(&[(
                "array_like",
                Value::Object(ArrayObject::new(data).into_ref())
            )])
        )
        .unwrap(),
        "bar"
    );
    sub_scenario("2");
    ok(expect_property(
        &render(&twig, "1_array_like", params()),
        "Twig\\Tests\\Extension\\ArrayLikeObject",
        "foo",
    ));
}

fn provide_non_string_array_access_keys() -> Vec<(String, (&'static str, &'static str))> {
    vec![
        // Float key: the one that triggers the implicit conversion deprecation on PHP 8.1+ before the fix.
        ("float key", ("{{ obj[3.14] }}", "3")),
        // Bool keys: do not deprecate today but serve as regression guards and exercise the same coercion branch.
        ("true key", ("{{ obj[true] }}", "1")),
        ("false key", ("{{ obj[false] }}", "0")),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case))
    .collect()
}

#[test]
fn test_sandbox_non_string_key_access_does_not_trigger_implicit_conversion_deprecation() {
    check_cases(
        "testSandboxNonStringKeyAccessDoesNotTriggerImplicitConversionDeprecation",
        provide_non_string_array_access_keys(),
        |(template, expected_key)| {
            // new Environment($loader): Twig's default options
            let twig = Environment::new(loader(&[("t", template)]));
            // E_DEPRECATED is promoted to an exception upstream; here every PHP deprecation is recorded and must not
            // appear
            twig.set_diagnostics_policy(DiagnosticsPolicy::Collect);
            let policy = SecurityPolicy::new();
            policy.set_allowed_filters(["escape"]);
            twig.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), true)))
                .unwrap();
            let result = twig.render("t", ctx(&[("obj", NullArrayAccess::value())]));
            let php_deprecations: Vec<String> = twig
                .take_notices()
                .into_iter()
                .filter_map(|notice| match notice {
                    twig::runtime::Notice::Php(d) => Some(d.to_string()),
                    _ => None,
                })
                .collect();
            if !php_deprecations.is_empty() {
                return Err(format!("PHP diagnostics: {php_deprecations:?}"));
            }
            expect_property(&result, "ArrayAccess@anonymous", expected_key)
        },
    );
}

#[test]
fn test_if_sand_box_is_disabled_after_syntax_error_legacy() {
    let twig = setup(false, TEMPLATES).env();
    let error = render(&twig, "1_syntax_error_wrapper_legacy", params()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Syntax, "{error}");
    assert!(!is_sandboxed(&twig));
}

#[test]
fn test_if_sand_box_is_disabled_after_syntax_error() {
    let twig = setup(false, TEMPLATES).env();
    let error = render(&twig, "1_syntax_error_wrapper", params()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Syntax, "{error}");
    assert!(!is_sandboxed(&twig));
}

fn with_string_loader(setup: Setup) -> Environment {
    let twig = setup.env();
    twig.add_extension(Arc::new(StringLoaderExtension::new()))
        .unwrap();
    twig
}

#[test]
fn test_sandbox_globally_false_unallowed_filter_with_include_template_from_string_sandboxed() {
    let twig = with_string_loader(setup(false, TEMPLATES));
    ok(expect_filter(
        &render(
            &twig,
            "1_basic2_include_template_from_string_sandboxed",
            params(),
        ),
        "upper",
    ));
}

#[test]
fn test_sandbox_globally_true_unallowed_filter_with_include_template_from_string_sandboxed() {
    let twig =
        with_string_loader(setup(true, TEMPLATES).functions(&["include", "template_from_string"]));
    ok(expect_filter(
        &render(
            &twig,
            "1_basic2_include_template_from_string_sandboxed",
            params(),
        ),
        "upper",
    ));
}

#[test]
fn test_sandbox_globally_false_unallowed_filter_with_include_template_from_string_not_sandboxed() {
    let twig = with_string_loader(setup(false, TEMPLATES));
    assert_eq!(
        render(&twig, "1_basic2_include_template_from_string", params()).unwrap(),
        "FABIEN"
    );
}

#[test]
fn test_sandbox_globally_true_unallowed_filter_with_include_template_from_string_not_sandboxed() {
    let twig =
        with_string_loader(setup(true, TEMPLATES).functions(&["include", "template_from_string"]));
    ok(expect_filter(
        &render(&twig, "1_basic2_include_template_from_string", params()),
        "upper",
    ));
}

#[test]
fn test_sandbox_unallowed_filter() {
    let twig = setup(true, TEMPLATES).env();
    ok(expect_filter(&render(&twig, "1_basic2", params()), "upper"));
}

#[test]
fn test_sandbox_unallowed_tag() {
    let twig = setup(true, TEMPLATES).env();
    ok(expect_tag(&render(&twig, "1_basic3", params()), "if"));
}

#[test]
fn test_sandbox_unallowed_property() {
    let twig = setup(true, TEMPLATES).env();
    ok(expect_property(
        &render(&twig, "1_basic4", params()),
        FOO_OBJECT,
        "bar",
    ));
}

#[test]
fn test_sandbox_allowed_magic_call_method() {
    let twig = setup(true, &[("index", "{{ o.hello }}")])
        .methods(&[(MAGIC_CALL_OBJECT, &["hello"])])
        .env();
    // Sandbox allows a virtual method routed through __call() when its name is allowed
    assert_eq!(
        render(&twig, "index", ctx(&[("o", MagicCallObject::value())])).unwrap(),
        "call:hello"
    );
}

#[test]
fn test_sandbox_unallowed_magic_call_method() {
    let twig = setup(true, &[("index", "{{ o.hello }}")]).env();
    ok(expect_property(
        &render(&twig, "index", ctx(&[("o", MagicCallObject::value())])),
        MAGIC_CALL_OBJECT,
        "hello",
    ));
}

#[test]
fn test_sandbox_unallowed_magic_call_method_with_method_syntax() {
    let twig = setup(true, &[("index", "{{ o.hello() }}")]).env();
    ok(expect_method(
        &render(&twig, "index", ctx(&[("o", MagicCallObject::value())])),
        MAGIC_CALL_OBJECT,
        "hello",
    ));
}

fn get_markup_constructor_templates() -> Vec<(String, &'static str)> {
    vec![
        (
            "context value",
            "{{ m.__construct(\"<script>\", \"UTF-8\") }}",
        ),
        ("mixed case", "{{ m.__CONSTRUCT(\"<script>\", \"UTF-8\") }}"),
        (
            "captured value",
            "{% set c %}safe{% endset %}{{ c.__construct(\"<script>\", \"UTF-8\") }}",
        ),
    ]
    .into_iter()
    .map(|(name, template)| (name.to_owned(), template))
    .collect()
}

#[test]
fn test_sandbox_does_not_allow_calling_the_markup_constructor() {
    check_cases(
        "testSandboxDoesNotAllowCallingTheMarkupConstructor",
        get_markup_constructor_templates(),
        |template| {
            let markup = Markup::new_value(PhpString::from("<b>safe</b>"), "UTF-8");
            let twig = setup(true, &[("index", template)]).tags(&["set"]).env();
            expect_method(
                &render(&twig, "index", ctx(&[("m", markup.clone())])),
                "Twig\\Markup",
                "__construct",
            )?;
            let Value::Object(object) = &markup else {
                unreachable!()
            };
            let content = object
                .to_php_string(&mut twig::value::Ignore)
                .map_err(|e| format!("{e:?}"))?;
            if content.as_bytes() == b"<b>safe</b>" {
                Ok(())
            } else {
                Err(format!("the markup changed: {content:?}"))
            }
        },
    );
}

#[test]
fn test_sandbox_allows_calling_markup_methods() {
    let twig = setup(true, &[("index", "{{ m.count }}-{{ m.getCharset() }}")]).env();
    assert_eq!(
        render(
            &twig,
            "index",
            ctx(&[("m", Markup::new_value(PhpString::from("<b>"), "UTF-8"))])
        )
        .unwrap(),
        "3-UTF-8"
    );
}

#[test]
fn test_sandbox_allowing_call_literal_does_not_allow_magic_call_method() {
    let twig = setup(true, &[("index", "{{ o.hello }}")])
        .methods(&[(MAGIC_CALL_OBJECT, &["__call"])])
        .env();
    // Sandbox does not allow every virtual method just because "__call" is allowed
    ok(expect_property(
        &render(&twig, "index", ctx(&[("o", MagicCallObject::value())])),
        MAGIC_CALL_OBJECT,
        "hello",
    ));
}

#[test]
fn test_sandbox_falls_back_to_magic_call_method_for_unallowed_property() {
    let twig = setup(true, &[("index", "{{ o.secret }}")])
        .methods(&[(MAGIC_CALL_OBJECT, &["secret"])])
        .env();
    // Sandbox falls back to __call() when a real property is not allowed but the method is
    assert_eq!(
        render(&twig, "index", ctx(&[("o", MagicCallObject::value())])).unwrap(),
        "call:secret"
    );
}

// ---- __toString --------------------------------------------------------------------------------------------------

fn get_sandbox_unallowed_to_string_tests() -> Vec<(String, &'static str)> {
    [
        ("simple", "{{ obj }}"),
        ("object_from_array", "{{ arr.obj }}"),
        ("object_chain", "{{ obj.anotherFooObject }}"),
        ("filter", "{{ obj|upper }}"),
        ("filter_from_array", "{{ arr.obj|upper }}"),
        ("function", "{{ random(obj) }}"),
        ("function_from_array", "{{ random(arr.obj) }}"),
        ("function_and_filter", "{{ random(obj|upper) }}"),
        (
            "function_and_filter_from_array",
            "{{ random(arr.obj|upper) }}",
        ),
        (
            "object_chain_and_filter",
            "{{ obj.anotherFooObject|upper }}",
        ),
        (
            "object_chain_and_function",
            "{{ random(obj.anotherFooObject) }}",
        ),
        ("concat", "{{ obj ~ \"\" }}"),
        ("concat_again", "{{ \"\" ~ obj }}"),
        (
            "object_in_arguments",
            "{{ \"__toString\"|replace({\"__toString\": obj}) }}",
        ),
        ("object_in_array", "{{ [12, \"foo\", obj]|join(\", \") }}"),
        ("object_in_array_var", "{{ some_array|join(\", \") }}"),
        (
            "object_in_array_nested",
            "{{ [12, \"foo\", [12, \"foo\", obj]]|join(\", \") }}",
        ),
        (
            "object_in_array_var_nested",
            "{{ [12, \"foo\", some_array]|join(\", \") }}",
        ),
        (
            "object_in_array_dynamic_key",
            "{{ {(obj): \"foo\"}|join(\", \") }}",
        ),
        (
            "object_in_array_dynamic_key_nested",
            "{{ {\"foo\": { (obj): \"foo\" }}|join(\", \") }}",
        ),
        ("context", "{{ _context|join(\", \") }}"),
        (
            "spread_array_operator",
            "{{ [1, 2, ...[5, 6, 7, obj]]|join(\",\") }}",
        ),
        (
            "spread_array_operator_var",
            "{{ [1, 2, ...some_array]|join(\",\") }}",
        ),
        (
            "spread_iterator_in_function_args",
            "{{ [\"x\", ...iterator]|join(\",\") }}",
        ),
        ("iterator_in_join", "{{ iterator|join(\", \") }}"),
        (
            "iterator_nested_in_join",
            "{{ iterator_nested|join(\", \") }}",
        ),
        (
            "iterator_in_replace",
            "{{ \"__toString\"|replace(iterator_map) }}",
        ),
        // 'recursion' => ['{{ recursion|join(", ") }}']: not portable (see the module documentation)
        ("ternary_print", "{{ true ? obj : \"\" }}"),
        ("ternary_filter_input", "{{ (true ? obj : \"\")|upper }}"),
        ("elvis_filter_input", "{{ (obj ?: \"\")|upper }}"),
        ("nullcoalesce_filter_input", "{{ (obj ?? \"\")|upper }}"),
        (
            "function_arg_with_ternary",
            "{{ random(true ? obj : \"\") }}",
        ),
        (
            "filter_arg_with_ternary",
            "{{ \"%s\"|format(true ? obj : \"\") }}",
        ),
        (
            "matches_in_print",
            "{{ obj matches \"/foo/\" ? \"1\" : \"0\" }}",
        ),
        ("equal_in_print", "{{ obj == \"x\" ? \"1\" : \"0\" }}"),
        ("equal_in_if", "{% if obj == \"x\" %}LEAK{% endif %}"),
        ("notequal_in_if", "{% if obj != \"x\" %}LEAK{% endif %}"),
        (
            "spaceship_in_if",
            "{% if (obj <=> \"x\") == 0 %}LEAK{% endif %}",
        ),
        ("less_in_if", "{% if obj < \"B\" %}LEAK{% endif %}"),
        ("greater_in_if", "{% if obj > \"A\" %}LEAK{% endif %}"),
        ("lessequal_in_if", "{% if obj <= \"z\" %}LEAK{% endif %}"),
        ("greaterequal_in_if", "{% if obj >= \"a\" %}LEAK{% endif %}"),
        ("concat_left_in_if", "{% if obj ~ \"\" %}LEAK{% endif %}"),
        ("concat_right_in_if", "{% if \"\" ~ obj %}LEAK{% endif %}"),
        ("range_left", "{% for x in obj..1 %}LEAK{% endfor %}"),
        ("range_right", "{% for x in 1..obj %}LEAK{% endfor %}"),
        (
            "in_array_right",
            "{% if \"needle\" in [obj] %}LEAK{% endif %}",
        ),
        (
            "in_array_left",
            "{% if obj in [\"needle\"] %}LEAK{% endif %}",
        ),
        (
            "notin_array_right",
            "{% if \"needle\" not in [obj] %}LEAK{% endif %}",
        ),
        (
            "notin_array_left",
            "{% if obj not in [\"needle\"] %}LEAK{% endif %}",
        ),
        (
            "in_iterator_right",
            "{% if \"needle\" in iterator %}LEAK{% endif %}",
        ),
        (
            "notin_iterator_right",
            "{% if \"needle\" not in iterator %}LEAK{% endif %}",
        ),
        ("do_tag_function_arg", "{% do my_func(obj) %}"),
        ("do_tag_filter_input", "{% do obj|upper %}"),
        ("do_tag_concat", "{% do obj ~ \"\" %}"),
        ("set_tag_filter_input", "{% set _ = obj|upper %}"),
        ("set_tag_concat", "{% set _ = obj ~ \"\" %}"),
        ("set_tag_array_dynamic_key", "{% set _ = {(obj): \"v\"} %}"),
        ("assignment_print", "{{ x = obj }}"),
        ("assignment_concat", "{{ (x = obj) ~ \"\" }}"),
        ("assignment_filter_input", "{{ (x = obj)|upper }}"),
        ("assignment_in_array", "{{ [x = obj]|join(\",\") }}"),
        (
            "sequence_destructuring_filter_input",
            "{{ ([a] = some_array)|join(\",\") }}",
        ),
        (
            "object_destructuring_filter_input",
            "{{ ({anotherFooObject: a} = obj)|join(\",\") }}",
        ),
        (
            "set_tag_array_dynamic_key_nested",
            "{% set _ = {\"foo\": {(obj): \"v\"}} %}",
        ),
        (
            "set_tag_array_dynamic_key_object_chain",
            "{% set _ = {(obj.anotherFooObject): \"v\"} %}",
        ),
        ("set_capture_print", "{% set _ %}{{ obj }}{% endset %}"),
        ("is_empty_in_if", "{% if obj is empty %}LEAK{% endif %}"),
        ("is_empty_in_print", "{{ obj is empty ? \"1\" : \"0\" }}"),
        ("method_argument", "{{ obj.foo(obj.anotherFooObject) }}"),
        (
            "filter_input_in_if",
            "{% if obj|upper == \"X\" %}LEAK{% endif %}",
        ),
        (
            "filter_arg_in_if",
            "{% if \"x\"|replace({\"x\": obj}) == \"y\" %}LEAK{% endif %}",
        ),
        (
            "function_arg_in_if",
            "{% if not random(obj) %}LEAK{% endif %}",
        ),
        (
            "filter_input_in_for",
            "{% for x in (obj|split(\",\")) %}LEAK{% endfor %}",
        ),
        (
            "function_arg_in_for",
            "{% for x in [random(obj)] %}LEAK{% endfor %}",
        ),
    ]
    .into_iter()
    .map(|(name, template)| (name.to_owned(), template))
    .collect()
}

#[test]
fn test_sandbox_unallowed_to_string() {
    check_cases(
        "testSandboxUnallowedToString",
        get_sandbox_unallowed_to_string_tests(),
        |template| {
            let twig = setup(true, &[("index", template)])
                .tags(&["if", "do", "for", "set"])
                .filters(&["upper", "join", "replace", "format", "split"])
                .methods(&[(FOO_OBJECT, &["getAnotherFooObject"])])
                .functions(&["random", "range", "my_func"])
                .env();
            twig.add_function(function("my_func", vec![P::required("a")], |call| {
                let a = call.arg(0).cloned().unwrap_or_default();
                to_string(call, &a)
            }))
            .unwrap();
            expect_to_string(&render(&twig, "index", params()))
        },
    );
}

/// `static fn () => new FooObject()`.
fn make_obj(_call: &mut Call<'_>) -> CallResult {
    Ok(FooObject::value())
}

#[test]
fn test_sandbox_blocks_to_string_on_function_return() {
    let twig = setup(true, &[("index", "{{ make_obj() }}")])
        .functions(&["make_obj"])
        .env();
    twig.add_function(function("make_obj", vec![], make_obj))
        .unwrap();
    ok(expect_to_string(&render(&twig, "index", Array::new())));
}

#[test]
fn test_sandbox_blocks_to_string_on_filter_return() {
    let twig = setup(true, &[("index", "{{ \"x\"|to_obj }}")])
        .filters(&["to_obj"])
        .env();
    // static fn () => new FooObject(): the filtered value is an extra argument PHP ignores
    twig.add_filter(filter_with(
        "to_obj",
        vec![],
        FilterOptions::default(),
        make_obj,
    ))
    .unwrap();
    ok(expect_to_string(&render(&twig, "index", Array::new())));
}

#[test]
fn test_sandbox_blocks_to_string_on_dynamic_attribute_name() {
    let twig = setup(true, &[("index", "{{ arr[obj] }}")])
        .strict_variables()
        .methods(&[(FOO_OBJECT, &["getAnotherFooObject"])])
        .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_sandbox_blocks_to_string_on_dynamic_macro_name() {
    let twig = setup(
        true,
        &[(
            "index",
            "{% import _self as macros %}\n{% macro foo() %}foo{% endmacro %}\n{{ macros.(obj)() }}",
        )],
    )
    .tags(&["import", "macro"])
    .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_sandbox_blocks_to_string_on_include_template_name() {
    let twig = setup(true, &[("index", "{% include obj %}")])
        .tags(&["include"])
        .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_sandbox_blocks_to_string_on_extends_template_name() {
    let twig = setup(true, &[("index", "{% extends obj %}")])
        .tags(&["extends"])
        .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_sandbox_blocks_to_string_on_block_function_template_name() {
    let twig = setup(true, &[("index", "{{ block(\"content\", obj) }}")])
        .functions(&["block"])
        .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_sandbox_blocks_to_string_on_embed_template_name() {
    let twig = setup(true, &[("index", "{% embed obj %}{% endembed %}")])
        .tags(&["embed", "extends"])
        .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_sandbox_blocks_to_string_on_dynamic_embed_parent() {
    let twig = setup(
        true,
        &[
            ("index", "{% embed obj ~ \".twig\" %}{% endembed %}"),
            ("foo.twig", "LEAK"),
        ],
    )
    .tags(&["embed", "extends"])
    .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_sandbox_checks_callables_in_dynamic_embed_parent() {
    let cases = vec![
        (
            "filter",
            (
                "{% embed \"LEAK\"|lower %}{% endembed %}",
                ErrorKind::SecurityNotAllowedFilter,
            ),
        ),
        (
            "function",
            (
                "{% embed leak() %}{% endembed %}",
                ErrorKind::SecurityNotAllowedFunction,
            ),
        ),
        (
            "nested function",
            (
                "{% embed \"\" ~ leak() %}{% endembed %}",
                ErrorKind::SecurityNotAllowedFunction,
            ),
        ),
        (
            "test",
            (
                "{% embed 1 is leak ? \"leak\" : \"none\" %}{% endembed %}",
                ErrorKind::SecurityNotAllowedTest,
            ),
        ),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case));
    check_cases(
        "testSandboxChecksCallablesInDynamicEmbedParent",
        cases,
        |(template, exception)| {
            let twig = setup(true, &[("index", template), ("leak", "LEAK")])
                .tags(&["embed", "extends"])
                .strict()
                .env();
            twig.add_function(function("leak", vec![], |_| Ok(Value::from("leak"))))
                .unwrap();
            twig.add_test(test_with("leak", vec![], TestOptions::default(), |_| {
                Ok(Value::Bool(true))
            }))
            .unwrap();
            match render(&twig, "index", Array::new()) {
                Err(e) if e.kind() == *exception => Ok(()),
                other => Err(format!(
                    "expected {}, got {}",
                    exception.class_name(),
                    describe(&other)
                )),
            }
        },
    );
}

#[test]
fn test_sandbox_blocks_to_string_on_is_constant_test_argument() {
    let twig = setup(
        true,
        &[("index", "{% if \"x\" is constant(obj) %}LEAK{% endif %}")],
    )
    .tags(&["if"])
    .tests(&["constant"])
    .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_sandbox_blocks_to_string_on_deprecated_message() {
    let twig = setup(true, &[("index", "{% deprecated obj %}")])
        .tags(&["deprecated"])
        .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_sandbox_keeps_self_import_shortcut() {
    let template = "{% macro local_lower(s) %}{{ s|lower }}{% endmacro %}{% from _self import local_lower %}{{ local_lower('A') }}";
    let twig = setup(true, &[("index", template)])
        .tags(&["from", "macro", "import"])
        .filters(&["lower"])
        .env();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "a");
}

fn get_sandbox_allowed_to_string_tests() -> Vec<(String, (&'static str, &'static str))> {
    [
        (
            "constant_test",
            "{{ obj is constant(\"PHP_INT_MAX\") }}",
            "",
        ),
        (
            "set_object",
            "{% set a = obj.anotherFooObject %}{{ a.foo }}",
            "foo",
        ),
        ("do_object_discarded", "{% do obj %}", ""),
        (
            "set_object_assigned",
            "{% set a = obj %}{{ a is defined ? \"1\" : \"0\" }}",
            "1",
        ),
        (
            "do_assignment_discarded",
            "{% do x = obj %}{{ x is same as(obj) ? \"1\" : \"0\" }}",
            "1",
        ),
        (
            "do_sequence_destructuring_discarded",
            "{% do [a] = some_array %}{{ a }}",
            "5",
        ),
        ("is_defined1", "{{ obj.anotherFooObject is defined }}", "1"),
        ("is_defined2", "{{ magic.foo is defined }}", ""),
        ("is_null", "{{ obj is null }}", ""),
        ("is_sameas", "{{ obj is same as(obj) }}", "1"),
        ("is_sameas_no_brackets", "{{ obj is same as obj }}", "1"),
        (
            "is_sameas_from_array",
            "{{ arr.obj is same as(arr.obj) }}",
            "1",
        ),
        (
            "is_sameas_from_array_no_brackets",
            "{{ arr.obj is same as arr.obj }}",
            "1",
        ),
        (
            "is_sameas_from_another_method",
            "{{ obj.anotherFooObject is same as(obj.anotherFooObject) }}",
            "",
        ),
        (
            "is_sameas_from_another_method_no_brackets",
            "{{ obj.anotherFooObject is same as obj.anotherFooObject }}",
            "",
        ),
    ]
    .into_iter()
    .map(|(name, template, output)| (name.to_owned(), (template, output)))
    .collect()
}

#[test]
fn test_sandbox_allowed_to_string() {
    check_cases(
        "testSandboxAllowedToString",
        get_sandbox_allowed_to_string_tests(),
        |(template, output)| {
            let twig = setup(true, &[("index", template)])
                .tags(&["set", "do"])
                .methods(&[(FOO_OBJECT, &["foo", "getAnotherFooObject"])])
                .tests(&["constant"])
                .env();
            expect_output(&render(&twig, "index", params()), output)
        },
    );
}

#[test]
fn test_sandbox_allow_method_to_string() {
    let twig = setup(true, TEMPLATES)
        .methods(&[(FOO_OBJECT, &["__toString"])])
        .env();
    let counters = Counters::new();
    // Sandbox allow some methods
    assert_eq!(
        render(&twig, "1_basic5", params_with(&counters)).unwrap(),
        "foo"
    );
    // Sandbox only calls method once
    assert_eq!(counters.to_string(), 1);
}

#[test]
fn test_sandbox_allows_array_dynamic_key_when_to_string_allowed() {
    let twig = setup(
        true,
        &[(
            "index",
            "{% set arr = {(obj): \"v\", (obj.anotherFooObject): \"v2\"} %}{{ arr|keys|join(\",\") }}",
        )],
    )
    .tags(&["set"])
    .filters(&["join", "keys"])
    .methods(&[(FOO_OBJECT, &["__toString", "getAnotherFooObject"])])
    .env();
    assert_eq!(render(&twig, "index", params()).unwrap(), "foo");
}

#[test]
fn test_sandbox_allow_method_to_string_disabled() {
    let twig = setup(false, TEMPLATES).env();
    let counters = Counters::new();
    // Sandbox allows __toString when sandbox disabled
    assert_eq!(
        render(&twig, "1_basic5", params_with(&counters)).unwrap(),
        "foo"
    );
    // Sandbox only calls method once
    assert_eq!(counters.to_string(), 1);
}

#[test]
fn test_sandbox_unallowed_function() {
    let twig = setup(true, TEMPLATES).env();
    ok(expect_function(
        &render(&twig, "1_basic7", params()),
        "cycle",
    ));
}

#[test]
fn test_sandbox_unallowed_range_operator() {
    let twig = setup(true, TEMPLATES).env();
    ok(expect_function(
        &render(&twig, "1_range_operator", params()),
        "range",
    ));
}

#[test]
fn test_sandbox_allow_method_foo() {
    let twig = setup(true, TEMPLATES)
        .methods(&[(FOO_OBJECT, &["foo"])])
        .env();
    let counters = Counters::new();
    assert_eq!(
        render(&twig, "1_basic1", params_with(&counters)).unwrap(),
        "foo"
    );
    assert_eq!(counters.foo(), 1);
}

#[test]
fn test_sandbox_allow_filter() {
    let twig = setup(true, TEMPLATES).filters(&["upper"]).env();
    assert_eq!(render(&twig, "1_basic2", params()).unwrap(), "FABIEN");
}

#[test]
fn test_sandbox_allow_tag() {
    let twig = setup(true, TEMPLATES).tags(&["if"]).env();
    assert_eq!(render(&twig, "1_basic3", params()).unwrap(), "foo");
}

#[test]
fn test_sandbox_allow_property() {
    let twig = setup(true, TEMPLATES)
        .properties(&[(FOO_OBJECT, &["bar"])])
        .env();
    assert_eq!(render(&twig, "1_basic4", params()).unwrap(), "bar");
}

#[test]
fn test_sandbox_allow_destructuring() {
    let template = "{% do {bar: x, foo: y} = obj %}{{ x }}-{{ y }}";
    let twig = setup(true, &[("index", template)])
        .tags(&["do"])
        .methods(&[(FOO_OBJECT, &["foo"])])
        .properties(&[(FOO_OBJECT, &["bar"])])
        .env();
    // Sandbox allows destructuring when properties and methods are allowed
    assert_eq!(render(&twig, "index", params()).unwrap(), "bar-foo");
}

#[test]
fn test_sandbox_unallowed_destructuring_property() {
    let template = "{% do {bar: x} = obj %}{{ x }}";
    let twig = setup(true, &[("index", template)]).tags(&["do"]).env();
    ok(expect_property(
        &render(&twig, "index", params()),
        FOO_OBJECT,
        "bar",
    ));
}

#[test]
fn test_sandbox_unallowed_destructuring_method() {
    let template = "{% do {foo: y} = obj %}{{ y }}";
    let twig = setup(true, &[("index", template)])
        .tags(&["do"])
        .properties(&[(FOO_OBJECT, &["foo"])])
        .env();
    ok(expect_method(
        &render(&twig, "index", params()),
        FOO_OBJECT,
        "foo",
    ));
}

#[test]
fn test_sandbox_allow_function() {
    let twig = setup(true, TEMPLATES).functions(&["cycle"]).env();
    assert_eq!(render(&twig, "1_basic7", params()).unwrap(), "bar");
}

#[test]
fn test_sandbox_allow_range_operator() {
    let twig = setup(true, TEMPLATES).functions(&["range"]).env();
    assert_eq!(render(&twig, "1_range_operator", params()).unwrap(), "1");
}

#[test]
fn test_sandbox_allow_methods_case_insensitive() {
    for name in ["getfoobar", "getFoobar", "getFooBar"] {
        let twig = setup(true, TEMPLATES)
            .methods(&[(FOO_OBJECT, &[name])])
            .env();
        let counters = Counters::new();
        // Sandbox allow methods in a case-insensitive way
        sub_scenario(&format!("{name}/1_basic8"));
        assert_eq!(
            render(&twig, "1_basic8", params_with(&counters)).unwrap(),
            "foobarfoobar"
        );
        // Sandbox only calls method once
        assert_eq!(counters.get_foo_bar(), 2);
        // Sandbox allow methods via shortcut names (ie. without get/set)
        sub_scenario(&format!("{name}/1_basic9"));
        assert_eq!(render(&twig, "1_basic9", params()).unwrap(), "foobarfoobar");
    }
}

#[test]
fn test_sandbox_locally_set_for_an_include() {
    let twig = setup(
        false,
        &[
            (
                "2_basic",
                "{{ obj.foo }}{% include \"2_included\" %}{{ obj.foo }}",
            ),
            (
                "2_included",
                "{% if obj.foo %}{{ obj.foo|upper }}{% endif %}",
            ),
        ],
    )
    .env();
    // Sandbox does nothing if disabled globally and sandboxed not used for the include
    sub_scenario("1");
    assert_eq!(render(&twig, "2_basic", params()).unwrap(), "fooFOOfoo");

    let twig = setup(
        true,
        &[
            ("3_basic", "{{ include(\"3_included\", sandboxed: true) }}"),
            ("3_included", "{% if true %}{{ \"foo\"|upper }}{% endif %}"),
        ],
    )
    .functions(&["include"])
    .env();
    sub_scenario("2");
    ok(expect_tag(&render(&twig, "3_basic", params()), "if"));
}

#[test]
fn test_macros_in_a_sandbox() {
    let twig = setup(
        true,
        &[(
            "index",
            "{%- import _self as macros %}\n\n{%- macro test(text) %}<p>{{ text }}</p>{% endmacro %}\n\n{{- macros.test('username') }}",
        )],
    )
    .autoescape_html()
    .tags(&["macro", "import"])
    .filters(&["escape"])
    .env();
    assert_eq!(
        render(&twig, "index", Array::new()).unwrap(),
        "<p>username</p>"
    );
}

#[test]
fn test_self_macro_reference_with_string_literal_does_not_inject_php() {
    let twig = setup(
        true,
        &[(
            "index",
            "{{ _self.('foo + 1; trigger_error(\"BAD-MACRO-REF\") //')() }}",
        )],
    )
    .env();
    // No PHP from the template literal must execute: the render fails or not, with no diagnostics raised
    let _ = render(&twig, "index", Array::new());
    assert!(
        twig.take_notices()
            .iter()
            .all(|n| !n.to_string().contains("BAD-MACRO-REF"))
    );
}

#[test]
fn test_imported_template_macro_reference_with_bad_identifier_does_not_inject_php() {
    let payload =
        "{% import \"m\" as m %}{{ m.('foo + 1; trigger_error(\"BAD-IMPORT-REF\") //')() }}";
    let twig = setup(
        true,
        &[
            ("index", payload),
            ("m", "{% macro greet() %}hi{% endmacro %}"),
        ],
    )
    .tags(&["import"])
    .env();
    let _ = render(&twig, "index", Array::new());
    assert!(
        twig.take_notices()
            .iter()
            .all(|n| !n.to_string().contains("BAD-IMPORT-REF"))
    );
}

#[test]
fn test_self_macro_reference_with_valid_identifier_still_works() {
    let twig = setup(
        true,
        &[(
            "index",
            "{%- macro greet(n) %}Hi {{ n }}{% endmacro %}\n{{- _self.('greet')('World') }}",
        )],
    )
    .autoescape_html()
    .tags(&["macro"])
    .filters(&["escape"])
    .env();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "Hi World");
}

#[test]
fn test_sandbox_disabled_after_include_function_error() {
    let twig = setup(false, TEMPLATES).env();
    // An exception should be thrown for this test to be valid.
    render(&twig, "1_include", params()).unwrap_err();
    // Sandboxed include() function call should not leave Sandbox enabled when an error occurs.
    assert!(!is_sandboxed(&twig));
}

fn sandbox_tag_include_with_preloaded_template(index: &str) {
    let twig = setup(
        false,
        &[("index", index), ("included", "{{ \"hello\"|upper }}")],
    )
    .env();
    twig.load("included").unwrap();
    let result = render(&twig, "index", Array::new());
    ok(expect_filter(&result, "upper"));
    expect_deprecation(
        &last_deprecations(),
        "Since twig/twig 3.15: The \"sandbox\" tag is deprecated in \"index\" at line 1.",
    );
}

#[test]
fn test_sandbox_tag_include_with_preloaded_template() {
    sandbox_tag_include_with_preloaded_template(
        "{% sandbox %}{% include \"included\" %}{% endsandbox %}",
    );
}

#[test]
fn test_sandbox_tag_include_ignore_missing_with_preloaded_template() {
    sandbox_tag_include_with_preloaded_template(
        "{% sandbox %}{% include \"included\" ignore missing %}{% endsandbox %}",
    );
}

#[test]
fn test_sandbox_with_no_closure_filter() {
    let twig = setup(
        true,
        &[(
            "index",
            "{{ [\"foo\", \"bar\", \"\"]|filter(\"trim\")|join(\", \") }}",
        )],
    )
    .autoescape_html()
    .filters(&["escape", "filter", "join"])
    .env();
    ok(expect_message(
        &render(&twig, "index", Array::new()),
        ErrorKind::Runtime,
        "The callable passed to the \"filter\" filter must be a Closure in sandbox mode in \"index\" at line 1.",
    ));
}

#[test]
fn test_sandbox_with_closure_filter() {
    let twig = setup(
        true,
        &[(
            "index",
            "{{ [\"foo\", \"bar\", \"\"]|filter(v => v != \"\")|join(\", \") }}",
        )],
    )
    .autoescape_html()
    .filters(&["escape", "filter", "join"])
    .env();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "foo, bar");
}

#[test]
fn test_multiple_class_matches_via_inheritance_in_allowed_methods() {
    let child = "Twig\\Tests\\Extension\\ChildClass";
    let parent = "Twig\\Tests\\Extension\\ParentClass";
    let twig_child_first = setup(true, TEMPLATES)
        .methods(&[(child, &["ChildMethod"]), (parent, &["ParentMethod"])])
        .env();
    let twig_parent_first = setup(true, TEMPLATES)
        .methods(&[(parent, &["ParentMethod"]), (child, &["ChildMethod"])])
        .env();
    for (n, (twig, template, failure)) in [
        (
            &twig_child_first,
            "1_childobj_childmethod",
            "This test case is malfunctioning as even the child class method which comes first is not being allowed.",
        ),
        (
            &twig_parent_first,
            "1_childobj_parentmethod",
            "This test case is malfunctioning as even the parent class method which comes first is not being allowed.",
        ),
        (
            &twig_parent_first,
            "1_childobj_childmethod",
            "checkMethodAllowed is exiting prematurely after matching a parent class and not seeing a method allowed on a child class later in the list",
        ),
        (
            &twig_child_first,
            "1_childobj_parentmethod",
            "checkMethodAllowed is exiting prematurely after matching a child class and not seeing a method allowed on its parent class later in the list",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        sub_scenario(&(n + 1).to_string());
        if let Err(e) = render(twig, template, params()) {
            panic!("{failure}: {e}");
        }
    }
}

// ---- column ------------------------------------------------------------------------------------------------------

const COLUMN_BAR: &str = "{{ [obj]|column('bar')|first }}";
const COLUMN_BAR_FOO: &str = "{{ [obj]|column('bar', 'foo')|keys|first }}";
const COLUMN_MAGIC: &str = "{{ [magic]|column('anything')|first }}";

#[test]
fn test_sandbox_allows_column_filter_on_allowed_property() {
    let twig = setup(true, &[("index", COLUMN_BAR)])
        .filters(&["column", "first"])
        .properties(&[(COLUMN_OBJECT, &["bar"])])
        .env();
    assert_eq!(
        render(&twig, "index", ctx(&[("obj", ColumnObject::value())])).unwrap(),
        "bar"
    );
}

#[test]
fn test_sandbox_blocks_column_filter_on_disallowed_property() {
    let twig = setup(true, &[("index", COLUMN_BAR)])
        .filters(&["column", "first"])
        .env();
    ok(expect_property(
        &render(&twig, "index", ctx(&[("obj", ColumnObject::value())])),
        COLUMN_OBJECT,
        "bar",
    ));
}

#[test]
fn test_sandbox_blocks_column_filter_on_disallowed_index() {
    let twig = setup(true, &[("index", COLUMN_BAR_FOO)])
        .filters(&["column", "first", "keys"])
        .properties(&[(COLUMN_OBJECT, &["bar"])])
        .env();
    ok(expect_property(
        &render(&twig, "index", ctx(&[("obj", ColumnObject::value())])),
        COLUMN_OBJECT,
        "foo",
    ));
}

#[test]
fn test_sandbox_blocks_column_filter_on_magic_getter() {
    let twig = setup(true, &[("index", COLUMN_MAGIC)])
        .filters(&["column", "first"])
        .env();
    ok(expect_property(
        &render(&twig, "index", ctx(&[("magic", MagicObject::value())])),
        "Twig\\Tests\\Extension\\MagicObject",
        "anything",
    ));
}

// ---- Stringable + Traversable ------------------------------------------------------------------------------------

fn get_stringable_traversable_bypass_templates() -> Vec<(String, &'static str)> {
    vec![
        ("join", "{{ stringable_iterator|join(\", \") }}"),
        (
            "replace",
            "{{ \"__toString\"|replace(stringable_iterator_map) }}",
        ),
    ]
    .into_iter()
    .map(|(name, template)| (name.to_owned(), template))
    .collect()
}

#[test]
fn test_sandbox_blocks_to_string_in_stringable_traversable() {
    check_cases(
        "testSandboxBlocksToStringInStringableTraversable",
        get_stringable_traversable_bypass_templates(),
        |template| {
            let twig = setup(true, &[("index", template)])
                .filters(&["join", "replace"])
                .methods(&[(STRINGABLE_TRAVERSABLE_OBJECT, &["__tostring"])])
                .env();
            expect_to_string(&render(&twig, "index", params()))
        },
    );
}

#[test]
fn test_source_policy_sandbox_blocks_to_string_in_stringable_traversable() {
    check_cases(
        "testSourcePolicySandboxBlocksToStringInStringableTraversable",
        get_stringable_traversable_bypass_templates(),
        |template| {
            let twig = setup(false, &[("index", template)])
                .filters(&["join", "replace"])
                .methods(&[(STRINGABLE_TRAVERSABLE_OBJECT, &["__tostring"])])
                .source_policy(always(true))
                .env();
            expect_to_string(&render(&twig, "index", params()))
        },
    );
}

#[test]
fn test_sandbox_allows_printing_stringable_traversable_when_to_string_allowed() {
    // Printing the container itself yields its `__toString()` value. The sandbox materialises the iterable to also
    // policy-check the elements (some consumers like `join`/`replace` would coerce them too), so the inner items must
    // not contain anything that violates the policy.
    let twig = setup(true, &[("index", "{{ obj }}")])
        .autoescape_html()
        .filters(&["escape"])
        .methods(&[(STRINGABLE_TRAVERSABLE_OBJECT, &["__tostring"])])
        .env();
    let obj = StringableTraversableObject::value(Array::from_values([
        Value::from("a"),
        Value::from("b"),
    ]));
    assert_eq!(
        render(&twig, "index", ctx(&[("obj", obj)])).unwrap(),
        "stringable-traversable"
    );
}

fn get_cyclic_traversable_templates() -> Vec<(String, &'static str)> {
    vec![
        ("join", "{{ obj|join(\",\") }}"),
        ("replace", "{{ \"x\"|replace(obj) }}"),
        ("spread", "{{ [\"a\", ...obj]|join(\",\") }}"),
    ]
    .into_iter()
    .map(|(name, template)| (name.to_owned(), template))
    .collect()
}

#[test]
fn test_sandbox_handles_cyclic_traversable_without_stack_overflow() {
    // A self-referencing IteratorAggregate must not cause the sandbox policy walker to recurse infinitely when
    // materialising the iterable. PHP itself throws a clean error when the cyclic object reaches `implode()` / string
    // coercion; the sandbox must NOT turn that into a stack overflow.
    check_cases(
        "testSandboxHandlesCyclicTraversableWithoutStackOverflow",
        get_cyclic_traversable_templates(),
        |template| {
            let twig = setup(true, &[("index", template)])
                .filters(&["join", "replace"])
                .env();
            match render(
                &twig,
                "index",
                ctx(&[("obj", CyclicTraversableObject::value())]),
            ) {
                Err(e) if e.kind() == ErrorKind::Runtime => Ok(()),
                other => Err(format!("expected a RuntimeError, got {}", describe(&other))),
            }
        },
    );
}

#[test]
fn test_source_policy_sandbox_blocks_to_string_in_traversable_join() {
    let twig = setup(false, &[("index", "{{ iterator|join(\", \") }}")])
        .filters(&["join"])
        .source_policy(always(true))
        .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

#[test]
fn test_source_policy_sandbox_blocks_to_string_in_traversable_replace() {
    let twig = setup(
        false,
        &[("index", "{{ \"__toString\"|replace(iterator_map) }}")],
    )
    .filters(&["replace"])
    .source_policy(always(true))
    .env();
    ok(expect_to_string(&render(&twig, "index", params())));
}

/// `render_traversable`: `static function ($obj)` checking that it receives the `StringableTraversableObject`.
fn render_traversable() -> TwigFunction {
    function("render_traversable", vec![P::required("obj")], |call| {
        let obj = call.arg(0).cloned().unwrap_or_default();
        match &obj {
            Value::Object(o) if o.instance_of(STRINGABLE_TRAVERSABLE_OBJECT) => {
                to_string(call, &obj)
            }
            other => Err(PhpError::new(
                ThrowableClass::RuntimeException,
                format!(
                    "Expected a StringableTraversableObject, got \"{}\".",
                    twig::value::types::get_debug_type(other)
                ),
            )
            .into()),
        }
    })
}

#[test]
fn test_sandbox_preserves_traversable_argument_identity() {
    // Regression for https://github.com/twigphp/Twig/issues/4820: a typed Traversable argument (e.g. Symfony's
    // FormView) must reach host code as-is, not as a plain array.
    let twig = setup(true, &[("index", "{{ render_traversable(obj) }}")])
        .methods(&[(STRINGABLE_TRAVERSABLE_OBJECT, &["__tostring"])])
        .env();
    twig.add_function(render_traversable()).unwrap();
    with_policy(&twig, |policy| {
        policy.set_allowed_functions(["render_traversable"])
    });
    let obj = StringableTraversableObject::value(Array::from_values([
        Value::from("a"),
        Value::from("b"),
    ]));
    assert_eq!(
        render(&twig, "index", ctx(&[("obj", obj)])).unwrap(),
        "stringable-traversable"
    );
}

#[test]
fn test_sandbox_still_blocks_disallowed_to_string_in_traversable_argument() {
    // The container is returned as-is, but yielded elements must still be policy-checked since host code can
    // string-coerce them.
    let twig = setup(
        true,
        &[("index", "{{ render_traversable(stringable_iterator) }}")],
    )
    .methods(&[(STRINGABLE_TRAVERSABLE_OBJECT, &["__tostring"])])
    .env();
    twig.add_function(function(
        "render_traversable",
        vec![P::required("obj")],
        |call| {
            let obj = call.arg(0).cloned().unwrap_or_default();
            to_string(call, &obj)
        },
    ))
    .unwrap();
    with_policy(&twig, |policy| {
        policy.set_allowed_functions(["render_traversable"])
    });
    ok(expect_to_string(&render(&twig, "index", params())));
}

// ---- PHP parameter types -----------------------------------------------------------------------------------------

/// A `safe_fn`/`unsafe_fn`: a closure with one typed parameter `$x` returning `$result(x)`.
#[derive(Clone)]
struct TypedClosure {
    parameter: P,
    ty: DeclaredType<'static>,
    to_string_result: bool,
}

impl TypedClosure {
    fn function(&self, name: &str) -> TwigFunction {
        let ty = self.ty;
        let to_string_result = self.to_string_result;
        let name_of = self.parameter.name.clone();
        let parameter_name: &'static str = Box::leak(name_of.into_boxed_str());
        function(name, vec![self.parameter.clone()], move |call| {
            let value = typed(call, 0, parameter_name, &ty)?;
            if to_string_result {
                to_string(call, &value)
            } else {
                Ok(Value::from("ok"))
            }
        })
    }
}

fn safe(parameter: &str, mask: TypeMask) -> TypedClosure {
    TypedClosure {
        parameter: P::required(parameter).string_coercion_safe(),
        ty: DeclaredType::builtin(mask),
        to_string_result: false,
    }
}

// `3.14` is the upstream test's value, not an approximation of pi
#[allow(clippy::approx_constant)]
fn get_safe_php_types_skip_to_string_wrap() -> Vec<(String, (&'static str, TypedClosure, Array))> {
    vec![
        (
            "int param",
            (
                "{{ safe_fn(n) }}",
                safe("n", TypeMask::INT),
                ctx(&[("n", Value::Int(42))]),
            ),
        ),
        (
            "float param",
            (
                "{{ safe_fn(n) }}",
                safe("n", TypeMask::FLOAT),
                ctx(&[("n", Value::Float(3.14))]),
            ),
        ),
        (
            "bool param",
            (
                "{{ safe_fn(b) }}",
                safe("b", TypeMask::BOOL),
                ctx(&[("b", Value::Bool(true))]),
            ),
        ),
        (
            // `ColumnObject` is not final: PHP reflection does not call the type safe, the argument stays wrapped
            // (ColumnObject has no __toString, so the check passes)
            "non-stringable class param",
            (
                "{{ safe_fn(obj) }}",
                TypedClosure {
                    parameter: P::required("o"),
                    ty: DeclaredType::new(&[COLUMN_OBJECT], TypeMask::NONE),
                    to_string_result: false,
                },
                ctx(&[("obj", ColumnObject::value())]),
            ),
        ),
        (
            "nullable int param with null value",
            (
                "{{ safe_fn(n) }}",
                safe("n", TypeMask::INT | TypeMask::NULL),
                ctx(&[("n", Value::Null)]),
            ),
        ),
        (
            "int|float union param",
            (
                "{{ safe_fn(n) }}",
                safe("n", TypeMask::INT | TypeMask::FLOAT),
                ctx(&[("n", Value::Int(7))]),
            ),
        ),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case))
    .collect()
}

#[test]
fn test_safe_php_param_types_skip_to_string_wrap() {
    // The sandbox visitor must not wrap arguments whose target PHP parameter type cannot implicitly coerce to string
    // (int, float, bool, non-Stringable/non-Traversable classes, ...).
    check_cases(
        "testSafePhpParamTypesSkipToStringWrap",
        get_safe_php_types_skip_to_string_wrap(),
        |(template, func, params)| {
            let twig = setup(true, &[("index", template)]).env();
            twig.add_function(func.function("safe_fn")).unwrap();
            with_policy(&twig, |policy| policy.set_allowed_functions(["safe_fn"]));
            expect_output(&render(&twig, "index", params.clone()), "ok")
        },
    );
}

fn unsafe_closure(parameter: &str, ty: DeclaredType<'static>) -> TypedClosure {
    TypedClosure {
        parameter: P::required(parameter),
        ty,
        to_string_result: true,
    }
}

const STRINGABLE: &[&str] = &["Stringable"];

fn get_unsafe_php_types_still_wrap() -> Vec<(String, TypedClosure)> {
    vec![
        (
            "untyped param",
            unsafe_closure("x", DeclaredType::builtin(TypeMask::MIXED)),
        ),
        (
            "mixed param",
            unsafe_closure("x", DeclaredType::builtin(TypeMask::MIXED)),
        ),
        ("string param", unsafe_closure("x", STRING)),
        (
            "object param",
            unsafe_closure("x", DeclaredType::builtin(TypeMask::OBJECT)),
        ),
        (
            "Stringable param",
            unsafe_closure("x", DeclaredType::new(STRINGABLE, TypeMask::NONE)),
        ),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case))
    .collect()
}

#[test]
fn test_unsafe_php_param_types_still_wrap() {
    // Conversely, an unsafe parameter type (`mixed`, untyped, `string`, `iterable`, `Stringable`, ...) must keep
    // wrapping arguments so the sandbox can still block disallowed `__toString` calls.
    check_cases(
        "testUnsafePhpParamTypesStillWrap",
        get_unsafe_php_types_still_wrap(),
        |func| {
            let twig = setup(true, &[("index", "{{ unsafe_fn(obj) }}")]).env();
            twig.add_function(func.function("unsafe_fn")).unwrap();
            with_policy(&twig, |policy| policy.set_allowed_functions(["unsafe_fn"]));
            expect_to_string(&render(&twig, "index", ctx(&[("obj", FooObject::value())])))
        },
    );
}

const COUNTABLE: &[&str] = &["Countable"];
const PLAIN_BASE_OBJECT: &[&str] = &["Twig\\Tests\\Extension\\PlainBaseObject"];

#[test]
fn test_open_php_param_types_still_wrap() {
    // Interfaces and non-final classes are "open": a Stringable subtype can satisfy them, so the sandbox must keep
    // gating __toString. Skipping the wrap on these would bypass the policy.
    let cases = vec![
        (
            "interface param",
            (
                unsafe_closure("x", DeclaredType::new(COUNTABLE, TypeMask::NONE)),
                FooObject::countable(),
                "Twig\\Tests\\Extension\\CountableFooObject",
            ),
        ),
        (
            "non-final class param",
            (
                unsafe_closure("x", DeclaredType::new(PLAIN_BASE_OBJECT, TypeMask::NONE)),
                StringablePlainObject::value(),
                STRINGABLE_PLAIN_OBJECT,
            ),
        ),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case));
    check_cases(
        "testOpenPhpParamTypesStillWrap",
        cases,
        |(func, obj, class)| {
            let twig = setup(true, &[("index", "{{ unsafe_fn(obj) }}")]).env();
            twig.add_function(func.function("unsafe_fn")).unwrap();
            with_policy(&twig, |policy| policy.set_allowed_functions(["unsafe_fn"]));
            expect_method(
                &render(&twig, "index", ctx(&[("obj", obj.clone())])),
                class,
                "__tostring",
            )
        },
    );
}

#[test]
fn test_test_arguments_map_after_the_tested_value_parameter() {
    // A test's tested value is its first PHP parameter, so its template arguments must be mapped to the parameters
    // *after* it. Mapping the first argument to the (safe-typed) value parameter would skip its __toString wrap and
    // bypass the policy.
    let twig = setup(true, &[("index", "{{ 5 is my_test(obj) }}")]).env();
    // static fn (int $value, $arg) => 'x' === (string) $arg
    twig.add_test(test_with(
        "my_test",
        vec![
            P::required("value").string_coercion_safe(),
            P::required("arg"),
        ],
        TestOptions::default(),
        |call| {
            typed(call, 0, "value", &INT)?;
            let arg = call.arg(1).cloned().unwrap_or_default();
            let string = to_string(call, &arg)?;
            Ok(Value::Bool(string == Value::from("x")))
        },
    ))
    .unwrap();
    with_policy(&twig, |policy| policy.set_allowed_tests(["my_test"]));
    ok(expect_to_string(&render(
        &twig,
        "index",
        ctx(&[("obj", FooObject::value())]),
    )));
}

#[test]
fn test_safe_variadic_php_type_skips_to_string_wrap() {
    // PHP-variadic with a safe type: all spilled arguments skip the wrap.
    let twig = setup(true, &[("index", "{{ safe_fn(1, 2, 3) }}")]).env();
    twig.add_function(function(
        "safe_fn",
        vec![P::variadic("x").string_coercion_safe()],
        |_| Ok(Value::from("ok")),
    ))
    .unwrap();
    with_policy(&twig, |policy| policy.set_allowed_functions(["safe_fn"]));
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "ok");
}

#[test]
fn test_spread_into_unsafe_variadic_still_wraps() {
    // A spread fills an unsafe (untyped) variadic param, so every spilled element must keep its __toString wrap.
    let twig = setup(true, &[("index", "{{ unsafe_fn(...args) }}")]).env();
    twig.add_function(function("unsafe_fn", vec![P::variadic("x")], |call| {
        let first = call.arg(0).cloned().unwrap_or_default();
        to_string(call, &first)
    }))
    .unwrap();
    with_policy(&twig, |policy| policy.set_allowed_functions(["unsafe_fn"]));
    ok(expect_to_string(&render(
        &twig,
        "index",
        ctx(&[("args", list([FooObject::value()]))]),
    )));
}

/// The closures of `getSpreadsReachingUnsafeParams()`, by case.
fn mixed_fn(case: &str) -> TwigFunction {
    let parameters = match case {
        "safe first param, string variadic" | "spread of a variable" => {
            vec![P::required("a").string_coercion_safe(), P::variadic("rest")]
        }
        "safe first param, untyped variadic" => {
            vec![P::required("a").string_coercion_safe(), P::variadic("rest")]
        }
        "safe first param, optional untyped param" => vec![
            P::required("a").string_coercion_safe(),
            P::optional("b", twig::ast::ConstantValue::Null),
        ],
        "spread of a mapping" => vec![
            P::optional("a", twig::ast::ConstantValue::Int(0)).string_coercion_safe(),
            P::optional("b", twig::ast::ConstantValue::String(Vec::new())),
        ],
        "after a positional argument" => vec![
            P::required("a").string_coercion_safe(),
            P::required("b").string_coercion_safe(),
            P::variadic("rest"),
        ],
        "safe variadic after an unsafe param" => {
            vec![P::required("a"), P::variadic("rest").string_coercion_safe()]
        }
        other => panic!("unknown case {other}"),
    };
    let case = case.to_owned();
    function("mixed_fn", parameters, move |call| {
        // every closure ends by converting one of its arguments to a string (`(string) $x` or a `string` parameter)
        let index = match case.as_str() {
            "after a positional argument" => 2,
            "safe variadic after an unsafe param" => 0,
            _ => 1,
        };
        let value = match call.named_args().first() {
            Some((_, value)) => value.clone(),
            None => call.arg(index).cloned().unwrap_or_default(),
        };
        to_string(call, &value)
    })
}

#[test]
fn test_spread_reaching_unsafe_param_still_wraps() {
    // A spread can fill any parameter from its position on, so a safe type on the first of them must not skip the
    // __toString check of the others.
    let cases = [
        (
            "safe first param, string variadic",
            "{{ mixed_fn(...[1, obj]) }}",
        ),
        (
            "safe first param, untyped variadic",
            "{{ mixed_fn(...[1, obj]) }}",
        ),
        (
            "safe first param, optional untyped param",
            "{{ mixed_fn(...[1, obj]) }}",
        ),
        ("spread of a variable", "{{ mixed_fn(...args) }}"),
        ("spread of a mapping", "{{ mixed_fn(...{b: obj}) }}"),
        (
            "after a positional argument",
            "{{ mixed_fn(1, ...[2, obj]) }}",
        ),
        (
            "safe variadic after an unsafe param",
            "{{ mixed_fn(...[obj, 1]) }}",
        ),
    ]
    .into_iter()
    .map(|(name, template)| (name.to_owned(), (name, template)));
    check_cases(
        "testSpreadReachingUnsafeParamStillWraps",
        cases,
        |(name, template)| {
            let twig = setup(true, &[("index", template)]).env();
            twig.add_function(mixed_fn(name)).unwrap();
            with_policy(&twig, |policy| policy.set_allowed_functions(["mixed_fn"]));
            expect_to_string(&render(
                &twig,
                "index",
                ctx(&[
                    ("obj", FooObject::value()),
                    ("args", list([Value::Int(1), FooObject::value()])),
                ]),
            ))
        },
    );
}

#[test]
fn test_normalized_named_argument_does_not_fall_through_to_safe_variadic() {
    // The compiler normalizes named arguments (`foo_bar` maps to `$fooBar`). The sandbox visitor must use the same
    // mapping and not fall back to the safe typed variadic tail, or it would skip the __toString check on `$fooBar`.
    let twig = setup(true, &[("index", "{{ unsafe_fn(foo_bar: obj) }}")]).env();
    twig.add_function(function_with(
        "unsafe_fn",
        vec![
            P::required("fooBar"),
            P::variadic("rest").string_coercion_safe(),
        ],
        FunctionOptions {
            is_variadic: true,
            ..Default::default()
        },
        |call| {
            let value = call.arg(0).cloned().unwrap_or_default();
            to_string(call, &value)
        },
    ))
    .unwrap();
    with_policy(&twig, |policy| policy.set_allowed_functions(["unsafe_fn"]));
    ok(expect_to_string(&render(
        &twig,
        "index",
        ctx(&[("obj", FooObject::value())]),
    )));
}

#[test]
fn test_normalized_named_filter_argument_does_not_fall_through_to_safe_variadic() {
    let twig = setup(true, &[("index", "{{ 1|unsafe_filter(foo_bar: obj) }}")]).env();
    twig.add_filter(filter_with(
        "unsafe_filter",
        vec![
            P::required("value"),
            P::required("fooBar"),
            P::variadic("rest").string_coercion_safe(),
        ],
        FilterOptions {
            is_variadic: true,
            ..Default::default()
        },
        |call| {
            let value = call.arg(1).cloned().unwrap_or_default();
            to_string(call, &value)
        },
    ))
    .unwrap();
    with_policy(&twig, |policy| {
        policy.set_allowed_filters(["unsafe_filter"])
    });
    ok(expect_to_string(&render(
        &twig,
        "index",
        ctx(&[("obj", FooObject::value())]),
    )));
}

#[test]
fn test_normalized_named_test_argument_does_not_fall_through_to_safe_variadic() {
    let twig = setup(
        true,
        &[(
            "index",
            "{{ 1 is unsafe_test(foo_bar: obj) ? \"yes\" : \"no\" }}",
        )],
    )
    .env();
    twig.add_test(test_with(
        "unsafe_test",
        vec![
            P::required("value"),
            P::required("fooBar"),
            P::variadic("rest").string_coercion_safe(),
        ],
        TestOptions {
            is_variadic: true,
            ..Default::default()
        },
        |call| {
            let value = call.arg(1).cloned().unwrap_or_default();
            let string = to_string(call, &value)?;
            Ok(Value::Bool(string == Value::from("x")))
        },
    ))
    .unwrap();
    with_policy(&twig, |policy| policy.set_allowed_tests(["unsafe_test"]));
    ok(expect_to_string(&render(
        &twig,
        "index",
        ctx(&[("obj", FooObject::value())]),
    )));
}

#[test]
fn test_filter_input_type_skips_to_string_wrap() {
    // A filter whose first PHP param has a safe type also skips the input (`node`) wrap.
    let twig = setup(true, &[("index", "{{ n|safe_filter }}")]).env();
    twig.add_filter(filter_with(
        "safe_filter",
        vec![P::required("n").string_coercion_safe()],
        FilterOptions::default(),
        |call| {
            typed(call, 0, "n", &INT)?;
            Ok(Value::from("ok"))
        },
    ))
    .unwrap();
    with_policy(&twig, |policy| policy.set_allowed_filters(["safe_filter"]));
    assert_eq!(
        render(&twig, "index", ctx(&[("n", Value::Int(42))])).unwrap(),
        "ok"
    );
}

const TO_STRING_MESSAGE: &str = "Calling \"__tostring\" method on a \"Twig\\Tests\\Extension\\FooObject\" object is not allowed";

/// `static fn (string $value): bool => 'foo' === $value`.
fn string_test(name: &str, options: TestOptions) -> TwigTest {
    test_with(name, vec![P::required("value")], options, |call| {
        let value = typed(call, 0, "value", &STRING)?;
        Ok(is_foo(&value))
    })
}

/// `static fn (int $value): bool => true`.
fn int_test(name: &str) -> TwigTest {
    test_with(
        name,
        vec![P::required("value").string_coercion_safe()],
        TestOptions::default(),
        |call| {
            typed(call, 0, "value", &INT)?;
            Ok(Value::Bool(true))
        },
    )
}

#[test]
fn test_sandbox_blocks_to_string_on_test_input() {
    let cases = [("string parameter", true), ("explicit cast", false)]
        .into_iter()
        .map(|(name, typed)| (name.to_owned(), typed));
    check_cases(
        "testSandboxBlocksToStringOnTestInput",
        cases,
        |typed_param| {
            let twig = setup(
                true,
                &[("index", "{{ obj is unsafe_test ? \"yes\" : \"no\" }}")],
            )
            .tests(&["unsafe_test"])
            .env();
            let test = if *typed_param {
                string_test("unsafe_test", TestOptions::default())
            } else {
                // static fn ($value): bool => 'foo' === (string) $value
                test_with(
                    "unsafe_test",
                    vec![P::required("value")],
                    TestOptions::default(),
                    |call| {
                        let value = call.arg(0).cloned().unwrap_or_default();
                        Ok(is_foo(&to_string(call, &value)?))
                    },
                )
            };
            twig.add_test(test).unwrap();
            expect_message(
                &render(&twig, "index", ctx(&[("obj", FooObject::value())])),
                ErrorKind::SecurityNotAllowedMethod,
                TO_STRING_MESSAGE,
            )
        },
    );
}

#[test]
fn test_safe_test_input_type_skips_to_string_wrap() {
    let twig = setup(
        true,
        &[("index", "{{ obj is safe_test ? \"yes\" : \"no\" }}")],
    )
    .tests(&["safe_test"])
    .env();
    twig.add_test(int_test("safe_test")).unwrap();
    // A safe test input type should skip the __toString check and let PHP reject the incompatible value.
    ok(expect_wrapped_type_error(&render(
        &twig,
        "index",
        ctx(&[("obj", FooObject::value())]),
    )));
}

#[test]
fn test_dynamic_test_input_maps_after_bound_arguments() {
    let twig = setup(
        true,
        &[("index", "{{ obj is unsafe_test_name ? \"yes\" : \"no\" }}")],
    )
    .tests(&["unsafe_test_name"])
    .env();
    // static fn (string $name, string $value): bool => 'foo' === $value
    twig.add_test(test_with(
        "unsafe_test_*",
        vec![P::required("name"), P::required("value")],
        TestOptions::default(),
        |call| {
            let value = typed(call, 0, "value", &STRING)?;
            Ok(is_foo(&value))
        },
    ))
    .unwrap();
    ok(expect_message(
        &render(&twig, "index", ctx(&[("obj", FooObject::value())])),
        ErrorKind::SecurityNotAllowedMethod,
        TO_STRING_MESSAGE,
    ));
}

#[test]
fn test_safe_dynamic_test_input_type_skips_to_string_wrap() {
    let twig = setup(
        true,
        &[("index", "{{ obj is safe_test_name ? \"yes\" : \"no\" }}")],
    )
    .tests(&["safe_test_name"])
    .env();
    // static fn (string $name, int $value): bool => true
    twig.add_test(test_with(
        "safe_test_*",
        vec![
            P::required("name"),
            P::required("value").string_coercion_safe(),
        ],
        TestOptions::default(),
        |call| {
            typed(call, 0, "value", &INT)?;
            Ok(Value::Bool(true))
        },
    ))
    .unwrap();
    ok(expect_wrapped_type_error(&render(
        &twig,
        "index",
        ctx(&[("obj", FooObject::value())]),
    )));
}

#[test]
fn test_sandbox_allows_to_string_on_test_input_when_method_is_allowed() {
    let twig = setup(
        true,
        &[("index", "{{ obj is safe_test ? \"yes\" : \"no\" }}")],
    )
    .methods(&[(FOO_OBJECT, &["__toString"])])
    .tests(&["safe_test"])
    .env();
    twig.add_test(string_test("safe_test", TestOptions::default()))
        .unwrap();
    let counters = Counters::new();
    assert_eq!(
        render(
            &twig,
            "index",
            ctx(&[("obj", FooObject::with_counters(&counters))])
        )
        .unwrap(),
        "yes"
    );
    assert_eq!(counters.to_string(), 1);
}

/// `static fn ($value) => new FooObject()`.
fn returns_object() -> TwigTest {
    test_with(
        "returns_object",
        vec![P::required("value")],
        TestOptions::default(),
        make_obj,
    )
}

#[test]
fn test_sandbox_blocks_to_string_on_test_return_used_as_test_input() {
    let twig = setup(
        true,
        &[(
            "index",
            "{{ (obj is returns_object) is unsafe_test ? \"yes\" : \"no\" }}",
        )],
    )
    .tests(&["returns_object", "unsafe_test"])
    .env();
    twig.add_test(returns_object()).unwrap();
    twig.add_test(string_test("unsafe_test", TestOptions::default()))
        .unwrap();
    ok(expect_message(
        &render(&twig, "index", ctx(&[("obj", FooObject::value())])),
        ErrorKind::SecurityNotAllowedMethod,
        TO_STRING_MESSAGE,
    ));
}

#[test]
fn test_safe_test_input_type_does_not_wrap_nested_test_return() {
    let twig = setup(
        true,
        &[(
            "index",
            "{{ (1 is returns_object) is safe_test ? \"yes\" : \"no\" }}",
        )],
    )
    .tests(&["returns_object", "safe_test"])
    .env();
    twig.add_test(returns_object()).unwrap();
    twig.add_test(int_test("safe_test")).unwrap();
    ok(expect_wrapped_type_error(&render(
        &twig,
        "index",
        Array::new(),
    )));
}

#[test]
fn test_sandbox_blocks_to_string_on_true_test_result() {
    let cases = vec![
        ("direct output", ("{{ obj is true }}", vec![])),
        ("concatenation", ("{{ (obj is true) ~ \"\" }}", vec![])),
        ("filter input", ("{{ (obj is true)|upper }}", vec!["upper"])),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case));
    check_cases(
        "testSandboxBlocksToStringOnTrueTestResult",
        cases,
        |(template, filters)| {
            let twig = setup(true, &[("index", template)]).filters(filters).env();
            expect_message(
                &render(&twig, "index", ctx(&[("obj", FooObject::value())])),
                ErrorKind::SecurityNotAllowedMethod,
                TO_STRING_MESSAGE,
            )
        },
    );
}

#[test]
fn test_sandbox_allows_true_test_in_boolean_context_without_to_string() {
    let twig = setup(true, &[("index", "{{ obj is true ? \"yes\" : \"no\" }}")]).env();
    let counters = Counters::new();
    assert_eq!(
        render(
            &twig,
            "index",
            ctx(&[("obj", FooObject::with_counters(&counters))])
        )
        .unwrap(),
        "yes"
    );
    assert_eq!(counters.to_string(), 0);
}

#[test]
fn test_sandbox_allows_non_string_coercing_built_in_test_input() {
    let cases = [
        ("iterable", ("iterable", "no")),
        ("sequence", ("sequence", "no")),
        ("mapping", ("mapping", "yes")),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case));
    check_cases(
        "testSandboxAllowsNonStringCoercingBuiltInTestInput",
        cases,
        |(test, expected)| {
            let template = format!("{{{{ obj is {test} ? \"yes\" : \"no\" }}}}");
            let twig = setup(true, &[("index", &template)]).env();
            let counters = Counters::new();
            expect_output(
                &render(
                    &twig,
                    "index",
                    ctx(&[("obj", FooObject::with_counters(&counters))]),
                ),
                expected,
            )?;
            match counters.to_string() {
                0 => Ok(()),
                n => Err(format!("__toString was called {n} times")),
            }
        },
    );
}

#[test]
fn test_source_policy_sandbox_blocks_to_string_on_test_input() {
    let twig = setup(
        false,
        &[("index", "{{ obj is unsafe_test ? \"yes\" : \"no\" }}")],
    )
    .source_policy(always(true))
    .tests(&["unsafe_test"])
    .env();
    twig.add_test(string_test("unsafe_test", TestOptions::default()))
        .unwrap();
    ok(expect_message(
        &render(&twig, "index", ctx(&[("obj", FooObject::value())])),
        ErrorKind::SecurityNotAllowedMethod,
        TO_STRING_MESSAGE,
    ));
}

#[test]
fn test_column_filter_unaffected_outside_sandbox() {
    let twig = setup(false, &[("index", COLUMN_BAR)]).env();
    assert_eq!(
        render(&twig, "index", ctx(&[("obj", ColumnObject::value())])).unwrap(),
        "bar"
    );
}

#[test]
fn test_source_policy_sandbox_blocks_column_filter_on_disallowed_property() {
    let twig = setup(false, &[("index", COLUMN_BAR)])
        .filters(&["column", "first"])
        .source_policy(always(true))
        .env();
    ok(expect_property(
        &render(&twig, "index", ctx(&[("obj", ColumnObject::value())])),
        COLUMN_OBJECT,
        "bar",
    ));
}

#[test]
fn test_source_policy_sandbox_blocks_column_filter_on_disallowed_index() {
    let twig = setup(false, &[("index", COLUMN_BAR_FOO)])
        .filters(&["column", "first", "keys"])
        .properties(&[(COLUMN_OBJECT, &["bar"])])
        .source_policy(always(true))
        .env();
    ok(expect_property(
        &render(&twig, "index", ctx(&[("obj", ColumnObject::value())])),
        COLUMN_OBJECT,
        "foo",
    ));
}

#[test]
fn test_source_policy_sandbox_allows_column_filter_on_allowed_property() {
    let twig = setup(false, &[("index", COLUMN_BAR)])
        .filters(&["column", "first"])
        .properties(&[(COLUMN_OBJECT, &["bar"])])
        .source_policy(always(true))
        .env();
    assert_eq!(
        render(&twig, "index", ctx(&[("obj", ColumnObject::value())])).unwrap(),
        "bar"
    );
}

#[test]
fn test_source_policy_sandbox_blocks_column_filter_on_magic_getter() {
    let twig = setup(false, &[("index", COLUMN_MAGIC)])
        .filters(&["column", "first"])
        .source_policy(always(true))
        .env();
    ok(expect_property(
        &render(&twig, "index", ctx(&[("magic", MagicObject::value())])),
        "Twig\\Tests\\Extension\\MagicObject",
        "anything",
    ));
}

#[test]
fn test_sandbox_source_policy_enable_returning_false() {
    let twig = setup(false, TEMPLATES)
        .source_policy(SourcePolicy(|source: &Source| source.name() != "1_basic"))
        .env();
    assert_eq!(render(&twig, "1_basic", params()).unwrap(), "FOO");
}

#[test]
fn test_sandbox_source_policy_enable_returning_true() {
    let twig = setup(false, TEMPLATES)
        .source_policy(SourcePolicy(|source: &Source| source.name() == "1_basic"))
        .env();
    ok(violation(&render(&twig, "1_basic", Array::new())).map(|_| ()));
}

#[test]
fn test_sandbox_source_policy_false_doesnt_override_other_enables() {
    let twig = setup(true, TEMPLATES).source_policy(always(false)).env();
    ok(violation(&render(&twig, "1_basic", Array::new())).map(|_| ()));
}

fn provide_source_policy_arrow_blocked_templates() -> Vec<(String, &'static str)> {
    vec![
        ("sort", "{{ [\"a\",\"b\"]|sort(\"strnatcasecmp\")|join }}"),
        ("filter", "{{ [\"a\",\"b\"]|filter(\"is_string\")|join }}"),
        ("map", "{{ [\"a\",\"b\"]|map(\"strtoupper\")|join }}"),
        ("reduce", "{{ [1,2]|reduce(\"intval\") }}"),
        ("find", "{{ [\"a\",\"b\"]|find(\"is_string\") }}"),
        (
            "has some",
            "{{ [1,2] has some \"is_string\" ? \"yes\" : \"no\" }}",
        ),
        (
            "has every",
            "{{ [1,2] has every \"is_int\" ? \"yes\" : \"no\" }}",
        ),
    ]
    .into_iter()
    .map(|(name, template)| (name.to_owned(), template))
    .collect()
}

#[test]
fn test_source_policy_blocks_non_closure_callable_in_arrow() {
    check_cases(
        "testSourcePolicyBlocksNonClosureCallableInArrow",
        provide_source_policy_arrow_blocked_templates(),
        |template| {
            let twig = setup(false, &[("1_basic", template)])
                .filters(&["sort", "filter", "map", "reduce", "find", "join"])
                .source_policy(always(true))
                .env();
            // expectExceptionMessageMatches('/must be a Closure in sandbox mode/')
            expect_message(
                &render(&twig, "1_basic", Array::new()),
                ErrorKind::Runtime,
                "must be a Closure in sandbox mode",
            )
        },
    );
}

#[test]
fn test_source_policy_allows_closure_in_arrow() {
    let twig = setup(
        false,
        &[(
            "1_basic",
            "{{ [\"b\",\"a\"]|sort((a, b) => a < b ? -1 : 1)|join(\",\") }}",
        )],
    )
    .filters(&["sort", "join"])
    .source_policy(always(true))
    .env();
    assert_eq!(render(&twig, "1_basic", Array::new()).unwrap(), "a,b");
}

// ---- needs_is_sandboxed ------------------------------------------------------------------------------------------

/// `static function (bool $isSandboxed, string $value) { return $value.':'.($isSandboxed ? 'on' : 'off'); }`
fn sandbox_aware_filter() -> TwigFilter {
    filter_with(
        "sandbox_aware",
        vec![P::required("isSandboxed"), P::required("value")],
        FilterOptions {
            needs_is_sandboxed: true,
            ..Default::default()
        },
        |call| {
            let sandboxed = call.is_sandboxed();
            let value = typed(call, 0, "value", &STRING)?;
            let value = value.as_str().cloned().unwrap_or_default();
            Ok(Value::String(PhpString::concat(&[
                value.as_bytes(),
                if sandboxed { b":on" } else { b":off" },
            ])))
        },
    )
}

#[test]
fn test_needs_is_sandboxed_filter_receives_true_when_sandboxed() {
    let twig = setup(true, &[("index", "{{ \"foo\"|sandbox_aware }}")])
        .filters(&["sandbox_aware"])
        .env();
    twig.add_filter(sandbox_aware_filter()).unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "foo:on");
}

#[test]
fn test_needs_is_sandboxed_filter_receives_false_when_not_sandboxed() {
    let twig = setup(false, &[("index", "{{ \"foo\"|sandbox_aware }}")]).env();
    twig.add_filter(sandbox_aware_filter()).unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "foo:off");
}

#[test]
fn test_needs_is_sandboxed_filter_follows_source_policy() {
    let twig = setup(
        false,
        &[
            ("in", "{{ \"foo\"|sandbox_aware }}"),
            ("out", "{{ \"foo\"|sandbox_aware }}"),
        ],
    )
    .filters(&["sandbox_aware"])
    .source_policy(SourcePolicy(|source: &Source| source.name() == "in"));
    // one environment per oracle scenario: both renders see the deprecation of the environment's creation
    for (name, expected) in [("in", "foo:on"), ("out", "foo:off")] {
        let twig = twig.env();
        twig.add_filter(sandbox_aware_filter()).unwrap();
        sub_scenario(name);
        assert_eq!(render(&twig, name, Array::new()).unwrap(), expected);
    }
}

#[test]
fn test_needs_is_sandboxed_function_without_sandbox_extension() {
    let twig = Environment::with_options(
        loader(&[("index", "{{ sandbox_aware(\"foo\") }}")]),
        EnvironmentOptions {
            debug: true,
            autoescape: DefaultStrategy::Disabled,
            ..Default::default()
        },
    );
    twig.add_function(function_with(
        "sandbox_aware",
        vec![P::required("isSandboxed"), P::required("value")],
        FunctionOptions {
            needs_is_sandboxed: true,
            ..Default::default()
        },
        |call| {
            let sandboxed = call.is_sandboxed();
            let value = typed(call, 0, "value", &STRING)?;
            let value = value.as_str().cloned().unwrap_or_default();
            Ok(Value::String(PhpString::concat(&[
                value.as_bytes(),
                if sandboxed { b":on" } else { b":off" },
            ])))
        },
    ))
    .unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "foo:off");
}

/// `static function (bool $isSandboxed, string $value) { return $isSandboxed === $expected && 'foo' === $value; }`
fn sandbox_aware_test(expected: bool) -> TwigTest {
    test_with(
        "sandbox_aware",
        vec![P::required("isSandboxed"), P::required("value")],
        TestOptions {
            needs_is_sandboxed: true,
            ..Default::default()
        },
        move |call| {
            let sandboxed = call.is_sandboxed();
            let value = typed(call, 0, "value", &STRING)?;
            Ok(Value::Bool(
                sandboxed == expected && value == Value::from("foo"),
            ))
        },
    )
}

#[test]
fn test_needs_is_sandboxed_test_receives_true_when_sandboxed() {
    let twig = setup(
        true,
        &[("index", "{{ \"foo\" is sandbox_aware ? \"on\" : \"off\" }}")],
    )
    .tests(&["sandbox_aware"])
    .env();
    twig.add_test(sandbox_aware_test(true)).unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "on");
}

#[test]
fn test_needs_is_sandboxed_test_receives_false_when_not_sandboxed() {
    let twig = setup(
        false,
        &[("index", "{{ \"foo\" is sandbox_aware ? \"on\" : \"off\" }}")],
    )
    .env();
    twig.add_test(sandbox_aware_test(false)).unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "on");
}

// ---- always_allowed_in_sandbox -----------------------------------------------------------------------------------

fn always_allowed_filter(name: &str) -> FilterOptions {
    let _ = name;
    FilterOptions {
        always_allowed_in_sandbox: true,
        ..Default::default()
    }
}

#[test]
fn test_always_allowed_in_sandbox_filter_bypasses_allow_list() {
    let twig = setup(true, &[("index", "{{ \"fabien\"|safe_upper }}")]).env();
    twig.add_filter(TwigFilter::new(
        "safe_upper",
        Some(strtoupper_callable()),
        always_allowed_filter("safe_upper"),
    ))
    .unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "FABIEN");
}

#[test]
fn test_always_allowed_in_sandbox_filter_still_enforced_when_flag_not_set() {
    let twig = setup(true, &[("index", "{{ \"fabien\"|gated_upper }}")]).env();
    twig.add_filter(TwigFilter::new(
        "gated_upper",
        Some(strtoupper_callable()),
        FilterOptions::default(),
    ))
    .unwrap();
    let result = render(&twig, "index", Array::new());
    ok(expect_filter(&result, "gated_upper"));
    ok(expect_message(
        &result,
        ErrorKind::SecurityNotAllowedFilter,
        "Filter \"gated_upper\" is not allowed",
    ));
}

/// `static fn (string $name) => "hi $name"`.
fn greet(name: &str, options: FunctionOptions) -> TwigFunction {
    function_with(name, vec![P::required("name")], options, |call| {
        let name = typed(call, 0, "name", &STRING)?;
        let name = name.as_str().cloned().unwrap_or_default();
        Ok(Value::String(PhpString::concat(&[b"hi ", name.as_bytes()])))
    })
}

fn always_allowed_function() -> FunctionOptions {
    FunctionOptions {
        always_allowed_in_sandbox: true,
        ..Default::default()
    }
}

#[test]
fn test_always_allowed_in_sandbox_function_bypasses_allow_list() {
    let twig = setup(true, &[("index", "{{ safe_greet(\"fabien\") }}")]).env();
    twig.add_function(greet("safe_greet", always_allowed_function()))
        .unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "hi fabien");
}

#[test]
fn test_always_allowed_in_sandbox_function_still_enforced_when_flag_not_set() {
    let twig = setup(true, &[("index", "{{ gated_greet(\"fabien\") }}")]).env();
    twig.add_function(greet("gated_greet", FunctionOptions::default()))
        .unwrap();
    let result = render(&twig, "index", Array::new());
    ok(expect_function(&result, "gated_greet"));
    ok(expect_message(
        &result,
        ErrorKind::SecurityNotAllowedFunction,
        "Function \"gated_greet\" is not allowed",
    ));
}

/// `static fn ($value) => 0 === $value % 2`.
fn even_test(name: &str, options: TestOptions) -> TwigTest {
    test_with(name, vec![P::required("value")], options, |call| {
        let value = call.arg(0).cloned().unwrap_or_default();
        let rest = twig::value::ops::modulo(&value, &Value::Int(2), call.diagnostics())?;
        Ok(Value::Bool(rest == Value::Int(0)))
    })
}

#[test]
fn test_always_allowed_in_sandbox_test_bypasses_allow_list() {
    let twig = setup(
        true,
        &[("index", "{{ 4 is safe_even ? \"yes\" : \"no\" }}")],
    )
    .strict()
    .env();
    twig.add_test(even_test(
        "safe_even",
        TestOptions {
            always_allowed_in_sandbox: true,
            ..Default::default()
        },
    ))
    .unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "yes");
}

#[test]
fn test_always_allowed_in_sandbox_test_still_enforced_when_flag_not_set() {
    let twig = setup(
        true,
        &[("index", "{{ 4 is gated_even ? \"yes\" : \"no\" }}")],
    )
    .strict()
    .env();
    twig.add_test(even_test("gated_even", TestOptions::default()))
        .unwrap();
    let result = render(&twig, "index", Array::new());
    ok(expect_test(&result, "gated_even"));
    ok(expect_message(
        &result,
        ErrorKind::SecurityNotAllowedTest,
        "Test \"gated_even\" is not allowed",
    ));
}

#[test]
fn test_always_allowed_in_sandbox_function_also_covers_range_operator() {
    let twig = setup(true, &[("index", "{{ (1..2)[0] }}")]).env();
    // override the built-in `range` function with one that is always allowed (the PHP function `range`, whose
    // runtime the environment installs)
    twig.add_function(TwigFunction::new(
        "range",
        Some(PhpCallable::function(
            "range",
            vec![
                P::required("start"),
                P::required("end"),
                P::optional("step", twig::ast::ConstantValue::Int(1)),
            ],
        )),
        always_allowed_function(),
    ))
    .unwrap();
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "1");
}

#[test]
fn test_always_allowed_in_sandbox_filter_from_undefined_callback_uses_parsed_callable() {
    let twig = setup(true, &[("index", "{{ \"fabien\"|callback_upper }}")]).env();
    let callback_called = Arc::new(AtomicBool::new(false));
    twig.register_undefined_filter_callback(Arc::new(move |name: &[u8]| {
        if callback_called.load(Ordering::SeqCst) || name != b"callback_upper" {
            return Ok(None);
        }
        callback_called.store(true, Ordering::SeqCst);
        Ok(Some(TwigFilter::new(
            "callback_upper",
            Some(strtoupper_callable()),
            always_allowed_filter("callback_upper"),
        )))
    }));
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "FABIEN");
}

#[test]
fn test_always_allowed_in_sandbox_function_from_undefined_callback_uses_parsed_callable() {
    let twig = setup(true, &[("index", "{{ callback_upper(\"fabien\") }}")]).env();
    let callback_called = Arc::new(AtomicBool::new(false));
    twig.register_undefined_function_callback(Arc::new(move |name: &[u8]| {
        if callback_called.load(Ordering::SeqCst) || name != b"callback_upper" {
            return Ok(None);
        }
        callback_called.store(true, Ordering::SeqCst);
        Ok(Some(TwigFunction::new(
            "callback_upper",
            Some(strtoupper_callable()),
            always_allowed_function(),
        )))
    }));
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "FABIEN");
}

#[test]
fn test_always_allowed_in_sandbox_parser_callable_function_from_undefined_callback_uses_parsed_callable()
 {
    let twig = setup(true, &[("index", "{{ callback_literal() }}")]).env();
    let callback_called = Arc::new(AtomicBool::new(false));
    twig.register_undefined_function_callback(Arc::new(move |name: &[u8]| {
        if callback_called.load(Ordering::SeqCst) || name != b"callback_literal" {
            return Ok(None);
        }
        callback_called.store(true, Ordering::SeqCst);
        Ok(Some(TwigFunction::new(
            "callback_literal",
            None,
            FunctionOptions {
                always_allowed_in_sandbox: true,
                parser_callable: Some(Arc::new(|_parser, _arguments, line| {
                    Ok(Expr::constant("literal", line))
                })),
                ..Default::default()
            },
        )))
    }));
    assert_eq!(render(&twig, "index", Array::new()).unwrap(), "literal");
}

/// `AlwaysAllowedSandboxTokenParser` / `GatedSandboxTokenParser`: a tag printing a text.
struct TextTokenParser {
    tag: &'static str,
    text: &'static str,
    always_allowed: bool,
}

impl TokenParser for TextTokenParser {
    fn tag(&self) -> &str {
        self.tag
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        Ok(Some(Node::text(self.text, token.line())))
    }

    fn is_always_allowed_in_sandbox(&self) -> bool {
        self.always_allowed
    }
}

#[test]
fn test_always_allowed_in_sandbox_tag_bypasses_allow_list() {
    let twig = setup(true, &[("index", "{% always_allowed_tag %}")]).env();
    twig.add_token_parser(Arc::new(TextTokenParser {
        tag: "always_allowed_tag",
        text: "always-allowed",
        always_allowed: true,
    }))
    .unwrap();
    assert_eq!(
        render(&twig, "index", Array::new()).unwrap(),
        "always-allowed"
    );
}

#[test]
fn test_always_allowed_in_sandbox_tag_still_enforced_when_flag_not_set() {
    let twig = setup(true, &[("index", "{% gated_tag %}")]).env();
    twig.add_token_parser(Arc::new(TextTokenParser {
        tag: "gated_tag",
        text: "gated",
        always_allowed: false,
    }))
    .unwrap();
    let result = render(&twig, "index", Array::new());
    ok(expect_tag(&result, "gated_tag"));
    ok(expect_message(
        &result,
        ErrorKind::SecurityNotAllowedTag,
        "Tag \"gated_tag\" is not allowed",
    ));
}

#[test]
fn test_always_allowed_in_sandbox_filter_still_enforces_to_string_policy_on_arguments() {
    let twig = setup(true, &[("index", "{{ obj|safe_upper }}")]).env();
    // static fn (string $s) => strtoupper($s)
    twig.add_filter(filter_with(
        "safe_upper",
        vec![P::required("s")],
        always_allowed_filter("safe_upper"),
        |call| {
            typed(call, 0, "s", &STRING)?;
            strtoupper(call)
        },
    ))
    .unwrap();
    ok(expect_message(
        &render(&twig, "index", ctx(&[("obj", FooObject::value())])),
        ErrorKind::SecurityNotAllowedMethod,
        TO_STRING_MESSAGE,
    ));
}

#[test]
fn test_always_allowed_in_sandbox_function_still_enforces_to_string_policy_on_arguments() {
    let twig = setup(true, &[("index", "{{ safe_greet(obj) }}")]).env();
    twig.add_function(greet("safe_greet", always_allowed_function()))
        .unwrap();
    ok(expect_message(
        &render(&twig, "index", ctx(&[("obj", FooObject::value())])),
        ErrorKind::SecurityNotAllowedMethod,
        TO_STRING_MESSAGE,
    ));
}

#[test]
fn test_always_allowed_in_sandbox_test_still_enforces_to_string_policy_on_input() {
    let twig = setup(
        true,
        &[("index", "{{ obj is always_safe_test ? \"yes\" : \"no\" }}")],
    )
    .env();
    twig.add_test(string_test(
        "always_safe_test",
        TestOptions {
            always_allowed_in_sandbox: true,
            ..Default::default()
        },
    ))
    .unwrap();
    ok(expect_message(
        &render(&twig, "index", ctx(&[("obj", FooObject::value())])),
        ErrorKind::SecurityNotAllowedMethod,
        TO_STRING_MESSAGE,
    ));
}
