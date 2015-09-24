extern crate twig;

use twig::node::Expr;
use twig::Environment;
use twig::Lexer;
use twig::node::Module;

#[test]
fn test_string_expression() {
    for (template, expected) in get_tests_for_string() {
        let env = Environment::default().init();
        let lexer = Lexer::default(&env);
        let module = Module::from_tokens(lexer.tokens(&template)).ok().expect("parse template");
        assert_eq!(module.body[0].expect_expr(), &expected);
    }
}

fn get_tests_for_string<'r>() -> Vec<(&'static str, Expr<'r>)> {
    vec![
        (r#"{{ "foo" }}"#, Expr::Constant("foo", 1))
    ]
}
