// Ported from PHPSRC/ext/mbstring/mbstring.c (php-src 8.5.13), Copyright (c) The PHP Group,
// PHP License 3.01 (https://www.php.net/license/3_01.txt).

//! PHP mbstring functions for UTF-8 byte strings.
//!
//! Each function behaves like its PHP 8.5.13 namesake called with `'UTF-8'` as encoding (the
//! default `mbstring.internal_encoding`), with mbstring's default substitute character `?`.
//! Ports of `PHPSRC/ext/mbstring/mbstring.c` (PHP License 3.01); the UTF-8 decoding lives in
//! [`crate::utf8`].
//!
//! The functions take no encoding argument. Callers that accept one must resolve it first:
//! PHP rejects an unknown name with `ValueError: <function>(): Argument #<n> ($encoding) must be
//! a valid encoding, "<name>" given` (`php_mb_get_encoding()`); the argument position of the
//! encoding and the point where it is checked are given on each function.

use std::fmt;

use crate::case::{CaseMode, convert_case};
use crate::utf8;

/// A PHP `ValueError` thrown by an mbstring function. [`ValueError::message`] is PHP's verbatim
/// message.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ValueError {
    message: String,
}

impl ValueError {
    fn new(message: impl Into<String>) -> ValueError {
        ValueError {
            message: message.into(),
        }
    }

