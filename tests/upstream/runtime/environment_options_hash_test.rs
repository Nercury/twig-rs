//! Behavioural equivalents of TWIG/tests/EnvironmentOptionsHashTest.php.
//!
//! The PHPUnit test checks that everything changing the compiled code is part of the generated class name (and so of
//! the compiled-template cache key). twig-rs has no class names and keeps compiled programs per environment, so the
//! ports keep the rendering assertions of each test: environments that differ only in such a setting must render
//! differently, and a setting changed after a template was compiled must be honoured.
//!
//! Not ported (generated class names or PHP-only machinery):
//! - `testSupportedAutoescapeStrategiesHaveDistinctTemplateClasses`,
//!   `testTruthyInvalidAutoescapeCallbackReturnDoesNotFailTemplateClassResolution`,
//!   `testInvalidAutoescapeStrategiesWithDifferentEffectiveValuesHaveDistinctTemplateClasses`: only compare
//!   `getTemplateClass()` results.
//! - `testLegacyTwigCallablesDoNotTriggerDeprecationsWhenBuildingSandboxSignature`: legacy `TwigCallableInterface`
//!   implementations and the class-name signature.
//! - `testEquivalentConfigurationIsDeterministicAcrossProcesses`: spawns PHP processes.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use twig::ast::{ConstantValue, Node};
use twig::escaper::DefaultStrategy;
use twig::extension::runtime::CallableRuntime;
use twig::extension::{
    CallableParameter, EscaperExtension, Extension, FilterOptions, FunctionOptions, PhpCallable,
    TestOptions, TokenParser, TwigFilter, TwigFunction, TwigTest,
};
use twig::lexer::{Token, TokenType};
use twig::parser::Parser;
use twig::runtime::{Environment, EnvironmentOptions, SandboxExtension, SecurityPolicy};
use twig::value::{Array, Value};
use twig::{ErrorKind, Result};

use crate::support::*;

fn autoescape(strategy: DefaultStrategy) -> EnvironmentOptions {
    EnvironmentOptions {
        autoescape: strategy,
        ..Default::default()
    }
}

fn callback(f: impl Fn(&str) -> ConstantValue + Send + Sync + 'static) -> DefaultStrategy {
    DefaultStrategy::Callback(Arc::new(move |name| Ok(f(name))))
}

fn strong() -> Array {
    ctx(&[("value", Value::from("<strong>"))])
}

#[test]
fn test_resolved_autoescape_strategy_is_part_of_the_template_class() {
    let escaped = env_with(
        &[("index", "{{ value }}")],
        autoescape(callback(|_| ConstantValue::String(b"html".to_vec()))),
    );
    let unescaped = env_with(
        &[("index", "{{ value }}")],
        autoescape(callback(|_| ConstantValue::Bool(false))),
    );
    assert_eq!(escaped.render("index", strong()).unwrap(), "&lt;strong&gt;");
    assert_eq!(unescaped.render("index", strong()).unwrap(), "<strong>");
}

/// `createSandboxedEnvironment()`: a strict policy allowing the given filters and functions, sandboxed globally.
fn sandboxed_env(
    templates: &[(&str, &str)],
    filters: &[&str],
    functions: &[&str],
    strategy: DefaultStrategy,
) -> Environment {
    let policy = SecurityPolicy::new();
    policy.set_allowed_filters(filters.iter().copied());
    policy.set_allowed_functions(functions.iter().copied());
    policy.set_strict(true);
    let twig = env_with(templates, autoescape(strategy));
    twig.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), true)))
        .unwrap();
    twig
}

fn safe_function(allowed: bool) -> TwigFunction {
    // static fn (): string => 'function'
    let callable = PhpCallable::closure(vec![]);
    CallableRuntime::new(|_| Ok(Value::from("function"))).attach(&callable);
    TwigFunction::new(
        "safe_function",
        Some(callable),
        FunctionOptions {
            always_allowed_in_sandbox: allowed,
            ..Default::default()
        },
    )
}

/// The `strtoupper` PHP function (byte-wise ASCII upper case since PHP 8.2).
fn strtoupper() -> PhpCallable {
    let callable = PhpCallable::function("strtoupper", vec![CallableParameter::required("string")]);
    CallableRuntime::new(|call| {
        let value = call.arg(0).cloned().unwrap_or_default();
        let string = twig::value::convert::to_string(&value, call.diagnostics())?;
        Ok(Value::String(string.as_bytes().to_ascii_uppercase().into()))
    })
    .attach(&callable);
    callable
}

fn safe_filter(allowed: bool) -> TwigFilter {
    TwigFilter::new(
        "safe_filter",
        Some(strtoupper()),
        FilterOptions {
            always_allowed_in_sandbox: allowed,
            ..Default::default()
        },
    )
}

