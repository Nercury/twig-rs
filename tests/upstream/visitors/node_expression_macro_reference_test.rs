//! Twig 3.31.1's `tests/Node/Expression/MacroReferenceTest.php`.
//!
//! Not ported: `testConstructorAcceptsDeprecatedTemplateVariable` (the deprecated `TemplateVariable` class is not
//! ported, reference/deviations.md "Legacy node classes and APIs").

use twig::ast::{AttrValue, Expr, ExprKind, MacroName, NodeRef};
use twig::value::{Array, Value};

use super::support::{Mismatches, Oracle, collecting, ctx, env, no_escape, render_full};

/// `testConstructorAcceptsAnExpressionAsName()`: a dynamic name is the `name` child, and the `name` attribute is
/// `null`.
#[test]
fn constructor_accepts_an_expression_as_name() {
    let node = Expr::macro_reference(
        Expr::macro_variable("foo", 1, 0).unwrap(),
        MacroName::Dynamic(Expr::context_variable("name", 1)),
        Expr::array(1),
        1,
    );
    let children: Vec<String> = NodeRef::Expr(&node)
        .children()
        .iter()
        .map(|(key, _)| key.to_string())
        .collect();
    assert!(children.contains(&"name".to_owned()), "{children:?}");
    let name = node
        .attributes()
        .into_iter()
        .find(|(attribute, _)| attribute == "name")
        .map(|(_, value)| value);
    assert_eq!(name, Some(AttrValue::Null));
    assert!(
        matches!(&node.kind, ExprKind::MacroReference(m) if matches!(m.name, MacroName::Dynamic(_)))
    );
}

/// The templates of `macro_reference_test/<i>` in the generator, with the value of `name`.
fn dynamic_names() -> Vec<(&'static str, Value)> {
    let hello = Value::from("hello");
    vec![
        (
            "{% macro hello(who = 'you') %}hi {{ who }}{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}",
            hello.clone(),
        ),
        (
            "{% macro hello(who = 'you') %}hi {{ who }}{% endmacro %}{% import _self as mac %}{{ mac.(name)('me') }}",
            hello.clone(),
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}",
            Value::from("missing"),
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name ~ 'lo')() }}",
            Value::from("hel"),
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{{ _self.(name)() }}",
            hello.clone(),
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name) is defined ? 'y' : 'n' }}",
            hello.clone(),
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name) is defined ? 'y' : 'n' }}",
            Value::from("nope"),
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}",
            Value::from("HELLO"),
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}",
            Value::Int(1),
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}",
            Value::Null,
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}",
            Value::Array(Array::from_values([hello.clone()])),
        ),
        (
            "{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name) }}",
            hello,
        ),
    ]
}

/// `testDynamicNameResolvesMacroAtRuntime()` (an assertion on generated PHP: `->call()` with the name expression),
/// rendered: the macro is looked up when the call runs, with the deprecations and errors of `MacroNamespace::call()`.
#[test]
fn dynamic_name_resolves_macro_at_runtime() {
    let oracle = Oracle::load("node");
    let mut mismatches = Mismatches::default();
    for (i, (template, name)) in dynamic_names().into_iter().enumerate() {
        let env = env(&[("index", template)], collecting(no_escape()));
        let actual = render_full(&env, "index", ctx(vec![("name", name)]));
        mismatches.check(
            template,
            &oracle.rendered(&format!("macro_reference_test/{i}")),
            &actual,
        );
    }
    mismatches.assert_none();
}
