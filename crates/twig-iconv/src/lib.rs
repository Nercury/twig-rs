//! PHP 8.5.13's `iconv()` exactly as it behaves on glibc 2.41, for (nearly) every charset glibc converts: an
//! [`IconvBackend`] for the twig template engine.
//!
//! twig converts charsets through a backend: Twig's `convert_encoding` filter, the escaping strategies under a
//! charset other than UTF-8, and the other callables that convert from and to the environment's charset. The default
//! backend, twig-value's clean-room `BasicIconv`, converts UTF-8, UTF-16, UTF-32, UCS-2, UCS-4, ASCII, ISO-8859-1 and
//! the single-byte codepages (ISO-8859-2 to -16, Windows-1250 to -1258, KOI8-R/U/RU, CP437/850/852/866, MACINTOSH), and throws `Error: iconv(): conversion of charset "X" is not supported by twig-rs` for the other charsets glibc
//! knows. [`GlibcIconv`] converts those too: the single-byte codepages (ISO-8859-*, Windows, DOS, EBCDIC, Mac, KOI8,
//! ...), EUC-JP, Shift_JIS and CP932, the ISO-2022 family, GB2312, GBK, GB18030, BIG5 and BIG5-HKSCS, EUC-KR and UHC,
//! with glibc's names, aliases, shift states, error handling and transliteration. The [`mod@iconv`] module lists what is
//! covered and the few charsets that are not.
//!
//! # Licence
//!
//! Reproducing glibc byte for byte (where `//IGNORE` stops, which characters a module rejects, how stateful charsets
//! flush) meant following glibc's iconv sources closely: the conversion driver (iconv/skeleton.c, loop.c,
//! gconv_simple.c, gconv_trans.c, gconv_charset.c) and the charset modules (iconvdata/*.c). This crate is therefore a
//! work based on the GNU C Library and is distributed under the **GNU Lesser General Public License, version 2.1 or
//! later** (the `LICENSE` file), like glibc. It also contains a port of PHP's ext/iconv/iconv.c (PHP License 3.01) and
//! transliteration data glibc generates from the Unicode Character Database (Unicode License v3); the repository's
//! `NOTICE` file has the details and the licence texts.
//!
//! That is why it is a separate crate: twig and twig-value are permissively licensed and do not depend on it unless
//! asked to. A program that uses it links LGPL code (statically, as Rust links crates) and must meet the conditions
//! of the LGPL when it is distributed.
//!
//! # Use
//!
//! - twig's `glibc-iconv` cargo feature makes [`GlibcIconv`] the default backend of every environment:
//!
//!   ```toml
//!   [dependencies]
//!   twig = { git = "https://github.com/Nercury/twig-rs", branch = "revive", features = ["glibc-iconv"] }
//!   ```
//!
//! - or an environment gets it explicitly, with the `iconv_backend` option:
//!
//!   ```text
//!   let options = EnvironmentOptions { iconv_backend: Arc::new(twig_iconv::GlibcIconv), ..Default::default() };
//!   ```
//!
//! - or call it directly, through the trait or as functions:
//!
//! ```
//! use twig_iconv::GlibcIconv;
//! use twig_value::builtins::string::IconvBackend;
//! use twig_value::Diagnostic;
//!
//! let mut diagnostics: Vec<Diagnostic> = Vec::new();
//! let jis = GlibcIconv.iconv_bytes(b"UTF-8", b"ISO-2022-JP", "愛".as_bytes(), &mut diagnostics)?;
//! assert_eq!(jis.unwrap().as_bytes(), b"\x1b$B0&\x1b(B");
//! let cp1252 = twig_iconv::iconv_bytes(b"UTF-8", b"CP1252//TRANSLIT", "€ ‽".as_bytes(), &mut diagnostics)?;
//! assert_eq!(cp1252.unwrap().as_bytes(), b"\x80 ?");
//! # Ok::<(), twig_value::PhpError>(())
//! ```

#![warn(missing_docs)]

pub mod iconv;
#[rustfmt::skip]
mod iconv_tables;

pub use iconv::{iconv, iconv_bytes};
pub use twig_value::builtins::string::IconvBackend;

use twig_value::param::{self, Arg};
use twig_value::{Diagnostics, PhpResult, PhpString, Value};

/// PHP's `iconv()` on glibc 2.41 as an [`IconvBackend`]: [`iconv_bytes`] and [`iconv()`] (see the [`mod@iconv`]
/// module for the charsets).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GlibcIconv;

impl IconvBackend for GlibcIconv {
    fn iconv_bytes(
        &self,
        from: &[u8],
        to: &[u8],
        string: &[u8],
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<PhpString>> {
        iconv_bytes(from, to, string, diag)
    }

    fn iconv(
        &self,
        from_encoding: &Value,
        to_encoding: &Value,
        string: &Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<PhpString>> {
        iconv(from_encoding, to_encoding, string, diag)
    }
}

/// `Z_PARAM_STR` / `Z_PARAM_STRING` for a parameter declared `string $name` (the argument parsing of [`iconv()`]).
fn str_param(
    v: &Value,
    function: &str,
    position: u32,
    name: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    param::parse_str(
        v,
        &Arg::new(function, position, name).declared("string"),
        diag,
    )
}

/// The example of `README.md`, compiled and run as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;
