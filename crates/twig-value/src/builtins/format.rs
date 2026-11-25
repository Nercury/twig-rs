//! Formatting builtins (reference/php/stdlib.md §5.1, §5.2, §5.11, §9.1-§9.3, §11):
//! `sprintf()`/`vsprintf()` ([`php_formatted_print`](formatted_print)), `number_format()`,
//! `round()` (PHP 8.4+ algorithm) with Twig's `round` filter, `floor()`, `ceil()`, `abs()`,
//! `json_encode()` and `hash('xxh128')`.
//!
//! | function | PHP level | typed core | upstream |
//! |---|---|---|---|
//! | `sprintf`, `vsprintf` | [`sprintf`], [`vsprintf`] | [`formatted_print`] | `ext/standard/formatted_print.c`, `main/snprintf.c` |
//! | `number_format` | [`number_format`] | [`math_number_format`], [`math_number_format_long`] | `ext/standard/math.c` |
//! | `round`, `floor`, `ceil`, `abs` | [`round`], [`floor`], [`ceil`], [`abs`] | [`math_round`] ([`RoundingMode`]) | `ext/standard/math.c` |
//! | `json_encode` | [`json_encode`] | [`json_encode_value`] ([`JsonError`], `JSON_*` flags) | `ext/json/json_encoder.c` |
//! | `hash('xxh128', ...)` | | [`hash_xxh128`], [`hash_xxh128_binary`] | `ext/hash/hash_xxhash.c` |
//! | Twig `round` filter | [`twig_round`] | | `CoreExtension::round()` |
//!
//! # Levels of API (the convention of the sibling `string` and `array` modules)
//!
//! * **PHP level**: functions named after the PHP builtin take their arguments as
//!   [`Value`]s exactly as PHP receives them (`None` for an omitted optional argument) and coerce
//!   them like `zend_parse_parameters()` in coercive mode: the `Passing null to parameter #N ($x)
//!   of type T is deprecated` deprecation, the verbatim `TypeError`/`ValueError` texts, objects
//!   with `__toString()`. Results are typed (`PhpString`, `f64`, [`Number`]).
//! * **Argument lists**: `*_call(args: &[Value], diag) -> PhpResult<Value>` ([`BuiltinFn`]) adds
//!   PHP's arity check (`abs() expects exactly 1 argument, 2 given`) for calls with an arbitrary
//!   argument list. Twig registers `abs` and `json_encode` as bare PHP callables, so extra filter
//!   arguments reach them; [`function`] looks these variants up by (case-insensitive) name.
//! * **Typed cores**, named after the C function they port, for callers that hold typed
//!   arguments already.
//!
//! Twig's `format` and `number_format` filters are thin userland wrappers for the engine:
//! `\sprintf($format ?? '', ...$values)` ([`sprintf`]) and `number_format((float) $number,
//! $decimal, $decimalPoint, $thousandSep)` ([`number_format`]); the `round` filter is
//! [`twig_round`].
//!
//! # Deviations
//!
//! * `Field width %zd is too long` is a fatal `E_ERROR` in PHP; it is returned as an `Error`.
//! * PHP's built-in `memory_limit` (128M) aborts with `Allowed memory size of 134217728 bytes
//!   exhausted (tried to allocate %zu bytes)` when a result outgrows it; here a single result
//!   string longer than 128 MiB is refused with that message as an `Error` (the byte count is the
//!   size of the `zend_string` that would hold the result; PHP's figure depends on its allocator
//!   state). The oracle runs with `memory_limit=-1` and would build such strings.
//! * `json_encode()`'s `JsonException` carries the error code in PHP (`getCode()`); [`PhpError`]
//!   has no code, [`JsonError::code`] gives it.
//!
//! [`Number`]: crate::Number

#![warn(missing_docs)]

mod conv;
mod hash;
mod json;
mod math;
mod number;
mod printf;

pub use hash::{hash_xxh128, hash_xxh128_binary};
pub use json::{
    JSON_FORCE_OBJECT, JSON_HEX_AMP, JSON_HEX_APOS, JSON_HEX_QUOT, JSON_HEX_TAG,
    JSON_INVALID_UTF8_IGNORE, JSON_INVALID_UTF8_SUBSTITUTE, JSON_NUMERIC_CHECK,
    JSON_PARTIAL_OUTPUT_ON_ERROR, JSON_PRESERVE_ZERO_FRACTION, JSON_PRETTY_PRINT,
    JSON_THROW_ON_ERROR, JSON_UNESCAPED_LINE_TERMINATORS, JSON_UNESCAPED_SLASHES,
    JSON_UNESCAPED_UNICODE, JsonEncoded, JsonError, json_encode, json_encode_call,
    json_encode_value,
};
pub use math::{
    RoundingMode, abs, abs_call, ceil, ceil_call, floor, floor_call, math_round, round, round_call,
    rounding_mode_class, twig_round,
};
pub use number::{math_number_format, math_number_format_long, number_format, number_format_call};
pub use printf::{formatted_print, sprintf, sprintf_call, vsprintf, vsprintf_call};

