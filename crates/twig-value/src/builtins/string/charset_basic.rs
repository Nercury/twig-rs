//! `iconv()` for the common charsets, written without glibc code, under every name glibc gives them:
//!
//! - the Unicode charsets: UTF-8, UTF-16, UTF-16BE, UTF-16LE, UTF-32, UTF-32BE, UTF-32LE, UCS-2, UCS-2BE, UCS-2LE,
//!   UCS-4, UCS-4BE, UCS-4LE;
//! - ANSI_X3.4-1968 (ASCII) and ISO-8859-1;
//! - the codepages: ISO-8859-2 to ISO-8859-16 (glibc has no ISO-8859-12), CP1250 to CP1258 (with the letters and
//!   marks that CP1255 and CP1258 combine), KOI8-R, KOI8-U, KOI8-RU, CP437, CP850, CP852, CP866 and MACINTOSH.
//!
//! Any other charset glibc knows raises `Error: iconv(): conversion of charset "X" is not supported by twig-rs`; a
//! name glibc does not know behaves as in PHP.
//!
//! Clean room: the PHP layer ports `PHP_FUNCTION(iconv)` and `php_iconv_string()` (PHPSRC/ext/iconv/iconv.c), the
//! Unicode charsets follow their published specifications (RFC 3629, RFC 2781, ISO/IEC 10646, ISO/IEC 8859-1), and
//! everything glibc 2.41 decides on its own — which values each charset rejects, byte order marks, the codepage
//! tables (compared with the Unicode Consortium's mapping files, whose differences they record), which characters
//! CP1255 and CP1258 combine, charset names and their aliases, the //TRANSLIT and //IGNORE suffixes, the
//! transliteration tables, how a call reports errors and where it stops — was learned black-box from the oracle
//! (`twig-oracle:3.31.1`, PHP 8.5.13 on glibc 2.41, `LC_CTYPE=C.UTF-8`) through PHP and through the public
//! `iconv_open()`/`iconv()` interface. No glibc source was read. The vectors in tests/vectors/string/charset_basic
//! and their generators show the observations.
//!
//! - `names`: how glibc parses a charset name, and the names it knows (`name_table.rs`, from `iconv -l`);
//! - `codec`: decoding and encoding each charset;
//! - `codepage`: the tables of the codepages (`codepage_table.rs`);
//! - `engine`: one `iconv(3)` call (rounds of 8160 characters, errors, `E2BIG`, //TRANSLIT and //IGNORE, the
//!   characters CP1255 and CP1258 hold back) and the flush;
//! - `translit`: the //TRANSLIT replacements (`translit_table.rs`);
//! - this module: PHP's loop around `iconv(3)`, its notices and its `false`.

mod codec;
mod codepage;
#[rustfmt::skip]
mod codepage_table;
mod engine;
mod multibyte;
#[rustfmt::skip]
mod multibyte_table;
#[rustfmt::skip]
mod name_table;
mod names;
mod translit;
#[rustfmt::skip]
mod translit_table;

use engine::{Converter, Status};
use names::Lookup;

use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::Value;

/// `ICONV_CSNMAXLEN` (PHPSRC/ext/iconv/php_iconv.h): charset names must be shorter.
const ICONV_CSNMAXLEN: usize = 64;

