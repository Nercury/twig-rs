extern crate twig;

use twig::node::Expr;
use twig::Environment;
use twig::Lexer;
use twig::node::Module;
use twig::parser::{ Context, Parse };

#[test]
fn test_string_expression() {
    for (template, expected) in get_tests_for_string() {
        let env = Environment::default().init();
        let lexer = Lexer::default(&env);
        let mut tokens = lexer.tokens(&template);
        let mut context = Context::new(&env, &mut tokens);
        // let module = Module::parse(&mut context).ok().expect("parse template");
        // assert_eq!(module.body.expect_list()[0].expect_print(), &expected);
    }
}

fn get_tests_for_string<'r>() -> Vec<(&'static str, Expr<'r>)> {
    vec![
        (r#"{{ "foo" }}"#, Expr::Constant("foo", 1))
    ]
}
