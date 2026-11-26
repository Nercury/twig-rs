//! Twig 3.31.1's `tests/Extension/SandboxStateChangeTest.php` (`@group legacy`): the security check of a cached
//! template runs on every render, so turning the sandbox on, changing the policy or a source policy's decision after
//! a first render takes effect, also for parents, macros and includes loaded before.
//!
//! The second render of a test is compared with the oracle too (`sandbox.json`, `sandbox_state_change_test/*`).

use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use twig::deprecation::{Deprecation, DeprecationSink};
use twig::extension::runtime::CallableRuntime;
use twig::extension::{
    CallableParameter, FilterOptions, FunctionOptions, PhpCallable, TwigFilter, TwigFunction,
};
use twig::runtime::{
    Environment, SandboxExtension, SecurityError, SecurityErrorKind, SecurityPolicy,
    SecurityPolicyInterface, SourcePolicyInterface,
};
use twig::value::object::ObjectRef;
use twig::value::{Array, Value};
use twig::{ErrorKind, Source};

use super::support::{Oracle, Outcome, ctx, env, no_escape, policy, tried};

/// `build()`: `new Environment(new ArrayLoader($templates), ['cache' => false, 'autoescape' => false])` with a
/// `SandboxExtension` of the policy.
fn build(
    templates: &[(&str, &str)],
    policy: Arc<dyn SecurityPolicyInterface>,
    sandboxed: bool,
) -> (Environment, Arc<SandboxExtension>) {
    let twig = env(templates, no_escape());
    let sandbox = Arc::new(SandboxExtension::new(policy, sandboxed));
    twig.add_extension(sandbox.clone()).unwrap();
    (twig, sandbox)
}

fn render(twig: &Environment, name: &str) -> Outcome {
    Outcome::of(twig.render(name, Array::new()))
}

/// The violation of a failed render.
fn violation(result: twig::Result<String>) -> SecurityErrorKind {
    let error = result.expect_err("a sandbox violation");
    SecurityError::of(&error)
        .unwrap_or_else(|| panic!("not a sandbox violation: {error}"))
        .kind()
        .clone()
}

/// The two renders the oracle recorded for a test.
fn oracle(name: &str) -> (Outcome, Outcome) {
    let value = Oracle::load("sandbox").value(&format!("sandbox_state_change_test/{name}"));
    let results = value.as_array();
    (tried(&results[0]), tried(&results[1]))
}

fn filter_not_allowed(name: &str, template: &str) -> Outcome {
    Outcome::Error {
        class: "Twig\\Sandbox\\SecurityNotAllowedFilterError".to_owned(),
        message: format!("Filter \"{name}\" is not allowed in \"{template}\" at line 1."),
    }
}

