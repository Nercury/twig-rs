//! Port of TWIG/tests/CallMacroTest.php.
//!
//! The PHPUnit test calls `Template::getMacroNamespace()->call($name, $arguments, [], 1, new Source('', 'index'))`
//! and `->has($name, [])`; twig-rs keeps `MacroNamespace` internal, so the ports make the same calls from templates:
//! `_self.name(args)` compiles to `$macros["_self"]->call("name", [args], $context, line, $this->source)` and
//! `_self.name is defined` to `->has("name", $context)` (`MacroReferenceExpression::compile()`). Errors then carry the
//! template line (`... in "index" at line 1.`, where the PHPUnit test only checks the message start). Every port is
//! VERIFIED with the oracle (same templates).
//!
//! Not ported:
//! - `testMacroNamespaceOnlyExposesMacroOperations`: PHP reflection on `MacroNamespace`.
//! - `testMacroNamespaceDoesNotBenefitFromTheTemplateSandboxExemption`: needs a `MacroNamespace` object to give to
//!   `SecurityPolicy::checkMethodAllowed()`; twig-rs does not expose macro namespaces as values (missing API).
//! - `testDeprecatedCoreExtensionCallMacroAcceptsMacroNamespace`: the deprecated PHP API `CoreExtension::callMacro()`.

use twig::ErrorKind;
use twig::value::Array;

use crate::support::*;

#[test]
fn test_call_macro_resolves_positional_and_named_arguments() {
    let twig = env(&[(
        "index",
        "{% macro greet(name, greeting = \"Hello\") %}{{ greeting }} {{ name }}{% endmacro %}{{ _self.greet('World') }}|{{ _self.greet(name='World', greeting='Hi') }}",
    )]);
    assert_eq!(
        twig.render("index", Array::new()).unwrap(),
        "Hello World|Hi World"
    );
}

#[test]
fn test_call_macro_looks_macros_up_in_parent_templates() {
    // the macro namespace of "index" has no "greet": MacroNamespace::call() resolves it through the parent
    let twig = env(&[
        (
            "index",
            "{% extends \"parent\" %}{% block body %}{{ _self.greet('World') }}{% endblock %}",
        ),
        (
            "parent",
            "{% macro greet(name) %}Hi {{ name }}{% endmacro %}{% block body %}{% endblock %}",
        ),
    ]);
    assert_eq!(twig.render("index", Array::new()).unwrap(), "Hi World");
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
    let twig = env(&[
        (
            "index",
            "{% extends \"parent\" %}{% block body %}{{ _self.Greet('World') }}{% endblock %}",
        ),
        (
            "parent",
            "{% macro greet(name = \"\") %}Hi {{ name }}{% endmacro %}{% block body %}{% endblock %}",
        ),
    ]);
    let (output, deprecations) =
        collect_deprecations(&twig, || twig.render("index", Array::new()).unwrap());
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
    let twig = env(&[(
        "index",
        "{% macro greet(name) %}Hi {{ name }}{% endmacro %}{{ _self.greet is defined ? 'y' : 'n' }}{{ _self.GREET is defined ? 'y' : 'n' }}{{ _self.missing is defined ? 'y' : 'n' }}",
    )]);
    let (output, deprecations) =
        collect_deprecations(&twig, || twig.render("index", Array::new()).unwrap());
    assert_eq!(output, "yyn");
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
    let twig = env(&[(
        "index",
        "{% macro greet(name = \"\") %}Hi {{ name }}{% endmacro %}{{ _self.macro_greet('World') }}",
    )]);
    let (output, deprecations) =
        collect_deprecations(&twig, || twig.render("index", Array::new()).unwrap());
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
    let twig = env(&[(
        "index",
        "{% macro macro_greet(name = \"\") %}Prefixed {{ name }}{% endmacro %}{% macro greet(name = \"\") %}Bare {{ name }}{% endmacro %}{{ _self.macro_greet('World') }}",
    )]);
    let (output, deprecations) =
        collect_deprecations(&twig, || twig.render("index", Array::new()).unwrap());
    assert_eq!(output, "Prefixed World");
    assert!(deprecations.is_empty(), "{deprecations:?}");
}

#[test]
fn test_call_macro_throws_with_the_prefixed_name_when_neither_name_exists() {
    let twig = env(&[("index", "no macro here{{ _self.macro_missing() }}")]);
    let error = expect_error(twig.render("index", Array::new()), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "Macro \"macro_missing\" is not defined in template \"index\" in \"index\" at line 1."
    );
}

#[test]
fn test_has_macro_resolves_a_legacy_prefixed_name_with_a_deprecation() {
    let twig = env(&[(
        "index",
        "{% macro greet(name) %}Hi {{ name }}{% endmacro %}{{ _self.macro_greet is defined ? 'y' : 'n' }}{{ _self.macro_missing is defined ? 'y' : 'n' }}",
    )]);
    let (output, deprecations) =
        collect_deprecations(&twig, || twig.render("index", Array::new()).unwrap());
    assert_eq!(output, "yn");
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Testing whether the macro \"greet\" is defined via the \"macro_\"-prefixed name \"macro_greet\" is deprecated; pass the bare macro name to \"Twig\\Node\\Expression\\MacroReferenceExpression\" instead."
        ]
    );
}

#[test]
fn test_call_macro_throws_for_an_unknown_macro() {
    let twig = env(&[("index", "no macro here{{ _self.missing() }}")]);
    let error = expect_error(twig.render("index", Array::new()), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "Macro \"missing\" is not defined in template \"index\" in \"index\" at line 1."
    );
}

#[test]
fn test_rendering_a_block_on_its_own_reports_macros_imported_in_the_template_body() {
    let twig = env(&[
        (
            "index",
            "{% import \"macros\" as helpers %}{% block field %}{{ helpers.label() }}{% endblock %}",
        ),
        ("macros", "{% macro label() %}label{% endmacro %}"),
    ]);
    let error = expect_error(
        twig.load("index")
            .unwrap()
            .render_block("field", Array::new()),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Macros imported in the body of template \"index\" are not available because the body was not rendered; move the \"import\" or \"from\" tag inside the block or the macro that uses it in \"index\" at line 1."
    );
}

#[test]
fn test_rendering_a_block_on_its_own_uses_macros_imported_in_the_block() {
    let twig = env(&[
        (
            "index",
            "{% block field %}{% import \"macros\" as helpers %}{{ helpers.label() }}{% endblock %}",
        ),
        ("macros", "{% macro label() %}label{% endmacro %}"),
    ]);
    assert_eq!(
        twig.load("index")
            .unwrap()
            .render_block("field", Array::new())
            .unwrap(),
        "label"
    );
}

#[test]
fn test_calling_a_macro_on_its_own_reports_macros_imported_in_the_template_body() {
    // `callMacro($template, 'row', [])` without rendering the body of "index": a template importing "index" calls it
    let twig = env(&[
        (
            "index",
            "{% from \"macros\" import label %}{% macro row() %}{{ label() }}{% endmacro %}",
        ),
        ("macros", "{% macro label() %}label{% endmacro %}"),
        ("caller", "{% import \"index\" as m %}{{ m.row() }}"),
    ]);
    let error = expect_error(twig.render("caller", Array::new()), ErrorKind::Runtime);
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
