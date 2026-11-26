# twig-iconv

PHP 8.5.13's `iconv()` exactly as it behaves on glibc 2.41, for nearly every charset glibc converts: the single-byte
codepages (ISO-8859-*, Windows, DOS, EBCDIC, Mac, KOI8, ...), EUC-JP, Shift_JIS and CP932, the ISO-2022 family,
GB2312, GBK, GB18030, BIG5 and BIG5-HKSCS, EUC-KR and UHC, with glibc's names and aliases, shift states, error
handling (`//IGNORE`) and transliteration (`//TRANSLIT`). Results and notices are checked against PHP on glibc with
oracle-generated test vectors.

It is the optional charset converter of [twig-rs](https://github.com/Nercury/twig-rs), a Rust implementation of Twig
that renders templates exactly like Twig 3.31.1 on PHP 8.5.13. Templates convert charsets with PHP's `iconv()`: the
`convert_encoding` filter, escaping under a charset other than UTF-8, and `reverse`, `shuffle` and `random` under such
a charset. By default the [`twig`](https://crates.io/crates/twig) crate converts them with the clean-room converter of
[`twig-value`](https://crates.io/crates/twig-value), which knows fewer charsets; twig's `glibc-iconv` cargo feature
makes `GlibcIconv` the default converter of every environment instead, and an environment can also be given one
explicitly (`EnvironmentOptions::iconv_backend`).

```rust
use twig_iconv::{GlibcIconv, IconvBackend};
use twig_value::{Diagnostic, PhpError};

fn main() -> Result<(), PhpError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // ISO-2022-JP switches to JIS X 0208 and back
    let jis = GlibcIconv.iconv_bytes(b"UTF-8", b"ISO-2022-JP", "愛".as_bytes(), &mut diagnostics)?;
    assert_eq!(jis.unwrap().as_bytes(), b"\x1b$B0&\x1b(B");

    // glibc's transliteration: CP1252 has the euro sign, not the interrobang
    let cp1252 = GlibcIconv.iconv_bytes(b"UTF-8", b"CP1252//TRANSLIT", "€ ‽".as_bytes(), &mut diagnostics)?;
    assert_eq!(cp1252.unwrap().as_bytes(), b"\x80 ?");

    // invalid input: `false` and PHP's notice
    assert_eq!(GlibcIconv.iconv_bytes(b"UTF-8", b"KOI8-R", b"\xff", &mut diagnostics)?, None);
    assert_eq!(diagnostics[0].to_string(), "Notice: iconv(): Detected an illegal character in input string");
    Ok(())
}
```

The [API documentation](https://docs.rs/twig-iconv) lists the charsets and the few that are not covered.

## Licence

**`LGPL-2.1-or-later AND PHP-3.01 AND Unicode-3.0`.** Reproducing glibc byte for byte meant following glibc's iconv
sources closely (the conversion driver and the charset modules), so this crate is a work based on the GNU C Library
and is distributed under the GNU Lesser General Public License, version 2.1 or later, like glibc. It also contains a
port of PHP's `ext/iconv/iconv.c` (PHP License 3.01) and transliteration data that glibc generates from the Unicode
Character Database (Unicode License v3). The `LICENSE` and `NOTICE` files that come with the crate hold the details
and the licence texts.

A program that uses this crate links LGPL code (statically, as Rust links crates) and must meet the conditions of the
LGPL when it is distributed. The rest of twig-rs is permissively licensed and depends on this crate only through the
opt-in `glibc-iconv` feature.
