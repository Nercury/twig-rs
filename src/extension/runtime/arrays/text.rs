//! The string side of the array group: `mb_strlen()` and `mb_substr()` with the environment charset, Twig's
//! `CoreExtension::convertEncoding()` and `CoreExtension::splitIntoCharacters()`.

use twig_value::builtins::string::iconv::iconv_bytes;
use twig_value::param::{self, Arg};
use twig_value::{Diagnostics, PhpError, PhpResult, PhpString, Value};

use crate::error::Error;
use crate::vm::Exception;

/// How an mbstring encoding cuts a string into characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Encoding {
    /// UTF-8 (`twig-unicode`, mbstring's decoder and `?` substitution).
    Utf8,
    /// A single-byte encoding (`MBFL_ENCTYPE_SBCS`): one character per byte.
    SingleByte,
}

/// `php_mb_get_encoding($charset, $position)` (PHPSRC/ext/mbstring/mbstring.c) for the encodings twig-rs implements:
/// UTF-8 and the single-byte encodings `Markup::count()` accepts. Any other name is rejected like an unknown
/// encoding: `ValueError: <function>(): Argument #<position> ($encoding) must be a valid encoding, "<name>" given`
/// (PHP also knows the multi-byte Asian and UTF-16/32 encodings; see `reference/deviations.md`).
pub(super) fn mb_encoding(charset: &[u8], function: &str, position: u32) -> PhpResult<Encoding> {
    let name = String::from_utf8_lossy(charset).to_ascii_uppercase();
    match name.as_str() {
        "UTF-8" | "UTF8" => Ok(Encoding::Utf8),
        "ASCII" | "US-ASCII" | "8BIT" | "BINARY" => Ok(Encoding::SingleByte),
        n if n.starts_with("ISO-8859-")
            || n.starts_with("WINDOWS-125")
            || n.starts_with("CP125")
            || n.starts_with("ISO8859-") =>
        {
            Ok(Encoding::SingleByte)
        }
        _ => Err(PhpError::value_error(PhpString::concat(&[
            format!("{function}(): Argument #{position} ($encoding) must be a valid encoding, \"")
                .as_bytes(),
            c_str(charset),
            b"\" given",
        ]))),
    }
}

/// The part of a byte string C's `%s` prints (up to the first NUL byte).
fn c_str(bytes: &[u8]) -> &[u8] {
    match memchr::memchr(0, bytes) {
        Some(nul) => &bytes[..nul],
        None => bytes,
    }
}

