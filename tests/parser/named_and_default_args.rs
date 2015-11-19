extern crate twig;

use super::support;

#[test]
#[should_panic(
    expected = r#"Arguments must be separated by a comma at line 1"#
)]
fn attribute_call_does_not_support_named_arguments() {
    support::unwrap_or_display(support::maybe_parsed(r#"{{ foo.bar(name="Foo") }}"#));
}

#[test]
#[should_panic(
    expected = r#"Arguments must be separated by a comma at line 1"#
)]
fn macro_call_does_not_support_named_arguments() {
    support::unwrap_or_display(
        support::maybe_parsed(r#"{% from _self import foo %}{% macro foo() %}{% endmacro %}{{ foo(name="Foo") }}"#)
    );
}

#[test]
#[should_panic(
    expected = r#"Expected "name" but received "string" with value "a" at line 1"#
)]
fn macro_definition_does_not_support_non_name_variable_name() {
    support::unwrap_or_display(
        support::maybe_parsed(r#"{% macro foo("a") %}{% endmacro %}"#)
    );
}

#[test]
fn macro_definition_does_not_support_non_constant_default_values() {
    for template in get_macro_definition_does_not_support_non_constant_default_values() {
        match support::maybe_parsed(template) {
            Ok(_) => panic!("expected {:?} to produce error", template),
            Err(e) => {
                println!("tmp {} produces {}", template, e);
                assert!(format!("{}", e)
                    .contains(r#"A default value for an argument must be a constant (a boolean, a string, a number, or an array) at line 1"#));
            },
        }
    }
}

fn get_macro_definition_does_not_support_non_constant_default_values() -> Vec<&'static str> {
    vec![
        r#"{% macro foo(name = "a #{foo} a") %}{% endmacro %}"#,
        r#"{% macro foo(name = [["b", "a #{foo} a"]]) %}{% endmacro %}"#,
    ]
}

#[test]
fn macro_definition_supports_constant_default_values() {
    for template in get_macro_definition_supports_constant_default_values() {
        support::unwrap_or_display(
            support::maybe_parsed(template)
        )
    }
}

fn get_macro_definition_supports_constant_default_values() -> Vec<&'static str> {
    vec![
        r#"{% macro foo(name = "aa") %}{% endmacro %}"#,
        r#"{% macro foo(name = 12) %}{% endmacro %}"#,
        r#"{% macro foo(name = true) %}{% endmacro %}"#,
        r#"{% macro foo(name = ["a"]) %}{% endmacro %}"#,
        r#"{% macro foo(name = [["a"]]) %}{% endmacro %}"#,
        r#"{% macro foo(name = {a: "a"}) %}{% endmacro %}"#,
        r#"{% macro foo(name = {a: {b: "a"}}) %}{% endmacro %}"#,
    ]
}
