extern crate twig;

use super::support;
use twig::nodes::expr::{ Expr, ExprValue };

#[test]
#[should_panic(
    expected = r#"Unexpected token "string" of value "b" ("end of print statement" expected)"#
)]
fn test_string_expression_does_not_concatenate_two_consecutive_strings() {
    support::unwrap_or_display(
        support::maybe_parsed(r#"{{ "a" "b" }}"#)
    );
}

#[test]
fn test_string_expression() {
    for (template, expected) in get_tests_for_string() {
        let module = support::expect_parsed(template);
        assert_eq!(module.body.expect_print(), &expected);
    }
}

fn get_tests_for_string<'r>() -> Vec<(&'static str, Expr<'r>)> {
    vec![
        (r#"{{ "foo" }}"#, Expr::new_str_constant("foo", 1)),
        (r#"{{ "foo #{bar}" }}"#, Expr::new_at(ExprValue::Concat {
            left: Box::new(Expr::new_str_constant("foo ", 1)),
            right: Box::new(Expr::new_name("bar", 1)),
        }, 1)),
        (r#"{{ "foo #{bar} baz" }}"#, Expr::new_at(ExprValue::Concat {
            left: Box::new(Expr::new_at(ExprValue::Concat {
                left: Box::new(Expr::new_str_constant("foo ", 1)),
                right: Box::new(Expr::new_name("bar", 1)),
            }, 1)),
            right: Box::new(Expr::new_str_constant(" baz", 1)),
        }, 1)),
        (r#"{{ "foo #{"foo #{bar} baz"} baz" }}"#, Expr::new_at(ExprValue::Concat {
            left: Box::new(Expr::new_at(ExprValue::Concat {
                left: Box::new(Expr::new_str_constant("foo ", 1)),
                right: Box::new(Expr::new_at(ExprValue::Concat {
                    left: Box::new(Expr::new_at(ExprValue::Concat {
                        left: Box::new(Expr::new_str_constant("foo ", 1)),
                        right: Box::new(Expr::new_name("bar", 1)),
                    }, 1)),
                    right: Box::new(Expr::new_str_constant(" baz", 1)),
                }, 1)),
            }, 1)),
            right: Box::new(Expr::new_str_constant(" baz", 1)),
        }, 1)),
    ]
}