/// `testEnableSandboxAfterFirstRender()`.
#[test]
fn enable_sandbox_after_first_render() {
    let (twig, sandbox) = build(
        &[("t", r#"{{ "foo"|upper }}"#)],
        policy(&[], &[], &[], false),
        false,
    );
    let first = render(&twig, "t");
    assert_eq!(first, Outcome::Output("FOO".to_owned()));
    sandbox.enable_sandbox(&mut Vec::<Deprecation>::new());
    let second = render(&twig, "t");
    assert_eq!(second, filter_not_allowed("upper", "t"));
    assert_eq!((first, second), oracle("enable_sandbox_after_first_render"));
}

/// `testDisableSandboxAfterFirstRender()`.
#[test]
fn disable_sandbox_after_first_render() {
    let (twig, sandbox) = build(
        &[("t", r#"{{ "foo"|upper }}"#)],
        policy(&[], &[], &[], false),
        false,
    );
    sandbox.enable_sandbox(&mut Vec::<Deprecation>::new());
    assert_eq!(
        violation(twig.render("t", Array::new())),
        SecurityErrorKind::Filter("upper".to_owned())
    );
    sandbox.disable_sandbox(&mut Vec::<Deprecation>::new());
    assert_eq!(render(&twig, "t"), Outcome::Output("FOO".to_owned()));
}

/// `testSetSecurityPolicyTightening()`.
#[test]
fn set_security_policy_tightening() {
    let (twig, sandbox) = build(
        &[("t", r#"{{ "foo"|upper }}"#)],
        policy(&[], &["upper", "escape"], &[], false),
        true,
    );
    let first = render(&twig, "t");
    assert_eq!(first, Outcome::Output("FOO".to_owned()));
    sandbox.set_security_policy(policy(&[], &["escape"], &[], false));
    let second = render(&twig, "t");
    assert_eq!(second, filter_not_allowed("upper", "t"));
    assert_eq!((first, second), oracle("set_security_policy_tightening"));
}

/// `testSetSecurityPolicyLoosening()`.
#[test]
fn set_security_policy_loosening() {
    let (twig, sandbox) = build(
        &[("t", r#"{{ "foo"|upper }}"#)],
        policy(&[], &[], &[], false),
        true,
    );
    assert_eq!(
        violation(twig.render("t", Array::new())),
        SecurityErrorKind::Filter("upper".to_owned())
    );
    sandbox.set_security_policy(policy(&[], &["upper"], &[], false));
    assert_eq!(render(&twig, "t"), Outcome::Output("FOO".to_owned()));
}

/// The source policy of `testSourcePolicyDecisionFlip()`: sandboxes the templates named in `sandbox_for`.
#[derive(Default)]
struct NamedSourcePolicy {
    sandbox_for: Mutex<Vec<String>>,
}

impl SourcePolicyInterface for NamedSourcePolicy {
    fn enable_sandbox(&self, source: &Source) -> bool {
        self.sandbox_for
            .lock()
            .unwrap()
            .iter()
            .any(|name| name == source.name())
    }
}

/// `testSourcePolicyDecisionFlip()`.
#[test]
fn source_policy_decision_flip() {
    let source_policy = Arc::new(NamedSourcePolicy::default());
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let sandbox = Arc::new(SandboxExtension::with_source_policy(
        policy(&[], &[], &[], false),
        false,
        source_policy.clone(),
        &mut deprecations,
    ));
    assert_eq!(
        deprecations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "Since twig/twig 3.27.0: The \"Twig\\Sandbox\\SourcePolicyInterface\" interface is deprecated with no replacement, do not pass an instance to \"Twig\\Extension\\SandboxExtension\"."
        ]
    );
    let twig = env(&[("t", r#"{{ "foo"|upper }}"#)], no_escape());
    twig.add_extension(sandbox).unwrap();
    assert_eq!(render(&twig, "t"), Outcome::Output("FOO".to_owned()));
    source_policy
        .sandbox_for
        .lock()
        .unwrap()
        .push("t".to_owned());
    assert_eq!(
        twig.render("t", Array::new()).unwrap_err().kind(),
        ErrorKind::SecurityNotAllowedFilter
    );
}

/// The tests that pre-warm templates with the sandbox off, then render with it on (or render another template that
/// reaches them sandboxed): name, templates, policy (tags, filters, functions), the template rendered first and
/// its output, the template rendered next and its violation, whether the sandbox is turned on in between.
struct PreWarmed {
    name: &'static str,
    templates: &'static [(&'static str, &'static str)],
    tags: &'static [&'static str],
    filters: &'static [&'static str],
    functions: &'static [&'static str],
    first: (&'static str, &'static str),
    second: &'static str,
    violation: SecurityErrorKindSpec,
    enable_sandbox: bool,
}

/// A violation by class and name.
#[derive(Clone, Copy)]
enum SecurityErrorKindSpec {
    Filter(&'static str),
    Tag(&'static str),
    Function(&'static str),
}

impl SecurityErrorKindSpec {
    fn kind(self) -> SecurityErrorKind {
        match self {
            SecurityErrorKindSpec::Filter(name) => SecurityErrorKind::Filter(name.to_owned()),
            SecurityErrorKindSpec::Tag(name) => SecurityErrorKind::Tag(name.to_owned()),
            SecurityErrorKindSpec::Function(name) => SecurityErrorKind::Function(name.to_owned()),
        }
    }
}

const PRE_WARMED: [PreWarmed; 6] = [
    PreWarmed {
        name: "pre_warmed_parent_template_through_extends",
        templates: &[
            (
                "parent.twig",
                r#"{% block c %}default{% endblock %}{{ "hi"|upper }}"#,
            ),
            (
                "child.twig",
                r#"{% extends "parent.twig" %}{% block c %}child{% endblock %}"#,
            ),
        ],
        tags: &["extends", "block"],
        filters: &[],
        functions: &[],
        first: ("child.twig", "childHI"),
        second: "child.twig",
        violation: SecurityErrorKindSpec::Filter("upper"),
        enable_sandbox: true,
    },
    PreWarmed {
        name: "pre_warmed_shared_via_sandboxed_include",
        templates: &[
            ("top", r#"{{ include("user", sandboxed=true) }}"#),
            (
                "user",
                r#"{% extends "shared" %}{% block c %}user{% endblock %}"#,
            ),
            ("shared", r#"{% block c %}{% endblock %}{{ "ok"|upper }}"#),
        ],
        tags: &["extends", "block"],
        filters: &["escape"],
        functions: &["include"],
        first: ("shared", "OK"),
        second: "top",
        violation: SecurityErrorKindSpec::Filter("upper"),
        enable_sandbox: false,
    },
    PreWarmed {
        name: "macro_from_pre_warmed_template",
        templates: &[
            (
                "macros.twig",
                "{% macro greet(name) %}{{ name|upper }}{% endmacro %}",
            ),
            (
                "caller.twig",
                r#"{% import "macros.twig" as m %}{{ m.greet("world") }}"#,
            ),
        ],
        tags: &["import", "macro"],
        filters: &[],
        functions: &[],
        first: ("caller.twig", "WORLD"),
        second: "caller.twig",
        violation: SecurityErrorKindSpec::Filter("upper"),
        enable_sandbox: true,
    },
    PreWarmed {
        name: "sandbox_tag_around_include_of_pre_warmed_template",
        templates: &[
            (
                "wrapper",
                r#"{% sandbox %}{% include "user" %}{% endsandbox %}"#,
            ),
            (
                "user",
                r#"{% extends "shared" %}{% block c %}user{% endblock %}"#,
            ),
            ("shared", r#"{% block c %}{% endblock %}{{ "ok"|upper }}"#),
        ],
        tags: &["extends", "block", "include"],
        filters: &["escape"],
        functions: &[],
        first: ("shared", "OK"),
        second: "wrapper",
        violation: SecurityErrorKindSpec::Filter("upper"),
        enable_sandbox: false,
    },
    PreWarmed {
        name: "tag_bypass_through_pre_warmed_parent",
        templates: &[
            (
                "parent.twig",
                "{% block c %}{% for i in 1..2 %}{{ i }}{% endfor %}{% endblock %}",
            ),
            (
                "child.twig",
                r#"{% extends "parent.twig" %}{% block c %}{{ parent() }}{% endblock %}"#,
            ),
        ],
        tags: &["extends", "block"],
        filters: &[],
        functions: &["parent", "range"],
        first: ("child.twig", "12"),
        second: "child.twig",
        violation: SecurityErrorKindSpec::Tag("for"),
        enable_sandbox: true,
    },
    PreWarmed {
        name: "function_bypass_through_pre_warmed_parent",
        templates: &[
            (
                "parent.twig",
                "{% block c %}{{ range(1, 2)|first }}{% endblock %}",
            ),
            (
                "child.twig",
                r#"{% extends "parent.twig" %}{% block c %}{{ parent() }}{% endblock %}"#,
            ),
        ],
        tags: &["extends", "block"],
        filters: &["first"],
        functions: &["parent"],
        first: ("child.twig", "1"),
        second: "child.twig",
        violation: SecurityErrorKindSpec::Function("range"),
        enable_sandbox: true,
    },
];

/// `testPreWarmedParentTemplateThroughExtends()`, `testPreWarmedSharedViaSandboxedInclude()`,
/// `testMacroFromPreWarmedTemplate()`, `testSandboxTagAroundIncludeOfPreWarmedTemplate()`,
/// `testTagBypassThroughPreWarmedParent()` and `testFunctionBypassThroughPreWarmedParent()`.
#[test]
fn pre_warmed_templates_are_checked_when_sandboxed() {
    let mut failures = Vec::new();
    for test in &PRE_WARMED {
        let (twig, sandbox) = build(
            test.templates,
            policy(test.tags, test.filters, test.functions, false),
            false,
        );
        let first = render(&twig, test.first.0);
        if test.enable_sandbox {
            sandbox.enable_sandbox(&mut Vec::<Deprecation>::new());
        }
        let result = twig.render(test.second, Array::new());
        let second = Outcome::of(result.clone());
        if first != Outcome::Output(test.first.1.to_owned()) {
            failures.push(format!("{}: first render {first:?}", test.name));
        }
        match result {
            Ok(output) => failures.push(format!("{}: second render gave {output:?}", test.name)),
            Err(error) => {
                if SecurityError::of(&error).map(SecurityError::kind)
                    != Some(&test.violation.kind())
                {
                    failures.push(format!("{}: second render failed with {error}", test.name));
                }
            }
        }
        if (first.clone(), second.clone()) != oracle(test.name) {
            failures.push(format!(
                "{}: {:?} differs from the oracle's {:?}",
                test.name,
                (first, second),
                oracle(test.name)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A filter or function `evil` that counts its calls and returns its argument.
fn evil_callable(calls: &Arc<AtomicUsize>) -> PhpCallable {
    let callable = PhpCallable::closure(vec![CallableParameter::required("v")]);
    let calls = Arc::clone(calls);
    CallableRuntime::new(move |call| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(call.arg(0).cloned().unwrap_or(Value::Null))
    })
    .attach(&callable);
    callable
}

const GRANDPARENT: (&str, &str) = ("grandparent.twig", "{% macro foo() %}grand{% endmacro %}");
const CALLER: (&str, &str) = (
    "caller.twig",
    r#"{% import "middle.twig" as m %}{{ m.foo() }}"#,
);

fn parent_name() -> Array {
    ctx(vec![("parent_name", Value::from("grandparent.twig"))])
}

/// `testDynamicParentFilterRejectedWhenReachedViaMacroImport()`.
#[test]
fn dynamic_parent_filter_rejected_when_reached_via_macro_import() {
    let (twig, _) = build(
        &[
            GRANDPARENT,
            ("middle.twig", "{% extends parent_name|evil %}"),
            CALLER,
        ],
        policy(&["extends", "import", "macro"], &[], &[], false),
        true,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    twig.add_filter(TwigFilter::new(
        "evil",
        Some(evil_callable(&calls)),
        FilterOptions::default(),
    ))
    .unwrap();
    let result = twig.render("caller.twig", parent_name());
    let actual = Outcome::of(result.clone());
    assert_eq!(
        violation(result),
        SecurityErrorKind::Filter("evil".to_owned())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let expected = Oracle::load("sandbox").value(
        "sandbox_state_change_test/dynamic_parent_filter_rejected_when_reached_via_macro_import",
    );
    assert_eq!(actual, tried(&expected.as_array()[0]));
}

/// `testDynamicParentFunctionRejectedWhenReachedViaMacroImport()`.
#[test]
fn dynamic_parent_function_rejected_when_reached_via_macro_import() {
    let (twig, _) = build(
        &[
            GRANDPARENT,
            ("middle.twig", "{% extends evil(parent_name) %}"),
            CALLER,
        ],
        policy(&["extends", "import", "macro"], &[], &[], false),
        true,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    twig.add_function(TwigFunction::new(
        "evil",
        Some(evil_callable(&calls)),
        FunctionOptions::default(),
    ))
    .unwrap();
    let result = twig.render("caller.twig", parent_name());
    let actual = Outcome::of(result.clone());
    assert_eq!(
        violation(result),
        SecurityErrorKind::Function("evil".to_owned())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let expected = Oracle::load("sandbox").value(
        "sandbox_state_change_test/dynamic_parent_function_rejected_when_reached_via_macro_import",
    );
    assert_eq!(actual, tried(&expected.as_array()[0]));
}

/// `testDynamicParentFilterRejectedOnPreWarmedTemplate()`.
#[test]
fn dynamic_parent_filter_rejected_on_pre_warmed_template() {
    let (twig, sandbox) = build(
        &[
            GRANDPARENT,
            ("middle.twig", "{% extends parent_name|evil %}"),
            CALLER,
        ],
        policy(&["extends", "import", "macro"], &[], &[], false),
        false,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    twig.add_filter(TwigFilter::new(
        "evil",
        Some(evil_callable(&calls)),
        FilterOptions::default(),
    ))
    .unwrap();
    assert_eq!(twig.render("caller.twig", parent_name()).unwrap(), "grand");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    sandbox.enable_sandbox(&mut Vec::<Deprecation>::new());
    assert_eq!(
        violation(twig.render("caller.twig", parent_name())),
        SecurityErrorKind::Filter("evil".to_owned())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// `CountingSecurityPolicy`: counts the `checkSecurity()` calls of the policy it wraps.
struct CountingSecurityPolicy {
    inner: Arc<SecurityPolicy>,
    call_count: AtomicUsize,
}

impl SecurityPolicyInterface for CountingSecurityPolicy {
    fn check_security(
        &self,
        tags: &[&[u8]],
        filters: &[&[u8]],
        functions: &[&[u8]],
        tests: &[&[u8]],
        deprecations: &mut dyn DeprecationSink,
    ) -> Result<(), SecurityError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.inner
            .check_security(tags, filters, functions, tests, deprecations)
    }

    fn check_method_allowed(&self, object: &ObjectRef, method: &[u8]) -> Result<(), SecurityError> {
        self.inner.check_method_allowed(object, method)
    }

    fn check_property_allowed(
        &self,
        object: &ObjectRef,
        property: &Value,
    ) -> Result<(), SecurityError> {
        self.inner.check_property_allowed(object, property)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn counting(inner: Arc<SecurityPolicy>) -> Arc<CountingSecurityPolicy> {
    Arc::new(CountingSecurityPolicy {
        inner,
        call_count: AtomicUsize::new(0),
    })
}

/// `testRepeatedRendersInStableSandboxedStateRunCheckEachTime()`.
#[test]
fn repeated_renders_in_stable_sandboxed_state_run_check_each_time() {
    let counter = counting(policy(&[], &["upper"], &[], false));
    let (twig, _) = build(&[("t", r#"{{ "x"|upper }}"#)], counter.clone(), true);
    for _ in 0..5 {
        assert_eq!(twig.render("t", Array::new()).unwrap(), "X");
    }
    assert!(counter.call_count.load(Ordering::SeqCst) >= 5);
}

/// `testNoCheckWhenSandboxRemainsOff()`.
#[test]
fn no_check_when_sandbox_remains_off() {
    let counter = counting(Arc::new(SecurityPolicy::new()));
    let (twig, _) = build(&[("t", r#"{{ "x"|upper }}"#)], counter.clone(), false);
    for _ in 0..5 {
        assert_eq!(twig.render("t", Array::new()).unwrap(), "X");
    }
    assert_eq!(counter.call_count.load(Ordering::SeqCst), 0);
}