    /// The exception message, for example
    /// `mb_str_split(): Argument #2 ($length) must be greater than 0`.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The PHP exception class, always `"ValueError"`.
    pub fn class(&self) -> &'static str {
        "ValueError"
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ValueError {}

/// `MBFL_SUBSTR_UNTIL_END` (`(size_t) -1`).
const SUBSTR_UNTIL_END: u64 = u64::MAX;

/// Number of characters in `s`, counting each ill-formed sequence as one character.
///
/// Port of `PHP_FUNCTION(mb_strlen)` / `mb_get_strlen()` (PHPSRC/ext/mbstring/mbstring.c). PHP
/// counts non-continuation bytes for strings it has already validated (`mb_fast_strlen_utf8()`)
/// and decodes the others; both give this count. The encoding is argument #2.
pub fn mb_strlen(s: &[u8]) -> usize {
    if utf8::is_valid(s) {
        // mb_fast_strlen_utf8(): byte length minus continuation bytes.
        return s.iter().filter(|&&b| (b as i8) >= -64).count();
    }
    utf8::units(s).count()
}

/// Port of `mb_get_substr()` and `mb_get_substr_slow()` (PHPSRC/ext/mbstring/mbstring.c) for
/// UTF-8: decodes 128 code points at a time, skips `from` of them and re-encodes the next `len`
/// (ill-formed sequences become `?`).
fn get_substr(input: &[u8], from: u64, len: u64) -> Vec<u8> {
    let in_len = input.len() as u64;
    if len == 0 || from >= in_len {
        // No supported text encoding other than MacJapanese decodes to more than one codepoint
        // per byte, so if the number of codepoints to skip >= number of input bytes, the output
        // is empty.
        return Vec::new();
    }
    // UTF-8 is not a fixed-width encoding: mb_get_substr_slow().
    let mut from = from;
    let mut len = len;
    let mut rest = input;
    let mut wchar_buf = [0u32; 128];
    let mut out = Vec::with_capacity(len.min(in_len - from) as usize);
    while !rest.is_empty() && len != 0 {
        let out_len = utf8::to_wchar(&mut rest, &mut wchar_buf) as u64;
        if from >= out_len {
            from -= out_len;
        } else {
            let needed_codepoints = (out_len - from).min(len);
            let start = from as usize;
            utf8::from_wchar(
                &wchar_buf[start..start + needed_codepoints as usize],
                &mut out,
            );
            from = 0;
            len -= needed_codepoints;
        }
    }
    out
}

/// Characters `start .. start + length` of `s` (`length` `None` means up to the end), with
/// PHP's rules for negative values: a negative `start` counts from the end, a negative `length`
/// stops that many characters before the end, and out-of-range values clamp to an empty
/// string. Ill-formed sequences count as one character and are returned as `?`.
///
/// Port of `PHP_FUNCTION(mb_substr)` (PHPSRC/ext/mbstring/mbstring.c).
///
/// # Errors
///
/// `start == i64::MIN` or `length == Some(i64::MIN)` throw a [`ValueError`] (checked in that
/// order, before the encoding, which is argument #4).
pub fn mb_substr(s: &[u8], start: i64, length: Option<i64>) -> Result<Vec<u8>, ValueError> {
    if start == i64::MIN {
        return Err(ValueError::new(
            "mb_substr(): Argument #2 ($start) must be between -9223372036854775807 and \
             9223372036854775807",
        ));
    }
    if length == Some(i64::MIN) {
        return Err(ValueError::new(
            "mb_substr(): Argument #3 ($length) must be between -9223372036854775807 and \
             9223372036854775807",
        ));
    }

    let mut mblen = 0u64;
    if start < 0 || length.is_some_and(|len| len < 0) {
        mblen = mb_strlen(s) as u64;
    }

    // if "from" position is negative, count start position from the end of the string
    let real_from = if start >= 0 {
        start as u64
    } else if start.unsigned_abs() < mblen {
        mblen - start.unsigned_abs()
    } else {
        0
    };

    // if "length" position is negative, set it to the length needed to stop that many chars
    // from the end of the string
    let real_len = match length {
        None => SUBSTR_UNTIL_END,
        Some(len) if len >= 0 => len as u64,
        Some(len) if real_from < mblen && len.unsigned_abs() < mblen - real_from => {
            (mblen - real_from) - len.unsigned_abs()
        }
        Some(_) => 0,
    };

    Ok(get_substr(s, real_from, real_len))
}

/// Splits `s` into pieces of `length` characters (the last piece may be shorter); an empty
/// string gives no pieces.
///
/// Port of `PHP_FUNCTION(mb_str_split)` (PHPSRC/ext/mbstring/mbstring.c). For UTF-8, PHP cuts
/// with the encoding's character length table ([`utf8::mblen`]) and does not validate: pieces
/// are raw slices of the input, ill-formed bytes included, and a truncated last character is
/// cut at the end of the string.
///
/// # Errors
///
/// `length < 1` and `length > 1073741823` (`UINT_MAX / 4`) throw a [`ValueError`], before the
/// encoding (argument #3) is checked.
pub fn mb_str_split(s: &[u8], length: i64) -> Result<Vec<Vec<u8>>, ValueError> {
    if length <= 0 {
        return Err(ValueError::new(
            "mb_str_split(): Argument #2 ($length) must be greater than 0",
        ));
    } else if length > i64::from(u32::MAX / 4) {
        return Err(ValueError::new(
            "mb_str_split(): Argument #2 ($length) is too large",
        ));
    }

    let mut split_len = length as usize;
    if split_len > s.len() {
        split_len = s.len();
    }

    if s.is_empty() {
        return Ok(Vec::new());
    }

    let e = s.len();
    let mut p = 0;
    // Assume that we have 1-byte characters
    let mut pieces = Vec::with_capacity(e.div_ceil(split_len));
    while p < e {
        let chunk = p; // start of chunk
        let mut char_count = 0;
        while char_count < split_len && p < e {
            p += utf8::mblen(s[p]);
            char_count += 1;
        }
        if p > e {
            p = e; // ensure chunk is in bounds
        }
        pieces.push(s[chunk..p].to_vec());
    }
    Ok(pieces)
}

/// `s` in upper case (full mapping with special casing; ill-formed sequences become `?`).
///
/// Port of `PHP_FUNCTION(mb_strtoupper)` (PHPSRC/ext/mbstring/mbstring.c); see
/// [`convert_case`]. The encoding is argument #2.
pub fn mb_strtoupper(s: &[u8]) -> Vec<u8> {
    convert_case(CaseMode::Upper, s)
}

/// `s` in lower case (full mapping with special casing and the final-sigma rule; ill-formed
/// sequences become `?`).
///
/// Port of `PHP_FUNCTION(mb_strtolower)` (PHPSRC/ext/mbstring/mbstring.c); see
/// [`convert_case`]. The encoding is argument #2.
pub fn mb_strtolower(s: &[u8]) -> Vec<u8> {
    convert_case(CaseMode::Lower, s)
}

/// `mb_convert_case($s, $mode)` with `$mode` given as a PHP `MB_CASE_*` integer
/// (`MB_CASE_TITLE` is 2). Use [`convert_case`] with a [`CaseMode`] when the mode is known.
///
/// Port of `PHP_FUNCTION(mb_convert_case)` (PHPSRC/ext/mbstring/mbstring.c).
///
/// # Errors
///
/// A mode outside `0..=7` throws a [`ValueError`]. PHP checks the encoding (argument #3) first.
pub fn mb_convert_case(s: &[u8], mode: i64) -> Result<Vec<u8>, ValueError> {
    match CaseMode::from_php(mode) {
        Some(mode) => Ok(convert_case(mode, s)),
        None => Err(ValueError::new(
            "mb_convert_case(): Argument #2 ($mode) must be one of the MB_CASE_* constants",
        )),
    }
}

/// Port of `php_mb_ulcfirst()` (PHPSRC/ext/mbstring/mbstring.c).
fn ulcfirst(s: &[u8], mode: CaseMode) -> Vec<u8> {
    let first = get_substr(s, 0, 1);
    let head = convert_case(mode, &first);

    if first == head {
        // PHP returns the original string, ill-formed bytes and all.
        return s.to_vec();
    }

    let second = get_substr(s, 1, SUBSTR_UNTIL_END);
    let mut retval = head;
    retval.extend_from_slice(&second);
    retval
}

/// `s` with its first character in title case (`MB_CASE_TITLE` applied to that character).
///
/// Port of `PHP_FUNCTION(mb_ucfirst)` (PHPSRC/ext/mbstring/mbstring.c): if converting the first
/// character changes nothing, `s` is returned unchanged (ill-formed bytes included); otherwise
/// the result is the converted character followed by `mb_substr($s, 1)`, in which ill-formed
/// sequences become `?`. The encoding is argument #2.
pub fn mb_ucfirst(s: &[u8]) -> Vec<u8> {
    ulcfirst(s, CaseMode::Title)
}

/// `s` with its first character in lower case (`MB_CASE_LOWER` applied to that character).
///
/// Port of `PHP_FUNCTION(mb_lcfirst)` (PHPSRC/ext/mbstring/mbstring.c), with the same
/// unchanged-string rule as [`mb_ucfirst`]. The encoding is argument #2.
pub fn mb_lcfirst(s: &[u8]) -> Vec<u8> {
    ulcfirst(s, CaseMode::Lower)
}

/// Whether `s` is well-formed UTF-8: `mb_check_encoding($s, 'UTF-8')` for a string argument.
///
/// Port of `mb_check_str_encoding()` for UTF-8 (PHPSRC/ext/mbstring/mbstring.c), whose
/// `mb_fast_check_utf8()` accepts exactly RFC 3629 UTF-8. The encoding is argument #2.
pub fn mb_check_encoding(s: &[u8]) -> bool {
    utf8::is_valid(s)
}

/// Code point of the first character of `s`, or `Ok(None)` (PHP `false`) if `s` starts with an
/// ill-formed sequence.
///
/// Port of `PHP_FUNCTION(mb_ord)` / `php_mb_ord()` (PHPSRC/ext/mbstring/mbstring.c).
///
/// # Errors
///
/// An empty string throws a [`ValueError`], before the encoding (argument #2) is checked.
pub fn mb_ord(s: &[u8]) -> Result<Option<u32>, ValueError> {
    if s.is_empty() {
        return Err(ValueError::new(
            "mb_ord(): Argument #1 ($string) must not be empty",
        ));
    }
    let (unit, _) = utf8::decode_unit(s);
    Ok((unit != utf8::BAD_INPUT).then_some(unit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strlen_counts_ill_formed_sequences_once() {
        assert_eq!(mb_strlen(b""), 0);
        assert_eq!(mb_strlen("héllo".as_bytes()), 5);
        assert_eq!(mb_strlen(b"a\xFFb\xE2\x82"), 4);
        assert_eq!(mb_strlen(b"\xF0\x9F\x98"), 1);
        assert_eq!(mb_strlen(b"\xED\xA0\x80"), 3);
    }

    #[test]
    fn substr_examples() {
        let s = "héllo wörld".as_bytes();
        let sub = |start, len| String::from_utf8(mb_substr(s, start, len).unwrap()).unwrap();
        assert_eq!(sub(1, Some(3)), "éll");
        assert_eq!(sub(-3, None), "rld");
        assert_eq!(sub(0, Some(-2)), "héllo wör");
        assert_eq!(sub(20, None), "");
        assert_eq!(sub(-20, Some(2)), "hé");
        assert_eq!(
            mb_substr(b"a\xFF\xC3\xA9\xE2\x82b", 0, None).unwrap(),
            "a?é?b".as_bytes()
        );
        assert!(mb_substr(s, i64::MIN, None).is_err());
        assert!(mb_substr(s, 0, Some(i64::MIN)).is_err());
        assert_eq!(mb_substr(s, i64::MAX, Some(i64::MAX)).unwrap(), b"");
        assert_eq!(mb_substr(s, -i64::MAX, Some(-i64::MAX)).unwrap(), b"");
    }

    #[test]
    fn str_split_cuts_raw_bytes() {
        assert_eq!(mb_str_split(b"", 1).unwrap(), Vec::<Vec<u8>>::new());
        assert_eq!(
            mb_str_split("aéb".as_bytes(), 2).unwrap(),
            [&b"a\xC3\xA9"[..], b"b"]
        );
        assert_eq!(
            mb_str_split(b"a\xFF\xE2\x82", 1).unwrap(),
            [&b"a"[..], b"\xFF", b"\xE2\x82"]
        );
        assert_eq!(
            mb_str_split(b"abc", 0).unwrap_err().message(),
            "mb_str_split(): Argument #2 ($length) must be greater than 0"
        );
        assert_eq!(
            mb_str_split(b"abc", 1_073_741_824).unwrap_err().message(),
            "mb_str_split(): Argument #2 ($length) is too large"
        );
        assert_eq!(mb_str_split(b"abc", 1_073_741_823).unwrap(), [b"abc"]);
    }

    #[test]
    fn ucfirst_and_lcfirst() {
        assert_eq!(mb_ucfirst("ǆa".as_bytes()), "ǅa".as_bytes());
        assert_eq!(mb_ucfirst("ßa".as_bytes()), "Ssa".as_bytes());
        assert_eq!(mb_ucfirst(b"\xFFab\xFF"), b"\xFFab\xFF");
        assert_eq!(mb_ucfirst(b"a\xFF"), b"A?");
        assert_eq!(mb_ucfirst(b"A\xFF"), b"A\xFF");
        assert_eq!(mb_lcfirst("ΣΑ".as_bytes()), "σΑ".as_bytes());
        assert_eq!(mb_lcfirst(b""), b"");
    }

    #[test]
    fn convert_case_rejects_unknown_modes() {
        assert_eq!(mb_convert_case(b"ab", 2).unwrap(), b"Ab");
        assert_eq!(
            mb_convert_case(b"ab", 8).unwrap_err().to_string(),
            "mb_convert_case(): Argument #2 ($mode) must be one of the MB_CASE_* constants"
        );
    }

    #[test]
    fn ord() {
        assert_eq!(mb_ord("éx".as_bytes()), Ok(Some(0xE9)));
        assert_eq!(mb_ord(b"\xFF"), Ok(None));
        assert_eq!(mb_ord(b"\xE2\x82"), Ok(None));
        assert_eq!(
            mb_ord(b"").unwrap_err().message(),
            "mb_ord(): Argument #1 ($string) must not be empty"
        );
    }
}
