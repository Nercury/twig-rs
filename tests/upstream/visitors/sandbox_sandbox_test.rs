//! Twig 3.31.1's `tests/Sandbox/SandboxTest.php`: `Twig\Sandbox\Sandbox`, an environment dedicated to rendering
//! untrusted templates. Where the upstream test checks an exception class or a message substring, the exact result
//! is compared with the oracle too (`sandbox.json`, `sandbox_test/*`; the test object class is named
//! `TwigRsSandboxTestObject` there and here).
//!
//! Not ported:
//!
//! - `testSecurityCheckerAddsTheSourceToPolicyErrors` calls `SecurityChecker::checkSecurity()` directly; twig-rs's
//!   checker is built by `SandboxExtension` and checks templates only: ported on the source of the error a render
//!   raises.
//! - `testAPolicyFailureWhileResolvingTraitsKeepsItsTwigContext`: its policy throws a `RuntimeException` from
//!   `checkSecurity()`; `SecurityPolicyInterface::check_security()` can only fail with a `SecurityError` (missing API).
//! - `testStream` and `testStreamBlock`: `Sandbox` has no `stream()`/`streamBlock()` (missing API); ported with the
//!   `TemplateWrapper` of the sandbox environment, which is what Twig's methods delegate to.

use std::any::Any;
use std::sync::{Arc, LazyLock};

use twig::deprecation::DeprecationSink;
use twig::escaper::DefaultStrategy;
use twig::extension::runtime::CallableRuntime;
use twig::extension::{
    CallableParameter, FunctionOptions, PhpCallable, TestOptions, TwigFunction, TwigTest,
};
use twig::loader::ArrayLoader;
use twig::runtime::{
    Environment, EnvironmentOptions, Markup, Sandbox, SandboxExtension, SecurityError,
    SecurityErrorKind, SecurityPolicy, SecurityPolicyInterface,
};
use twig::value::object::{
    CallArgs, ClassBuilder, ClassRef, Object, ObjectRef, PropertyInfo, PropertyStore, Visibility,
};
use twig::value::{Array, Diagnostics, PhpResult, PhpString, Value};
use twig::{Error, ErrorKind};

use super::support::{Mismatches, Oracle, Outcome, ctx, env as env_with, no_escape, s, tried};

/// `self::env($templates)`: `new Environment(new ArrayLoader($templates), ['autoescape' => false])`.
fn env(templates: &[(&str, &str)]) -> Environment {
    env_with(templates, no_escape())
}

/// `self::strictPolicy(...)`: a strict `SecurityPolicy` with the given lists.
#[derive(Default)]
struct Lists {
    tags: &'static [&'static str],
    filters: &'static [&'static str],
    methods: &'static [(&'static str, &'static [&'static str])],
    properties: &'static [(&'static str, &'static [&'static str])],
    functions: &'static [&'static str],
    tests: &'static [&'static str],
}

fn strict_policy(lists: Lists) -> Arc<SecurityPolicy> {
    let policy = SecurityPolicy::new();
    policy.set_allowed_tags(lists.tags.iter().copied());
    policy.set_allowed_filters(lists.filters.iter().copied());
    policy.set_allowed_methods(
        lists
            .methods
            .iter()
            .map(|(class, methods)| (*class, methods.iter().copied())),
    );
    policy.set_allowed_properties(
        lists
            .properties
            .iter()
            .map(|(class, properties)| (*class, properties.iter().copied())),
    );
    policy.set_allowed_functions(lists.functions.iter().copied());
    policy.set_allowed_tests(lists.tests.iter().copied());
    policy.set_strict(true);
    Arc::new(policy)
}

fn strict() -> Arc<SecurityPolicy> {
    strict_policy(Lists::default())
}

fn sandbox(templates: &[(&str, &str)], policy: Arc<SecurityPolicy>) -> Sandbox {
    Sandbox::new(env(templates), policy).unwrap()
}

fn oracle(name: &str) -> Outcome {
    tried(&Oracle::load("sandbox").value(&format!("sandbox_test/{name}")))
}

fn class_of(result: &twig::Result<String>) -> &str {
    result.as_ref().unwrap_err().class_name()
}

