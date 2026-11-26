# Twig template engine for Rust

twig-rs renders [Twig](https://twig.symfony.com/) templates exactly like **Twig 3.31.1 on PHP 8.5.13** renders them:
the same output bytes and the same error messages, so that templates work copy-pasted from PHP projects. Data,
filters, functions, extensions, loaders and security policies are Rust; there is no PHP interpreter involved.

```rust
use twig::loader::ArrayLoader;
use twig::{Environment, context};

fn main() -> twig::Result<()> {
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

    // auto-escaping is on by default (`html`), as in Twig
    let html = env.render("index.html", context! { name => "<world>", items => ["a", "b"] })?;
    assert_eq!(html, "<h1>Hello &lt;world&gt;!</h1><ul><li>A</li><li>B</li></ul>");
    Ok(())
}
```

A context is anything that converts into a PHP array (`context!`, a map, an array of pairs), and templates call the
methods of Rust objects like those of PHP objects:

```rust
use twig::loader::ArrayLoader;
use twig::value::object::ObjectBuilder;
use twig::{Environment, context};

fn main() -> twig::Result<()> {
    let env = Environment::new(ArrayLoader::new([(
        "index",
        "{{ user.name }}{{ user.admin ? ' (admin)' }}: {{ greeting }}",
    )]));
    let user = ObjectBuilder::new("App\\User")
        .property("name", "Fabien")
        // `user.admin` calls `isAdmin()`, as in Twig
        .method("isAdmin", [], |_, _, _| Ok(true))
        .build();
    let html = env.render("index", context! { user, greeting => "hello" })?;
    assert_eq!(html, "Fabien (admin): hello");
    assert_eq!(env.render_string("{{ a }}+{{ b }}", [("a", 1), ("b", 2)])?, "1+2");
    Ok(())
}
```

Errors carry Twig's messages, template names and lines:

```rust
use twig::loader::ArrayLoader;
use twig::{Array, Environment, EnvironmentOptions};

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

The crate documentation (`cargo doc --open`) is the guide: data and the `context!` macro, serde, Rust types as PHP
objects (`Object`, `ObjectBuilder`), autoescaping, filters, functions, tests, extensions and tags, loaders, the
sandbox, errors and PHP diagnostics, threads.

## Status

- The engine is complete: the lexer, the parser with the whole expression grammar and every core tag, the node
  visitors (correctness checks, auto-escaping with every strategy, the optimizer, the sandbox), a compiler to a
  low-level instruction set and a virtual machine with PHP's value semantics, every core filter, function and test,
  dates (a port of PHP's timelib), the `matches` operator (PCRE2 as PHP bundles it), the sandbox, the debug and
  string-loader extensions, and the array, filesystem and chain loaders.
- All 500 runnable cases of Twig 3.31.1's integration fixtures pass (two more are skipped by their `--CONDITION--`,
  as in PHPUnit), and Twig's behaviour-level PHPUnit tests are ported.
- Differential fuzzing against PHP finds a handful of edge cases per round of 240,000 generated templates; they are
  tracked in [`plans/parity-fixes.md`](plans/parity-fixes.md) until fixed. Deliberate differences are listed in
  [`reference/deviations.md`](reference/deviations.md).
- The API is being prepared for a first release ([`plans/product.md`](plans/product.md)); nothing is published on
  crates.io yet.

## Installation

Version 0.2.0 is not on crates.io yet. Until it is, depend on the repository's `revive` branch:

```toml
[dependencies]
twig = { git = "https://github.com/Nercury/twig-rs", branch = "revive" }
# or, with `twig::to_context` and `twig::to_value` for data that implements serde's `Serialize`:
# twig = { git = "https://github.com/Nercury/twig-rs", branch = "revive", features = ["serde"] }
```

Once published, this becomes `twig = "0.2"` (`features = ["serde"]` as above). Changes between versions are listed
in [`CHANGELOG.md`](CHANGELOG.md).

Building needs Rust 1.88 or newer and a C compiler (PCRE2 is compiled from the sources PHP bundles).

### Charsets other than Unicode, ASCII and ISO-8859-1: the `glibc-iconv` feature

Templates convert charsets with PHP's `iconv()`: the `convert_encoding` filter, escaping with a `charset` option other
than UTF-8, and `reverse`, `shuffle` and `random` under such a charset. By default twig-rs converts UTF-8, UTF-16,
UTF-32, UCS-2, UCS-4 (with their byte-order variants), ASCII and ISO-8859-1 exactly like PHP on glibc, and any other
charset glibc knows fails with `iconv(): conversion of charset "X" is not supported by twig-rs`.

The `glibc-iconv` feature adds the other charsets (the ISO-8859 family, Windows and DOS codepages, KOI8, EUC-JP,
Shift_JIS, ISO-2022-JP, GB2312, GBK, GB18030, BIG5, EUC-KR, ...) through the `twig-iconv` crate:

```toml
twig = { git = "https://github.com/Nercury/twig-rs", branch = "revive", features = ["glibc-iconv"] }
```

`twig-iconv` reproduces glibc's iconv by following glibc's sources, so it is licensed under the **GNU LGPL 2.1 or
later**, unlike the rest of twig-rs: a program built with the feature contains LGPL code and must meet the LGPL's
conditions when it is distributed. Without the feature nothing LGPL-licensed is compiled in. An environment can also
be given a converter explicitly (`EnvironmentOptions::iconv_backend`), for example `twig_iconv::GlibcIconv` or one of
the program's own.

## Compatibility

| Component | Version |
|---|---|
| Twig | 3.31.1 |
| PHP | 8.5.13, 64-bit, PHP's built-in ini defaults (`precision=14`, `serialize_precision=-1`, `date.timezone=UTC`) |
| PCRE2 (`matches`) | 10.44, as bundled with PHP |
| timelib, timezonedb (dates) | 2022.17, 2026.6 |
| Unicode (mbstring case mapping) | 17.0 |
| iconv (`convert_encoding`, escaping charsets) | glibc 2.41: the Unicode charsets, ASCII and ISO-8859-1 by default; with the `glibc-iconv` feature nearly every charset glibc converts |

"The same" covers output bytes, error classes and messages with template names and lines, PHP warnings and
deprecations. What cannot or deliberately does not match (PHP-internal file names in messages, object handle numbers,
unseeded randomness, recursion limits, charsets not converted yet, host APIs without a PHP equivalent) is listed in
[`reference/deviations.md`](reference/deviations.md); anything else that differs is a bug. The pinned versions and
the oracle image are in [`reference/target.md`](reference/target.md).

## Examples

```bash
cargo run --example hello                            # ArrayLoader, inheritance, a loop, auto-escaping
cargo run --example filesystem                       # FilesystemLoader with a namespace (examples/templates/)
cargo run --example host_objects                     # Rust types as PHP objects: Object by hand, ObjectBuilder
cargo run --example custom_filter                    # filters made of Rust closures
cargo run --example extension                        # an Extension with filters, a global and a tag
cargo run --example sandbox                          # untrusted templates under a strict security policy
cargo run --example serde_context --features serde   # template data from serde
cargo run --example lex_tokens                       # the tokens of templates/fos_login.html.twig
cargo run --example parse_nodes -- '{% for user in users %}{{ user.name|upper }}{% endfor %}'
```

## Performance

[`research/benchmarks/README.md`](research/benchmarks/README.md) compares twig-rs with PHP Twig over the scenarios
of the criterion benchmarks in [`benches/`](benches/README.md) (measured on 2026-11-26, both engines producing
identical output): twig-rs compiles templates 11.5 to 13.5 times faster than PHP Twig; with warm caches it renders
the escaping-heavy scenario 3 times faster, inheritance and macros 1.1 to 1.4 times faster, and dates, filters and
loops 1.1 to 1.2 times slower, after a first profiling pass that made rendering 1.9 to 3.4 times faster. twig-rs
does not choose a global allocator; on Windows, hosts that render a lot gain from a fast one such as `mimalloc`.

## Layout

| Path | Contents |
|---|---|
| `src/` (crate `twig`) | lexer, parser, node visitors, code generator, VM, runtime, extensions, loaders |
| `crates/twig-value` | PHP 8.5 value model, operators, comparison, conversions and the PHP builtins Twig calls |
| `crates/twig-iconv` | (optional, LGPL-2.1-or-later) glibc's `iconv()` for every charset, behind the `glibc-iconv` feature |
| `crates/twig-unicode` | Unicode 17.0 tables and mbstring-compatible case mapping |
| `crates/twig-pcre` | PCRE2 10.44 with PHP's `preg_match()` semantics |
| `crates/twig-timelib`, `crates/twig-date` | timelib port with timezonedb 2026.6, PHP's `DateTime` family |
| `crates/twig-test-support` | (development only) Twig's integration fixtures as values, PHP test classes |
| `crates/twig-fuzz` | (development only) differential testing against the PHP oracle |
| `tests/` | Twig's fixture suite, ported PHPUnit tests, oracle-generated vectors, fuzz regression vectors |
| `benches/` | criterion benchmarks |
| `plans/`, `reference/`, `research/` | open work, facts about the target, investigations (including the PHP oracle) |

## Contributing

PHP decides what is correct. [`research/php-oracle`](research/php-oracle/README.md) runs Twig 3.31.1 on PHP 8.5.13
in Docker and answers JSON batches: render templates, evaluate PHP, run an upstream fixture. A change to template
behaviour goes like this:

1. Reproduce the difference with [`twig-fuzz`](crates/twig-fuzz/src/lib.rs) (`cargo run -p twig-fuzz -- help`):
   `twig-fuzz oracle` runs a batch of cases through PHP, `twig-fuzz diff` compares the results with twig-rs, and
   `twig-fuzz drive` does both, then clusters and minimises the mismatches (`twig-fuzz generate` writes random
   batches).
2. Port the fix from the upstream sources (Twig 3.31.1, php-src 8.5.13), keeping their algorithm and verbatim
   messages, and cite them in the doc comments ([`reference/conventions.md`](reference/conventions.md)).
3. Pin the behaviour with oracle-generated vectors: regression vectors in `tests/vectors/fuzz/` (rules in its
   [README](tests/vectors/fuzz/README.md); `twig-fuzz replay --update` removes the marker of a fixed vector) or
   vector files next to the tests.
4. `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` must pass; the
   tests need no Docker.

Open work is listed in [`plans/`](plans/roadmap.md): the roadmap, the known mismatches and the first release.

## Licence

twig-rs is distributed under the BSD 3-Clause licence ([`LICENSE`](LICENSE)). It contains code ported from Twig
(BSD-3-Clause), PHP (PHP License 3.01), the Zend Engine (Zend Engine License 2.00) and other third-party sources and
data, listed in [`NOTICE`](NOTICE); the `license` field of each crate combines the licences of what that crate
contains. The exception is the optional `twig-iconv` crate (the `glibc-iconv` feature), which follows the GNU C
Library's iconv sources and is distributed under the GNU Lesser General Public License 2.1 or later
([`crates/twig-iconv/LICENSE`](crates/twig-iconv/LICENSE)).
