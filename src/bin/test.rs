extern crate twig;

use twig::environment::{ Environment };
use twig::lexer::{ Lexer };
use std::fs::File;
use std::io::Read;
use std::env;

fn main() {
    let mut path = env::current_dir().unwrap();
    path.push("templates/fos_login.html.twig");

    println!("Open {:?}", path);

    let mut f = File::open(path)
        .unwrap_or_else(|e| panic!("{}", e));
    let mut template = String::new();
    f.read_to_string(&mut template);

    let lexer = Lexer::default(&Environment::default());

    for i in lexer.tokens(&template) {
        println!("TOKEN {:?}", i);
    }
}
