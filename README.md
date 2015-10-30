# Twig template engine for Rust

[![Build Status](https://travis-ci.org/Nercury/twig-rs.svg)](https://travis-ci.org/Nercury/twig-rs)

[Read `twig-rs` library docs](http://nercury.github.io/twig-rs)

Flexible, fast, secure template engine for Rust.
The aim is to be 100% syntactically compatible with [Twig for PHP][twig-for-php].
The secondary aim is to provide functionally equivalent ways to extend
and customize template with extensions.

Note that at this moment this is very much work in progress, and is not usable.

The goal of 1.0 version is to pass test suite functionally equivalent to Twig 2.0 ([issue #1](https://github.com/Nercury/twig-rs/issues/1)).

[twig-for-php]: http://twig.sensiolabs.org/

## Motivation

- Designers are familiar with Twig.
- Reuse existing IDE support for Twig.

## Build Requirements

- Minimum Rust version: 1.3.0.

## TODO list

- Parser implementation is not finished ([issue #3](https://github.com/Nercury/twig-rs/issues/3)).
- LLTL (low level template language), basics implemented in little-rs subproject, [issue #4](https://github.com/Nercury/twig-rs/issues/4).

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

# Example of working parser

Run example that parses this template:

```twig
test {{ var + 1 }}
```

```bash
cargo run --example parse_nodes
```

Will output parsed module in console:

```
Ok(
    Module {
        body: List {
            items: [
                Text {
                    value: "test ",
                    line: 1
                },
                Print {
                    expr: Expr {
                        line: 1,
                        value: BinaryOperator {
                            value: "+",
                            left: Expr {
                                line: 1,
                                value: Name(
                                    "var"
                                )
                            },
                            right: Expr {
                                line: 1,
                                value: Constant(
                                    Int(
                                        1
                                    )
                                )
                            }
                        }
                    },
                    line: 1
                }
            ]
        }
    }
)
```

[tmp]: https://github.com/Nercury/twig-rs/blob/master/templates/fos_login.html.twig
