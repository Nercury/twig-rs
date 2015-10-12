#![allow(dead_code)]

use twig::Environment;
use twig::Lexer;
use twig::node::Module;
use twig::parser::{ Context, Parse };
use twig::Result;

pub fn maybe_parsed(template: &'static str) -> Result<Module> {
    let env = Environment::default().init_all();
    let lexer = Lexer::default(&env.lexing);
    let mut tokens = lexer.tokens(template);
    let mut context = Context::new(&env.parsing, &mut tokens);
    Module::parse(&mut context)
}

pub fn expect_parsed(template: &'static str) -> Module {
    match maybe_parsed(template) {
        Ok(m) => m,
        Err(e) => panic!("parsing error: {:?}", e),
    }
}
