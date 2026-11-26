//! Twig 3.31.1's `tests/Extension/SandboxBridgeExtensionTest.php`: `render_sandboxed()`.
//!
//! Not ported: `testLastModifiedIncludesTheRuntime` (the `filemtime()` of PHP class files) and
//! `testOutputStrategyIsNotPassedToTheRuntime` (an assertion on generated PHP; the runtime is a concrete type that
//! receives the name and the context only). `testLoadsTheSandboxLazily` registers a runtime factory that throws;
//! twig-rs registers runtime objects (`Environment::add_runtime()`), so the port registers none: a template that does
//! not call `render_sandboxed()` renders without the runtime.

use std::sync::Arc;

use twig::escaper::DefaultStrategy;
use twig::extension::SandboxBridgeExtension;
use twig::runtime::{Environment, EnvironmentOptions, Sandbox, SandboxBridgeRuntime};
use twig::value::Array;

use super::support::{Mismatches, Oracle, Outcome, env, policy, tried};

/// `createEnvironment()`: an environment with `SandboxBridgeExtension` whose runtime renders the templates of a
/// sandbox (`content`) under a strict policy that allows nothing.
fn create_environment(template: &str) -> Environment {
    let sandbox_environment = env(
        &[("content", "<strong>{{ name }}</strong>")],
        EnvironmentOptions {
            autoescape: DefaultStrategy::Disabled,
            ..Default::default()
        },
    );
    let sandbox = Sandbox::new(sandbox_environment, policy(&[], &[], &[], true)).unwrap();
    let twig = env(&[("index", template)], EnvironmentOptions::default());
    twig.add_extension(Arc::new(SandboxBridgeExtension::new()))
        .unwrap();
    twig.add_runtime(
        SandboxBridgeRuntime::CLASS,
        Arc::new(SandboxBridgeRuntime::new(sandbox)),
    );
    twig
}

/// The upstream expectation of a test: the output, or a substring of the `SyntaxError` of loading the template.
enum Expected {
    Output(&'static str),
    LoadError(&'static str),
}

const STRATEGY_ERROR: &str = "The \"output_strategy\" argument of the \"render_sandboxed\" function must be a non-empty literal string other than \"all\"";

/// The tests in the order of `BRIDGE_TEMPLATES` in the generator.
const TESTS: [(&str, &str, Expected); 7] = [
    (
        "testRendersOutputSafeForTheDeclaredStrategy",
        r#"{{ render_sandboxed("content", {name: "Fabien"}, "html") }}"#,
        Expected::Output("<strong>Fabien</strong>"),
    ),
    (
        "testEscapesOutputForAnotherStrategy",
        r#"{% autoescape "js" %}{{ render_sandboxed("content", {name: "Fabien"}, "html") }}{% endautoescape %}"#,
        Expected::Output("\\u003Cstrong\\u003EFabien\\u003C\\/strong\\u003E"),
    ),
    (
        "testSupportsANamedOutputStrategy",
        r#"{{ render_sandboxed("content", {name: "Fabien"}, output_strategy: "html") }}"#,
        Expected::Output("<strong>Fabien</strong>"),
    ),
    (
        "testSupportsANormalizedNamedOutputStrategy",
        r#"{{ render_sandboxed("content", {name: "Fabien"}, outputStrategy: "html") }}"#,
        Expected::Output("<strong>Fabien</strong>"),
    ),
    (
        "testRejectsADynamicStrategy",
        r#"{{ render_sandboxed("content", {name: "Fabien"}, strategy) }}"#,
        Expected::LoadError(STRATEGY_ERROR),
    ),
    (
        "testRejectsTheAllStrategy",
        r#"{{ render_sandboxed("content", {name: "Fabien"}, "all") }}"#,
        Expected::LoadError(STRATEGY_ERROR),
    ),
    (
        "testOutputStrategyIsRequired",
        r#"{{ render_sandboxed("content", {name: "Fabien"}) }}"#,
        Expected::LoadError("Value for argument \"output_strategy\" is required"),
    ),
];

/// `testRendersOutputSafeForTheDeclaredStrategy()`, `testEscapesOutputForAnotherStrategy()`,
/// `testSupportsANamedOutputStrategy()`, `testSupportsANormalizedNamedOutputStrategy()`,
/// `testRejectsADynamicStrategy()`, `testRejectsTheAllStrategy()` and `testOutputStrategyIsRequired()`.
#[test]
fn render_sandboxed() {
    let expected = Oracle::load("sandbox").value("sandbox_bridge_extension_test");
    let mut mismatches = Mismatches::default();
    for ((name, template, upstream), oracle) in TESTS.iter().zip(expected.as_array()) {
        let twig = create_environment(template);
        let actual = match upstream {
            Expected::Output(output) => {
                let actual = Outcome::of(twig.render("index", Array::new()));
                mismatches.check(name, &Outcome::Output((*output).to_owned()), &actual);
                actual
            }
            Expected::LoadError(message) => {
                let actual = Outcome::of(twig.load("index").map(|_| String::new()));
                match &actual {
                    Outcome::Error {
                        class,
                        message: actual_message,
                    } => {
                        mismatches.check(name, &"Twig\\Error\\SyntaxError", &class.as_str());
                        mismatches.check(name, &true, &actual_message.contains(message));
                    }
                    Outcome::Output(_) => mismatches.push(format!("{name}: the template loaded")),
                }
                actual
            }
        };
        mismatches.check(name, &tried(oracle), &actual);
    }
    mismatches.assert_none();
}

/// `testLoadsTheSandboxLazily()`: the runtime is not needed by a template that does not call `render_sandboxed()`.
#[test]
fn loads_the_sandbox_lazily() {
    let twig = env(
        &[("index", "trusted content")],
        EnvironmentOptions::default(),
    );
    twig.add_extension(Arc::new(SandboxBridgeExtension::new()))
        .unwrap();
    assert_eq!(
        twig.render("index", Array::new()).unwrap(),
        "trusted content"
    );
}
