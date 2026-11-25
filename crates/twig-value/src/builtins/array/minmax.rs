//! `min()` and `max()`: ports of `PHP_FUNCTION(min)`, `PHP_FUNCTION(max)`, their frameless
//! two-argument variants `ZEND_FRAMELESS_FUNCTION(min, 2)` / `(max, 2)` (PHPSRC/ext/standard/array.c)
//! and `zend_hash_minmax()` (PHPSRC/Zend/zend_hash.c).
//!
//! The call shape decides the result on ties and for uncomparable pairs (reference/php/stdlib.md
//! §4.7): PHP 8.4+ compiles a direct call with exactly two arguments (`{{ min(a, b) }}` in a
//! compiled template) to the frameless variant, which returns the *second* argument on a tie for
//! `min` (`min('1.0', 1)` is `1`) and the first for `max`; calls with one array, three or more
//! arguments, or argument unpacking run the regular function, which keeps the first of equal
//! values (`min(...['1.0', 1])` is `'1.0'`).

use crate::array::Array;
use crate::compare;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::param::Arg;
use crate::value::Value;

/// Whether `l` survives the trip through a double (`zend_dval_to_lval_silent((double) l) == l`).
fn exact_double(l: i64) -> bool {
    convert::dval_to_lval_silent(l as f64) == l
}

/// Which extreme to find.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    Min,
    Max,
}

impl Pick {
    fn name(self) -> &'static str {
        match self {
            Pick::Min => "min",
            Pick::Max => "max",
        }
    }

    /// Whether `candidate` replaces `current` in the int and float fast paths (`min_lval > x`,
    /// `max_lval < x`, same for doubles: NaN never replaces and is never replaced).
    fn replaces_long(self, current: i64, candidate: i64) -> bool {
        match self {
            Pick::Min => current > candidate,
            Pick::Max => current < candidate,
        }
    }

    fn replaces_double(self, current: f64, candidate: f64) -> bool {
        match self {
            Pick::Min => current > candidate,
            Pick::Max => current < candidate,
        }
    }

    /// The generic path: `zend_compare(&args[i], current) < 0` for min, `> 0` for max.
    fn replaces_generic(self, cmp: i32) -> bool {
        match self {
            Pick::Min => cmp < 0,
            Pick::Max => cmp > 0,
        }
    }
}

/// Port of `zend_hash_minmax(ht, php_data_compare, flag)`: starting with the first element, every
/// element (the first one included) is compared as `zend_compare(result, element)`; max takes the
/// element when the result is `< 0`, min when it is `> 0`. `None` for an empty array.
fn hash_minmax(array: &Array, pick: Pick, diag: &mut dyn Diagnostics) -> PhpResult<Option<Value>> {
    let Some(mut res) = array.values().next() else {
        return Ok(None);
    };
    for v in array.values() {
        let c = compare::compare(res, v, diag)?;
        let take = match pick {
            Pick::Max => c < 0,
            Pick::Min => c > 0,
        };
        if take {
            res = v;
        }
    }
    Ok(Some(res.clone()))
}

/// The fast-path state of the variadic loops (`min_lval` / `min_dval` / generic).
#[derive(Clone, Copy)]
enum State {
    Long(i64),
    Double(f64),
    Generic,
}

/// Port of `PHP_FUNCTION(min)` / `PHP_FUNCTION(max)`.
fn minmax(args: &[Value], pick: Pick, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    let name = pick.name();
    let first = Arg::new(name, 1, "value");
    match args {
        [] => Err(PhpError::argument_count_error(format!(
            "{name}() expects at least 1 argument, 0 given"
        ))),
        // mixed min ( array $values )
        [only] => match only {
            Value::Array(a) => hash_minmax(a, pick, diag)?
                .ok_or_else(|| first.value_error("must contain at least one element")),
            other => Err(first.type_error("of type array", other)),
        },
        // mixed min ( mixed $value1 , mixed $value2 [, mixed $value3... ] )
        _ => {
            let mut best = 0;
            let mut state = match &args[0] {
                Value::Int(l) => State::Long(*l),
                Value::Float(d) => State::Double(*d),
                _ => State::Generic,
            };
            let mut i = 1;
            while i < args.len() {
                match state {
                    State::Long(lval) => match &args[i] {
                        Value::Int(x) => {
                            if pick.replaces_long(lval, *x) {
                                state = State::Long(*x);
                                best = i;
                            }
                            i += 1;
                        }
                        // if lval can be exactly represented as a double, go to double dedicated
                        // code (args[i] is compared there)
                        Value::Float(_) if exact_double(lval) => state = State::Double(lval as f64),
                        _ => state = State::Generic,
                    },
                    State::Double(dval) => match &args[i] {
                        Value::Float(x) => {
                            if pick.replaces_double(dval, *x) {
                                state = State::Double(*x);
                                best = i;
                            }
                            i += 1;
                        }
                        // if the value can be exactly represented as a double, use double
                        // dedicated code otherwise generic
                        Value::Int(x) if exact_double(*x) => {
                            if pick.replaces_double(dval, *x as f64) {
                                state = State::Double(*x as f64);
                                best = i;
                            }
                            i += 1;
                        }
                        _ => state = State::Generic,
                    },
                    State::Generic => {
                        if pick.replaces_generic(compare::compare(&args[i], &args[best], diag)?) {
                            best = i;
                        }
                        i += 1;
                    }
                }
            }
            Ok(args[best].clone())
        }
    }
}