/// Port of `mb_strlen($string, $charset)` (`PHP_FUNCTION(mb_strlen)`, PHPSRC/ext/mbstring/mbstring.c): the string
/// parameter coerced like an internal function's (`TypeError` for arrays and objects without `__toString()`), then
/// the encoding (argument #2).
pub(super) fn mb_strlen(
    value: &Value,
    charset: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<i64> {
    let string = param::parse_str(value, &Arg::new("mb_strlen", 1, "string"), diag)?;
    Ok(match mb_encoding(charset, "mb_strlen", 2)? {
        Encoding::Utf8 => twig_unicode::mb_strlen(&string) as i64,
        Encoding::SingleByte => string.len() as i64,
    })
}

/// Port of `mb_substr($string, $start, $length, $charset)` (`PHP_FUNCTION(mb_substr)`): `int $start`, `?int
/// $length` (coerced, with their `TypeError`s and deprecations), the `ZEND_LONG_MIN` checks, then the encoding
/// (argument #4).
pub(super) fn mb_substr(
    value: &Value,
    start: &Value,
    length: &Value,
    charset: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let string = param::parse_str(value, &Arg::new("mb_substr", 1, "string"), diag)?;
    let start = param::parse_long(start, &Arg::new("mb_substr", 2, "start"), diag)?;
    let length = param::parse_long_or_null(length, &Arg::new("mb_substr", 3, "length"), diag)?;
    let bounds = "must be between -9223372036854775807 and 9223372036854775807";
    if start == i64::MIN {
        return Err(Arg::new("mb_substr", 2, "start").value_error(bounds));
    }
    if length == Some(i64::MIN) {
        return Err(Arg::new("mb_substr", 3, "length").value_error(bounds));
    }
    match mb_encoding(charset, "mb_substr", 4)? {
        Encoding::Utf8 => twig_unicode::mb_substr(&string, start, length)
            .map(PhpString::from)
            .map_err(|e| PhpError::value_error(e.message())),
        Encoding::SingleByte => Ok(PhpString::from(substr_bytes(&string, start, length))),
    }
}

/// `PHP_FUNCTION(mb_substr)` for a fixed-width single-byte encoding: the character count is the byte count and
/// `mb_get_substr()` slices the bytes.
fn substr_bytes(s: &[u8], start: i64, length: Option<i64>) -> &[u8] {
    let mblen = s.len() as u64;
    let real_from = if start >= 0 {
        start as u64
    } else if start.unsigned_abs() < mblen {
        mblen - start.unsigned_abs()
    } else {
        0
    };
    let real_len = match length {
        None => u64::MAX,
        Some(len) if len >= 0 => len as u64,
        Some(len) if real_from < mblen && len.unsigned_abs() < mblen - real_from => {
            (mblen - real_from) - len.unsigned_abs()
        }
        Some(_) => 0,
    };
    if real_len == 0 || real_from >= mblen {
        return &[];
    }
    let end = real_from.saturating_add(real_len).min(mblen);
    &s[real_from as usize..end as usize]
}

/// Twig: `CoreExtension::convertEncoding($string, $to, $from): string` (TWIG/src/Extension/CoreExtension.php) on a
/// string: `iconv($from, $to, $string)`, whose `false` (after iconv's warning or notice) the `string` return type
/// turns into `''`.
pub(super) fn convert_encoding(
    string: &[u8],
    to: &[u8],
    from: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    Ok(iconv_bytes(from, to, string, diag)?.unwrap_or_default())
}

/// Twig: `CoreExtension::splitIntoCharacters(string $string, string $name): array` — `preg_match_all('/./us',
/// $string)`: every code point of a valid UTF-8 string, `['']` for the empty string. The `u` modifier makes PCRE2
/// reject ill-formed UTF-8 (RFC 3629, the rules of `std::str::from_utf8`), and Twig turns the `false` into
/// `RuntimeError('Unable to split the string passed to "%s" into characters: %s.')` with `preg_last_error_msg()`.
pub(super) fn split_into_characters(
    string: &PhpString,
    name: &str,
) -> Result<Vec<PhpString>, Exception> {
    let Ok(text) = std::str::from_utf8(string.as_bytes()) else {
        return Err(Error::runtime(format!(
            "Unable to split the string passed to \"{name}\" into characters: {}.",
            twig_pcre::PregError::BadUtf8.message()
        ))
        .into());
    };
    if text.is_empty() {
        return Ok(vec![PhpString::empty()]);
    }
    let mut buffer = [0u8; 4];
    Ok(text
        .chars()
        .map(|c| PhpString::from(c.encode_utf8(&mut buffer).as_bytes()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_byte_substr_matches_mb_substr_rules() {
        assert_eq!(substr_bytes(b"abcdef", 1, Some(2)), b"bc");
        assert_eq!(substr_bytes(b"abcdef", -3, Some(-1)), b"de");
        assert_eq!(substr_bytes(b"abc", -10, Some(2)), b"ab");
        assert_eq!(substr_bytes(b"abc", 5, None), b"");
        assert_eq!(substr_bytes(b"abc", 1, None), b"bc");
        assert_eq!(substr_bytes(b"abc", 0, Some(-5)), b"");
    }

    #[test]
    fn splits_code_points() {
        let parts = split_into_characters(&PhpString::from("aé€😀"), "split").unwrap();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[3].as_bytes(), "😀".as_bytes());
        let empty = split_into_characters(&PhpString::empty(), "split").unwrap();
        assert_eq!(empty, vec![PhpString::empty()]);
        let error = split_into_characters(&PhpString::from(&b"a\xffb"[..]), "reverse").unwrap_err();
        assert_eq!(
            error.message(),
            "Unable to split the string passed to \"reverse\" into characters: Malformed UTF-8 characters, possibly incorrectly encoded."
        );
        assert_eq!(
            mb_encoding(b"NOPE-1", "mb_strlen", 2)
                .unwrap_err()
                .message()
                .to_string_lossy(),
            "mb_strlen(): Argument #2 ($encoding) must be a valid encoding, \"NOPE-1\" given"
        );
    }
}
