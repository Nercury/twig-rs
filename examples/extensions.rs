extern crate twig;
extern crate env_logger;

use twig::operator::Operator;
use twig::environment::Environment;
use twig::tokens::Lexer;
use twig::nodes::{ Parser, Parse, Module };

fn main() {
    env_logger::init().unwrap();

    let mut env = Environment::default();
    env.push_operators(vec![
        Operator::new_binary_left("newop", 10, |_, _| unimplemented!()),
    ]);

    let env = env.init_all();

    let lexer = Lexer::default(&env.lexing);
    let maybe_module = Module::parse(
        &mut Parser::new(&env.parsing, &mut lexer.tokens(
            "test {{ var newop 1 }}"
        ))
    );

    println!("{:#?}", maybe_module);
}
