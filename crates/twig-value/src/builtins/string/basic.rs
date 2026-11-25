//! Small byte-string builtins: lengths, substrings, searching, ASCII case mapping, comparison,
//! `ord()`/`chr()` and `str_repeat()` (reference/php/stdlib.md §5.10).
//!
//! Everything here is byte based and locale independent, like PHP 8.x: `strtolower()` and
//! friends only map ASCII letters (`zend_tolower_ascii()`), since PHP 8.2.

use super::{long_or_null_param, long_param, memnstr, str_param};
use crate::compare;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::param::Arg;
use crate::string::PhpString;
use crate::value::Value;

/// `zend_argument_value_error(num, "must be contained in argument #1 ($haystack)")`.
fn not_contained(function: &str, position: u32, name: &str) -> PhpError {
    Arg::new(function, position, name).value_error("must be contained in argument #1 ($haystack)")
}

/// Port of `ZEND_FUNCTION(strlen)` (PHPSRC/Zend/zend_builtin_functions.c) and of the `ZEND_STRLEN`
/// opcode that `\strlen()` compiles to: the length in bytes.
pub fn strlen(string: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    if let Value::String(s) = string {
        return Ok(s.len() as i64);
    }
    Ok(str_param(string, "strlen", 1, "string", diag)?.len() as i64)
}

/// Port of `_zend_substr()` (PHPSRC/ext/standard/string.c): `offset` counts from the end when
/// negative (clamped to the start), an offset past the end gives `""`; a negative `length` stops
/// that many bytes before the end; `None` means "to the end".
pub fn substr_bytes(s: &PhpString, offset: i64, length: Option<i64>) -> PhpString {
    let len = s.len();
    let f: usize = if offset < 0 {
        if offset.unsigned_abs() > len as u64 {
            0
        } else {
            (len as i64 + offset) as usize
        }
    } else if offset as u64 > len as u64 {
        return PhpString::empty();
    } else {
        offset as usize
    };
    let l: usize = match length {
        Some(l) if l < 0 => {
            if l.unsigned_abs() > (len - f) as u64 {
                0
            } else {
                (len as i64 - f as i64 + l) as usize
            }
        }
        Some(l) if l as u64 > (len - f) as u64 => len - f,
        Some(l) => l as usize,
        None => len - f,
    };
    if l == len {
        s.clone()
    } else {
        PhpString::from(&s[f..f + l])
    }
}

/// Port of `PHP_FUNCTION(substr)` (`substr(string $string, int $offset, ?int $length = null)`) and
/// its frameless variants (same parsing).
pub fn substr(
    string: &Value,
    offset: &Value,
    length: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let s = str_param(string, "substr", 1, "string", diag)?;
    let f = long_param(offset, "substr", 2, "offset", diag)?;
    let l = long_or_null_param(length, "substr", 3, "length", diag)?;
    Ok(substr_bytes(&s, f, l))
}

/// Port of `_zend_strpos()` (PHPSRC/ext/standard/string.c): byte offset of the first occurrence of
/// `needle` at or after `offset` (negative offsets count from the end), `None` for PHP's `false`.
/// An offset outside the haystack throws `ValueError: strpos(): Argument #3 ($offset) must be
/// contained in argument #1 ($haystack)`.
pub fn strpos_bytes(haystack: &[u8], needle: &[u8], offset: i64) -> PhpResult<Option<i64>> {
    let len = haystack.len() as i64;
    let mut offset = offset;
    if offset < 0 {
        offset = offset.wrapping_add(len);
    }
    if offset < 0 || offset > len {
        return Err(not_contained("strpos", 3, "offset"));
    }
    let start = offset as usize;
    Ok(memnstr(&haystack[start..], needle).map(|p| (start + p) as i64))
}

