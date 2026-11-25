//! `range()`: port of `PHP_FUNCTION(range)` and `php_range_process_input()`
//! (PHPSRC/ext/standard/array.c) with the PHP 8.3+ rules (reference/php/stdlib.md §10.1). Twig's
//! `a..b` operator compiles to `range(a, b)`.

use super::HT_MAX_SIZE;
use crate::array::Array;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::math;
use crate::numeric;
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::{Number, Value};

/// What an argument can be interpreted as (`php_range_process_input()`). The order is significant:
/// upstream compares the type codes `IS_LONG < IS_DOUBLE < IS_STRING < IS_ARRAY`, where `IS_ARRAY`
/// stands for a single-byte string holding a digit (a character and a number at the same time).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Input {
    Long,
    Double,
    Str,
    Digit,
}

/// Port of `php_range_process_input()`: the interpretation plus the numeric value (`lval`, `dval`);
/// non-numeric strings fall back to `0`.
fn process_input(
    input: &Value,
    arg: &Arg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<(Input, i64, f64)> {
    let finite = |d: f64| -> PhpResult<(Input, i64, f64)> {
        if d.is_infinite() {
            return Err(arg.value_error("must be a finite number, INF provided"));
        }
        if d.is_nan() {
            return Err(arg.value_error("must be a finite number, NAN provided"));
        }
        Ok((Input::Double, 0, d))
    };
    match input {
        Value::Int(l) => Ok((Input::Long, *l, *l as f64)),
        Value::Float(d) => finite(*d),
        Value::String(s) => {
            if s.is_empty() {
                diag.warn(format!(
                    "range(): Argument #{} (${}) must not be empty, casted to 0",
                    arg.position, arg.name
                ))?;
                return Ok((Input::Long, 0, 0.0));
            }
            match numeric::is_numeric_str_function(s) {
                Some(Number::Float(d)) => finite(d),
                Some(Number::Int(l)) => {
                    let kind = if s.len() == 1 {
                        Input::Digit
                    } else {
                        Input::Long
                    };
                    Ok((kind, l, l as f64))
                }
                None => {
                    if s.len() != 1 {
                        diag.warn(format!(
                            "range(): Argument #{} (${}) must be a single byte, subsequent bytes are ignored",
                            arg.position, arg.name
                        ))?;
                    }
                    // Set fall back values to 0 in case the other argument is not a string
                    Ok((Input::Str, 0, 0.0))
                }
            }
        }
        // Z_PARAM_NUMBER_OR_STR only lets ints, floats and strings through.
        _ => Err(arg.type_error("of type string|int|float", input)),
    }
}

/// Port of `_php_math_round(value, 0, PHP_ROUND_HALF_UP)` (PHPSRC/ext/standard/math.c) with
/// `php_round_helper()`: `range()` rounds its element count with it.
fn round_half_up(value: f64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    if value == math::trunc(value) {
        return value;
    }
    let exponent = 1.0;
    let (mut tmp_value, tmp_value2) = if value >= 0.0 {
        let t = math::floor(value / exponent);
        (t, t + 1.0)
    } else {
        let t = math::ceil(value / exponent);
        (t, t - 1.0)
    };
    if tmp_value2 * exponent == value {
        tmp_value = tmp_value2;
    }
    // This value is beyond our precision, so rounding it is pointless
    if tmp_value.abs() >= 1e16 {
        return value;
    }
    let edge_case = ((tmp_value + 0.5f64.copysign(tmp_value)) * exponent).abs();
    if value.abs() >= edge_case {
        tmp_value += 1.0f64.copysign(tmp_value);
    }
    tmp_value * exponent
}

/// PHP's default `memory_limit` (128M).
const MEMORY_LIMIT: u64 = 134_217_728;

/// Stand-in for the fatal `Allowed memory size of 134217728 bytes exhausted (tried to allocate %zu
/// bytes)` PHP raises when `zend_hash_real_finit_packed()` allocates the table of a range with the
/// default `memory_limit`: a packed table of `zend_hash_check_size(size)` zvals plus its 8-byte hash
/// part, rounded to whole pages for huge blocks. PHP's exact threshold also depends on the memory
/// already in use; ranges of up to 2^22 elements pass, larger ones fail, in a fresh process as here.
/// Returned as an `Error` (PHP's fatal error cannot be caught).
fn check_memory(size: u32) -> PhpResult<()> {
    let table = u64::from(size.max(8)).next_power_of_two();
    let bytes = table * 16 + 8;
    if bytes > MEMORY_LIMIT {
        let page = 4096;
        return Err(PhpError::error(format!(
            "Allowed memory size of {MEMORY_LIMIT} bytes exhausted (tried to allocate {} bytes)",
            bytes.div_ceil(page) * page
        )));
    }
    Ok(())
}

/// C `%.*f` as PHP's `spprintf()` formats it (`php_conv_fp()`): correctly rounded, no sign for an
/// exact zero, `inf`/`nan` for non-finite values.
fn printf_f(d: f64, precision: usize) -> String {
    if d.is_nan() {
        "nan".to_string()
    } else if d.is_infinite() {
        "inf".to_string()
    } else if d == 0.0 {
        format!("{:.*}", precision, 0.0)
    } else {
        format!("{d:.precision$}")
    }
}

/// `RANGE_CHECK_DOUBLE_INIT_ARRAY(start, end, step)`: the element count of a float range (rounded
/// half up), or the size `ValueError`. The macro prints its `end` argument as "start" and `start`
/// as "end", so the message always shows the smaller bound first.
fn double_size(start: f64, end: f64, step: f64) -> PhpResult<u32> {
    let calc_size = ((start - end) / step) + 1.0;
    let max = f64::from(HT_MAX_SIZE);
    if calc_size >= max {
        let exceed_by = calc_size - max;
        return Err(PhpError::value_error(format!(
            "The supplied range exceeds the maximum array size by {} elements: start={}, end={}, step={}. Max size: {}",
            printf_f(exceed_by, 1),
            printf_f(end, 1),
            printf_f(start, 1),
            printf_f(step, 1),
            printf_f(max, 0)
        )));
    }
    let size = round_half_up(calc_size) as u32;
    check_memory(size)?;
    Ok(size)
}

/// `RANGE_CHECK_LONG_INIT_ARRAY(start, end, step)`: the element count of an int range (unsigned
/// arithmetic), or the size `ValueError` (bounds printed smaller first, like the float variant).
fn long_size(start: i64, end: i64, step: u64) -> PhpResult<u32> {
    let calc_size = (start as u64).wrapping_sub(end as u64) / step;
    let max = u64::from(HT_MAX_SIZE);
    if calc_size >= max - 1 {
        let excess = calc_size - (max - 1);
        return Err(PhpError::value_error(format!(
            "The supplied range exceeds the maximum array size by {excess} elements: start={end}, end={start}, step={}. Calculated size: {calc_size}. Maximum size: {max}.",
            step as i64
        )));
    }
    let size = (calc_size + 1) as u32;
    check_memory(size)?;
    Ok(size)
}

/// Port of `range(string|int|float $start, string|int|float $end, int|float $step = 1)`
/// (PHPSRC/ext/standard/array.c, PHP 8.3+ semantics; Twig's `a..b` is `range(a, b)`).
///
/// * Arguments are parsed first (`Z_PARAM_NUMBER_OR_STR`, `Z_PARAM_NUMBER`: `null` is deprecated
///   and becomes `0`, bools become ints, arrays throw `TypeError`), then the step is checked
///   (`ValueError`s for `INF`/`NAN`, `PHP_INT_MIN` and `0`); a negative step is made positive.
/// * Strings: `""` warns and counts as `0`; numeric strings are numbers, except a single digit,
///   which is both a character and a number; other strings warn unless a single byte.
/// * Two character operands produce a character range between their first bytes (descending
///   ranges allowed, a negative step only for descending ones); a character and a number warn and
///   use the number (the character side counts as `0`); a non-integral step turns characters into
///   `0` with a warning.
/// * Numbers: any float (or a non-integral step) gives a float range whose elements are computed as
///   `start ± i * step` (not accumulated); otherwise an int range in unsigned arithmetic.
/// * `ValueError`s: `range(): Argument #3 ($step) must be greater than 0 for increasing ranges`,
///   `... must be less than the range spanned by argument #1 ($start) and argument #2 ($end)`, and
///   the `HT_MAX_SIZE` limit (`The supplied range exceeds the maximum array size by ...`).
///
/// PHP's default `memory_limit` stops ranges of more than 2^22 elements with a fatal error, which is
/// returned here as `Error: Allowed memory size of 134217728 bytes exhausted (tried to allocate N
/// bytes)`.
pub fn range(
    start: &Value,
    end: &Value,
    step: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let arg_start = Arg::new("range", 1, "start");
    let arg_end = Arg::new("range", 2, "end");
    let arg_step = Arg::new("range", 3, "step");
    let user_start = param::parse_number_or_str(start, &arg_start, diag)?;
    let user_end = param::parse_number_or_str(end, &arg_end, diag)?;
    let user_step = match step {
        Some(s) => Some(param::parse_number(s, &arg_step, diag)?),
        None => None,
    };

    let mut is_step_double = false;
    let mut is_step_negative = false;
    let mut step_double = 1.0;
    let mut step: i64 = 1;
    if let Some(user_step) = user_step {
        match user_step {
            Number::Float(d) => {
                step_double = d;
                if step_double.is_infinite() {
                    return Err(arg_step.value_error("must be a finite number, INF provided"));
                }
                if step_double.is_nan() {
                    return Err(arg_step.value_error("must be a finite number, NAN provided"));
                }
                // We only want positive step values.
                if step_double < 0.0 {
                    is_step_negative = true;
                    step_double *= -1.0;
                }
                step = convert::dval_to_lval_silent(step_double);
                if !convert::is_long_compatible(step_double, step) {
                    is_step_double = true;
                }
            }
            Number::Int(l) => {
                step = l;
                // We only want positive step values.
                if step < 0 {
                    if step == i64::MIN {
                        return Err(arg_step.value_error(&format!("must be greater than {step}")));
                    }
                    is_step_negative = true;
                    step = -step;
                }
                step_double = step as f64;
            }
        }
        if step_double == 0.0 {
            return Err(arg_step.value_error("cannot be 0"));
        }
    }

    let (mut start_type, start_long, start_double) = process_input(&user_start, &arg_start, diag)?;
    let (mut end_type, end_long, end_double) = process_input(&user_end, &arg_end, diag)?;

    let negative_step_error =
        || arg_step.value_error("must be greater than 0 for increasing ranges");
    let boundary_error = || {
        arg_step.value_error(
            "must be less than the range spanned by argument #1 ($start) and argument #2 ($end)",
        )
    };

    // If the range is given as strings, generate an array of characters.
    if start_type >= Input::Str || end_type >= Input::Str {
        if start_type < Input::Str || end_type < Input::Str {
            // One of the inputs is NOT a string nor single-byte string
            if start_type < Input::Str {
                if end_type != Input::Digit {
                    diag.warn("range(): Argument #1 ($start) must be a single byte string if argument #2 ($end) is a single byte string, argument #2 ($end) converted to 0")?;
                }
                end_type = Input::Long;
            } else if end_type < Input::Str {
                if start_type != Input::Digit {
                    diag.warn("range(): Argument #2 ($end) must be a single byte string if argument #1 ($start) is a single byte string, argument #1 ($start) converted to 0")?;
                }
                start_type = Input::Long;
            }
        } else if is_step_double {
            // Only emit warning if one of the input is not a numeric digit
            if start_type == Input::Str || end_type == Input::Str {
                diag.warn("range(): Argument #3 ($step) must be of type int when generating an array of characters, inputs converted to 0")?;
            }
            end_type = Input::Long;
            start_type = Input::Long;
        } else {
            return char_range(&user_start, &user_end, step, is_step_negative).map_err(
                |e| match e {
                    CharRangeError::Boundary => boundary_error(),
                    CharRangeError::NegativeStep => negative_step_error(),
                },
            );
        }
    }

    // handle_numeric_inputs
    if start_type == Input::Double || end_type == Input::Double || is_step_double {
        let mut out;
        if start_double > end_double {
            // Decreasing float range
            if start_double - end_double < step_double {
                return Err(boundary_error());
            }
            let size = double_size(start_double, end_double, step_double)?;
            out = Array::with_capacity(size as usize);
            let mut i: u32 = 0;
            let mut element = start_double;
            while i < size && element >= end_double {
                let _ = out.push(Value::Float(element));
                i += 1;
                element = start_double - (f64::from(i) * step_double);
            }
        } else if end_double > start_double {
            // Increasing float range
            if is_step_negative {
                return Err(negative_step_error());
            }
            if end_double - start_double < step_double {
                return Err(boundary_error());
            }
            let size = double_size(end_double, start_double, step_double)?;
            out = Array::with_capacity(size as usize);
            let mut i: u32 = 0;
            let mut element = start_double;
            while i < size && element <= end_double {
                let _ = out.push(Value::Float(element));
                i += 1;
                element = start_double + (f64::from(i) * step_double);
            }
        } else {
            out = Array::new();
            let _ = out.push(Value::Float(start_double));
        }
        Ok(out)
    } else {
        // unsigned_step is a zend_ulong so that comparisons to it don't overflow
        let unsigned_step = step as u64;
        let mut out;
        if start_long > end_long {
            // Decreasing int range
            if (start_long as u64).wrapping_sub(end_long as u64) < unsigned_step {
                return Err(boundary_error());
            }
            let size = long_size(start_long, end_long, unsigned_step)?;
            out = Array::with_capacity(size as usize);
            for i in 0..u64::from(size) {
                let v = (start_long as u64).wrapping_sub(i.wrapping_mul(unsigned_step));
                let _ = out.push(Value::Int(v as i64));
            }
        } else if end_long > start_long {
            // Increasing int range
            if is_step_negative {
                return Err(negative_step_error());
            }
            if (end_long as u64).wrapping_sub(start_long as u64) < unsigned_step {
                return Err(boundary_error());
            }
            let size = long_size(end_long, start_long, unsigned_step)?;
            out = Array::with_capacity(size as usize);
            for i in 0..u64::from(size) {
                let v = (start_long as u64).wrapping_add(i.wrapping_mul(unsigned_step));
                let _ = out.push(Value::Int(v as i64));
            }
        } else {
            out = Array::new();
            let _ = out.push(Value::Int(start_long));
        }
        Ok(out)
    }
}

/// The two `goto` targets of the character branch.
enum CharRangeError {
    Boundary,
    NegativeStep,
}

/// The character branch of `range()`: one-byte strings from the first byte of `start` to the
/// first byte of `end`, never stepping below 0 or above 255.
fn char_range(
    start: &Value,
    end: &Value,
    step: i64,
    is_step_negative: bool,
) -> Result<Array, CharRangeError> {
    let first = |v: &Value| -> i64 {
        match v {
            Value::String(s) => i64::from(s.first().copied().unwrap_or(0)),
            _ => 0,
        }
    };
    let (low, high) = (first(start), first(end));
    let byte = |c: i64| Value::String(PhpString::from(vec![c as u8]));
    let mut out;
    if low > high {
        // Decreasing char range
        if low - high < step {
            return Err(CharRangeError::Boundary);
        }
        out = Array::with_capacity(((low - high) / step + 1) as usize);
        let mut c = low;
        while c >= high {
            let _ = out.push(byte(c));
            if c - step < 0 {
                break;
            }
            c -= step;
        }
    } else if high > low {
        // Increasing char range
        if is_step_negative {
            return Err(CharRangeError::NegativeStep);
        }
        if high - low < step {
            return Err(CharRangeError::Boundary);
        }
        out = Array::with_capacity(((high - low) / step + 1) as usize);
        let mut c = low;
        while c <= high {
            let _ = out.push(byte(c));
            if c + step > 255 {
                break;
            }
            c += step;
        }
    } else {
        out = Array::new();
        let _ = out.push(byte(low));
    }
    Ok(out)
}

/// `range()` called with an argument list (the shape a compiled template's `range(...)` call
/// has): two or three positional arguments, else
/// `ArgumentCountError: range() expects at least 2 arguments, 1 given` /
/// `range() expects at most 3 arguments, 4 given`.
pub fn range_call(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Array> {
    match args {
        [start, end] => range(start, end, None, diag),
        [start, end, step] => range(start, end, Some(step), diag),
        _ => {
            let (word, n) = if args.len() < 2 {
                ("at least", 2)
            } else {
                ("at most", 3)
            };
            Err(PhpError::argument_count_error(format!(
                "range() expects {word} {n} arguments, {} given",
                args.len()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floats(a: &Array) -> Vec<f64> {
        a.values().map(|v| v.as_float().unwrap()).collect()
    }

    #[test]
    fn examples() {
        // reference/php/stdlib.md §10.1, verified with PHP 8.5.13
        let mut d = Vec::new();
        let r = range(
            &Value::Int(1),
            &Value::Int(2),
            Some(&Value::Float(0.3)),
            &mut d,
        )
        .unwrap();
        assert_eq!(floats(&r), [1.0, 1.3, 1.6, 1.9]);
        let r = range(
            &Value::from("A"),
            &Value::from("z"),
            Some(&Value::Int(10)),
            &mut d,
        )
        .unwrap();
        let s: Vec<String> = r
            .values()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(s, ["A", "K", "U", "_", "i", "s"]);
        assert!(d.is_empty());
        let e = range(&Value::Int(0), &Value::Float(1e10), None, &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "The supplied range exceeds the maximum array size by 8926258177.0 elements: start=0.0, end=10000000000.0, step=1.0. Max size: 1073741824"
        );
        let e = range(&Value::Int(i64::MAX), &Value::Int(0), None, &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "The supplied range exceeds the maximum array size by 9223372035781033984 elements: start=0, end=9223372036854775807, step=1. Calculated size: 9223372036854775807. Maximum size: 1073741824."
        );
        let r = range(&Value::from("aa"), &Value::from("cc"), None, &mut d).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(d.len(), 2);
    }
}
