//! `iconv()` (reference/php/stdlib.md §7): Twig's `convert_encoding` filter, the escaper's charset conversions and
//! the other callables that convert from and to the environment's charset.
//!
//! PHP's `iconv()` hands the conversion to the C library's `iconv(3)`, glibc 2.41 in the oracle. Here a whole
//! `iconv()` builtin (PHP's argument handling, output buffer loop, notices and `false`, on top of a model of the C
//! library) is an [`IconvBackend`], and code that converts on behalf of a template takes the backend to use: twig's
//! `Environment` holds one.
//!
//! - [`BasicIconv`], the default, is the clean-room converter of [`charset_basic`](super::charset_basic): UTF-8,
//!   UTF-16, UTF-32, UCS-2 and UCS-4 with their byte-order variants, ASCII, ISO-8859-1 and the single-byte codepages (ISO-8859-2 to -16, Windows-1250 to -1258, KOI8-R/U/RU, CP437/850/852/866, MACINTOSH), under every name glibc
//!   gives them, byte for byte like the oracle. Any other charset glibc knows throws
//!   `Error: iconv(): conversion of charset "X" is not supported by twig-rs`; a name glibc does not know behaves as in
//!   PHP (the `Wrong encoding` warning, then `false`). It contains no glibc code.
//! - The `twig-iconv` crate provides `GlibcIconv`, glibc's behaviour for every charset it converts (the single-byte
//!   codepages, the CJK charsets, the ISO-2022 family, ...). It follows glibc's iconv sources and is therefore
//!   licensed under the LGPL (2.1 or later); twig uses it as the default backend with its `glibc-iconv` feature.
//!
//! [`iconv`] and [`iconv_bytes`] are PHP's `iconv()` through the default backend.

use std::fmt;

use super::str_param;
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::string::PhpString;
use crate::value::Value;

pub use super::charset_basic::{iconv, iconv_bytes};

/// An implementation of PHP's `iconv()` builtin: what converts strings between charsets for twig's callables
/// (`convert_encoding`, the escaping strategies under a charset other than UTF-8, `reverse`, `shuffle`, `random`).
///
/// A backend reproduces `PHP_FUNCTION(iconv)` (PHPSRC/ext/iconv/iconv.c) on top of a model of the C library's
/// `iconv(3)`: `Some(converted)`, or `None` (PHP's `false`) after the warning or notice PHP raises, reported to the
/// diagnostics sink. A charset the backend does not convert throws `Error: iconv(): conversion of charset "X" is not
/// supported by twig-rs` (reference/deviations.md) instead of guessing.
///
/// Environments share their backend between threads. Every call is a conversion of its own, like PHP's, which opens
/// an iconv descriptor per call: a backend keeps no state from one call to the next.
pub trait IconvBackend: fmt::Debug + Send + Sync {
    /// `iconv($from, $to, $string)` on PHP strings: the body of `PHP_FUNCTION(iconv)` after argument parsing.
    ///
    /// # Errors
    ///
    /// The `Error` for a charset the backend does not convert, and diagnostics the sink turns into exceptions.
    fn iconv_bytes(
        &self,
        from: &[u8],
        to: &[u8],
        string: &[u8],
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<PhpString>>;

    /// PHP's `iconv(string $from_encoding, string $to_encoding, string $string): string|false`, `None` being `false`:
    /// the arguments parsed in coercive mode (`Z_PARAM_STR` for each, in order), then
    /// [`iconv_bytes`](Self::iconv_bytes).
    ///
    /// # Errors
    ///
    /// The arguments' `TypeError`s and the deprecations the sink turns into exceptions, then the errors of
    /// [`iconv_bytes`](Self::iconv_bytes).
    fn iconv(
        &self,
        from_encoding: &Value,
        to_encoding: &Value,
        string: &Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<PhpString>> {
        let from = str_param(from_encoding, "iconv", 1, "from_encoding", diag)?;
        let to = str_param(to_encoding, "iconv", 2, "to_encoding", diag)?;
        let string = str_param(string, "iconv", 3, "string", diag)?;
        self.iconv_bytes(&from, &to, &string, diag)
    }
}

/// The default backend: [`charset_basic`](super::charset_basic), the clean-room `iconv()` for the Unicode charsets,
/// ASCII, ISO-8859-1 and the single-byte codepages (ISO-8859-2 to -16, Windows-1250 to -1258, KOI8-R/U/RU, CP437/850/852/866, MACINTOSH) (see the module documentation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BasicIconv;

impl IconvBackend for BasicIconv {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{Diagnostic, Level};

    /// A backend with only the required method: the provided `iconv()` parses the arguments like PHP.
    #[derive(Debug)]
    struct Echo;

    impl IconvBackend for Echo {
        fn iconv_bytes(
            &self,
            from: &[u8],
            to: &[u8],
            string: &[u8],
            _diag: &mut dyn Diagnostics,
        ) -> PhpResult<Option<PhpString>> {
            Ok(Some(PhpString::concat(&[from, b">", to, b":", string])))
        }
    }

    #[test]
    fn provided_iconv_parses_arguments_like_php() {
        let mut diag: Vec<Diagnostic> = Vec::new();
        let converted = Echo
            .iconv(&Value::from("a"), &Value::Int(7), &Value::Null, &mut diag)
            .unwrap();
        assert_eq!(converted.unwrap().as_bytes(), b"a>7:");
        assert_eq!(
            diag,
            [Diagnostic::new(
                Level::Deprecated,
                "iconv(): Passing null to parameter #3 ($string) of type string is deprecated"
            )]
        );
        let error = Echo
            .iconv(
                &Value::Array(crate::Array::new()),
                &Value::from("b"),
                &Value::from("c"),
                &mut diag,
            )
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: iconv(): Argument #1 ($from_encoding) must be of type string, array given"
        );
    }

    #[test]
    fn the_default_backend_is_the_clean_room_converter() {
        let backend: &dyn IconvBackend = &BasicIconv;
        let mut diag: Vec<Diagnostic> = Vec::new();
        let latin1 = backend
            .iconv_bytes(b"UTF-8", b"ISO-8859-1", "\u{e9}".as_bytes(), &mut diag)
            .unwrap();
        assert_eq!(latin1.unwrap().as_bytes(), b"\xe9");
        let error = backend
            .iconv(
                &Value::from("UTF-8"),
                &Value::from("SJIS"),
                &Value::from("a"),
                &mut diag,
            )
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Error: iconv(): conversion of charset \"SJIS\" is not supported by twig-rs"
        );
        assert!(diag.is_empty());
    }
}
