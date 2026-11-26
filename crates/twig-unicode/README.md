# twig-unicode

Unicode 17.0 tables and PHP's mbstring functions for UTF-8 byte strings: `mb_strlen()`, `mb_substr()`,
`mb_str_split()`, `mb_strtoupper()`, `mb_strtolower()`, `mb_convert_case()`, `mb_ucfirst()`, `mb_lcfirst()`,
`mb_check_encoding()` and `mb_ord()`, with the case mapping and character properties of mbstring's tables. Strings
are bytes, as in PHP: input need not be valid UTF-8, and ill-formed sequences are treated exactly as PHP 8.5.13's
mbstring treats them. Results match PHP byte for byte, which oracle-generated test vectors check. Only UTF-8 is
supported.

It is part of [twig-rs](https://github.com/Nercury/twig-rs), a Rust implementation of Twig that renders templates
exactly like Twig 3.31.1 on PHP 8.5.13: the string filters of the [`twig`](https://crates.io/crates/twig) crate
(`upper`, `lower`, `title`, `capitalize`, `length`, `slice`, `split`, ...) use it.

```rust
use twig_unicode::{ValueError, mb_strlen, mb_strtoupper, mb_substr, mb_ucfirst};

fn main() -> Result<(), ValueError> {
    // full case mapping, as mbstring does it: ß becomes SS, ǆ has a title case of its own
    assert_eq!(mb_strtoupper("straße".as_bytes()), "STRASSE".as_bytes());
    assert_eq!(mb_ucfirst("ǆemal".as_bytes()), "ǅemal".as_bytes());

    // characters, not bytes: an ill-formed sequence counts as one character and comes back as `?`
    let text = b"a\xc3\xb1b\xff";
    assert_eq!(mb_strlen(text), 4);
    assert_eq!(mb_substr(text, 1, Some(3))?, "ñb?".as_bytes());
    Ok(())
}
```

The [API documentation](https://docs.rs/twig-unicode) describes each function and the PHP function it ports.

## Licence

`BSD-3-Clause AND PHP-3.01 AND Unicode-3.0 AND MIT`: twig-rs is distributed under the BSD 3-Clause licence; the tables
are converted from php-src's `ext/mbstring/unicode_data.h`, which php-src generates from the Unicode Character
Database (Unicode License v3, PHP License 3.01); the case mapping and the mbstring functions port php-src's mbstring
code (PHP License 3.01), whose `php_unicode.c` is based on ucdata-2.5 (MIT, Computing Research Labs). No code from
libmbfl (LGPL) is used. The `LICENSE` and `NOTICE` files that come with the crate hold the details and the licence
texts.
