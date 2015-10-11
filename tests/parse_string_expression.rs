extern crate twig;

use twig::node::{ Expr, ExprValue };
use twig::Environment;
use twig::Lexer;
use twig::node::Module;
use twig::parser::{ Context, Parse };
use twig::Result;

#[test]
#[should_panic(
    expected = r#"Unexpected token "string" of value "b" ("end of print statement" expected)"#
)]
fn test_string_expression_does_not_concatenate_two_consecutive_strings() {
    maybe_parsed(r#"{{ "a" "b" }}"#).unwrap();
}

#[test]
fn test_string_expression() {
    for (template, expected) in get_tests_for_string() {
        let module = expect_parsed(template);
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

fn maybe_parsed(template: &'static str) -> Result<Module> {
    let env = Environment::default().init_all();
    let lexer = Lexer::default(&env.lexing);
    let mut tokens = lexer.tokens(template);
    let mut context = Context::new(&env.parsing, &mut tokens);
    Module::parse(&mut context)
}

fn expect_parsed(template: &'static str) -> Module {
    match maybe_parsed(template) {
        Ok(m) => m,
        Err(e) => panic!("parsing error: {:?}", e),
    }
}
