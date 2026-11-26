//! Twig 3.31.1's `tests/Extension/SandboxDeprecationsTest.php` (`@group legacy`): the deprecations of the stateful
//! sandbox API of `SandboxExtension` and of the `sandboxed` argument of `include()`.

use std::sync::Arc;

use twig::deprecation::Deprecation;
use twig::runtime::{SandboxExtension, SecurityPolicy};
use twig::value::Array;

use super::support::{Outcome, collecting, env, no_escape, policy, render_full};

fn messages(deprecations: &[Deprecation]) -> Vec<String> {
    deprecations.iter().map(ToString::to_string).collect()
}

/// `testEnableSandboxIsDeprecated()`.
#[test]
fn enable_sandbox_is_deprecated() {
    let extension = SandboxExtension::new(Arc::new(SecurityPolicy::new()), false);
    let mut deprecations: Vec<Deprecation> = Vec::new();
    extension.enable_sandbox(&mut deprecations);
    assert_eq!(
        messages(&deprecations),
        [
            "Since twig/twig 3.29: The \"Twig\\Extension\\SandboxExtension::enableSandbox()\" method is deprecated, use \"Twig\\Sandbox\\Sandbox\" to render untrusted templates instead."
        ]
    );
    assert!(extension.is_sandboxed(None));
}

/// `testDisableSandboxIsDeprecated()`.
#[test]
fn disable_sandbox_is_deprecated() {
    let extension = SandboxExtension::new(Arc::new(SecurityPolicy::new()), false);
    extension.checker().set_sandboxed(true);
    let mut deprecations: Vec<Deprecation> = Vec::new();
    extension.disable_sandbox(&mut deprecations);
    assert_eq!(
        messages(&deprecations),
        [
            "Since twig/twig 3.29: The \"Twig\\Extension\\SandboxExtension::disableSandbox()\" method is deprecated, use \"Twig\\Sandbox\\Sandbox\" to render untrusted templates instead."
        ]
    );
    assert!(!extension.is_sandboxed(None));
}

/// `testIsSandboxedGloballyIsDeprecated()`.
#[test]
fn is_sandboxed_globally_is_deprecated() {
    let extension = SandboxExtension::new(Arc::new(SecurityPolicy::new()), true);
    let mut deprecations: Vec<Deprecation> = Vec::new();
    assert!(extension.is_sandboxed_globally(&mut deprecations));
    assert_eq!(
        messages(&deprecations),
        [
            "Since twig/twig 3.29: The \"Twig\\Extension\\SandboxExtension::isSandboxedGlobally()\" method is deprecated, use \"Twig\\Sandbox\\Sandbox\" to render untrusted templates instead."
        ]
    );
}

/// `testIncludeFunctionSandboxedArgumentIsDeprecated()`.
#[test]
fn include_function_sandboxed_argument_is_deprecated() {
    let env = env(
        &[
            ("index", r#"{{ include("partial", sandboxed: true) }}"#),
            ("partial", "partial content"),
        ],
        collecting(no_escape()),
    );
    env.add_extension(Arc::new(SandboxExtension::new(
        policy(&[], &[], &["include"], false),
        false,
    )))
    .unwrap();
    let rendered = render_full(&env, "index", Array::new());
    assert_eq!(
        rendered.outcome,
        Outcome::Output("partial content".to_owned())
    );
    let expected = "Since twig/twig 3.29: The \"sandboxed\" argument of the \"include\" function is deprecated, use the \"render_sandboxed\" function to render untrusted templates instead.";
    assert!(
        rendered.deprecations.iter().any(|d| d == expected),
        "{:?}",
        rendered.deprecations
    );
}

/// `testIncludeFunctionFalseSandboxedArgumentIsDeprecated()`.
#[test]
fn include_function_false_sandboxed_argument_is_deprecated() {
    let env = env(
        &[
            ("index", r#"{{ include("partial", sandboxed: false) }}"#),
            ("partial", "partial content"),
        ],
        collecting(no_escape()),
    );
    let rendered = render_full(&env, "index", Array::new());
    assert_eq!(
        rendered.outcome,
        Outcome::Output("partial content".to_owned())
    );
    let expected = "Since twig/twig 3.29: The \"sandboxed\" argument of the \"include\" function is deprecated, remove the argument as \"false\" has no effect.";
    assert!(
        rendered.deprecations.iter().any(|d| d == expected),
        "{:?}",
        rendered.deprecations
    );
}
