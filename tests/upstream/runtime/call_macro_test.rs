//! Port of TWIG/tests/CallMacroTest.php.
//!
//! The PHPUnit test calls `Template::getMacroNamespace()->call($name, $arguments, [], 1, new Source('', 'index'))`
//! and `->has($name, [])`; the ports make the same calls on [`TemplateWrapper::macro_namespace`]. The outputs, errors
//! and deprecations are those of the oracle (the PHPUnit expectations, which only check the start of some messages,
//! are compared in full; `tests/vectors/upstream/runtime/host_api.json`, `macro-ns-*`).
//!
//! Not ported: from `testMacroNamespaceOnlyExposesMacroOperations`, the PHP reflection of `MacroNamespace`'s public
//! methods (its other assertions are ported).

use twig::extension::CoreExtension;
use twig::runtime::{MacroNamespace, SecurityErrorKind, SecurityPolicy, SecurityPolicyInterface};
use twig::value::{Array, Ignore, Value, convert};
use twig::{ErrorKind, Source, TemplateWrapper};

use crate::support::*;

/// `$twig->load('index')->unwrap($twig)`: the environment and the template `index`.
fn load(templates: &[(&str, &str)]) -> (twig::Environment, TemplateWrapper) {
    let twig = env(templates);
    let template = twig.load("index").unwrap();
    template.unwrap(Some(&twig)).unwrap();
    (twig, template)
}

/// `$template->getMacroNamespace()->call($name, $arguments, [], 1, new Source('', 'index'))`.
fn call_macro(namespace: &MacroNamespace, name: &str, arguments: Array) -> twig::Result<Value> {
    namespace.call(name, arguments, &Array::new(), 1, &Source::new("", "index"))
}

/// `(string) $result`.
fn string(value: &Value) -> String {
    convert::to_string(value, &mut Ignore)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// A list of arguments.
fn list(values: &[&str]) -> Array {
    Array::from_values(values.iter().map(|value| Value::from(*value)))
}

#[test]
fn test_call_macro_resolves_positional_and_named_arguments() {
    let (_, template) = load(&[(
        "index",
        "{% macro greet(name, greeting = \"Hello\") %}{{ greeting }} {{ name }}{% endmacro %}",
    )]);
    let namespace = template.macro_namespace();
    assert_eq!(
        string(&call_macro(&namespace, "greet", list(&["World"])).unwrap()),
        "Hello World"
    );
    assert_eq!(
        string(
            &call_macro(
                &namespace,
                "greet",
                ctx(&[
                    ("name", Value::from("World")),
                    ("greeting", Value::from("Hi"))
                ])
            )
            .unwrap()
        ),
        "Hi World"
    );
}

#[test]
fn test_macro_namespace_only_exposes_macro_operations() {
    let (_, template) = load(&[(
        "index",
        "{% macro greet(name) %}Hi {{ name }}{% endmacro %}",
    )]);
    let namespace = template.macro_namespace();
    assert!(namespace.has("greet", &Array::new()).unwrap());
    assert_eq!(
        string(
            &namespace
                .call(
                    "greet",
                    list(&["World"]),
                    &Array::new(),
                    1,
                    &Source::new("", "index")
                )
                .unwrap()
        ),
        "Hi World"
    );
    // `assertSame($namespace, $template->getMacroNamespace())`
    let (Value::Object(first), Value::Object(second)) =
        (namespace.to_value(), template.macro_namespace().to_value())
    else {
        panic!("a macro namespace is an object");
    };
    assert!(twig::value::object::ObjectRef::ptr_eq(&first, &second));
    // `assertInstanceOf(MacroNamespace::class, $namespace)`
    assert!(first.instance_of("Twig\\MacroNamespace"));
}

#[test]
fn test_macro_namespace_does_not_benefit_from_the_template_sandbox_exemption() {
    let policy = SecurityPolicy::new();
    let (_, template) = load(&[("index", "")]);
    let Value::Object(namespace) = template.macro_namespace().to_value() else {
        panic!("a macro namespace is an object");
    };
    let error = policy
        .check_method_allowed(&namespace, b"call")
        .unwrap_err();
    assert!(matches!(error.kind(), SecurityErrorKind::Method { .. }));
    assert_eq!(
        error.message(),
        "Calling \"call\" method on a \"Twig\\MacroNamespace\" object is not allowed."
    );
}

#[test]
fn test_call_macro_looks_macros_up_in_parent_templates() {
    let (_, template) = load(&[
        ("index", "{% extends \"parent\" %}"),
        (
            "parent",
            "{% macro greet(name) %}Hi {{ name }}{% endmacro %}",
        ),
    ]);
    assert_eq!(
        string(&call_macro(&template.macro_namespace(), "greet", list(&["World"])).unwrap()),
        "Hi World"
    );
}

#[test]
fn test_lenient_macro_call_reports_a_deprecation_at_the_call_site() {
    let twig = env(&[(
        "index",
        "{% from _self import greet %}\n{% macro greet(name) %}{% endmacro %}\n{{ greet('a', 'b') }}",
    )]);
    let (_, deprecations) =
        collect_deprecations(&twig, || twig.render("index", Array::new()).unwrap());
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Passing more arguments than the macro \"greet\" accepts is deprecated and will throw in Twig 4.0; declare a variadic argument (\"...name\") in the macro definition to accept extra arguments (in \"index\" at line 3)."
        ]
    );
}