fn safe_test(allowed: bool) -> TwigTest {
    // static fn (int $value): bool => 0 === $value % 2
    let callable = PhpCallable::closure(vec![CallableParameter::required("value")]);
    CallableRuntime::new(|call| {
        let value = call.arg(0).and_then(Value::as_int).unwrap_or(1);
        Ok(Value::Bool(value % 2 == 0))
    })
    .attach(&callable);
    TwigTest::new(
        "safe_test",
        Some(callable),
        TestOptions {
            always_allowed_in_sandbox: allowed,
            ..Default::default()
        },
    )
}

/// `EnvironmentOptionsHashTokenParser`: `{% safe_tag %}` is the text `tag`.
struct SafeTagTokenParser(bool);

impl TokenParser for SafeTagTokenParser {
    fn tag(&self) -> &str {
        "safe_tag"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        Ok(Some(Node::text("tag", token.line())))
    }

    fn is_always_allowed_in_sandbox(&self) -> bool {
        self.0
    }

    fn class_name(&self) -> &str {
        "Twig\\Tests\\EnvironmentOptionsHashTokenParser"
    }
}

type Register = fn(&Environment, bool);

#[test]
fn test_staged_sandbox_registration_is_part_of_the_template_class() {
    let data: [(&str, &str, &str, Register, ErrorKind); 4] = [
        (
            "function",
            "{{ safe_function() }}",
            "function",
            |twig, allowed| twig.add_function(safe_function(allowed)).unwrap(),
            ErrorKind::SecurityNotAllowedFunction,
        ),
        (
            "filter",
            "{{ \"filter\"|safe_filter }}",
            "FILTER",
            |twig, allowed| twig.add_filter(safe_filter(allowed)).unwrap(),
            ErrorKind::SecurityNotAllowedFilter,
        ),
        (
            "test",
            "{{ 2 is safe_test ? \"test\" : \"no\" }}",
            "test",
            |twig, allowed| twig.add_test(safe_test(allowed)).unwrap(),
            ErrorKind::SecurityNotAllowedTest,
        ),
        (
            "tag",
            "{% safe_tag %}",
            "tag",
            |twig, allowed| {
                twig.add_token_parser(Arc::new(SafeTagTokenParser(allowed)))
                    .unwrap()
            },
            ErrorKind::SecurityNotAllowedTag,
        ),
    ];
    let mut failures = Failures::default();
    for (name, template, expected, register, kind) in data {
        let allowed = sandboxed_env(&[("index", template)], &[], &[], DefaultStrategy::Disabled);
        register(&allowed, true);
        let denied = sandboxed_env(&[("index", template)], &[], &[], DefaultStrategy::Disabled);
        register(&denied, false);
        failures.check(
            &format!("{name} allowed"),
            allowed
                .render("index", Array::new())
                .map_err(|e| describe(&e)),
            Ok(expected.to_owned()),
        );
        failures.check(
            &format!("{name} denied"),
            denied.render("index", Array::new()).map_err(|e| e.kind()),
            Err(kind),
        );
    }
    failures.assert_none();
}

#[test]
fn test_autoescape_callback_is_resolved_once_per_template() {
    let invocations = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&invocations);
    let twig = env_with(
        &[("callback_once", "{{ value }}")],
        autoescape(callback(move |_| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                ConstantValue::String(b"html".to_vec())
            } else {
                ConstantValue::Bool(false)
            }
        })),
    );
    assert_eq!(
        twig.render("callback_once", strong()).unwrap(),
        "&lt;strong&gt;"
    );
    assert_eq!(
        twig.render("callback_once", strong()).unwrap(),
        "&lt;strong&gt;"
    );
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
}

#[test]
fn test_resolved_autoescape_strategies_are_bounded() {
    let invocations = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&invocations);
    let extension = EscaperExtension::with_default_strategy(callback(move |name| {
        counter.fetch_add(1, Ordering::SeqCst);
        ConstantValue::String(format!("strategy{name}").into_bytes())
    }));
    let strategy = |name: &str| extension.default_strategy(name).unwrap();
    for i in 0..=1000 {
        strategy(&i.to_string());
    }
    assert_eq!(strategy("1"), ConstantValue::String(b"strategy1".to_vec()));
    assert_eq!(
        strategy("1000"),
        ConstantValue::String(b"strategy1000".to_vec())
    );
    assert_eq!(invocations.load(Ordering::SeqCst), 1001);

    assert_eq!(strategy("0"), ConstantValue::String(b"strategy0".to_vec()));
    assert_eq!(invocations.load(Ordering::SeqCst), 1002);
}

