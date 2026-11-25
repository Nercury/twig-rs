//! Dimension access on any value: `$container[$offset]` reads, `isset()` / `empty()` of an offset,
//! string offsets, and `count()`.
//!
//! reference/php/engine-semantics.md §8.1 and §8.8-§8.10. The functions port the VM paths PHP runs
//! for these expressions (PHPSRC/Zend/zend_execute.c, PHPSRC/Zend/zend_vm_def.h):
//!
//! | expression | function | upstream |
//! |---|---|---|
//! | `$c[$k]` | [`read`] with [`FetchMode::Read`] | `zend_fetch_dimension_address_read_R()` |
//! | `$c[$k] ?? …` | [`read`] with [`FetchMode::Isset`] | `zend_fetch_dimension_address_read_IS()` |
//! | `isset($c[$k])` | [`isset`] | `ZEND_ISSET_ISEMPTY_DIM_OBJ`, `zend_isset_dim_slow()` |
//! | `empty($c[$k])` | [`empty`] | `ZEND_ISSET_ISEMPTY_DIM_OBJ`, `zend_isempty_dim_slow()` |
//! | `count($v, $mode)` | [`count`] | `PHP_FUNCTION(count)` (PHPSRC/ext/standard/array.c) |
//!
//! Containers: arrays use PHP's offset normalisation ([`ArrayKey::from_value`]); strings use the
//! string-offset rules ([`string_offset`]); objects go through their `read_dimension` /
//! `has_dimension` handlers (`ArrayAccess`); `null`, booleans and numbers read as `null` (with
//! `Trying to access array offset on %s` outside `isset`/`??`).

use crate::array::Array;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::key::{ArrayKey, KeyContext, illegal_offset};
use crate::numeric;
use crate::object::FetchMode;
use crate::string::PhpString;
use crate::types;
use crate::value::{Number, Value};

/// Port of `zend_undefined_offset()` / `zend_undefined_index()`:
/// `Undefined array key 5` / `Undefined array key "k"` (the string printed with C `%s`, so up to
/// the first NUL byte).
pub fn undefined_key(key: &ArrayKey, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    match key {
        ArrayKey::Int(i) => diag.warn(format!("Undefined array key {i}")),
        ArrayKey::Str(s) => diag.warn(PhpString::concat(&[
            b"Undefined array key \"",
            s.c_str(),
            b"\"",
        ])),
    }
}

/// Port of `zend_fetch_dimension_address_inner()` for `BP_VAR_R` / `BP_VAR_IS` on an array: the
/// offset is normalised with `slow_index_convert()` (`null` deprecation, float→int diagnostics,
/// `Cannot access offset of type X on array` for arrays/objects) and a missing key warns
/// `Undefined array key ...` in [`FetchMode::Read`]. Missing keys read as `null`.
pub fn array_read(
    array: &Array,
    offset: &Value,
    mode: FetchMode,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let key = ArrayKey::from_value(offset, KeyContext::Read, diag)?;
    match array.get(&key) {
        Some(v) => Ok(v.clone()),
        None => {
            if mode == FetchMode::Read {
                undefined_key(&key, diag)?;
            }
            Ok(Value::Null)
        }
    }
}

/// Port of `zend_fetch_dimension_address_read()` (PHPSRC/Zend/zend_execute.c): `$container[$offset]`
/// ([`FetchMode::Read`]) or the quiet fetch of `$container[$offset] ?? …` ([`FetchMode::Isset`]).
///
/// * array: [`array_read`];
/// * string: [`string_offset`];
/// * object: the `read_dimension` handler (`ArrayAccess::offsetGet()`, with `offsetExists()` first
///   in [`FetchMode::Isset`]; `Cannot use object of type X as array` otherwise);
/// * `null`, bool, int, float: `null`, with `Trying to access array offset on %s` (value name) in
///   [`FetchMode::Read`]. The offset is not converted at all.
pub fn read(
    container: &Value,
    offset: &Value,
    mode: FetchMode,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match container {
        Value::Array(a) => array_read(a, offset, mode, diag),
        Value::String(s) => string_offset(s, offset, mode, diag),
        Value::Object(o) => o.read_dimension(offset, mode, diag),
        _ => {
            if mode == FetchMode::Read {
                diag.warn(format!(
                    "Trying to access array offset on {}",
                    types::zend_zval_value_name(container)
                ))?;
            }
            Ok(Value::Null)
        }
    }
}

