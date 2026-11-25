//! The array functions Twig's runtime and compiled templates call (reference/php/stdlib.md §10.3):
//! ports of PHPSRC/ext/standard/array.c and of `iterator_to_array()` / `iterator_count()`
//! (PHPSRC/ext/spl/spl_iterators.c).
//!
//! Results are built the way upstream builds them (`array_init_size()` + appends, `zend_hash_add`,
//! ...), so keys, order and `nNextFreeElement` match; packed vs hash layout is reproduced where it
//! follows from the inserts.

use super::HT_MAX_SIZE;
use super::callback::Callback;
use crate::array::Array;
use crate::compare;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::key::{ArrayKey, KeyContext};
use crate::object::{self, CastTarget, FetchMode, PropertyCheck};
use crate::param::{self, Arg, StrOrLong};
use crate::string::PhpString;
use crate::types;
use crate::value::Value;

/// `ARRAY_FILTER_USE_BOTH`: [`array_filter`] passes `(value, key)`.
pub const ARRAY_FILTER_USE_BOTH: i64 = 1;
/// `ARRAY_FILTER_USE_KEY`: [`array_filter`] passes the key only.
pub const ARRAY_FILTER_USE_KEY: i64 = 2;

/// `Z_PARAM_ARRAY` for argument `position` of `function` named `name`.
fn array_arg(v: &Value, function: &str, position: u32, name: &str) -> PhpResult<Array> {
    param::parse_array(v, &Arg::new(function, position, name))
}

/// The array check of an argument bound to a variadic parameter (`array ...$arrays`): PHP names
/// only declared parameters in messages (`get_function_arg_name()` returns `NULL` beyond
/// `num_args`), so the error reads `f(): Argument #3 must be of type array, int given`.
fn variadic_array_arg(v: &Value, function: &str, position: u32) -> PhpResult<Array> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        _ => Err(PhpError::type_error(PhpString::concat(&[
            format!("{function}(): Argument #{position} must be of type array, ").as_bytes(),
            types::zend_zval_value_name(v).as_bytes(),
            b" given",
        ]))),
    }
}

/// `Z_PARAM_BOOL` of an optional argument (`default` when not passed).
fn bool_arg(
    v: Option<&Value>,
    default: bool,
    arg: &Arg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    match v {
        Some(v) => param::parse_bool(v, arg, diag),
        None => Ok(default),
    }
}

