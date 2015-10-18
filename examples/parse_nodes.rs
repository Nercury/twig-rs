extern crate twig;
extern crate env_logger;

use twig::Environment;
use twig::tokens::Lexer;
use twig::nodes::{ Parser, Parse, Module };

fn main() {
    env_logger::init().unwrap();

    let env = Environment::default().init_all();

    let lexer = Lexer::default(&env.lexing);
    let maybe_module = Module::parse(
        &mut Parser::new(&env.parsing, &mut lexer.tokens(
            "test {{ var + 1 }}"
        ))
    );

    println!("{:#?}", maybe_module);
}
