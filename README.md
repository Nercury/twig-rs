# Twig templating engine for Rust

Flexible, fast, secure templating engine for Rust.
The aim is to be 100% syntactically compatible with [Twig for PHP][twig-for-php].
The secondary aim is to provide functionally equivalent ways to extend
and customize templating with extensions.

Note that at this moment this is very much work in progress, and is not usable.

The goal of 1.0 version is to pass test suite functionally equivalent to Twig 2.0 (issue #1).

[twig-for-php]: http://twig.sensiolabs.org/

## Motivation

- Designers are familiar with Twig.
- Reuse existing IDE support for Twig.

## Short-term goals

- At least a basic parser implementation is needed, with extendable AST (issue #3).
- Fixture runner is needed (issue #2). However, it requires finished #3.

## Long-term goals

- LLTL (low level template language) #4.

# Example of working lexer

Run example that iterates over template in [templates/fos_login.html.twig][tmp]:

```bash
cargo run --example lex_tokens
```

Will produce list of tokens in console:

```
Ok(Token { value: BlockStart, line_num: 1 })
Ok(Token { value: Name("extends"), line_num: 1 })
Ok(Token { value: String(FOSUserBundle::layout.html.twig), line_num: 1 })
Ok(Token { value: BlockEnd, line_num: 1 })
Ok(Token { value: Text("\n"), line_num: 2 })
Ok(Token { value: BlockStart, line_num: 3 })
...
```

[tmp]: https://github.com/Nercury/twig-rs/blob/master/templates/fos_login.html.twig
