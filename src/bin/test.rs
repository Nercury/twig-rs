extern crate twig;

use twig::lexer::{ Lexer };

fn main() {
    let template = r#"
        {% block test %}\n
            Some text <hml>{{- output | raw }}</htm>\n
            {# some comment #}\n
        {% endblock %}\n
        The end.
    "#;
    let lexer = Lexer::default();
    let mut stream = lexer.tokenize(template);

    for i in stream {
        println!("{:?}", i);
    }
}