#[test]
fn test_calling_a_macro_with_a_case_mismatched_name_triggers_a_deprecation() {
    let twig = env(&[(
        "index",
        "{% import _self as m %}\n{% macro greet(name = '') %}Hi {{ name }}{% endmacro %}\n{{ m.GREET('World') }}",
    )]);
    let (output, deprecations) =
        collect_deprecations(&twig, || twig.render("index", Array::new()).unwrap());
    assert_eq!(output.trim(), "Hi World");
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Calling the macro \"greet\" (defined in template \"index\") as \"GREET\" is deprecated; macro names will be case-sensitive in Twig 4.0."
        ]
    );
}

#[test]
fn test_call_macro_looks_case_mismatched_macros_up_in_parent_templates() {
    let (twig, template) = load(&[
        ("index", "{% extends \"parent\" %}"),
        (
            "parent",
            "{% macro greet(name = \"\") %}Hi {{ name }}{% endmacro %}",
        ),
    ]);
    let (output, deprecations) = collect_deprecations(&twig, || {
        string(&call_macro(&template.macro_namespace(), "Greet", list(&["World"])).unwrap())
    });
    assert_eq!(output, "Hi World");
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Calling the macro \"greet\" (defined in template \"parent\") as \"Greet\" is deprecated; macro names will be case-sensitive in Twig 4.0."
        ]
    );
}

#[test]
fn test_has_macro_deprecates_a_case_mismatched_match_only() {
    let (twig, template) = load(&[(
        "index",
        "{% macro greet(name) %}Hi {{ name }}{% endmacro %}",
    )]);
    let (_, deprecations) = collect_deprecations(&twig, || {
        let namespace = template.macro_namespace();
        assert!(namespace.has("greet", &Array::new()).unwrap());
        assert!(namespace.has("GREET", &Array::new()).unwrap());
        assert!(!namespace.has("missing", &Array::new()).unwrap());
    });
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Testing whether the macro \"greet\" (defined in template \"index\") is defined as \"GREET\" is deprecated; macro names will be case-sensitive in Twig 4.0 and this test will return false."
        ],
        "The \"is defined\" test must deprecate only a case-mismatched macro name."
    );
}

#[test]
fn test_call_macro_resolves_a_legacy_prefixed_name_with_a_deprecation() {
    let (twig, template) = load(&[(
        "index",
        "{% macro greet(name = \"\") %}Hi {{ name }}{% endmacro %}",
    )]);
    let (output, deprecations) = collect_deprecations(&twig, || {
        string(&call_macro(&template.macro_namespace(), "macro_greet", list(&["World"])).unwrap())
    });
    assert_eq!(output, "Hi World");
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Calling the macro \"greet\" via the \"macro_\"-prefixed name \"macro_greet\" is deprecated; pass the bare macro name to \"Twig\\Node\\Expression\\MacroReferenceExpression\" instead."
        ]
    );
}

#[test]
fn test_call_macro_prefers_a_macro_actually_named_with_the_prefix() {
    let (twig, template) = load(&[(
        "index",
        "{% macro macro_greet(name = \"\") %}Prefixed {{ name }}{% endmacro %}{% macro greet(name = \"\") %}Bare {{ name }}{% endmacro %}",
    )]);
    let (output, deprecations) = collect_deprecations(&twig, || {
        string(&call_macro(&template.macro_namespace(), "macro_greet", list(&["World"])).unwrap())
    });
    assert_eq!(output, "Prefixed World");
    assert!(deprecations.is_empty(), "{deprecations:?}");
}

#[test]
fn test_call_macro_throws_with_the_prefixed_name_when_neither_name_exists() {
    let (_, template) = load(&[("index", "no macro here")]);
    let error = expect_error(
        call_macro(&template.macro_namespace(), "macro_missing", Array::new()),
        ErrorKind::Runtime,
    );
    // the oracle's whole message (PHPUnit checks its start): the line and source given to call()
    assert_eq!(
        error.to_string(),
        "Macro \"macro_missing\" is not defined in template \"index\" in \"index\" at line 1."
    );
}

#[test]
fn test_has_macro_resolves_a_legacy_prefixed_name_with_a_deprecation() {
    let (twig, template) = load(&[(
        "index",
        "{% macro greet(name) %}Hi {{ name }}{% endmacro %}",
    )]);
    let (_, deprecations) = collect_deprecations(&twig, || {
        let namespace = template.macro_namespace();
        assert!(namespace.has("macro_greet", &Array::new()).unwrap());
        assert!(!namespace.has("macro_missing", &Array::new()).unwrap());
    });
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Testing whether the macro \"greet\" is defined via the \"macro_\"-prefixed name \"macro_greet\" is deprecated; pass the bare macro name to \"Twig\\Node\\Expression\\MacroReferenceExpression\" instead."
        ]
    );
}

