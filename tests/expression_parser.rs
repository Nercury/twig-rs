extern crate twig;

use twig::node::{ Expr, ExprValue };
use twig::Environment;
use twig::Lexer;
use twig::node::Module;
use twig::parser::{ Context, Parse };

#[test]
fn test_string_expression() {
    for (template, expected) in get_tests_for_string() {
        let env = Environment::default().init_all();
        let lexer = Lexer::default(&env.lexing);
        let mut tokens = lexer.tokens(&template);
        let mut context = Context::new(&env.parsing, &mut tokens);
        let module = match Module::parse(&mut context) {
            Ok(m) => m,
            Err(e) => panic!("parsing error: {:?}", e),
        };
        assert_eq!(module.body.expect_print(), &expected);
    }
}

fn get_tests_for_string<'r>() -> Vec<(&'static str, Expr<'r>)> {
    vec![
        (r#"{{ "foo" }}"#, Expr::new_at(ExprValue::Constant { value: "foo" }, 1))
    ]
}