/// PHP's `iconv(string $from_encoding, string $to_encoding, string $string): string|false`; `None` is `false`.
///
/// Port of `PHP_FUNCTION(iconv)` (PHPSRC/ext/iconv/iconv.c): the arguments are parsed in coercive mode, then
/// [`iconv_bytes`] converts.
///
/// # Errors
///
/// Argument `TypeError`s, the `Error` for a charset twig-rs does not convert, and diagnostics the sink turns into
/// exceptions.
pub fn iconv(
    from_encoding: &Value,
    to_encoding: &Value,
    string: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<PhpString>> {
    let from = param::parse_str(
        from_encoding,
        &Arg::new("iconv", 1, "from_encoding").declared("string"),
        diag,
    )?;
    let to = param::parse_str(
        to_encoding,
        &Arg::new("iconv", 2, "to_encoding").declared("string"),
        diag,
    )?;
    let string = param::parse_str(
        string,
        &Arg::new("iconv", 3, "string").declared("string"),
        diag,
    )?;
    iconv_bytes(&from, &to, &string, diag)
}

/// `iconv($from, $to, $string)` on PHP strings: `Some(converted)`, or `None` (PHP's `false`) after the warning or
/// notice PHP raises.
///
/// Port of the body of `PHP_FUNCTION(iconv)` after argument parsing, with `php_iconv_string()` and
/// `_php_iconv_show_error()` (PHPSRC/ext/iconv/iconv.c) on top of `engine::Converter`, the model of glibc's `iconv(3)`.
///
/// # Errors
///
/// `Error: iconv(): conversion of charset "X" is not supported by twig-rs` for a charset glibc knows but this module
/// does not convert (`X` is the name as given; the source charset is reported first), and diagnostics the sink
/// turns into exceptions.
pub fn iconv_bytes(
    from: &[u8],
    to: &[u8],
    string: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<PhpString>> {
    if from.len() >= ICONV_CSNMAXLEN || to.len() >= ICONV_CSNMAXLEN {
        diag.warn(format!(
            "iconv(): Encoding parameter exceeds the maximum allowed length of {ICONV_CSNMAXLEN} characters"
        ))?;
        return Ok(None);
    }
    // C sees the names up to their first NUL
    let in_charset = c_str(from);
    let out_charset = c_str(to);
    let source = names::parse(in_charset);
    let target = names::parse(out_charset);
    let (from_charset, to_charset) = match (source.lookup, target.lookup) {
        (Lookup::Unknown, _) | (_, Lookup::Unknown) => {
            // PHP_ICONV_ERR_WRONG_CHARSET
            diag.warn(PhpString::concat(&[
                b"iconv(): Wrong encoding, conversion from \"",
                in_charset,
                b"\" to \"",
                out_charset,
                b"\" is not allowed",
            ]))?;
            return Ok(None);
        }
        (Lookup::Unsupported, _) => return Err(unsupported(in_charset)),
        (_, Lookup::Unsupported) => return Err(unsupported(out_charset)),
        (Lookup::Supported(f), Lookup::Supported(t)) => (f, t),
    };
    let mut converter = Converter::new(from_charset, to_charset, target.translit, target.ignore);
    match php_iconv_string(string, &mut converter, php_check_ignore(out_charset)) {
        Ok(out) => Ok(Some(PhpString::from(out))),
        Err(Status::Incomplete) => {
            diag.notice("iconv(): Detected an incomplete multibyte character in input string")?;
            Ok(None)
        }
        Err(Status::Illegal) => {
            diag.notice("iconv(): Detected an illegal character in input string")?;
            Ok(None)
        }
        Err(Status::TooBig | Status::Done) => {
            // "should not happen" in PHP: glibc never reports E2BIG with the input used up
            diag.warn("iconv(): Buffer length exceeded")?;
            Ok(None)
        }
    }
}

/// Port of the conversion loop of `php_iconv_string()` (PHPSRC/ext/iconv/iconv.c): an output buffer of the input
/// length plus 32 bytes that grows by the input length on `E2BIG`, and, with `ignore_ilseq`, glibc's `EILSEQ` taken
/// as success when at most one input byte is left, else skipping one input byte and calling again. Then the flush
/// (`iconv(cd, NULL, NULL, ...)`, the buffer growing by 16 bytes on `E2BIG`), which writes the character CP1255 and
/// CP1258 may still hold back. `Err` is the status that makes PHP return `false`.
fn php_iconv_string(
    input: &[u8],
    converter: &mut Converter,
    ignore_ilseq: bool,
) -> Result<Vec<u8>, Status> {
    let in_len = input.len();
    let mut out = Vec::with_capacity(in_len + 32);
    let mut buffer_size = in_len + 32;
    let mut pos = 0;
    while pos < in_len {
        let space = buffer_size - out.len();
        let (consumed, status) = converter.call(&input[pos..], &mut out, space);
        pos += consumed;
        match status {
            Status::Done => {}
            Status::Illegal if ignore_ilseq => {
                if in_len - pos <= 1 {
                    break;
                }
                pos += 1;
            }
            Status::TooBig if pos < in_len => buffer_size += in_len,
            other => return Err(other),
        }
    }
    // the flush, growing the buffer by 16 bytes on E2BIG
    loop {
        let space = buffer_size - out.len();
        match converter.flush(&mut out, space) {
            Status::Done => return Ok(out),
            Status::TooBig => buffer_size += 16,
            other => return Err(other),
        }
    }
}

/// `_php_check_ignore()` (PHPSRC/ext/iconv/iconv.c, compiled in because glibc's //IGNORE makes `iconv()` fail):
/// whether the target name ends in `//IGNORE` or `//IGNORE//TRANSLIT`, case-sensitively.
fn php_check_ignore(charset: &[u8]) -> bool {
    (charset.len() >= 9 && charset.ends_with(b"//IGNORE"))
        || (charset.len() >= 19 && charset.ends_with(b"//IGNORE//TRANSLIT"))
}

/// The bytes before the first NUL.
fn c_str(s: &[u8]) -> &[u8] {
    memchr::memchr(0, s).map_or(s, |end| &s[..end])
}

/// The `Error` for a charset glibc knows but twig-rs does not convert (reference/deviations.md).
fn unsupported(charset: &[u8]) -> PhpError {
    PhpError::error(PhpString::concat(&[
        b"iconv(): conversion of charset \"",
        charset,
        b"\" is not supported by twig-rs",
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{Diagnostic, Level};

    fn conv(from: &str, to: &str, s: &[u8]) -> (Option<Vec<u8>>, Vec<Diagnostic>) {
        let mut diag: Vec<Diagnostic> = Vec::new();
        let r = iconv_bytes(from.as_bytes(), to.as_bytes(), s, &mut diag).expect("no exception");
        (r.map(|s| s.as_bytes().to_vec()), diag)
    }

    #[test]
    fn basics() {
        assert_eq!(
            conv("UTF-8", "ISO-8859-1", "a\u{e9}".as_bytes()).0.unwrap(),
            b"a\xe9"
        );
        assert_eq!(conv("UTF-8", "UTF-16", b"a").0.unwrap(), b"\xff\xfea\x00");
        assert_eq!(
            conv("UTF-8", "ASCII//TRANSLIT", "\u{20ac}".as_bytes())
                .0
                .unwrap(),
            b"EUR"
        );
        assert_eq!(
            conv("UTF-8", "ASCII//IGNORE", "a\u{e9}b".as_bytes())
                .0
                .unwrap(),
            b"ab"
        );
        let (r, d) = conv("UTF-8", "ASCII", "a\u{e9}b".as_bytes());
        assert_eq!(r, None);
        assert_eq!(
            d,
            [Diagnostic::new(
                Level::Notice,
                "iconv(): Detected an illegal character in input string"
            )]
        );
        let (r, d) = conv("UTF-8", "UTF-16", b"a\xc3");
        assert_eq!(r, None);
        assert_eq!(
            d,
            [Diagnostic::new(
                Level::Notice,
                "iconv(): Detected an incomplete multibyte character in input string"
            )]
        );
        let (r, d) = conv("UTF-8", "NOPE", b"a");
        assert_eq!(r, None);
        assert_eq!(
            d,
            [Diagnostic::new(
                Level::Warning,
                "iconv(): Wrong encoding, conversion from \"UTF-8\" to \"NOPE\" is not allowed"
            )]
        );
    }

    #[test]
    fn php_ignore_skips_a_byte_after_an_early_stop() {
        // glibc stops after its first round with EILSEQ; PHP then skips one input byte
        let mut input = "\u{20ac}".as_bytes().to_vec();
        input.extend(std::iter::repeat_n(b'a', 9000));
        assert_eq!(
            conv("UTF-8", "ISO-8859-1//IGNORE", &input).0.unwrap().len(),
            8999
        );
        // with //ignore PHP does not know glibc skips; the lost status of an early round makes it succeed
        input[..3].copy_from_slice(b"\xff\xff\xff");
        assert_eq!(
            conv("UTF-8", "ISO-8859-1//ignore", &input).0.unwrap().len(),
            9000
        );
    }

    #[test]
    fn codepages() {
        assert_eq!(
            conv("UTF-8", "CP1252", "\u{20ac}".as_bytes()).0.unwrap(),
            b"\x80"
        );
        assert_eq!(
            conv("koi8r", "UTF-8", b"\xc1").0.unwrap(),
            "\u{430}".as_bytes()
        );
        // the Cyrillic alternative of U+20BD where the target has Cyrillic letters
        assert_eq!(
            conv("UTF-8", "KOI8-R//TRANSLIT", "\u{20bd}".as_bytes())
                .0
                .unwrap(),
            b"\xd2\xd5\xc2"
        );
        // CP1258 combines a letter and a mark, and writes a character it has no byte for as both
        assert_eq!(
            conv("CP1258", "UTF-8", b"A\xcc").0.unwrap(),
            "\u{c0}".as_bytes()
        );
        assert_eq!(
            conv("UTF-8", "CP1258", "\u{c3}".as_bytes()).0.unwrap(),
            b"A\xde"
        );
        // the flush writes the held last character, which //IGNORE cannot skip without failing
        assert_eq!(
            conv("CP1258", "ISO-8859-1//IGNORE", b"\xd5a").0.unwrap(),
            b"a"
        );
        let (r, d) = conv("CP1258", "ISO-8859-1//IGNORE", b"a\xd5");
        assert_eq!(r, None);
        assert_eq!(
            d,
            [Diagnostic::new(
                Level::Notice,
                "iconv(): Detected an illegal character in input string"
            )]
        );
    }

    #[test]
    fn unsupported_charsets() {
        let mut diag: Vec<Diagnostic> = Vec::new();
        let e = iconv_bytes(b"IBM930", b"UTF-8", b"a", &mut diag).unwrap_err();
        assert_eq!(
            e.to_string(),
            "Error: iconv(): conversion of charset \"IBM930\" is not supported by twig-rs"
        );
        let e = iconv_bytes(b"UTF-8", b"ibm-930//TRANSLIT", b"", &mut diag).unwrap_err();
        assert_eq!(
            e.message_lossy(),
            "iconv(): conversion of charset \"ibm-930//TRANSLIT\" is not supported by twig-rs"
        );
        // a name glibc does not know wins: PHP's warning
        assert_eq!(
            iconv_bytes(b"IBM930", b"NOPE", b"a", &mut diag).unwrap(),
            None
        );
        assert_eq!(diag.len(), 1);
    }
}
