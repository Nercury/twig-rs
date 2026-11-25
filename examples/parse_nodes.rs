use twig::environment::Environment;
use twig::nodes::{Module, Parse, Parser};
use twig::tokens::Lexer;

fn main() {
    let env = Environment::default().init_all();

    let lexer = Lexer::default(&env.lexing);
    let maybe_module = Module::parse(&mut Parser::new(
        &env.parsing,
        &mut lexer.tokens("test {{ var + 1 }}"),
    ));

    println!("{:#?}", maybe_module);
}
