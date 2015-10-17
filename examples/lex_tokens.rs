extern crate twig;

use twig::Environment;
use twig::tokens::Lexer;
use std::fs::File;
use std::io::Read;
use std::env;

/// Tokenises twig template and prints the tokens.
fn main() {
    let example_template_file = "templates/fos_login.html.twig";
    let mut path = env::current_dir().unwrap();
    path.push(example_template_file);

    let mut f = File::open(&path)
        .ok()
        .expect(&format!("failed to open example template at {:?}", path));
    let mut template = String::new();
    f.read_to_string(&mut template).unwrap();

    let env = Environment::default().init_all();
    let lexer = Lexer::default(&env.lexing);

    for token in lexer.tokens(&template) {
        println!("{:?}", token);
    }
}
