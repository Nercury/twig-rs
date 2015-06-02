extern crate twig;

use twig::environment::{ Environment };
use twig::lexer::{ Lexer };

fn main() {
    let template = r#"
        {% raw %}baaaaaaah{% endraw %}
        {% block test %}
            Some text <hml>{{- output | raw }}</htm>
            {# some comment #}
        {% endblock %}
        The end {{ "hello \" aa" }}.
    "#;
    let lexer = Lexer::default(&Environment::default());
    let mut stream = lexer.tokens(template);

    for i in stream {
        println!("TOKEN {:?}", i);
    }
}
