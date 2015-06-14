extern crate twig;

use twig::environment::{ StagedEnvironment, Environment };
use twig::lexer::{ Lexer };
use twig::ast::{ Module };
use twig::extension::Apply;

use std::fs::File;
use std::io::Read;
use std::env;

mod extension;

fn main() {
    let example_template_file = "templates/fos_login.html.twig";
    let mut path = env::current_dir().unwrap();
    path.push(example_template_file);

    let mut f = File::open(&path)
        .ok()
        .expect(&format!("failed to open example template at {:?}", path));
    let mut template = String::new();
    f.read_to_string(&mut template).unwrap();

    let mut staged = StagedEnvironment::default();
    extension::TranslationExtension::apply(&mut staged);
    let env = Environment::new(staged);

    let lexer = Lexer::default(&env);
    let maybe_module = Module::from_tokens(lexer.tokens(&template));
}
