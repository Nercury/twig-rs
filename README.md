# Twig template engine for Rust

This project is being revived to target exact compatibility with
[Twig 3.31.1 for PHP](https://twig.symfony.com/): identical output and identical error messages, so that templates
work copy-pasted from PHP projects.

At the moment the front end exists: the lexer, the parser with the complete expression grammar and every core tag,
and the node traverser with Twig's correctness checks, all verified against PHP Twig. Templates cannot be rendered
yet; the code generator, the virtual machine and the runtime come next.

## Build requirements

- Rust 1.88+

## Examples

```bash
cargo run --example lex_tokens                  # print the tokens of templates/fos_login.html.twig
cargo run --example lex_tokens -- page.twig     # ... or of another template
cargo run --example parse_nodes -- '{% for user in users %}{{ user.name|upper }}{% endfor %}'
cargo run --example parse_nodes -- --file page.twig   # print the node tree, the error and the deprecations
```
