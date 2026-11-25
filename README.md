# Twig template engine for Rust

This project is being revived to target exact compatibility with
[Twig 3.x for PHP](https://twig.symfony.com/). At the moment only an incomplete
lexer and parser exist; templates cannot be rendered yet.

## Build requirements

- Rust 1.88+

## Examples

```bash
cargo run --example lex_tokens    # print the tokens of templates/fos_login.html.twig
cargo run --example parse_nodes   # print the node tree of `test {{ var + 1 }}`
```
