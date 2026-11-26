//! The string side of the array group: `mb_strlen()` and `mb_substr()` with the environment charset (over the
//! string group's mbstring encodings, [`crate::extension::runtime::strings::mb`]), Twig's
//! `CoreExtension::convertEncoding()` and `CoreExtension::splitIntoCharacters()`.

use twig_value::builtins::string::IconvBackend;
use twig_value::param::{self, Arg};
use twig_value::{Diagnostics, PhpResult, PhpString, Value};

use super::super::strings::mb;
use crate::error::Error;
use crate::vm::Exception;

/// Port of `mb_strlen($string, $charset)` (`PHP_FUNCTION(mb_strlen)`, PHPSRC/ext/mbstring/mbstring.c): the string
/// parameter coerced like an internal function's (`TypeError` for arrays and objects without `__toString()`), then
/// the encoding (argument #2, `php_mb_get_encoding()`), then `mb_get_strlen()`.
///
/// # Errors
///
/// The `TypeError` of the string parameter, the errors of [`mb::get_encoding`].
pub(super) fn mb_strlen(
    value: &Value,
    charset: &[u8],
    diag: &mut dyn Diagnostics,
) -> Result<i64, Exception> {
    let string = param::parse_str(value, &Arg::new("mb_strlen", 1, "string"), diag)?;
    Ok(mb::mb_strlen(&string, Some(charset), diag)?)
}

/// Port of `mb_substr($string, $start, $length, $charset)` (`PHP_FUNCTION(mb_substr)`): `int $start`, `?int
/// $length` (coerced, with their `TypeError`s and deprecations), the `ZEND_LONG_MIN` checks, then the encoding
/// (argument #4), then `mb_get_substr()`.
///
/// # Errors
///
/// The parameters' `TypeError`s, the `ValueError`s of [`mb::substr_range_error`], the errors of
/// [`mb::get_encoding`].
pub(super) fn mb_substr(
    value: &Value,
    start: &Value,
    length: &Value,
    charset: &[u8],
    diag: &mut dyn Diagnostics,
) -> Result<PhpString, Exception> {
    let string = param::parse_str(value, &Arg::new("mb_substr", 1, "string"), diag)?;
    let start = param::parse_long(start, &Arg::new("mb_substr", 2, "start"), diag)?;
    let length = param::parse_long_or_null(length, &Arg::new("mb_substr", 3, "length"), diag)?;
    if let Some(error) = mb::substr_range_error(start, length) {
        return Err(error.into());
    }
    let charset = mb::get_encoding("mb_substr", 4, charset, diag)?;
    mb::substr(&string, start, length, charset)
}

/// Twig: `CoreExtension::convertEncoding($string, $to, $from): string` (TWIG/src/Extension/CoreExtension.php) on a
/// string: `iconv($from, $to, $string)` (with the environment's backend, `iconv`), whose `false` (after iconv's
/// warning or notice) the `string` return type turns into `''`.
pub(super) fn convert_encoding(
    string: &[u8],
    to: &[u8],
    from: &[u8],
    iconv: &dyn IconvBackend,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    Ok(iconv
        .iconv_bytes(from, to, string, diag)?
        .unwrap_or_default())
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
    }

    #[test]
    fn counts_and_cuts_in_the_charset() {
        let mut diag: Vec<twig_value::Diagnostic> = Vec::new();
        let abc = Value::from("abcdef");
        assert_eq!(
            mb_strlen(&Value::from("é"), b"UTF-8", &mut diag).unwrap(),
            1
        );
        assert_eq!(
            mb_strlen(&Value::from("é"), b"KOI8-R", &mut diag).unwrap(),
            2
        );
        let cut = |start: i64, length: Value, diag: &mut Vec<twig_value::Diagnostic>| {
            mb_substr(&abc, &Value::Int(start), &length, b"ISO-8859-1", diag).unwrap()
        };
        assert_eq!(cut(1, Value::Int(2), &mut diag).as_bytes(), b"bc");
        assert_eq!(cut(-3, Value::Int(-1), &mut diag).as_bytes(), b"de");
        assert_eq!(cut(5, Value::Null, &mut diag).as_bytes(), b"f");
        assert_eq!(
            mb_strlen(&abc, b"NOPE-1", &mut diag).unwrap_err().message(),
            "mb_strlen(): Argument #2 ($encoding) must be a valid encoding, \"NOPE-1\" given"
        );
        assert_eq!(
            mb_strlen(&abc, b"SJIS", &mut diag).unwrap_err().message(),
            "mb_strlen(): encoding \"SJIS\" is not supported by twig-rs"
        );
        assert!(diag.is_empty());
    }
}
