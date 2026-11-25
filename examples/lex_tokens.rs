use std::env;
use std::fs;
use twig::environment::Environment;
use twig::tokens::Lexer;

/// Tokenises twig template and prints the tokens.
fn main() {
    let path = env::current_dir()
        .unwrap()
        .join("templates/fos_login.html.twig");

    let template = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to open example template at {path:?}: {e}"));

    let env = Environment::default().init_all();
    let lexer = Lexer::default(&env.lexing);

    for token in lexer.tokens(&template) {
        println!("{token:?}");
    }
}
