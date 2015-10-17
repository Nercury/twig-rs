extern crate twig;
extern crate env_logger;

use twig::Environment;
use twig::Lexer;
use twig::node::Module;
use twig::extension::Extension;
use twig::parser::Context as ParserContext;
use twig::parser::Parse;

use std::fs::File;
use std::io::Read;
use std::env;

mod extension;

fn main() {
    env_logger::init().unwrap();

    let mut staged = Environment::default();
    extension::TranslationExtension::apply(&mut staged);
    let env = staged.init_all();

    let lexer = Lexer::default(&env.lexing);
    let maybe_module = Module::parse(
        &mut ParserContext::new(&env.parsing, &mut lexer.tokens(
            "test {{ var + 1 }}"
        ))
    );

    println!("{:#?}", maybe_module);
}