/// `Z_PARAM_STR_OR_LONG_OR_NULL`: `null` (`None`), ints and strings as they are, otherwise the weak
/// int rules first and the weak string rules second; the error names `string|int|null`.
fn str_or_long_or_null(
    v: &Value,
    arg: &Arg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<StrOrLong>> {
    let expected = "of type string|int|null";
    match v {
        Value::Null => Ok(None),
        Value::Array(_) => Err(arg.type_error(expected, v)),
        Value::Object(o) => match o.cast(CastTarget::String, diag)? {
            Some(Value::String(s)) => Ok(Some(StrOrLong::Str(s))),
            Some(other) => Ok(Some(StrOrLong::Str(convert::to_string(&other, diag)?))),
            None => Err(arg.type_error(expected, v)),
        },
        // Ints, strings, floats and bools are never rejected by the str-or-long parser.
        _ => Ok(Some(param::parse_str_or_long(v, arg, diag)?)),
    }
}

/// Port of `array_merge(array ...$arrays)` (`php_array_merge_wrapper()`, `php_array_merge()`):
/// string keys keep their first position and take the last value; int keys are renumbered from 0
/// in order of appearance (`{a:1, 5:'x'} + {a:2, 5:'z'}` gives `{a:2, 0:'x', 1:'z'}`).
///
/// No arguments give `[]`. A non-array throws `array_merge(): Argument #N must be of type array, X
/// given` (variadic arguments are not named); more than `HT_MAX_SIZE` elements in total `Error: The
/// total number of elements must be lower than 1073741824`. With exactly two arguments of which one
/// is empty, the other is returned as it is when it is a list or has string keys only (upstream's
/// shortcut, which keeps its `nNextFreeElement`).
///
/// Known deviation: upstream decides the shortcut on the packed/hash layout, which this model does
/// not expose; a hash-layout array with keys `0..n` (rare: a list whose string keys were all
/// unset) is returned as it is instead of renumbered, which only shows in `nNextFreeElement`.
pub fn array_merge(arrays: &[Value]) -> PhpResult<Array> {
    if arrays.is_empty() {
        return Ok(Array::new());
    }
    let mut parts = Vec::with_capacity(arrays.len());
    let mut count: u64 = 0;
    for (i, a) in arrays.iter().enumerate() {
        let a = variadic_array_arg(a, "array_merge", i as u32 + 1)?;
        count += a.len() as u64;
        parts.push(a);
    }
    if count >= u64::from(HT_MAX_SIZE) {
        return Err(PhpError::error(format!(
            "The total number of elements must be lower than {HT_MAX_SIZE}"
        )));
    }
    if parts.len() == 2 {
        let ret = if parts[0].is_empty() {
            Some(&parts[1])
        } else if parts[1].is_empty() {
            Some(&parts[0])
        } else {
            None
        };
        // A packed table without holes, or a hash without int keys, is returned as it is.
        if let Some(r) = ret
            && (r.is_list() || r.keys().all(|k| matches!(k, ArrayKey::Str(_))))
        {
            return Ok(r.clone());
        }
    }
    let mut dest = Array::with_capacity(count as usize);
    for part in &parts {
        for (k, v) in part {
            match k {
                ArrayKey::Str(_) => {
                    dest.insert(k.clone(), v.clone());
                }
                // zend_hash_next_index_insert_new(): keys are dense from 0, never occupied.
                ArrayKey::Int(_) => {
                    let _ = dest.push(v.clone());
                }
            }
        }
    }
    Ok(dest)
}

/// Port of `array_chunk(array $array, int $length, bool $preserve_keys = false)`: a list of chunks
/// of `length` elements (the last one may be shorter), keys renumbered per chunk unless
/// `preserve_keys`. `ValueError: array_chunk(): Argument #2 ($length) must be greater than 0` for
/// `length < 1`.
pub fn array_chunk(
    array: &Value,
    length: &Value,
    preserve_keys: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let input = array_arg(array, "array_chunk", 1, "array")?;
    let length_arg = Arg::new("array_chunk", 2, "length");
    let mut size = param::parse_long(length, &length_arg, diag)?;
    let preserve_keys = bool_arg(
        preserve_keys,
        false,
        &Arg::new("array_chunk", 3, "preserve_keys"),
        diag,
    )?;
    // Do bounds checking for size parameter.
    if size < 1 {
        return Err(length_arg.value_error("must be greater than 0"));
    }
    let num_in = input.len() as i64;
    if size > num_in {
        if num_in == 0 {
            return Ok(Array::new());
        }
        size = num_in;
    }
    let size = size as usize;
    let mut out = Array::with_capacity((num_in as usize - 1) / size + 1);
    let mut chunk: Option<Array> = None;
    for (k, v) in &input {
        // If new chunk, create and initialize it.
        let c = chunk.get_or_insert_with(|| Array::with_capacity(size));
        // Add entry to the chunk, preserving keys if necessary.
        if preserve_keys {
            c.insert(k.clone(), v.clone());
        } else {
            let _ = c.push(v.clone());
        }
        // If reached the chunk size, add it to the result array, and reset the pointer.
        if c.len() == size {
            let _ = out.push(Value::Array(chunk.take().unwrap_or_default()));
        }
    }
    // Add the final chunk if there is one.
    if let Some(c) = chunk {
        let _ = out.push(Value::Array(c));
    }
    Ok(out)
}

/// Port of `array_column_fetch_prop()`: an array element (symtable lookup for string names) or an
/// object property that exists or passes `__isset()`, read with `BP_VAR_R`.
fn column_fetch(
    data: &Value,
    name: &StrOrLong,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<Value>> {
    match data {
        Value::Object(o) => {
            // If name is an integer convert integer to string
            let name: PhpString = match name {
                StrOrLong::Str(s) => s.clone(),
                StrOrLong::Long(l) => convert::long_to_string(*l),
            };
            // "exists" mode first (true for properties that are null), then "has" mode for
            // objects implementing __isset
            if o.has_property(&name, PropertyCheck::Exists, diag)?
                || o.has_property(&name, PropertyCheck::Isset, diag)?
            {
                return Ok(Some(o.read_property(&name, FetchMode::Read, diag)?));
            }
            Ok(None)
        }
        Value::Array(a) => Ok(match name {
            StrOrLong::Str(s) => a.get_sym(s).cloned(),
            StrOrLong::Long(l) => a.get_int(*l).cloned(),
        }),
        _ => Ok(None),
    }
}

/// Port of `array_column(array $array, int|string|null $column_key, int|string|null $index_key =
/// null)`: for every row (array or object) the `column_key` element (the whole row for `null`);
/// rows without it are skipped. With `index_key`, the row's index element becomes the key
/// (`array_set_zval_key()`: floats truncate with a deprecation when fractional, `null` is `""`
/// with `Using null as an array offset is deprecated, use an empty string instead`, arrays throw
/// `Cannot access offset of type array on array`); rows without it are appended.
///
/// A key argument that is an array (or an object without `__toString()`) throws `array_column():
/// Argument #2 ($column_key) must be of type string|int|null, array given`.
pub fn array_column(
    array: &Value,
    column_key: &Value,
    index_key: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let input = array_arg(array, "array_column", 1, "array")?;
    let column = str_or_long_or_null(column_key, &Arg::new("array_column", 2, "column_key"), diag)?;
    let index = match index_key {
        Some(v) => str_or_long_or_null(v, &Arg::new("array_column", 3, "index_key"), diag)?,
        None => None,
    };
    let mut out = Array::with_capacity(input.len());
    for data in input.values() {
        let colval = match &column {
            None => data.clone(),
            Some(name) => match column_fetch(data, name, diag)? {
                Some(v) => v,
                None => continue,
            },
        };
        match &index {
            // Index param is not passed
            None => {
                let _ = out.push(colval);
            }
            Some(index) => match column_fetch(data, index, diag)? {
                Some(keyval) => {
                    let key = ArrayKey::from_value(&keyval, KeyContext::Write, diag)?;
                    out.insert(key, colval);
                }
                None => {
                    // zend_hash_next_index_insert() fails silently on an occupied next key.
                    let _ = out.push(colval);
                }
            },
        }
    }
    Ok(out)
}

/// Port of `array_slice(array $array, int $offset, ?int $length = null, bool $preserve_keys =
/// false)`: `length` elements from position `offset` (negative values count from the end, `null`
/// means up to the end). String keys are always kept; int keys are renumbered unless
/// `preserve_keys`.
pub fn array_slice(
    array: &Value,
    offset: &Value,
    length: Option<&Value>,
    preserve_keys: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let input = array_arg(array, "array_slice", 1, "array")?;
    let mut offset = param::parse_long(offset, &Arg::new("array_slice", 2, "offset"), diag)?;
    let length = match length {
        Some(v) => param::parse_long_or_null(v, &Arg::new("array_slice", 3, "length"), diag)?,
        None => None,
    };
    let preserve_keys = bool_arg(
        preserve_keys,
        false,
        &Arg::new("array_slice", 4, "preserve_keys"),
        diag,
    )?;

    // Get number of entries in the input hash
    let num_in = input.len() as i64;
    // We want all entries from offset to the end if length is not passed or is null
    let mut length = length.unwrap_or(num_in);

    // Clamp the offset..
    if offset > num_in {
        return Ok(Array::new());
    } else if offset < 0 {
        offset += num_in;
        if offset < 0 {
            offset = 0;
        }
    }
    // ..and the length
    if length < 0 {
        length += num_in - offset;
    } else if (offset as u64).wrapping_add(length as u64) > num_in as u64 {
        length = num_in - offset;
    }
    if length <= 0 {
        return Ok(Array::new());
    }

    let mut out = Array::with_capacity(length as usize);
    for (k, v) in input.iter().skip(offset as usize).take(length as usize) {
        match k {
            ArrayKey::Str(_) => {
                out.insert(k.clone(), v.clone());
            }
            ArrayKey::Int(h) => {
                if preserve_keys {
                    out.insert(*h, v.clone());
                } else {
                    let _ = out.push(v.clone());
                }
            }
        }
    }
    Ok(out)
}

/// Port of `array_keys(array $array, mixed $filter_value = UNKNOWN, bool $strict = false)`: the
/// keys as a list (numeric-string keys are ints); with `filter_value` (`Some`, even `null`) only
/// the keys whose value equals it (`==`, or `===` with `strict`).
pub fn array_keys(
    array: &Value,
    filter_value: Option<&Value>,
    strict: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let input = array_arg(array, "array_keys", 1, "array")?;
    let strict = bool_arg(strict, false, &Arg::new("array_keys", 3, "strict"), diag)?;
    // Base case: empty input
    if input.is_empty() {
        return Ok(Array::new());
    }
    match filter_value {
        Some(search) => {
            let mut out = Array::new();
            for (k, v) in &input {
                let hit = if strict {
                    search.identical(v)
                } else {
                    compare::equals(search, v, diag)?
                };
                if hit {
                    let _ = out.push(k.to_value());
                }
            }
            Ok(out)
        }
        None => Ok(Array::from_values(input.keys().map(ArrayKey::to_value))),
    }
}

/// Port of `array_values(array $array)`: the values as a list; a list that is already one (keys
/// `0..n`, next free key `n`) is returned as it is.
pub fn array_values(array: &Value) -> PhpResult<Array> {
    let input = array_arg(array, "array_values", 1, "array")?;
    Ok(to_list(&input))
}

/// `zend_array_to_list()` with `array_values()`'s shortcut for vector-like arrays.
fn to_list(input: &Array) -> Array {
    if input.is_empty() {
        return Array::new();
    }
    if input.is_list() && input.next_free() == Some(input.len() as i64) {
        return input.clone();
    }
    Array::from_values(input.values().cloned())
}

/// Port of `array_reverse(array $array, bool $preserve_keys = false)`: the elements in reverse
/// order; string keys are kept, int keys renumbered unless `preserve_keys`.
pub fn array_reverse(
    array: &Value,
    preserve_keys: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let input = array_arg(array, "array_reverse", 1, "array")?;
    let preserve_keys = bool_arg(
        preserve_keys,
        false,
        &Arg::new("array_reverse", 2, "preserve_keys"),
        diag,
    )?;
    let mut out = Array::with_capacity(input.len());
    for (k, v) in input.iter().rev() {
        match k {
            ArrayKey::Str(_) => {
                out.insert(k.clone(), v.clone());
            }
            ArrayKey::Int(h) => {
                if preserve_keys {
                    out.insert(*h, v.clone());
                } else {
                    let _ = out.push(v.clone());
                }
            }
        }
    }
    Ok(out)
}

/// Port of `_php_search_array()`: the first matching element's key.
fn search(
    function: &str,
    needle: &Value,
    haystack: &Value,
    strict: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<ArrayKey>> {
    let haystack = array_arg(haystack, function, 2, "haystack")?;
    let strict = bool_arg(strict, false, &Arg::new(function, 3, "strict"), diag)?;
    for (k, v) in &haystack {
        let hit = if strict {
            // fast_is_identical_function()
            needle.identical(v)
        } else {
            // fast_equal_check_long() / fast_equal_check_string() / fast_equal_check_function()
            compare::equals(needle, v, diag)?
        };
        if hit {
            return Ok(Some(k.clone()));
        }
    }
    Ok(None)
}

/// Port of `in_array(mixed $needle, array $haystack, bool $strict = false)`: whether an element is
/// `==` to the needle (`===` with `strict`), searching in order.
pub fn in_array(
    needle: &Value,
    haystack: &Value,
    strict: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    Ok(search("in_array", needle, haystack, strict, diag)?.is_some())
}

/// Port of `array_search(mixed $needle, array $haystack, bool $strict = false)`: the key of the
/// first matching element, or `false`.
pub fn array_search(
    needle: &Value,
    haystack: &Value,
    strict: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    Ok(
        match search("array_search", needle, haystack, strict, diag)? {
            Some(k) => k.to_value(),
            None => Value::Bool(false),
        },
    )
}

/// Port of `array_key_exists($key, array $array)`: string keys with symtable normalisation (`"1"`
/// finds `1`), ints, floats (`zend_dval_to_lval_safe()`, deprecation when fractional), bools as
/// `0`/`1`, `null` as `""` with `Using null as the key parameter for array_key_exists() is
/// deprecated, use an empty string instead`; arrays and objects throw `array_key_exists():
/// Argument #1 ($key) must be a valid array offset type`.
pub fn array_key_exists(key: &Value, array: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    let ht = array_arg(array, "array_key_exists", 2, "array")?;
    if matches!(key, Value::Array(_) | Value::Object(_)) {
        return Err(PhpError::type_error(
            "array_key_exists(): Argument #1 ($key) must be a valid array offset type",
        ));
    }
    let k = ArrayKey::from_value(key, KeyContext::ArrayKeyExists, diag)?;
    Ok(ht.contains_key(&k))
}

/// Port of `array_pad(array $array, int $length, mixed $value)`: the array extended to `|length|`
/// elements with `value` (appended for a positive length, prepended for a negative one); int keys
/// are renumbered when padding happens. `ValueError: array_pad(): Argument #2 ($length) must not
/// exceed the maximum allowed array size` beyond `±HT_MAX_SIZE`. Twig's sequence destructuring
/// (`[a, b] = value`) compiles to it.
pub fn array_pad(
    array: &Value,
    length: &Value,
    value: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let input = array_arg(array, "array_pad", 1, "array")?;
    let length_arg = Arg::new("array_pad", 2, "length");
    let pad_size = param::parse_long(length, &length_arg, diag)?;
    let max = i64::from(HT_MAX_SIZE);
    if pad_size < -max || pad_size > max {
        return Err(length_arg.value_error("must not exceed the maximum allowed array size"));
    }
    let input_size = input.len() as i64;
    let pad_size_abs = pad_size.abs();
    if input_size >= pad_size_abs {
        // Copy the original array
        return Ok(input);
    }
    let num_pads = pad_size_abs - input_size;
    let mut out = Array::with_capacity(pad_size_abs as usize);
    if pad_size < 0 {
        for _ in 0..num_pads {
            let _ = out.push(value.clone());
        }
    }
    for (k, v) in &input {
        match k {
            ArrayKey::Str(_) => {
                out.insert(k.clone(), v.clone());
            }
            ArrayKey::Int(_) => {
                let _ = out.push(v.clone());
            }
        }
    }
    if pad_size > 0 {
        for _ in 0..num_pads {
            let _ = out.push(value.clone());
        }
    }
    Ok(out)
}

/// Port of `array_fill_keys(array $keys, mixed $value)`: every element of `keys` becomes a key
/// mapped to `value` (ints as they are, everything else through string conversion with its
/// warnings and errors, then symtable normalisation).
pub fn array_fill_keys(
    keys: &Value,
    value: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let keys = array_arg(keys, "array_fill_keys", 1, "keys")?;
    let mut out = Array::with_capacity(keys.len());
    for entry in keys.values() {
        match entry {
            Value::Int(l) => {
                out.insert(*l, value.clone());
            }
            other => {
                let key = convert::to_string(other, diag)?;
                out.insert(ArrayKey::from_string(key), value.clone());
            }
        }
    }
    Ok(out)
}

/// Port of `array_flip(array $array)`: values become keys and keys values; values that are neither
/// ints nor strings are skipped with `Warning: array_flip(): Can only flip string and integer
/// values, entry skipped`.
pub fn array_flip(array: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Array> {
    let input = array_arg(array, "array_flip", 1, "array")?;
    let mut out = Array::with_capacity(input.len());
    for (k, v) in &input {
        match v {
            Value::Int(l) => {
                out.insert(*l, k.to_value());
            }
            Value::String(s) => {
                out.insert(ArrayKey::from_string(s.clone()), k.to_value());
            }
            _ => {
                diag.warn("array_flip(): Can only flip string and integer values, entry skipped")?
            }
        }
    }
    Ok(out)
}

/// Shared by [`array_intersect_key`] and [`array_diff_key`]: the elements of the first array whose
/// key is present in all (`keep_present`) or in none of the other arrays, keys preserved.
fn key_set_op(function: &str, arrays: &[Value], keep_present: bool) -> PhpResult<Array> {
    if arrays.is_empty() {
        return Err(PhpError::argument_count_error(format!(
            "{function}() expects at least 1 argument, 0 given"
        )));
    }
    let mut parts = Vec::with_capacity(arrays.len());
    for (i, a) in arrays.iter().enumerate() {
        parts.push(if i == 0 {
            array_arg(a, function, 1, "array")?
        } else {
            variadic_array_arg(a, function, i as u32 + 1)?
        });
    }
    let mut out = Array::new();
    for (k, v) in &parts[0] {
        let ok = if keep_present {
            parts[1..].iter().all(|p| p.contains_key(k))
        } else {
            !parts[1..].iter().any(|p| p.contains_key(k))
        };
        if ok {
            out.insert(k.clone(), v.clone());
        }
    }
    Ok(out)
}

/// Port of `array_intersect_key(array $array, array ...$arrays)` (`php_array_intersect_key()`): the
/// elements of the first array whose key exists in every other array, in the first array's order.
/// Twig's `for` loop restores its context with `array_intersect_key($context, $_parent)`.
pub fn array_intersect_key(arrays: &[Value]) -> PhpResult<Array> {
    key_set_op("array_intersect_key", arrays, true)
}

/// Port of `array_diff_key(array $array, array ...$arrays)` (`php_array_diff_key()`): the elements
/// of the first array whose key exists in none of the other arrays.
pub fn array_diff_key(arrays: &[Value]) -> PhpResult<Array> {
    key_set_op("array_diff_key", arrays, false)
}

/// Port of `array_key_first(array $array)`: the first key, or `null` for an empty array.
pub fn array_key_first(array: &Value) -> PhpResult<Value> {
    let a = array_arg(array, "array_key_first", 1, "array")?;
    Ok(a.first().map_or(Value::Null, |(k, _)| k.to_value()))
}

/// Port of `array_key_last(array $array)`: the last key, or `null` for an empty array.
pub fn array_key_last(array: &Value) -> PhpResult<Value> {
    let a = array_arg(array, "array_key_last", 1, "array")?;
    Ok(a.last().map_or(Value::Null, |(k, _)| k.to_value()))
}

/// Port of `array_is_list(array $array)` (PHPSRC/ext/standard/type.c): keys `0..n` in order (`[]`
/// is a list). Twig's `sequence`/`mapping` tests use it.
pub fn array_is_list(array: &Value) -> PhpResult<bool> {
    Ok(array_arg(array, "array_is_list", 1, "array")?.is_list())
}

/// `current($array)` for an array whose internal pointer is at its start, which is the case for
/// every array Twig passes (`first`/`last` call `current()` on a fresh `array_slice()` result):
/// the first value, or `false` when the array is empty. Arrays have no internal pointer here.
pub fn current(array: &Array) -> Value {
    array.values().next().cloned().unwrap_or(Value::Bool(false))
}

/// Port of `array_filter(array $array, ?callable $callback = null, int $mode = 0)`: the elements
/// for which the callback's result is truthy (without a callback: the truthy elements), keys
/// preserved. The callback receives the value, or `(value, key)` with [`ARRAY_FILTER_USE_BOTH`]
/// (Twig's `filter` filter on arrays), or the key with [`ARRAY_FILTER_USE_KEY`]; other mode values
/// pass the value.
pub fn array_filter(
    array: &Value,
    callback: Option<&mut Callback<'_>>,
    mode: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let input = array_arg(array, "array_filter", 1, "array")?;
    let use_type = match mode {
        Some(m) => param::parse_long(m, &Arg::new("array_filter", 3, "mode"), diag)?,
        None => 0,
    };
    if input.is_empty() {
        return Ok(Array::new());
    }
    let mut out = Array::new();
    let mut callback = callback;
    for (k, v) in &input {
        let keep = match callback.as_deref_mut() {
            Some(f) => {
                let retval = match use_type {
                    ARRAY_FILTER_USE_BOTH => f(&[v.clone(), k.to_value()], diag)?,
                    ARRAY_FILTER_USE_KEY => f(&[k.to_value()], diag)?,
                    _ => f(std::slice::from_ref(v), diag)?,
                };
                // php_is_true()
                convert::to_bool(&retval, diag)?
            }
            None => convert::to_bool(v, diag)?,
        };
        if keep {
            out.insert(k.clone(), v.clone());
        }
    }
    Ok(out)
}

/// Port of `array_map(?callable $callback, array $array, array ...$arrays)`.
///
/// * One array: the callback applied to every value, keys preserved; without a callback (or for an
///   empty array) the array itself.
/// * Several arrays: a list where element `i` is the callback applied to the `i`-th values (`null`
///   past the end of shorter arrays); without a callback, lists of those values.
///
/// Non-arrays throw `array_map(): Argument #2 ($array) must be of type array, X given`, or
/// `array_map(): Argument #3 must be of type array, X given` (without a name) for the variadic
/// arguments.
pub fn array_map(
    callback: Option<&mut Callback<'_>>,
    arrays: &[Value],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    match arrays {
        // The callback is the only argument.
        [] => Err(PhpError::argument_count_error(
            "array_map() expects at least 2 arguments, 1 given",
        )),
        [only] => {
            let input = array_arg(only, "array_map", 2, "array")?;
            let Some(f) = callback else {
                return Ok(input);
            };
            if input.is_empty() {
                return Ok(input);
            }
            let mut out = Array::with_capacity(input.len());
            for (k, v) in &input {
                let result = f(std::slice::from_ref(v), diag)?;
                out.insert(k.clone(), result);
            }
            Ok(out)
        }
        _ => {
            let mut inputs = Vec::with_capacity(arrays.len());
            let mut maxlen = 0;
            for (i, a) in arrays.iter().enumerate() {
                let a = if i == 0 {
                    array_arg(a, "array_map", 2, "array")?
                } else {
                    variadic_array_arg(a, "array_map", i as u32 + 2)?
                };
                maxlen = maxlen.max(a.len());
                inputs.push(a);
            }
            let mut iters: Vec<_> = inputs.iter().map(|a| a.values()).collect();
            let mut out = Array::with_capacity(maxlen);
            let mut callback = callback;
            for _ in 0..maxlen {
                // If this array still has elements, add the current one to the parameter list,
                // otherwise use null value.
                let params: Vec<Value> = iters
                    .iter_mut()
                    .map(|it| it.next().cloned().unwrap_or(Value::Null))
                    .collect();
                let result = match callback.as_deref_mut() {
                    Some(f) => f(&params, diag)?,
                    None => Value::Array(Array::from_values(params)),
                };
                let _ = out.push(result);
            }
            Ok(out)
        }
    }
}

/// Port of `array_reduce(array $array, callable $callback, mixed $initial = null)`: folds the values
/// from the left, calling the callback with `(carry, value)`.
pub fn array_reduce(
    array: &Value,
    callback: &mut Callback<'_>,
    initial: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let input = array_arg(array, "array_reduce", 1, "array")?;
    let mut carry = initial.cloned().unwrap_or(Value::Null);
    for v in input.values() {
        carry = callback(&[carry, v.clone()], diag)?;
    }
    Ok(carry)
}

/// `Z_PARAM_ITERABLE`: an array or a `Traversable` object.
fn iterable_arg(v: &Value, function: &str) -> PhpResult<()> {
    match v {
        Value::Array(_) => Ok(()),
        Value::Object(o) if o.instance_of("Traversable") => Ok(()),
        _ => Err(PhpError::type_error(PhpString::concat(&[
            function.as_bytes(),
            b"(): Argument #1 ($iterator) must be of type Traversable|array, ",
            types::zend_zval_value_name(v).as_bytes(),
            b" given",
        ]))),
    }
}

/// Port of `iterator_to_array(Traversable|array $iterator, bool $preserve_keys = true)`
/// (PHPSRC/ext/spl/spl_iterators.c): arrays are returned as they are (or as a list without keys);
/// objects are traversed with PHP's call order, keys going through `array_set_zval_key()`
/// ([`object::iterator_to_array`]). Other values throw `iterator_to_array(): Argument #1
/// ($iterator) must be of type Traversable|array, X given`.
pub fn iterator_to_array(
    iterator: &Value,
    preserve_keys: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    iterable_arg(iterator, "iterator_to_array")?;
    let use_keys = bool_arg(
        preserve_keys,
        true,
        &Arg::new("iterator_to_array", 2, "preserve_keys"),
        diag,
    )?;
    match iterator {
        Value::Array(a) => Ok(if use_keys {
            a.clone()
        } else {
            // zend_array_to_list()
            Array::from_values(a.values().cloned())
        }),
        Value::Object(o) => object::iterator_to_array(o, use_keys, diag),
        _ => unreachable!("checked by iterable_arg"),
    }
}

/// Port of `iterator_count(Traversable|array $iterator)`: the element count of an array, or the
/// number of `valid()` iterations of a `Traversable` ([`object::iterator_count`]).
pub fn iterator_count(iterator: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    iterable_arg(iterator, "iterator_count")?;
    match iterator {
        Value::Array(a) => Ok(a.len() as i64),
        Value::Object(o) => object::iterator_count(o, diag),
        _ => unreachable!("checked by iterable_arg"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(a: &Array) -> Vec<String> {
        a.keys().map(|k| k.to_string()).collect()
    }

    #[test]
    fn merge_and_slice() {
        let mut d = Vec::new();
        // reference/php/stdlib.md §10.3, verified with PHP 8.5.13
        let a: Array = vec![
            (ArrayKey::from("a"), Value::Int(1)),
            (ArrayKey::Int(5), Value::from("x")),
            (ArrayKey::Int(7), Value::from("y")),
        ]
        .into_iter()
        .collect();
        let b: Array = vec![
            (ArrayKey::from("a"), Value::Int(2)),
            (ArrayKey::Int(5), Value::from("z")),
            (ArrayKey::from("b"), Value::Int(3)),
        ]
        .into_iter()
        .collect();
        let m = array_merge(&[Value::Array(a.clone()), Value::Array(b)]).unwrap();
        assert_eq!(keys(&m), ["a", "0", "1", "2", "b"]);
        assert_eq!(m.get_sym(b"a"), Some(&Value::Int(2)));
        let s = array_slice(&Value::Array(a), &Value::Int(1), None, None, &mut d).unwrap();
        assert_eq!(keys(&s), ["0", "1"]);
        let l: Array = (1..=5).map(Value::from).collect();
        let c = array_chunk(
            &Value::Array(l),
            &Value::Int(2),
            Some(&Value::Bool(true)),
            &mut d,
        )
        .unwrap();
        let last = c.get_int(2).unwrap().as_array().unwrap();
        assert_eq!(keys(last), ["4"]);
        assert!(d.is_empty());
        let e = array_chunk(&Value::Array(Array::new()), &Value::Int(0), None, &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "array_chunk(): Argument #2 ($length) must be greater than 0"
        );
    }

    #[test]
    fn current_and_object_callbacks() {
        assert_eq!(current(&Array::new()), Value::Bool(false));
        let l: Array = [7, 8].into_iter().map(Value::from).collect();
        assert_eq!(current(&l), Value::Int(7));

        let closure = crate::object::Closure::new(|args, _diag| {
            Ok(Value::Bool(
                matches!(args.get(0), Some(Value::Int(i)) if i % 2 == 0),
            ))
        })
        .into_ref();
        let mut f = super::super::object_callback(&closure);
        let mut d = Vec::new();
        let input: Array = (1..=6).map(Value::from).collect();
        let even = array_filter(&Value::Array(input), Some(&mut f), None, &mut d).unwrap();
        assert_eq!(keys(&even), ["1", "3", "5"]);
        let not_callable = Value::Object(crate::object::StdClass::new().into_ref());
        let o = not_callable.as_object().unwrap();
        let mut g = super::super::object_callback(o);
        let e = array_map(Some(&mut g), &[Value::Array(l)], &mut d).unwrap_err();
        assert_eq!(e.message(), "Object of type stdClass is not callable");
    }
}