/// Port of `PHP_FUNCTION(strpos)` (`strpos(string $haystack, string $needle, int $offset = 0):
/// int|false`); `None` is `false`.
pub fn strpos(
    haystack: &Value,
    needle: &Value,
    offset: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<i64>> {
    let h = str_param(haystack, "strpos", 1, "haystack", diag)?;
    let n = str_param(needle, "strpos", 2, "needle", diag)?;
    let o = match offset {
        Some(o) => long_param(o, "strpos", 3, "offset", diag)?,
        None => 0,
    };
    strpos_bytes(&h, &n, o)
}

/// Port of `zend_memnrstr()` (PHPSRC/Zend/zend_operators.h): the last occurrence of `needle` that
/// lies completely inside `haystack[..end]`, starting at or after `start`.
fn memnrstr(haystack: &[u8], start: usize, end: usize, needle: &[u8]) -> Option<usize> {
    if needle.len() > end - start {
        return None;
    }
    if needle.is_empty() {
        return Some(end);
    }
    memchr::memmem::rfind(&haystack[start..end], needle).map(|p| start + p)
}

/// Port of `PHP_FUNCTION(strrpos)` (PHPSRC/ext/standard/string.c) on parsed arguments: the last
/// occurrence of `needle`; a non-negative `offset` is where the search starts, a negative one
/// makes the match start at most `-offset` bytes from the end (counting the needle).
pub fn strrpos_bytes(haystack: &[u8], needle: &[u8], offset: i64) -> PhpResult<Option<i64>> {
    let len = haystack.len();
    let (p, e) = if offset >= 0 {
        if offset as u64 > len as u64 {
            return Err(not_contained("strrpos", 3, "offset"));
        }
        (offset as usize, len)
    } else {
        if offset < -i64::MAX || offset.unsigned_abs() > len as u64 {
            return Err(not_contained("strrpos", 3, "offset"));
        }
        let back = offset.unsigned_abs() as usize;
        let e = if back < needle.len() {
            len
        } else {
            len - back + needle.len()
        };
        (0, e)
    };
    Ok(memnrstr(haystack, p, e, needle).map(|p| p as i64))
}

/// Port of `PHP_FUNCTION(strrpos)` (`strrpos(string $haystack, string $needle, int $offset = 0):
/// int|false`); `None` is `false`.
pub fn strrpos(
    haystack: &Value,
    needle: &Value,
    offset: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<i64>> {
    let h = str_param(haystack, "strrpos", 1, "haystack", diag)?;
    let n = str_param(needle, "strrpos", 2, "needle", diag)?;
    let o = match offset {
        Some(o) => long_param(o, "strrpos", 3, "offset", diag)?,
        None => 0,
    };
    strrpos_bytes(&h, &n, o)
}

/// Port of `PHP_FUNCTION(substr_count)` (`substr_count(string $haystack, string $needle, int
/// $offset = 0, ?int $length = null): int`): non-overlapping occurrences inside the window.
/// `ValueError`s: empty needle, offset or length outside the haystack.
pub fn substr_count(
    haystack: &Value,
    needle: &Value,
    offset: Option<&Value>,
    length: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i64> {
    let h = str_param(haystack, "substr_count", 1, "haystack", diag)?;
    let n = str_param(needle, "substr_count", 2, "needle", diag)?;
    let offset = match offset {
        Some(o) => long_param(o, "substr_count", 3, "offset", diag)?,
        None => 0,
    };
    let length = long_or_null_param(length, "substr_count", 4, "length", diag)?;
    if n.is_empty() {
        return Err(Arg::new("substr_count", 2, "needle").value_error("must not be empty"));
    }
    let mut start = 0usize;
    let mut hlen = h.len() as i64;
    if offset != 0 {
        let mut o = offset;
        if o < 0 {
            o = o.wrapping_add(hlen);
        }
        if o < 0 || o > hlen {
            return Err(not_contained("substr_count", 3, "offset"));
        }
        start = o as usize;
        hlen -= o;
    }
    let length = match length {
        Some(mut l) => {
            if l < 0 {
                l = l.wrapping_add(hlen);
            }
            if l < 0 || l > hlen {
                return Err(not_contained("substr_count", 4, "length"));
            }
            l
        }
        None => hlen,
    };
    let window = &h[start..start + length as usize];
    let count = if n.len() == 1 {
        memchr::memchr_iter(n[0], window).count()
    } else {
        memchr::memmem::find_iter(window, &n[..]).count()
    };
    Ok(count as i64)
}

/// Port of `PHP_FUNCTION(str_contains)` and its frameless variant.
pub fn str_contains(
    haystack: &Value,
    needle: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    let h = str_param(haystack, "str_contains", 1, "haystack", diag)?;
    let n = str_param(needle, "str_contains", 2, "needle", diag)?;
    Ok(memnstr(&h, &n).is_some())
}

/// Port of `PHP_FUNCTION(str_starts_with)` and its frameless variant (`zend_string_starts_with()`).
pub fn str_starts_with(
    haystack: &Value,
    needle: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    let h = str_param(haystack, "str_starts_with", 1, "haystack", diag)?;
    let n = str_param(needle, "str_starts_with", 2, "needle", diag)?;
    Ok(h.starts_with(&n))
}

/// Port of `PHP_FUNCTION(str_ends_with)`.
pub fn str_ends_with(
    haystack: &Value,
    needle: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    let h = str_param(haystack, "str_ends_with", 1, "haystack", diag)?;
    let n = str_param(needle, "str_ends_with", 2, "needle", diag)?;
    Ok(h.ends_with(&n))
}

/// Port of `zend_string_tolower()` (PHPSRC/Zend/zend_operators.c): ASCII `A-Z` to `a-z`; the
/// input itself when nothing changes.
pub fn ascii_lowercase(s: &PhpString) -> PhpString {
    match s.iter().position(u8::is_ascii_uppercase) {
        None => s.clone(),
        Some(_) => PhpString::from(s.to_ascii_lowercase()),
    }
}

/// Port of `zend_string_toupper()` (PHPSRC/Zend/zend_operators.c): ASCII `a-z` to `A-Z`.
pub fn ascii_uppercase(s: &PhpString) -> PhpString {
    match s.iter().position(u8::is_ascii_lowercase) {
        None => s.clone(),
        Some(_) => PhpString::from(s.to_ascii_uppercase()),
    }
}

/// Port of `PHP_FUNCTION(strtolower)` (ASCII only since PHP 8.2).
pub fn strtolower(string: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    Ok(ascii_lowercase(&str_param(
        string,
        "strtolower",
        1,
        "string",
        diag,
    )?))
}

/// Port of `PHP_FUNCTION(strtoupper)` (ASCII only since PHP 8.2).
pub fn strtoupper(string: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    Ok(ascii_uppercase(&str_param(
        string,
        "strtoupper",
        1,
        "string",
        diag,
    )?))
}

/// Replaces the first byte through `map` (`php_ucfirst()`/`php_lcfirst()`).
fn map_first(s: &PhpString, map: fn(&u8) -> u8) -> PhpString {
    match s.first() {
        None => PhpString::empty(),
        Some(c) if map(c) == *c => s.clone(),
        Some(c) => {
            let mut v = s.to_vec();
            v[0] = map(c);
            PhpString::from(v)
        }
    }
}

/// Port of `PHP_FUNCTION(ucfirst)`: the first byte ASCII-uppercased.
pub fn ucfirst(string: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let s = str_param(string, "ucfirst", 1, "string", diag)?;
    Ok(map_first(&s, u8::to_ascii_uppercase))
}

/// Port of `PHP_FUNCTION(lcfirst)`: the first byte ASCII-lowercased.
pub fn lcfirst(string: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let s = str_param(string, "lcfirst", 1, "string", diag)?;
    Ok(map_first(&s, u8::to_ascii_lowercase))
}

/// Port of `PHP_FUNCTION(ucwords)` (`ucwords(string $string, string $separators = " \t\r\n\f\v")`):
/// uppercases the first byte and every byte following a separator; the separators go through
/// `php_charmask()` (ranges and their warnings, prefixed `ucwords(): `).
pub fn ucwords(
    string: &Value,
    separators: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let s = str_param(string, "ucwords", 1, "string", diag)?;
    let delims = match separators {
        Some(d) => str_param(d, "ucwords", 2, "separators", diag)?,
        None => PhpString::from(" \t\r\n\x0c\x0b"),
    };
    if s.is_empty() {
        return Ok(PhpString::empty());
    }
    let mask = super::trim::charmask(&delims, "ucwords", diag)?;
    let mut out = s.to_vec();
    out[0] = out[0].to_ascii_uppercase();
    for i in 0..out.len() - 1 {
        if mask[out[i] as usize] {
            out[i + 1] = out[i + 1].to_ascii_uppercase();
        }
    }
    Ok(PhpString::from(out))
}

/// Port of `ZEND_FUNCTION(strcmp)` (`zend_binary_strcmp()`): the `memcmp()` difference of the
/// first differing bytes, else `-1`/`0`/`1` by length.
pub fn strcmp(string1: &Value, string2: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    let a = str_param(string1, "strcmp", 1, "string1", diag)?;
    let b = str_param(string2, "strcmp", 2, "string2", diag)?;
    Ok(i64::from(compare::binary_strcmp(&a, &b)))
}

/// Port of `zend_binary_strncasecmp()` with `length = usize::MAX` covering
/// `zend_binary_strcasecmp()` (PHPSRC/Zend/zend_operators.c): the difference of the first
/// differing ASCII-lowercased bytes, else the three-way comparison of the (limited) lengths.
pub fn binary_strncasecmp(a: &[u8], b: &[u8], length: usize) -> i64 {
    let n = length.min(a.len()).min(b.len());
    for i in 0..n {
        let c1 = a[i].to_ascii_lowercase();
        let c2 = b[i].to_ascii_lowercase();
        if c1 != c2 {
            return i64::from(c1) - i64::from(c2);
        }
    }
    match length.min(a.len()).cmp(&length.min(b.len())) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Port of `ZEND_FUNCTION(strcasecmp)` (`zend_binary_strcasecmp()`).
pub fn strcasecmp(string1: &Value, string2: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    let a = str_param(string1, "strcasecmp", 1, "string1", diag)?;
    let b = str_param(string2, "strcasecmp", 2, "string2", diag)?;
    Ok(binary_strncasecmp(&a, &b, usize::MAX))
}

/// Port of `ZEND_FUNCTION(strncasecmp)`; a negative length throws `ValueError: strncasecmp():
/// Argument #3 ($length) must be greater than or equal to 0`.
pub fn strncasecmp(
    string1: &Value,
    string2: &Value,
    length: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i64> {
    let a = str_param(string1, "strncasecmp", 1, "string1", diag)?;
    let b = str_param(string2, "strncasecmp", 2, "string2", diag)?;
    let len = long_param(length, "strncasecmp", 3, "length", diag)?;
    if len < 0 {
        return Err(
            Arg::new("strncasecmp", 3, "length").value_error("must be greater than or equal to 0")
        );
    }
    Ok(binary_strncasecmp(&a, &b, len as usize))
}

/// Port of `PHP_FUNCTION(ord)`: the first byte (0 for `""`), with the PHP 8.5 deprecations
/// `ord(): Providing an empty string is deprecated` and `ord(): Providing a string that is not one
/// byte long is deprecated. Use ord($str[0]) instead`.
pub fn ord(character: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    let s = str_param(character, "ord", 1, "character", diag)?;
    if s.len() != 1 {
        if s.is_empty() {
            diag.deprecated("ord(): Providing an empty string is deprecated")?;
        } else {
            diag.deprecated(
                "ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead",
            )?;
        }
    }
    Ok(i64::from(s.first().copied().unwrap_or(0)))
}

/// Port of `PHP_FUNCTION(chr)`: the byte `codepoint & 0xFF`, with the PHP 8.5 deprecation for
/// values outside `0..=255`.
pub fn chr(codepoint: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let c = long_param(codepoint, "chr", 1, "codepoint", diag)?;
    if !(0..=255).contains(&c) {
        diag.deprecated(
            "chr(): Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256",
        )?;
    }
    Ok(PhpString::from(&[(c & 0xff) as u8]))
}

/// Port of `PHP_FUNCTION(str_repeat)`: a negative count throws `ValueError: str_repeat():
/// Argument #2 ($times) must be greater than or equal to 0`. PHP's memory limit is not modelled:
/// a result that cannot be allocated returns an `Error` instead of PHP's fatal error.
pub fn str_repeat(
    string: &Value,
    times: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let s = str_param(string, "str_repeat", 1, "string", diag)?;
    let mult = long_param(times, "str_repeat", 2, "times", diag)?;
    if mult < 0 {
        return Err(
            Arg::new("str_repeat", 2, "times").value_error("must be greater than or equal to 0")
        );
    }
    if s.is_empty() || mult == 0 {
        return Ok(PhpString::empty());
    }
    match s.len().checked_mul(mult as usize) {
        Some(total) if total <= isize::MAX as usize / 2 => {
            Ok(PhpString::from(s.repeat(mult as usize)))
        }
        _ => Err(PhpError::error(format!(
            "str_repeat(): result of {} bytes exceeds the memory available to twig-rs",
            (s.len() as u128) * (mult as u128)
        ))),
    }
}
