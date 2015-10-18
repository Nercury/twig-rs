#![allow(dead_code)]

use twig::Environment;
use twig::tokens::Lexer;
use twig::nodes::{ Parser, Parse, Module };
use twig::error::Result;

pub fn maybe_parsed(template: &'static str) -> Result<Module> {
    let env = Environment::default().init_all();
    let lexer = Lexer::default(&env.lexing);
    let mut tokens = lexer.tokens(template);
    let mut parser = Parser::new(&env.parsing, &mut tokens);
    Module::parse(&mut parser)
}

pub fn expect_parsed(template: &'static str) -> Module {
    match maybe_parsed(template) {
        Ok(m) => m,
        Err(e) => panic!("parsing error: {:?}", e),
    }
}
