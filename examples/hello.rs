//! A first program: templates held in memory by an `ArrayLoader`, a context built with `context!`, template
//! inheritance, a loop and auto-escaping.
//!
//! ```bash
//! cargo run --example hello
//! ```

use twig::loader::ArrayLoader;
use twig::{Environment, context};

// The newline right after a `{% ... %}` tag is dropped, as in Twig.
const BASE: &str = r#"<!DOCTYPE html>
<html>
  <head><title>{% block title %}{% endblock %}</title></head>
  <body>
{% block body %}{% endblock %}
  </body>
</html>
"#;

const HELLO: &str = r#"{% extends "base.html.twig" %}

{% block title %}Hello {{ name }}{% endblock %}

{% block body %}
    <h1>Hello {{ name }}!</h1>
    <ol>
{% for step in steps %}
      <li>{{ step|capitalize }}{{ loop.last ? "." : "," }}</li>
{% endfor %}
    </ol>
{% endblock %}
"#;

fn main() -> twig::Result<()> {
    let env = Environment::new(ArrayLoader::new([
        ("base.html.twig", BASE),
        ("hello.html.twig", HELLO),
    ]));

    let context = context! {
        // auto-escaping is on (`html`), as in Twig: this prints `&lt;World&gt;`
        name => "<World>",
        steps => ["lexer", "parser", "compiler", "virtual machine"],
    };
    print!("{}", env.render("hello.html.twig", context)?);
    Ok(())
}
