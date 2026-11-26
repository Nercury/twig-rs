# twig-pcre

PCRE2 10.44 built exactly as PHP 8.5.13 bundles it (the vendored C sources of `ext/pcre/pcre2lib`, compiled with
PHP's own configuration: Unicode, newline LF, the JIT), with a port of PHP's `preg_match()` front end: delimiter and
modifier parsing, the warning texts, the compiled-pattern cache, the backtrack and recursion limits, the JIT stack,
`preg_last_error()`, `preg_last_error_msg()` and `preg_quote()`. Results, warnings and error codes are checked against
PHP with oracle-generated test vectors, including the points where the limits are reached. Building needs a C
compiler; the `jit` feature (on by default) compiles the JIT, as PHP's default build does.

It is part of [twig-rs](https://github.com/Nercury/twig-rs), a Rust implementation of Twig that renders templates
exactly like Twig 3.31.1 on PHP 8.5.13: the [`twig`](https://crates.io/crates/twig) crate's `matches` operator uses
it, with the exact messages Twig builds from PHP's errors.

```rust
use twig_pcre::{Diagnostic, matches, preg_match};

fn main() {
    // Twig's `matches` operator: PHP's result, or the message of the RuntimeError Twig throws
    assert_eq!(matches(b"/^\\d+$/", b"123"), Ok(1));
    let error = matches(b"/(/", b"x").unwrap_err();
    assert_eq!(
        error.message(b"/(/"),
        b"Regexp \"/(/\" passed to \"matches\" is not valid: \
          Compilation failed: missing closing parenthesis at offset 1",
    );

    // preg_match(): `false` (None) with PHP's warning
    let mut warnings: Vec<Diagnostic> = Vec::new();
    assert_eq!(preg_match(b"abc", b"abc", &mut |warning| warnings.push(warning)), None);
    assert_eq!(warnings[0].message, b"Delimiter must not be alphanumeric, backslash, or NUL byte");
}
```

The [API documentation](https://docs.rs/twig-pcre) has the layers of the crate and its few known differences from
PHP, all outside normal use.

## Licence

`BSD-3-Clause AND BSD-2-Clause AND PHP-3.01`: twig-rs, the checks of Twig's `matches` operator and the vendored PCRE2
are under the BSD 3-Clause licence, PCRE2's JIT compiler sljit under the BSD 2-Clause licence, and the port of PHP's
`ext/pcre` front end under the PHP License 3.01. The `LICENSE` and `NOTICE` files that come with the crate, and
`pcre2/LICENCE` for PCRE2 and sljit, hold the details and the licence texts.
