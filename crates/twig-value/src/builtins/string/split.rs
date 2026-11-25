//! `implode()` and `explode()` (reference/php/stdlib.md §5.7).
//!
//! Twig's `join` filter, `reverse` and the "Did you mean" suggestions call `implode($glue, $array)`
//! directly with two arguments, which PHP 8.4+ compiles to the frameless variant
//! ([`implode_frameless`]); `split` calls `explode()`.

use super::{long_param, str_param, str_weak};
use crate::array::Array;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::Value;

/// `implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of
/// type array, null given`.
fn missing_array_error() -> PhpError {
    PhpError::type_error(
        "implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given",
    )
}

/// Port of `php_implode()` (PHPSRC/ext/standard/string.c): no elements give `""`, one element is
/// converted with `zval_get_string()` (no glue), otherwise every element is converted in order
/// (strings as is, ints in decimal, everything else with PHP string conversion: floats at
/// precision 14, `Array to string conversion`, `__toString()`) and joined with `glue`.
pub fn php_implode(
    glue: &[u8],
    pieces: &Array,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    match pieces.len() {
        0 => return Ok(PhpString::empty()),
        1 => {
            let (_, v) = pieces.first().expect("one element");
            return convert::to_string(v, diag);
        }
        _ => {}
    }
    let mut parts: Vec<PhpString> = Vec::with_capacity(pieces.len());
    for v in pieces.values() {
        parts.push(match v {
            Value::String(s) => s.clone(),
            Value::Int(i) => convert::long_to_string(*i),
            other => convert::to_string(other, diag)?,
        });
    }
    let total = parts.iter().map(PhpString::len).sum::<usize>() + glue.len() * (parts.len() - 1);
    let mut out = Vec::with_capacity(total);
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(glue);
        }
        out.extend_from_slice(p);
    }
    Ok(PhpString::from(out))
}

/// Port of `PHP_FUNCTION(implode)` (`implode(string|array $separator, ?array $array = null)`), the
/// variant used by dynamic calls (`call_user_func('implode', ...)`, more than two arguments,
/// spread or named arguments).
///
/// The separator accepts arrays and anything a string parameter accepts (`null` deprecated);
/// otherwise `TypeError: implode(): Argument #1 ($separator) must be of type array|string, X
/// given`. With one argument it must be the array; with two the first must be a string.
pub fn implode(
    separator: &Value,
    array: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let arg1 = Arg::new("implode", 1, "separator").declared("array|string");
    let (sep_str, sep_arr) = match separator {
        Value::Array(a) => (None, Some(a)),
        other => match str_weak(other, &arg1, diag)? {
            Some(s) => (Some(s), None),
            None => return Err(arg1.type_error("of type array|string", other)),
        },
    };
    let pieces = match array {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => Some(a),
        Some(other) => {
            return Err(Arg::new("implode", 2, "array").type_error("of type ?array", other));
        }
    };
    match pieces {
        None => match sep_arr {
            Some(arr) => php_implode(b"", arr, diag),
            None => Err(missing_array_error()),
        },
        Some(pieces) => match sep_str {
            Some(glue) => php_implode(&glue, pieces, diag),
            None => Err(arg1.type_error("of type string", separator)),
        },
    }
}

/// Port of `ZEND_FRAMELESS_FUNCTION(implode, 1)` and `(implode, 2)`
/// (PHPSRC/ext/standard/string.c): what a direct call `implode($array)` / `implode($glue,
/// $array)` runs, which is how Twig calls it.
///
/// One argument: it must be an array, anything else (even a string) throws `implode(): If argument
/// #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given`.
/// Two arguments: the separator is parsed as a string (`null` deprecated with type
/// `array|string`; arrays and other objects: `must be of type string, X given`), the second must
/// be an array or `null` (`must be of type ?array, X given`), and `null` throws the error above.
pub fn implode_frameless(
    separator: &Value,
    array: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let Some(array) = array else {
        return match separator {
            Value::Array(a) => php_implode(b"", a, diag),
            _ => Err(missing_array_error()),
        };
    };
    let glue = param::parse_str(
        separator,
        &Arg::new("implode", 1, "separator").declared("array|string"),
        diag,
    )?;
    match array {
        Value::Array(a) => php_implode(&glue, a, diag),
        Value::Null => Err(missing_array_error()),
        other => Err(Arg::new("implode", 2, "array").type_error("of type ?array", other)),
    }
}

/// Port of `php_explode()`, `php_explode_negative_limit()` and the body of
/// `PHP_FUNCTION(explode)` (PHPSRC/ext/standard/string.c) on parsed arguments.
///
/// An empty delimiter throws `ValueError: explode(): Argument #1 ($separator) must not be empty`.
/// An empty string gives `[""]` (or `[]` for a negative limit). `limit > 1` returns at most
/// `limit` pieces (the last one holds the rest), `limit < 0` drops the last `-limit` pieces, and
/// `0`/`1` return `[string]`.
pub fn explode_bytes(delim: &[u8], s: &PhpString, limit: i64) -> PhpResult<Array> {
    if delim.is_empty() {
        return Err(Arg::new("explode", 1, "separator").value_error("must not be empty"));
    }
    let mut out = Array::new();
    if s.is_empty() {
        if limit >= 0 {
            out.push(Value::String(PhpString::empty()))?;
        }
        return Ok(out);
    }
    if limit == 0 || limit == 1 {
        out.push(Value::String(s.clone()))?;
        return Ok(out);
    }
    let bytes = s.as_bytes();
    let finder = memchr::memmem::Finder::new(delim);
    if limit > 1 {
        let mut p1 = 0;
        let Some(mut p2) = finder.find(bytes) else {
            out.push(Value::String(s.clone()))?;
            return Ok(out);
        };
        let mut limit = limit;
        loop {
            out.push(Value::String(PhpString::from(&bytes[p1..p1 + p2])))?;
            p1 += p2 + delim.len();
            limit -= 1;
            match finder.find(&bytes[p1..]) {
                Some(next) if limit > 1 => p2 = next,
                _ => break,
            }
        }
        out.push(Value::String(PhpString::from(&bytes[p1..])))?;
        return Ok(out);
    }
    // limit < 0: all pieces except the last -limit ones.
    let mut positions = vec![0usize];
    let mut p1 = 0;
    while let Some(p2) = finder.find(&bytes[p1..]) {
        p1 += p2 + delim.len();
        positions.push(p1);
    }
    if positions.len() == 1 {
        return Ok(out);
    }
    let found = positions.len() as i64;
    let to_return = limit.saturating_add(found);
    for i in 0..to_return.max(0) as usize {
        let start = positions[i];
        let end = positions[i + 1] - delim.len();
        out.push(Value::String(PhpString::from(&bytes[start..end])))?;
    }
    Ok(out)
}

/// Port of `PHP_FUNCTION(explode)` (`explode(string $separator, string $string, int $limit =
/// PHP_INT_MAX): array`).
pub fn explode(
    separator: &Value,
    string: &Value,
    limit: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let delim = str_param(separator, "explode", 1, "separator", diag)?;
    let s = str_param(string, "explode", 2, "string", diag)?;
    let limit = match limit {
        Some(l) => long_param(l, "explode", 3, "limit", diag)?,
        None => i64::MAX,
    };
    explode_bytes(&delim, &s, limit)
}
