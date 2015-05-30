extern crate twig;

use twig::environment::{ Environment };
use twig::lexer::{ Lexer };

fn main() {
    let template = r#"
        {% block test %}\n
            Some text <hml>{{- output | raw }}</htm>\n
            {# some comment #}\n
        {% endblock %}\n
        The end.
    "#;
    let lexer = Lexer::default(&Environment::default());
    let mut stream = lexer.tokens(template);

    for i in stream {
        println!("TOKEN {:?}", i);
    }
}
