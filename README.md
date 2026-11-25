# Twig template engine for Rust

twig-rs renders [Twig](https://twig.symfony.com/) templates in Rust, aiming for exact compatibility with
**Twig 3.31.1 on PHP 8.5.13**: identical output and identical error messages, so that templates work copy-pasted from
PHP projects. Host integration (passing data, registering filters, functions and extensions) is Rust-native.

## Status

The whole engine is in place and checked against PHP Twig (a Docker oracle running the pinned versions):

- the lexer, the parser with the complete expression grammar and every core tag;
- the node visitors: correctness checks, auto-escaping (`html`, `js`, `css`, `html_attr`, `url`, custom strategies),
  the optimizer and the sandbox;
- a code generator and a virtual machine with PHP's value semantics (the `twig-value` crate): template inheritance,
  blocks, macros, `include`, `embed`, `use`, `with`, `apply`, `set`, `for` with `loop`, `guard`, `types`, ...;
- every core filter, function and test, dates (`date`, `date_modify`, DateTime objects; the `twig-date` crate) and the
  `matches` operator on PCRE2 10.44 as bundled by PHP;
- the sandbox (security policies, `{% sandbox %}`, `Sandbox`, `render_sandboxed()`), the debug and string-loader
  extensions;
- the `ArrayLoader`, `FilesystemLoader` and `ChainLoader`.

All 500 runnable cases of Twig 3.31.1's integration fixtures pass (two more are skipped by their `--CONDITION--`, as
in PHPUnit). Behaviour that cannot or deliberately does not match PHP is listed in
[`reference/deviations.md`](reference/deviations.md).

## Usage

```rust
use twig::loader::ArrayLoader;
use twig::runtime::Environment;
use twig::value::{Array, Value};

fn main() -> Result<(), twig::Error> {
    let env = Environment::new(ArrayLoader::new([
        (
            "base.html",
            "<h1>{% block title %}{% endblock %}</h1><ul>{% block body %}{% endblock %}</ul>",
        ),
        (
            "index.html",
            "{% extends 'base.html' %}\
             {% block title %}Hello {{ name }}!{% endblock %}\
             {% block body %}{% for item in items %}<li>{{ item|upper }}</li>{% endfor %}{% endblock %}",
        ),
    ]));

    let mut context = Array::new();
    context.insert("name", Value::from("<world>"));
    context.insert(
        "items",
        Value::Array(Array::from_values([Value::from("a"), Value::from("b")])),
    );

    // auto-escaping is on by default (`html`), as in Twig
    assert_eq!(
        env.render("index.html", context)?,
        "<h1>Hello &lt;world&gt;!</h1><ul><li>A</li><li>B</li></ul>"
    );
    Ok(())
}
```

Errors carry Twig's messages, template names and lines:

```rust
use twig::loader::ArrayLoader;
use twig::runtime::{Environment, EnvironmentOptions};
use twig::value::Array;

fn main() {
    let env = Environment::with_options(
        ArrayLoader::new([("index.html", "{{ user.name }}")]),
        EnvironmentOptions {
            strict_variables: true,
            ..Default::default()
        },
    );
    let error = env.render("index.html", Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Variable \"user\" does not exist in \"index.html\" at line 1."
    );
}
```

Custom filters, functions and tests attach a Rust implementation to the PHP-like signature Twig binds arguments to
(see `twig::extension::runtime`); `Environment::escaper_runtime()` registers custom escaping strategies and
`Environment::core_settings()` the default date and number formats.

## Build requirements

- Rust 1.88+
- a C compiler (PCRE2 is built from the sources PHP bundles)

## Examples

```bash
cargo run --example lex_tokens                  # print the tokens of templates/fos_login.html.twig
cargo run --example lex_tokens -- page.twig     # ... or of another template
cargo run --example parse_nodes -- '{% for user in users %}{{ user.name|upper }}{% endfor %}'
cargo run --example parse_nodes -- --file page.twig   # print the node tree, the error and the deprecations
```

## Layout

| Crate | Contents |
|---|---|
| `twig` (this directory) | lexer, parser, node visitors, code generator, VM, runtime, extensions, loaders |
| `crates/twig-value` | PHP 8.5 value model, operators, comparison, conversions and the PHP builtins Twig calls |
| `crates/twig-unicode` | Unicode 17.0 tables and mbstring-compatible case mapping |
| `crates/twig-pcre` | PCRE2 10.44 with PHP's `preg_match()` semantics |
| `crates/twig-timelib`, `crates/twig-date` | timelib port with timezonedb 2026.6, PHP's `DateTime` family |
| `crates/twig-test-support` | (dev only) Twig's integration fixtures as values |