#[test]
fn test_changing_default_autoescape_strategy_invalidates_resolved_strategies() {
    let data: [(&str, DefaultStrategy, DefaultStrategy, &str, &str); 2] = [
        (
            "disabled to html",
            DefaultStrategy::Disabled,
            DefaultStrategy::html(),
            "<strong>",
            "&lt;strong&gt;",
        ),
        (
            "html to disabled",
            DefaultStrategy::html(),
            DefaultStrategy::Disabled,
            "&lt;strong&gt;",
            "<strong>",
        ),
    ];
    let mut failures = Failures::default();
    for (name, initial, updated, initial_output, updated_output) in data {
        let twig = env_with(&[("index", "{{ value }}")], autoescape(initial));
        failures.check(
            &format!("{name} (initial)"),
            twig.render("index", strong()).map_err(|e| describe(&e)),
            Ok(initial_output.to_owned()),
        );
        // `$twig->getExtension(EscaperExtension::class)->setDefaultStrategy($updatedStrategy)`
        twig.with_extensions(|set| {
            set.extension::<EscaperExtension>()
                .expect("the escaper extension")
                .set_default_strategy(updated)
        });
        failures.check(
            &format!("{name} (updated)"),
            twig.render("index", strong()).map_err(|e| describe(&e)),
            Ok(updated_output.to_owned()),
        );
    }
    failures.assert_none();
}

#[test]
fn test_falsey_invalid_autoescape_callback_return_preserves_disabled_autoescape() {
    let values: [(&str, ConstantValue); 4] = [
        ("null", ConstantValue::Null),
        ("integer", ConstantValue::Int(0)),
        ("float", ConstantValue::Float(0.0)),
        ("array", ConstantValue::Array(Vec::new())),
    ];
    let mut failures = Failures::default();
    for (name, value) in values {
        let twig = env_with(
            &[("index", "{{ value }}")],
            autoescape(callback(move |_| value.clone())),
        );
        failures.check(
            name,
            twig.render("index", strong()).map_err(|e| describe(&e)),
            Ok("<strong>".to_owned()),
        );
    }
    failures.assert_none();
}

/// `EnvironmentOptionsHashExtension`: the four registrations of the staged test in one extension class.
struct OptionsHashExtension(bool);

impl Extension for OptionsHashExtension {
    fn class_name(&self) -> &str {
        "Twig\\Tests\\EnvironmentOptionsHashExtension"
    }

    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        vec![Arc::new(SafeTagTokenParser(self.0))]
    }

    fn filters(&self) -> Vec<TwigFilter> {
        vec![safe_filter(self.0)]
    }

    fn tests(&self) -> Vec<TwigTest> {
        vec![safe_test(self.0)]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        vec![safe_function(self.0)]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn test_same_class_extension_sandbox_registrations_are_part_of_the_template_class() {
    let template = "{% safe_tag %}{{ safe_function() }}{{ \"filter\"|safe_filter }}{{ 2 is safe_test ? \"test\" : \"no\" }}";
    let allowed = sandboxed_env(&[("index", template)], &[], &[], DefaultStrategy::Disabled);
    allowed
        .add_extension(Arc::new(OptionsHashExtension(true)))
        .unwrap();
    let denied = sandboxed_env(&[("index", template)], &[], &[], DefaultStrategy::Disabled);
    denied
        .add_extension(Arc::new(OptionsHashExtension(false)))
        .unwrap();
    assert_eq!(
        allowed.render("index", Array::new()).unwrap(),
        "tagfunctionFILTERtest"
    );
    expect_error(
        denied.render("index", Array::new()),
        ErrorKind::SecurityNotAllowedTag,
    );
}

#[test]
fn test_unrelated_callable_identity_does_not_change_the_template_class() {
    let mut outputs = Vec::new();
    for _ in ["first", "equivalent"] {
        let twig = sandboxed_env(
            &[("index", "{{ dynamic() }} {{ \"<b>\"|safe_upper }}")],
            &["safe_upper", "escape"],
            &["dynamic"],
            DefaultStrategy::html(),
        );
        // EnvironmentOptionsHashCallables::first(...) / ::equivalent(...): both return 'first'
        let callable = PhpCallable::closure(vec![]);
        CallableRuntime::new(|_| Ok(Value::from("first"))).attach(&callable);
        twig.add_function(TwigFunction::new(
            "dynamic",
            Some(callable),
            FunctionOptions::default(),
        ))
        .unwrap();
        twig.add_filter(TwigFilter::new(
            "safe_upper",
            Some(strtoupper()),
            FilterOptions {
                is_safe_callback: Some(Arc::new(|_| vec![ConstantValue::String(b"html".to_vec())])),
                ..Default::default()
            },
        ))
        .unwrap();
        outputs.push(twig.render("index", Array::new()).map_err(|e| describe(&e)));
    }
    assert_eq!(
        outputs,
        [Ok("first <B>".to_owned()), Ok("first <B>".to_owned())]
    );
}