/// Port of `ZEND_FRAMELESS_FUNCTION(min, 2)` / `(max, 2)`.
fn minmax_frameless(
    lhs: &Value,
    rhs: &Value,
    pick: Pick,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    // min: lhs < rhs ? lhs : rhs; max: lhs >= rhs ? lhs : rhs
    let take_lhs_d = |l: f64, r: f64| match pick {
        Pick::Min => l < r,
        Pick::Max => l >= r,
    };
    let take_lhs = match (lhs, rhs) {
        (Value::Int(l), Value::Int(r)) => match pick {
            Pick::Min => l < r,
            Pick::Max => l >= r,
        },
        (Value::Int(l), Value::Float(r)) if exact_double(*l) => take_lhs_d(*l as f64, *r),
        (Value::Float(l), Value::Float(r)) => take_lhs_d(*l, *r),
        (Value::Float(l), Value::Int(r)) if exact_double(*r) => take_lhs_d(*l, *r as f64),
        _ => {
            let c = compare::compare(lhs, rhs, diag)?;
            match pick {
                Pick::Min => c < 0,
                Pick::Max => c >= 0,
            }
        }
    };
    Ok(if take_lhs { lhs.clone() } else { rhs.clone() })
}

/// Port of `min(mixed $value, mixed ...$values)` as a regular call (PHPSRC/ext/standard/array.c):
/// one array argument gives its smallest element (`zend_hash_minmax()`), two or more arguments the
/// smallest argument; of equal values the first wins.
///
/// Errors: no arguments → `ArgumentCountError: min() expects at least 1 argument, 0 given`; one
/// non-array argument → `TypeError: min(): Argument #1 ($value) must be of type array, X given`;
/// an empty array → `ValueError: min(): Argument #1 ($value) must contain at least one element`.
/// Comparisons use `zend_compare()` except for int/float fast paths.
///
/// A compiled template calls this for `min(a)`, `min(a, b, c, …)` and `min(...args)`; a direct
/// two-argument call is [`min_frameless`].
pub fn min(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    minmax(args, Pick::Min, diag)
}

/// Port of `max(mixed $value, mixed ...$values)` as a regular call: like [`min`] for the largest
/// value (of equal values the first wins).
pub fn max(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    minmax(args, Pick::Max, diag)
}

/// Port of `ZEND_FRAMELESS_FUNCTION(min, 2)`: the function PHP 8.4+ runs for a direct call with
/// exactly two arguments, `zend_compare(lhs, rhs) < 0 ? lhs : rhs` (with int/float fast paths), so
/// ties and uncomparable pairs return `rhs` (`min('1.0', 1)` is `1`, `min(1, '1.0')` is `'1.0'`).
pub fn min_frameless(lhs: &Value, rhs: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    minmax_frameless(lhs, rhs, Pick::Min, diag)
}

/// Port of `ZEND_FRAMELESS_FUNCTION(max, 2)`: `zend_compare(lhs, rhs) >= 0 ? lhs : rhs`, so ties and
/// uncomparable pairs return `lhs`.
pub fn max_frameless(lhs: &Value, rhs: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    minmax_frameless(lhs, rhs, Pick::Max, diag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Value {
        Value::from(s)
    }

    #[test]
    fn tie_rules() {
        let mut d = Vec::new();
        // reference/php/stdlib.md §4.7, verified with PHP 8.5.13
        assert_eq!(
            min_frameless(&v("1.0"), &Value::Int(1), &mut d).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            min_frameless(&Value::Int(1), &v("1.0"), &mut d).unwrap(),
            v("1.0")
        );
        assert_eq!(
            min(&[v("1.0"), Value::Int(1), Value::Int(5)], &mut d).unwrap(),
            v("1.0")
        );
        assert_eq!(min(&[v("1.0"), Value::Int(1)], &mut d).unwrap(), v("1.0"));
        assert_eq!(
            max_frameless(&v("1.0"), &Value::Int(1), &mut d).unwrap(),
            v("1.0")
        );
        let nan = Value::Float(f64::NAN);
        assert_eq!(
            max_frameless(&nan, &Value::Int(1), &mut d).unwrap(),
            Value::Int(1)
        );
        assert!(
            max_frameless(&Value::Int(1), &nan, &mut d)
                .unwrap()
                .as_float()
                .unwrap()
                .is_nan()
        );
        let e = max(&[Value::Int(1)], &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "max(): Argument #1 ($value) must be of type array, int given"
        );
        let e = min(&[Value::Array(Array::new())], &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "min(): Argument #1 ($value) must contain at least one element"
        );
        assert!(d.is_empty());
    }
}