use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult, ThrowableClass};
use crate::value::Value;

/// A PHP builtin called with an argument list: positional arguments in, return value out.
pub type BuiltinFn = fn(&[Value], &mut dyn Diagnostics) -> PhpResult<Value>;

/// The argument-list variants of this module's PHP functions by name (case-insensitive, like PHP
/// function names): `sprintf`, `vsprintf`, `number_format`, `round`, `floor`, `ceil`, `abs`,
/// `json_encode`.
pub fn function(name: &str) -> Option<BuiltinFn> {
    const TABLE: &[(&str, BuiltinFn)] = &[
        ("sprintf", sprintf_call),
        ("vsprintf", vsprintf_call),
        ("number_format", number_format_call),
        ("round", round_call),
        ("floor", floor_call),
        ("ceil", ceil_call),
        ("abs", abs_call),
        ("json_encode", json_encode_call),
    ];
    let name = name.strip_prefix('\\').unwrap_or(name);
    TABLE
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|&(_, f)| f)
}

/// Port of `zend_wrong_parameters_count_error()` (PHPSRC/Zend/zend_API.c), the arity check of
/// `ZEND_PARSE_PARAMETERS_START(min, max)` (`max == None` for variadic functions):
/// `%s() expects exactly|at least|at most %d argument%s, %d given`.
pub(crate) fn check_arg_count(
    function: &str,
    args: &[Value],
    min: usize,
    max: Option<usize>,
) -> PhpResult<()> {
    let given = args.len();
    if given >= min && max.is_none_or(|max| given <= max) {
        return Ok(());
    }
    // A variadic function has max_num_args == (uint32_t) -1, never equal to min.
    let max = max.unwrap_or(u32::MAX as usize);
    let expected = if given < min { min } else { max };
    Err(PhpError::argument_count_error(format!(
        "{}() expects {} {} argument{}, {} given",
        function,
        if min == max {
            "exactly"
        } else if given < min {
            "at least"
        } else {
            "at most"
        },
        expected,
        if expected == 1 { "" } else { "s" },
        given
    )))
}

/// PHP's default `memory_limit` (128M), used as the cap of a single result string (see the
/// module documentation, "Deviations").
const MEMORY_LIMIT: usize = 128 * 1024 * 1024;

/// Refuses a result string of `len` bytes that PHP could not allocate under the default
/// `memory_limit`: `Allowed memory size of 134217728 bytes exhausted (tried to allocate %zu
/// bytes)` with the `zend_string` allocation size (`_ZSTR_STRUCT_SIZE(len)`, 8-byte aligned).
pub(crate) fn check_alloc(len: usize) -> PhpResult<()> {
    if len <= MEMORY_LIMIT {
        return Ok(());
    }
    let struct_size = len.saturating_add(24 + 1 + 7) & !7;
    Err(PhpError::error(format!(
        "Allowed memory size of {MEMORY_LIMIT} bytes exhausted (tried to allocate {struct_size} bytes)"
    )))
}

/// `Twig\Error\RuntimeError` (extends `Twig\Error\Error` extends `Exception`) with `message`, for
/// the Twig-level helpers. The engine adds the template name and line.
pub(crate) fn twig_runtime_error(message: &str) -> PhpError {
    PhpError::new(
        ThrowableClass::other(
            "Twig\\Error\\RuntimeError",
            ThrowableClass::other("Twig\\Error\\Error", ThrowableClass::Exception),
        ),
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arity_messages() {
        let none: [Value; 0] = [];
        let e = check_arg_count("sprintf", &none, 1, None).unwrap_err();
        assert_eq!(e.class().name(), "ArgumentCountError");
        assert_eq!(
            e.message(),
            "sprintf() expects at least 1 argument, 0 given"
        );
        let one = [Value::Null];
        let e = check_arg_count("vsprintf", &one, 2, Some(2)).unwrap_err();
        assert_eq!(
            e.message(),
            "vsprintf() expects exactly 2 arguments, 1 given"
        );
        let five = [
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ];
        let e = check_arg_count("number_format", &five, 1, Some(4)).unwrap_err();
        assert_eq!(
            e.message(),
            "number_format() expects at most 4 arguments, 5 given"
        );
        let e = check_arg_count("abs", &none, 1, Some(1)).unwrap_err();
        assert_eq!(e.message(), "abs() expects exactly 1 argument, 0 given");
        assert!(check_arg_count("round", &one, 1, Some(3)).is_ok());
    }

    #[test]
    fn lookup() {
        assert!(function("ABS").is_some());
        assert!(function("\\json_encode").is_some());
        assert!(function("strlen").is_none());
    }
}