/// Port of the string-offset branch of `zend_fetch_dimension_address_read()`: `$str[$offset]`.
///
/// Offset conversion: ints as is; strings must be integer-like numeric strings
/// (`is_numeric_string_ex(allow_errors)` returning `IS_LONG`; leading-numeric ones warn
/// `Illegal string offset "%s"` and use the prefix, even in [`FetchMode::Isset`]), otherwise
/// `TypeError: Cannot access offset of type string on string` (`null` in [`FetchMode::Isset`]);
/// floats, `null` and booleans warn `String offset cast occurred` (not in [`FetchMode::Isset`]) and
/// convert silently (`zend_dval_to_lval_silent()`); arrays and objects throw
/// `Cannot access offset of type X on string` in both modes.
///
/// Negative offsets count from the end. Out of range: `Uninitialized string offset %d` and `""`
/// ([`FetchMode::Read`]) or `null` ([`FetchMode::Isset`]). The result is a one-byte string.
pub fn string_offset(
    s: &PhpString,
    offset: &Value,
    mode: FetchMode,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let quiet = mode == FetchMode::Isset;
    let offset = match offset {
        Value::Int(i) => *i,
        Value::String(dim) => match numeric::is_numeric_string_ex(dim, true) {
            Some(numeric::NumericString {
                number: Number::Int(l),
                trailing_data,
                ..
            }) => {
                if trailing_data {
                    diag.warn(PhpString::concat(&[
                        b"Illegal string offset \"",
                        dim.c_str(),
                        b"\"",
                    ]))?;
                }
                l
            }
            _ => {
                if quiet {
                    return Ok(Value::Null);
                }
                return Err(illegal_offset(offset, KeyContext::Read, b"string"));
            }
        },
        Value::Float(d) => {
            if !quiet {
                diag.warn("String offset cast occurred")?;
            }
            convert::dval_to_lval_silent(*d)
        }
        Value::Null | Value::Bool(_) => {
            if !quiet {
                diag.warn("String offset cast occurred")?;
            }
            i64::from(matches!(offset, Value::Bool(true)))
        }
        Value::Array(_) | Value::Object(_) => {
            return Err(illegal_offset(offset, KeyContext::Read, b"string"));
        }
    };
    match string_byte(s, offset) {
        Some(b) => Ok(Value::String(PhpString::from(&[b][..]))),
        None => {
            if quiet {
                Ok(Value::Null)
            } else {
                diag.warn(format!("Uninitialized string offset {offset}"))?;
                Ok(Value::String(PhpString::empty()))
            }
        }
    }
}

/// The byte at a PHP string offset (negative offsets count from the end), if in range.
fn string_byte(s: &[u8], offset: i64) -> Option<u8> {
    let len = s.len() as u64;
    let pos = if offset < 0 {
        len.checked_sub(offset.unsigned_abs())?
    } else {
        offset as u64
    };
    if pos < len {
        Some(s[pos as usize])
    } else {
        None
    }
}