fn property(name: &str, visibility: Visibility, default: Value, class: &str) -> PropertyInfo {
    PropertyInfo {
        name: PhpString::from(name),
        visibility,
        is_static: false,
        is_readonly: false,
        type_name: None,
        default: Some(default),
        declaring_class: class.into(),
    }
}

// ---- SandboxTestObject ---------------------------------------------------------------------------------------------

const OBJECT_CLASS: &str = "TwigRsSandboxTestObject";

static OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class(OBJECT_CLASS)
        .final_class()
        .property_info(property(
            "name",
            Visibility::Public,
            Value::from("fabien"),
            OBJECT_CLASS,
        ))
        .method("getName")
        .method("__toString")
        .build()
});

/// `final class SandboxTestObject implements \Stringable { public $name = 'fabien'; public function getName():
/// string { return 'fabien'; } public function __toString(): string { return 'object'; } }`.
struct SandboxTestObject {
    properties: PropertyStore,
}

impl Object for SandboxTestObject {
    fn class(&self) -> &ClassRef {
        &OBJECT
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "getname" => Ok(Value::from("fabien")),
            "__tostring" => Ok(Value::from("object")),
            _ => Err(twig::value::object::undefined_method(&OBJECT, method)),
        }
    }
}

fn test_object() -> Value {
    Value::Object(ObjectRef::new(SandboxTestObject {
        properties: PropertyStore::new(&OBJECT),
    }))
}

// ---- the anonymous class of testJoinDoesNotExposeTheInternalsOfNonIterableObjects ---------------------------------

const SECRET_CLASS: &str = "class@anonymous";

static SECRET: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class(SECRET_CLASS)
        .property_info(property(
            "secret",
            Visibility::Private,
            Value::from("SECRET_PROPERTY"),
            SECRET_CLASS,
        ))
        .property_info(property(
            "nested",
            Visibility::Private,
            Value::Null,
            SECRET_CLASS,
        ))
        .method("__construct")
        .build()
});

struct Secret {
    properties: PropertyStore,
}

impl Object for Secret {
    fn class(&self) -> &ClassRef {
        &SECRET
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }
}

fn secret_object() -> Value {
    let properties = PropertyStore::new(&SECRET);
    properties.set(b"\0class@anonymous\0nested", test_object());
    Value::Object(ObjectRef::new(Secret { properties }))
}

// ---- tests ---------------------------------------------------------------------------------------------------------

/// `testRendersAllowedConstructs()`.
#[test]
fn renders_allowed_constructs() {
    let sandbox = sandbox(
        &[("index", "{% if greet %}Hello {{ name|upper }}{% endif %}")],
        strict_policy(Lists {
            tags: &["if"],
            filters: &["upper"],
            ..Default::default()
        }),
    );
    assert_eq!(
        sandbox
            .render(
                "index",
                ctx(vec![("greet", Value::Bool(true)), ("name", s("fabien"))])
            )
            .unwrap(),
        "Hello FABIEN"
    );
}

/// `testNonStrictSecurityPolicyIsRejected()`.
#[test]
fn non_strict_security_policy_is_rejected() {
    let error = Sandbox::new(env(&[]), Arc::new(SecurityPolicy::new())).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "The sandbox requires a strict security policy, call \"setStrict(true)\" on a dedicated policy for this sandbox."
    );
}

fn sandbox_error(result: twig::Result<Sandbox>) -> Outcome {
    Outcome::of(result.map(|_| "created".to_owned()))
}

/// `testAnAlreadyUsedEnvironmentIsRejected()`.
#[test]
fn an_already_used_environment_is_rejected() {
    let env = env(&[("index", "plain content")]);
    env.render("index", Array::new()).unwrap();
    let result = Sandbox::new(env, strict());
    let error = result.as_ref().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert!(error.to_string().contains("must be dedicated to it"));
    assert_eq!(sandbox_error(result), oracle("already_used_environment"));
}

