#![allow(dead_code)]

use std::fmt;
use twig::environment::Environment;
use twig::tokens::Lexer;
use twig::nodes::{ Parser, Parse, Module };
use twig::error::TemplateResult;

pub fn maybe_parsed(template: &'static str) -> TemplateResult<Module> {
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

pub fn unwrap_or_display<T, E: fmt::Display>(value: Result<T, E>) {
    match value {
        Ok(_) => (),
        Err(e) => panic!("{}", e),
    }
}