#[test]
fn test_call_macro_throws_for_an_unknown_macro() {
    let (_, template) = load(&[("index", "no macro here")]);
    let error = expect_error(
        call_macro(&template.macro_namespace(), "missing", Array::new()),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Macro \"missing\" is not defined in template \"index\" in \"index\" at line 1."
    );
    // a source of another name and no line (oracle)
    let error = expect_error(
        template.macro_namespace().call(
            "missing",
            Array::new(),
            &Array::new(),
            -1,
            &Source::new("", "other"),
        ),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Macro \"missing\" is not defined in template \"index\" in \"other\"."
    );
}

#[test]
fn test_rendering_a_block_on_its_own_reports_macros_imported_in_the_template_body() {
    let (_, template) = load(&[
        (
            "index",
            "{% import \"macros\" as helpers %}{% block field %}{{ helpers.label() }}{% endblock %}",
        ),
        ("macros", "{% macro label() %}label{% endmacro %}"),
    ]);
    let error = expect_error(
        template.render_block("field", Array::new()),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Macros imported in the body of template \"index\" are not available because the body was not rendered; move the \"import\" or \"from\" tag inside the block or the macro that uses it in \"index\" at line 1."
    );
}

#[test]
fn test_rendering_a_block_on_its_own_uses_macros_imported_in_the_block() {
    let (_, template) = load(&[
        (
            "index",
            "{% block field %}{% import \"macros\" as helpers %}{{ helpers.label() }}{% endblock %}",
        ),
        ("macros", "{% macro label() %}label{% endmacro %}"),
    ]);
    assert_eq!(
        template.render_block("field", Array::new()).unwrap(),
        "label"
    );
}

#[test]
fn test_calling_a_macro_on_its_own_reports_macros_imported_in_the_template_body() {
    let (_, template) = load(&[
        (
            "index",
            "{% from \"macros\" import label %}{% macro row() %}{{ label() }}{% endmacro %}",
        ),
        ("macros", "{% macro label() %}label{% endmacro %}"),
    ]);
    let error = expect_error(
        call_macro(&template.macro_namespace(), "row", Array::new()),
        ErrorKind::Runtime,
    );
    assert!(
        error.to_string().starts_with(
            "Macros imported in the body of template \"index\" are not available because the body was not rendered"
        ),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "Macros imported in the body of template \"index\" are not available because the body was not rendered; move the \"import\" or \"from\" tag inside the block or the macro that uses it in \"index\" at line 1."
    );
}

#[test]
fn test_deprecated_core_extension_call_macro_accepts_macro_namespace() {
    let (_, template) = load(&[(
        "index",
        "{% macro greet(name) %}Hi {{ name }}{% endmacro %}",
    )]);
    let output = CoreExtension::call_macro(
        &template.macro_namespace(),
        "macro_greet",
        list(&["Bob"]),
        1,
        &Array::new(),
        &Source::new("", "index"),
    )
    .unwrap();
    assert_eq!(string(&output), "Hi Bob");
}

/// The results of the oracle for calls made from host code (`macro-ns-call`): a `Twig\Markup`, `''` for an empty
/// macro, and the deprecations at the caller's line and source.
#[test]
fn macro_namespace_results_and_deprecations() {
    let twig = env(&[(
        "m",
        "{% macro greet(name) %}Hi {{ name }}{% endmacro %}{% macro e() %}{% endmacro %}",
    )]);
    let namespace = twig.load("m").unwrap().macro_namespace();
    let greeting = call_macro(&namespace, "greet", list(&["World"])).unwrap();
    let Value::Object(markup) = &greeting else {
        panic!("a Markup, got {greeting:?}");
    };
    assert_eq!(markup.class_name(), "Twig\\Markup");
    assert_eq!(string(&greeting), "Hi World");
    assert_eq!(
        call_macro(&namespace, "e", Array::new()).unwrap(),
        Value::from("")
    );
    let caller = Source::new("", "caller");
    let (_, deprecations) = collect_deprecations(&twig, || {
        namespace
            .call("greet", list(&["a", "b"]), &Array::new(), 7, &caller)
            .unwrap();
        namespace
            .call(
                "greet",
                ctx(&[("x", Value::Int(1))]),
                &Array::new(),
                7,
                &caller,
            )
            .unwrap();
    });
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Passing more arguments than the macro \"greet\" accepts is deprecated and will throw in Twig 4.0; declare a variadic argument (\"...name\") in the macro definition to accept extra arguments (in \"caller\" at line 7).",
            "Since twig/twig 3.29: Not passing a value for the \"name\" argument of macro \"greet\" is deprecated and the argument will be required in Twig 4.0; give it a default value in the macro definition or pass a value when calling it (in \"caller\" at line 7).",
            "Since twig/twig 3.29: Passing the unknown named argument \"x\" to the macro \"greet\" is deprecated and will throw in Twig 4.0; declare a variadic argument (\"...name\") in the macro definition to accept it (in \"caller\" at line 7).",
        ]
    );
}