/// `testRenderingDirectlyOnTheSandboxEnvironmentIsSandboxed()`.
#[test]
fn rendering_directly_on_the_sandbox_environment_is_sandboxed() {
    let env = env(&[("index", r#"{{ "a"|upper }}"#)]);
    Sandbox::new(env.clone(), strict()).unwrap();
    let result = env.render("index", Array::new());
    assert_eq!(
        class_of(&result),
        "Twig\\Sandbox\\SecurityNotAllowedFilterError"
    );
    assert_eq!(
        Outcome::of(result),
        oracle("rendering_directly_on_the_sandbox_environment")
    );
}

/// `testTheEnvironmentCanBeConfiguredAfterTheSandboxIsCreated()`: `shout` is the PHP function `strtoupper`.
#[test]
fn the_environment_can_be_configured_after_the_sandbox_is_created() {
    let env = env(&[("index", r#"{{ shout("hi") }}"#)]);
    let sandbox = Sandbox::new(
        env.clone(),
        strict_policy(Lists {
            functions: &["shout"],
            ..Default::default()
        }),
    )
    .unwrap();
    let callable = PhpCallable::function("strtoupper", vec![CallableParameter::required("string")]);
    CallableRuntime::new(|call| {
        let value = call
            .arg(0)
            .and_then(Value::as_str)
            .cloned()
            .unwrap_or_default();
        Ok(Value::String(PhpString::from(
            value.as_bytes().to_ascii_uppercase(),
        )))
    })
    .attach(&callable);
    env.add_function(TwigFunction::new(
        "shout",
        Some(callable),
        FunctionOptions::default(),
    ))
    .unwrap();
    assert_eq!(sandbox.render("index", Array::new()).unwrap(), "HI");
}

/// The custom policy of `testCustomSecurityPolicyImplementationsAreAccepted()`: rejects the first filter.
struct FirstFilterPolicy;

impl SecurityPolicyInterface for FirstFilterPolicy {
    fn check_security(
        &self,
        _tags: &[&[u8]],
        filters: &[&[u8]],
        _functions: &[&[u8]],
        _tests: &[&[u8]],
        _deprecations: &mut dyn DeprecationSink,
    ) -> Result<(), SecurityError> {
        match filters.first() {
            Some(filter) => {
                let name = String::from_utf8_lossy(filter).into_owned();
                Err(SecurityError::new(
                    SecurityErrorKind::Filter(name.clone()),
                    format!("Filter \"{name}\" is not allowed."),
                ))
            }
            None => Ok(()),
        }
    }

    fn check_method_allowed(
        &self,
        _object: &ObjectRef,
        _method: &[u8],
    ) -> Result<(), SecurityError> {
        Ok(())
    }

    fn check_property_allowed(
        &self,
        _object: &ObjectRef,
        _property: &Value,
    ) -> Result<(), SecurityError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `testCustomSecurityPolicyImplementationsAreAccepted()`.
#[test]
fn custom_security_policy_implementations_are_accepted() {
    let sandbox = Sandbox::new(
        env(&[
            ("static", "static text"),
            ("filtered", r#"{{ "a"|upper }}"#),
        ]),
        Arc::new(FirstFilterPolicy),
    )
    .unwrap();
    assert_eq!(
        sandbox.render("static", Array::new()).unwrap(),
        "static text"
    );
    let result = sandbox.render("filtered", Array::new());
    assert_eq!(
        class_of(&result),
        "Twig\\Sandbox\\SecurityNotAllowedFilterError"
    );
}

/// `testDisallowedTagIsRejected()`, `testDisallowedFilterIsRejected()`, `testDisallowedFunctionIsRejected()` and
/// `testDisallowedTestIsRejected()`.
#[test]
fn disallowed_constructs_are_rejected() {
    let mut mismatches = Mismatches::default();
    for (name, template, class, message) in [
        (
            "disallowed_tag",
            "{% if 1 %}x{% endif %}",
            "Twig\\Sandbox\\SecurityNotAllowedTagError",
            "Tag \"if\" is not allowed",
        ),
        (
            "disallowed_filter",
            r#"{{ "a"|upper }}"#,
            "Twig\\Sandbox\\SecurityNotAllowedFilterError",
            "Filter \"upper\" is not allowed",
        ),
        (
            "disallowed_function",
            "{{ max(1, 2) }}",
            "Twig\\Sandbox\\SecurityNotAllowedFunctionError",
            "Function \"max\" is not allowed",
        ),
        (
            "disallowed_test",
            r#"{{ 1 is funky ? "y" : "n" }}"#,
            "Twig\\Sandbox\\SecurityNotAllowedTestError",
            "Test \"funky\" is not allowed",
        ),
    ] {
        let env = env(&[("index", template)]);
        env.add_test(funky()).unwrap();
        let result = Sandbox::new(env, strict())
            .unwrap()
            .render("index", Array::new());
        match &result {
            Ok(output) => mismatches.push(format!("{name}: rendered {output:?}")),
            Err(error) => {
                mismatches.check(name, &class, &error.class_name());
                mismatches.check(name, &true, &error.to_string().contains(message));
            }
        }
        mismatches.check(name, &oracle(name), &Outcome::of(result));
    }
    mismatches.assert_none();
}

/// `new TwigTest('funky', static fn ($value) => true)`.
fn funky() -> TwigTest {
    let callable = PhpCallable::closure(vec![CallableParameter::required("value")]);
    CallableRuntime::new(|_| Ok(Value::Bool(true))).attach(&callable);
    TwigTest::new("funky", Some(callable), TestOptions::default())
}

/// `testSecurityCheckerAddsTheSourceToPolicyErrors()`, on the error of a render: the violation carries the source of
/// the template. Upstream passes its own `Source` to `checkSecurity()`; through a render the source is the compiled
/// template's `getSourceContext()`, whose code is `''` unless `debug` is on (checked with the oracle).
#[test]
fn security_checker_adds_the_source_to_policy_errors() {
    let error = sandbox(&[("index", "{% if 1 %}x{% endif %}")], strict())
        .render("index", Array::new())
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SecurityNotAllowedTag);
    let source = error.source_context().expect("a source");
    assert_eq!(source.name(), "index");
    assert_eq!(source.code(), b"");
}

/// `testAllowedTestIsAccepted()`.
#[test]
fn allowed_test_is_accepted() {
    let env = env(&[("index", r#"{{ 1 is funky ? "y" : "n" }}"#)]);
    env.add_test(funky()).unwrap();
    let sandbox = Sandbox::new(
        env,
        strict_policy(Lists {
            tests: &["funky"],
            ..Default::default()
        }),
    )
    .unwrap();
    assert_eq!(sandbox.render("index", Array::new()).unwrap(), "y");
}

/// The allowing and denying renders of the method, property and `__toString` tests.
fn governed(
    template: &str,
    allowing: Lists,
    denying: Lists,
    context: impl Fn() -> Array,
) -> (twig::Result<String>, twig::Result<String>) {
    (
        sandbox(&[("index", template)], strict_policy(allowing)).render("index", context()),
        sandbox(&[("index", template)], strict_policy(denying)).render("index", context()),
    )
}

fn object_context() -> Array {
    ctx(vec![("obj", test_object())])
}

/// `testMethodCallsAreGovernedByThePolicy()`.
#[test]
fn method_calls_are_governed_by_the_policy() {
    let (allowing, denying) = governed(
        "{{ obj.getName() }}",
        Lists {
            methods: &[(OBJECT_CLASS, &["getName"])],
            ..Default::default()
        },
        Lists::default(),
        object_context,
    );
    assert_eq!(allowing.unwrap(), "fabien");
    assert_eq!(
        class_of(&denying),
        "Twig\\Sandbox\\SecurityNotAllowedMethodError"
    );
    assert_eq!(Outcome::of(denying), oracle("method_denied"));
}

/// `testPropertyAccessesAreGovernedByThePolicy()`.
#[test]
fn property_accesses_are_governed_by_the_policy() {
    let (allowing, denying) = governed(
        "{{ obj.name }}",
        Lists {
            properties: &[(OBJECT_CLASS, &["name"])],
            ..Default::default()
        },
        Lists::default(),
        object_context,
    );
    assert_eq!(allowing.unwrap(), "fabien");
    assert_eq!(
        class_of(&denying),
        "Twig\\Sandbox\\SecurityNotAllowedPropertyError"
    );
    assert_eq!(Outcome::of(denying), oracle("property_denied"));
}

/// `testToStringCoercionIsGovernedByThePolicy()`.
#[test]
fn to_string_coercion_is_governed_by_the_policy() {
    let (allowing, denying) = governed(
        "{{ obj }}",
        Lists {
            methods: &[(OBJECT_CLASS, &["__toString"])],
            ..Default::default()
        },
        Lists::default(),
        object_context,
    );
    assert_eq!(allowing.unwrap(), "object");
    assert_eq!(
        class_of(&denying),
        "Twig\\Sandbox\\SecurityNotAllowedMethodError"
    );
    assert_eq!(Outcome::of(denying), oracle("to_string_denied"));
}

/// `testJoinDoesNotExposeTheInternalsOfNonIterableObjects()`: a `RuntimeError` matching `could not be converted to
/// string`.
#[test]
fn join_does_not_expose_the_internals_of_non_iterable_objects() {
    let result = sandbox(
        &[("index", r#"{{ obj|join(",") }}"#)],
        strict_policy(Lists {
            filters: &["join"],
            ..Default::default()
        }),
    )
    .render("index", ctx(vec![("obj", secret_object())]));
    let error = result.as_ref().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Runtime);
    assert!(
        error
            .to_string()
            .contains("could not be converted to string"),
        "{error}"
    );
    assert_eq!(Outcome::of(result), oracle("join_of_non_iterable_object"));
}

/// `testJoinOfAStringableObjectIsGovernedByThePolicy()`.
#[test]
fn join_of_a_stringable_object_is_governed_by_the_policy() {
    let (allowing, denying) = governed(
        r#"{{ obj|join(",") }}"#,
        Lists {
            filters: &["join"],
            methods: &[(OBJECT_CLASS, &["__toString"])],
            ..Default::default()
        },
        Lists {
            filters: &["join"],
            ..Default::default()
        },
        object_context,
    );
    assert_eq!(allowing.unwrap(), "object");
    assert_eq!(
        class_of(&denying),
        "Twig\\Sandbox\\SecurityNotAllowedMethodError"
    );
    assert_eq!(Outcome::of(denying), oracle("join_of_stringable_denied"));
}

fn markup(content: &str, strategies: Option<Vec<&str>>) -> Value {
    Value::Object(Markup::create(
        PhpString::from(content),
        Some(PhpString::from("UTF-8")),
        strategies.map(|list| list.into_iter().map(PhpString::from).collect()),
    ))
}

/// `testMarkupValuesCanBePrinted()`.
#[test]
fn markup_values_can_be_printed() {
    let sandbox = sandbox(&[("index", "{{ m }}")], strict());
    assert_eq!(
        sandbox
            .render("index", ctx(vec![("m", markup("<b>b</b>", None))]))
            .unwrap(),
        "<b>b</b>"
    );
}

/// `testMarkupSafeStrategiesCanBeChecked()`.
#[test]
fn markup_safe_strategies_can_be_checked() {
    let sandbox = sandbox(
        &[("index", r#"{{ m.isSafeFor("html") ? "yes" : "no" }}"#)],
        strict(),
    );
    assert_eq!(
        sandbox
            .render("index", ctx(vec![("m", markup("b", Some(vec!["html"])))]))
            .unwrap(),
        "yes"
    );
}

/// `testMarkupWithContentIsGovernedByThePolicy()`.
#[test]
fn markup_with_content_is_governed_by_the_policy() {
    let (allowing, denying) = governed(
        r#"{{ m.withContent("<b>b</b>") }}"#,
        Lists {
            methods: &[("Twig\\Markup", &["withContent"])],
            ..Default::default()
        },
        Lists::default(),
        || ctx(vec![("m", markup("a", None))]),
    );
    assert_eq!(allowing.unwrap(), "<b>b</b>");
    let message = denying.as_ref().unwrap_err().to_string();
    assert!(
        message
            .contains("Calling \"withcontent\" method on a \"Twig\\Markup\" object is not allowed"),
        "{message}"
    );
    assert_eq!(Outcome::of(denying), oracle("markup_with_content_denied"));
}

/// `testIncludedTemplatesAreSandboxed()`.
#[test]
fn included_templates_are_sandboxed() {
    let result = sandbox(
        &[
            ("index", r#"{{ include("partial") }}"#),
            ("partial", r#"{{ "a"|upper }}"#),
        ],
        strict_policy(Lists {
            functions: &["include"],
            ..Default::default()
        }),
    )
    .render("index", Array::new());
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("Filter \"upper\" is not allowed")
    );
    assert_eq!(
        Outcome::of(result),
        oracle("included_templates_are_sandboxed")
    );
}

/// A data set of `provideForeignTemplateWrapperUsages()`: name, template, foreign template, allowed tags and
/// functions.
type ForeignUsage = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

/// `provideForeignTemplateWrapperUsages()`.
const FOREIGN_TEMPLATE_WRAPPER_USAGES: [ForeignUsage; 9] = [
    (
        "include tag",
        "{% include foreign %}",
        "foreign content",
        &["include"],
        &[],
    ),
    (
        "include function",
        "{{ include(foreign) }}",
        "foreign content",
        &[],
        &["include"],
    ),
    (
        "include function fallback",
        r#"{{ include(["missing", foreign]) }}"#,
        "foreign content",
        &[],
        &["include"],
    ),
    (
        "include_only function",
        "{{ include_only(foreign) }}",
        "foreign content",
        &[],
        &["include_only"],
    ),
    (
        "extends tag",
        "{% extends foreign %}",
        "foreign content",
        &["extends"],
        &[],
    ),
    (
        "embed tag",
        "{% embed foreign %}{% endembed %}",
        "foreign content",
        &["embed", "extends"],
        &[],
    ),
    (
        "import tag",
        "{% import foreign as macros %}{{ macros.foo() }}",
        "{% macro foo() %}foreign content{% endmacro %}",
        &["import"],
        &[],
    ),
    (
        "from tag",
        "{% from foreign import foo %}{{ foo() }}",
        "{% macro foo() %}foreign content{% endmacro %}",
        &["from"],
        &[],
    ),
    (
        "block function",
        r#"{{ block("content", foreign) }}"#,
        "{% block content %}foreign content{% endblock %}",
        &[],
        &["block"],
    ),
];

/// `testRejectsTemplateWrapperFromAnotherEnvironment()`: a `RuntimeError` containing `can only be used with the
/// "Twig\Environment" that created it`.
#[test]
fn rejects_template_wrapper_from_another_environment() {
    let mut mismatches = Mismatches::default();
    for (i, (name, template, foreign_template, tags, functions)) in
        FOREIGN_TEMPLATE_WRAPPER_USAGES.iter().enumerate()
    {
        let foreign = env(&[("foreign", foreign_template)]);
        let policy = SecurityPolicy::new();
        policy.set_allowed_tags(tags.iter().copied());
        policy.set_allowed_functions(functions.iter().copied());
        policy.set_strict(true);
        let sandbox = Sandbox::new(env(&[("index", template)]), Arc::new(policy)).unwrap();
        let wrapper = foreign.load("foreign").unwrap().to_value();
        let result = sandbox.render("index", ctx(vec![("foreign", wrapper)]));
        match &result {
            Ok(output) => mismatches.push(format!("{name}: rendered {output:?}")),
            Err(error) => {
                mismatches.check(name, &ErrorKind::Runtime, &error.kind());
                mismatches.check(
                    name,
                    &true,
                    &error.to_string().contains(
                        "can only be used with the \"Twig\\Environment\" that created it",
                    ),
                );
            }
        }
        mismatches.check(
            name,
            &oracle(&format!("foreign_template_wrapper/{i}")),
            &Outcome::of(result),
        );
    }
    mismatches.assert_none();
}

/// `testTheExtendsTagMustBeAllowed()`.
#[test]
fn the_extends_tag_must_be_allowed() {
    let templates = [
        ("index", r#"{% extends "layout" %}"#),
        ("layout", "layout content"),
    ];
    let allowing = sandbox(
        &templates,
        strict_policy(Lists {
            tags: &["extends"],
            ..Default::default()
        }),
    );
    assert_eq!(
        allowing.render("index", Array::new()).unwrap(),
        "layout content"
    );
    let result = sandbox(&templates, strict()).render("index", Array::new());
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("Tag \"extends\" is not allowed")
    );
    assert_eq!(Outcome::of(result), oracle("extends_denied"));
}

/// `testTheUseTagMustBeAllowed()`.
#[test]
fn the_use_tag_must_be_allowed() {
    let allowing = sandbox(
        &[
            (
                "index",
                r#"{% use "blocks" with content as base_content %}{{ block("base_content") }}"#,
            ),
            ("blocks", "{% block content %}trait content{% endblock %}"),
        ],
        strict_policy(Lists {
            tags: &["use", "block"],
            functions: &["block"],
            ..Default::default()
        }),
    );
    assert_eq!(
        allowing.render("index", Array::new()).unwrap(),
        "trait content"
    );
    let result = sandbox(
        &[(
            "index",
            r#"{% use "missing" with content as base_content %}"#,
        )],
        strict(),
    )
    .render("index", Array::new());
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("Tag \"use\" is not allowed")
    );
    assert_eq!(Outcome::of(result), oracle("use_denied"));
}

/// `testTheUseTagIsCheckedWithoutCheckingTheRestOfThePolicy()`.
#[test]
fn the_use_tag_is_checked_without_checking_the_rest_of_the_policy() {
    let sandbox = sandbox(
        &[
            (
                "index",
                r#"{% use "middle" %}{% block content %}SAFE{% endblock %}"#,
            ),
            (
                "middle",
                r#"{% use "leaf" %}{% block content %}{{ "bad"|upper }}{% endblock %}"#,
            ),
            ("leaf", ""),
        ],
        strict_policy(Lists {
            tags: &["use", "block"],
            ..Default::default()
        }),
    );
    assert_eq!(sandbox.render("index", Array::new()).unwrap(), "SAFE");
}

/// `testTheUseTagIsCheckedBeforeTheTraitTemplateIsLoaded()`.
#[test]
fn the_use_tag_is_checked_before_the_trait_template_is_loaded() {
    let result = sandbox(
        &[
            (
                "index",
                r#"{{ block("b", "receiver") is defined ? "YES" : "NO" }}"#,
            ),
            ("receiver", r#"{% use "missing" %}"#),
        ],
        strict_policy(Lists {
            tags: &["block"],
            functions: &["block"],
            ..Default::default()
        }),
    )
    .render("index", Array::new());
    assert_eq!(
        class_of(&result),
        "Twig\\Sandbox\\SecurityNotAllowedTagError"
    );
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("Tag \"use\" is not allowed")
    );
    assert_eq!(
        Outcome::of(result),
        oracle("use_checked_before_the_trait_is_loaded")
    );
}

/// `testARenderOnAnotherEnvironmentDuringASandboxedRenderIsNotSandboxed()`.
#[test]
fn a_render_on_another_environment_during_a_sandboxed_render_is_not_sandboxed() {
    let app = env(&[("trusted", "{{ value|upper }}")]);
    let env = env(&[("index", "{{ render_trusted() }}")]);
    let callable = PhpCallable::closure(Vec::new());
    CallableRuntime::new(move |_| {
        app.render("trusted", ctx(vec![("value", s("ok"))]))
            .map(Value::from)
            .map_err(twig::vm::Exception::from)
    })
    .attach(&callable);
    env.add_function(TwigFunction::new(
        "render_trusted",
        Some(callable),
        FunctionOptions::default(),
    ))
    .unwrap();
    let sandbox = Sandbox::new(
        env,
        strict_policy(Lists {
            functions: &["render_trusted"],
            ..Default::default()
        }),
    )
    .unwrap();
    assert_eq!(sandbox.render("index", Array::new()).unwrap(), "OK");
}

/// `testAnEnvironmentWithASandboxExtensionIsRejected()`.
#[test]
fn an_environment_with_a_sandbox_extension_is_rejected() {
    let env = env(&[]);
    env.add_extension(Arc::new(SandboxExtension::new(strict(), false)))
        .unwrap();
    let result = Sandbox::new(env, strict());
    let error = result.as_ref().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert!(error.to_string().contains("must be dedicated to it"));
    assert_eq!(
        sandbox_error(result),
        oracle("environment_with_a_sandbox_extension")
    );
}

/// `testAutoEscapingRequiresTheEscapeFilterToBeAllowed()`: with Twig's default options (auto-escaping on).
#[test]
fn auto_escaping_requires_the_escape_filter_to_be_allowed() {
    let templates = [("index", "{{ v }}")];
    let denying = Sandbox::new(
        env_with(&templates, EnvironmentOptions::default()),
        strict(),
    )
    .unwrap();
    let result = denying.render("index", ctx(vec![("v", s("a&b"))]));
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("Filter \"escape\" is not allowed")
    );
    assert_eq!(
        Outcome::of(result),
        oracle("auto_escaping_requires_the_escape_filter")
    );
    let allowing = Sandbox::new(
        env_with(
            &templates,
            EnvironmentOptions {
                autoescape: DefaultStrategy::html(),
                ..Default::default()
            },
        ),
        strict_policy(Lists {
            filters: &["escape"],
            ..Default::default()
        }),
    )
    .unwrap();
    assert_eq!(
        allowing
            .render("index", ctx(vec![("v", s("a&b"))]))
            .unwrap(),
        "a&amp;b"
    );
}

fn greeting_sandbox(templates: &[(&str, &str)], tags: &'static [&'static str]) -> Sandbox {
    sandbox(
        templates,
        strict_policy(Lists {
            tags,
            filters: &["upper"],
            ..Default::default()
        }),
    )
}

fn fabien() -> Array {
    ctx(vec![("name", s("fabien"))])
}

const BLOCKS: (&str, &str) = (
    "blocks",
    "{% block greeting %}Hello {{ name|upper }}{% endblock %} not part of the block",
);

/// `testDisplay()`.
#[test]
fn display() {
    let sandbox = greeting_sandbox(&[("index", "Hello {{ name|upper }}")], &[]);
    let mut output: Vec<u8> = Vec::new();
    sandbox.display("index", fabien(), &mut output).unwrap();
    assert_eq!(output, b"Hello FABIEN");
}

/// `testStream()`, through the template wrapper (`Sandbox` has no `stream()`).
#[test]
fn stream() {
    let sandbox = greeting_sandbox(&[("index", "Hello {{ name|upper }}")], &[]);
    let mut output: Vec<u8> = Vec::new();
    sandbox
        .environment()
        .load("index")
        .unwrap()
        .stream(fabien(), &mut output)
        .unwrap();
    assert_eq!(output, b"Hello FABIEN");
}

/// `testRenderBlock()`.
#[test]
fn render_block() {
    let sandbox = greeting_sandbox(&[BLOCKS], &["block"]);
    assert_eq!(
        sandbox
            .render_block("blocks", "greeting", fabien())
            .unwrap(),
        "Hello FABIEN"
    );
}

/// `testDisplayBlock()`.
#[test]
fn display_block() {
    let sandbox = greeting_sandbox(&[BLOCKS], &["block"]);
    let mut output: Vec<u8> = Vec::new();
    sandbox
        .display_block("blocks", "greeting", fabien(), &mut output)
        .unwrap();
    assert_eq!(output, b"Hello FABIEN");
}

/// `testStreamBlock()`, through the template wrapper (`Sandbox` has no `streamBlock()`).
#[test]
fn stream_block() {
    let sandbox = greeting_sandbox(&[BLOCKS], &["block"]);
    let mut output: Vec<u8> = Vec::new();
    sandbox
        .environment()
        .load("blocks")
        .unwrap()
        .stream_block("greeting", fabien(), &mut output)
        .unwrap();
    assert_eq!(output, b"Hello FABIEN");
}

/// `testCreateTemplateIsSandboxed()`.
#[test]
fn create_template_is_sandboxed() {
    let sandbox = sandbox(
        &[],
        strict_policy(Lists {
            filters: &["upper"],
            ..Default::default()
        }),
    );
    assert_eq!(
        sandbox
            .create_template(b"{{ name|upper }}", None)
            .unwrap()
            .render(fabien())
            .unwrap(),
        "FABIEN"
    );
    let result: Result<String, Error> = sandbox
        .create_template(b"{{ name|lower }}", None)
        .and_then(|template| template.render(fabien()));
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("Filter \"lower\" is not allowed")
    );
    assert_eq!(Outcome::of(result), oracle("create_template_denied"));
}

/// `testCreateTemplateCanReferenceLoaderTemplates()`.
#[test]
fn create_template_can_reference_loader_templates() {
    let sandbox = Sandbox::new(
        Environment::with_options(
            ArrayLoader::new([("partial", "from the loader")]),
            no_escape(),
        ),
        strict_policy(Lists {
            functions: &["include"],
            ..Default::default()
        }),
    )
    .unwrap();
    assert_eq!(
        sandbox
            .create_template(br#"{{ include("partial") }}"#, None)
            .unwrap()
            .render(Array::new())
            .unwrap(),
        "from the loader"
    );
}