/// The offset `isset()`/`empty()` use on a string container, or `None` when PHP answers "not
/// set" without looking (the non-`IS_LONG` branch of `zend_isset_dim_slow()`): simple scalars
/// convert with `zval_get_long_ex(offset, is_strict = true)` (so fractional floats deprecate),
/// strings only when fully integer-numeric (`is_numeric_string(allow_errors = false)`).
fn isset_string_offset(offset: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Option<i64>> {
    match offset {
        Value::Int(i) => Ok(Some(*i)),
        Value::Null | Value::Bool(_) | Value::Float(_) => {
            convert::to_long_ex(offset, true, diag).map(Some)
        }
        Value::String(s) => match numeric::is_numeric_string(s, false) {
            Some(Number::Int(_)) => convert::to_long_ex(offset, true, diag).map(Some),
            _ => Ok(None),
        },
        Value::Array(_) | Value::Object(_) => Ok(None),
    }
}

/// `str_offset:` of `zend_isset_dim_slow()` / `zend_isempty_dim_slow()`: the in-range byte.
fn isset_string_byte(s: &[u8], offset: i64) -> Option<u8> {
    let lval = if offset < 0 {
        offset.wrapping_add(s.len() as i64)
    } else {
        offset
    };
    if lval >= 0 && (lval as u64) < s.len() as u64 {
        Some(s[lval as usize])
    } else {
        None
    }
}

/// Port of `isset($container[$offset])` (`ZEND_ISSET_ISEMPTY_DIM_OBJ` without `ZEND_ISEMPTY`,
/// `zend_find_array_dim_slow()`, `zend_isset_dim_slow()`).
///
/// Arrays: offset normalised for isset (`null` deprecation; arrays/objects throw
/// `Cannot access offset of type X in isset or empty`), true when the element exists and is not
/// `null`. Objects: `has_dimension` (`offsetExists()`). Strings: see [`string_offset`] (only
/// integer-like offsets can be set). Anything else: false.
pub fn isset(container: &Value, offset: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    match container {
        Value::Array(a) => {
            let key = ArrayKey::from_value(offset, KeyContext::Isset, diag)?;
            Ok(a.get(&key).is_some_and(|v| !v.is_null()))
        }
        Value::Object(o) => o.has_dimension(offset, false, diag),
        Value::String(s) => Ok(match isset_string_offset(offset, diag)? {
            Some(l) => isset_string_byte(s, l).is_some(),
            None => false,
        }),
        _ => Ok(false),
    }
}

/// Port of `empty($container[$offset])` (`ZEND_ISSET_ISEMPTY_DIM_OBJ` with `ZEND_ISEMPTY`,
/// `zend_isempty_dim_slow()`): the negation of "set and truthy". Arrays: missing or falsy
/// (truthiness may warn for NaN). Objects: `!has_dimension(check_empty)` (`offsetExists()` then
/// `offsetGet()`). Strings: the byte is missing or `'0'`. Anything else: true.
pub fn empty(container: &Value, offset: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    match container {
        Value::Array(a) => {
            let key = ArrayKey::from_value(offset, KeyContext::Isset, diag)?;
            match a.get(&key) {
                Some(v) => Ok(!convert::to_bool(v, diag)?),
                None => Ok(true),
            }
        }
        Value::Object(o) => Ok(!o.has_dimension(offset, true, diag)?),
        Value::String(s) => Ok(match isset_string_offset(offset, diag)? {
            Some(l) => isset_string_byte(s, l).is_none_or(|b| b == b'0'),
            None => true,
        }),
        _ => Ok(true),
    }
}

/// `COUNT_NORMAL`.
pub const COUNT_NORMAL: i64 = 0;
/// `COUNT_RECURSIVE`.
pub const COUNT_RECURSIVE: i64 = 1;

/// Port of `count($value, $mode)` / `sizeof()` (PHPSRC/ext/standard/array.c).
///
/// Arrays: the number of elements, or with [`COUNT_RECURSIVE`] also the elements of nested arrays
/// (`php_count_recursive()`; objects inside are not descended into). Objects: the `count_elements`
/// handler, else `Countable::count()` converted with `zval_get_long()`
/// ([`ObjectRef::count`](crate::ObjectRef::count)). Anything else:
/// `TypeError: count(): Argument #1 ($value) must be of type Countable|array, X given`; a `mode`
/// other than 0/1: `ValueError: count(): Argument #2 ($mode) must be either COUNT_NORMAL or
/// COUNT_RECURSIVE`.
pub fn count(value: &Value, mode: i64, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    if mode != COUNT_NORMAL && mode != COUNT_RECURSIVE {
        return Err(PhpError::value_error(
            "count(): Argument #2 ($mode) must be either COUNT_NORMAL or COUNT_RECURSIVE",
        ));
    }
    match value {
        Value::Array(a) => Ok(if mode == COUNT_RECURSIVE {
            count_recursive(a)
        } else {
            a.len() as i64
        }),
        Value::Object(o) => match o.count(diag)? {
            Some(n) => Ok(n),
            None => Err(count_type_error(value)),
        },
        _ => Err(count_type_error(value)),
    }
}

fn count_type_error(value: &Value) -> PhpError {
    PhpError::type_error(PhpString::concat(&[
        b"count(): Argument #1 ($value) must be of type Countable|array, ",
        types::zend_zval_value_name(value).as_bytes(),
        b" given",
    ]))
}

/// Port of `php_count_recursive()`. Arrays cannot contain themselves here (no PHP references), so
/// the `Recursion detected` branch is unreachable.
fn count_recursive(a: &Array) -> i64 {
    let mut n = a.len() as i64;
    for v in a.values() {
        if let Value::Array(inner) = v {
            n += count_recursive(inner);
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{Diagnostic, Level};

    fn s(x: &str) -> Value {
        Value::from(x)
    }

    #[test]
    fn string_offsets() {
        let abc = PhpString::from("abc");
        let mut d: Vec<Diagnostic> = Vec::new();
        assert_eq!(
            string_offset(&abc, &Value::Int(-1), FetchMode::Read, &mut d).unwrap(),
            s("c")
        );
        assert_eq!(
            string_offset(&abc, &Value::Int(3), FetchMode::Read, &mut d).unwrap(),
            s("")
        );
        assert_eq!(
            string_offset(&abc, &Value::Int(-4), FetchMode::Isset, &mut d).unwrap(),
            Value::Null
        );
        assert_eq!(
            string_offset(&abc, &s(" 1"), FetchMode::Read, &mut d).unwrap(),
            s("b")
        );
        assert_eq!(
            string_offset(&abc, &s("1x"), FetchMode::Read, &mut d).unwrap(),
            s("b")
        );
        assert_eq!(
            string_offset(&abc, &Value::Float(1.7), FetchMode::Read, &mut d).unwrap(),
            s("b")
        );
        assert_eq!(
            string_offset(&abc, &Value::Int(i64::MIN), FetchMode::Isset, &mut d).unwrap(),
            Value::Null
        );
        assert_eq!(
            d.iter().map(|d| d.message.to_string()).collect::<Vec<_>>(),
            vec![
                "Uninitialized string offset 3",
                "Illegal string offset \"1x\"",
                "String offset cast occurred",
            ]
        );
        let e = string_offset(&abc, &s("1.0"), FetchMode::Read, &mut d).unwrap_err();
        assert_eq!(e.message(), "Cannot access offset of type string on string");
        let e =
            string_offset(&abc, &Value::Array(Array::new()), FetchMode::Isset, &mut d).unwrap_err();
        assert_eq!(e.message(), "Cannot access offset of type array on string");
    }

    #[test]
    fn reads_and_isset() {
        let mut a = Array::new();
        a.insert("k", Value::Null);
        a.insert(5, "five");
        let arr = Value::Array(a);
        let mut d: Vec<Diagnostic> = Vec::new();
        assert_eq!(
            read(&arr, &s("5"), FetchMode::Read, &mut d).unwrap(),
            s("five")
        );
        assert_eq!(
            read(&arr, &s("x\0y"), FetchMode::Read, &mut d).unwrap(),
            Value::Null
        );
        assert_eq!(
            read(&arr, &Value::Int(7), FetchMode::Isset, &mut d).unwrap(),
            Value::Null
        );
        assert_eq!(
            read(&Value::Null, &Value::Int(0), FetchMode::Read, &mut d).unwrap(),
            Value::Null
        );
        assert_eq!(
            read(&Value::Bool(true), &Value::Int(0), FetchMode::Read, &mut d).unwrap(),
            Value::Null
        );
        assert_eq!(
            d,
            vec![
                Diagnostic::new(Level::Warning, "Undefined array key \"x\""),
                Diagnostic::new(Level::Warning, "Trying to access array offset on null"),
                Diagnostic::new(Level::Warning, "Trying to access array offset on true"),
            ]
        );
        assert!(!isset(&arr, &s("k"), &mut d).unwrap());
        assert!(isset(&arr, &Value::Float(5.0), &mut d).unwrap());
        assert!(empty(&arr, &s("k"), &mut d).unwrap());
        assert!(isset(&s("abc"), &s("2"), &mut d).unwrap());
        assert!(!isset(&s("abc"), &s("1x"), &mut d).unwrap());
        assert!(empty(&s("a0"), &Value::Int(-1), &mut d).unwrap());
        assert_eq!(count(&arr, COUNT_NORMAL, &mut d).unwrap(), 2);
        let e = count(&s("x"), COUNT_NORMAL, &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "count(): Argument #1 ($value) must be of type Countable|array, string given"
        );
    }
}
