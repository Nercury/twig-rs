//! `round()`, `floor()`, `ceil()`, `abs()` (PHPSRC/ext/standard/math.c, reference/php/stdlib.md
//! §9.1-§9.3) and Twig's `round` filter (`CoreExtension::round()`).

use std::sync::LazyLock;

use super::{check_arg_count, conv, twig_runtime_error};
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::math as cmath;
use crate::numeric;
use crate::object::{ClassBuilder, ClassRef};
use crate::ops;
use crate::param::{self, Arg};
use crate::value::{Number, Value};

/// PHP's rounding modes: the `PHP_ROUND_*` constants (PHPSRC/ext/standard/php_math_round_mode.h)
/// and the cases of the `RoundingMode` enum (PHP 8.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoundingMode {
    /// `PHP_ROUND_HALF_UP` (1), `RoundingMode::HalfAwayFromZero`: the default.
    HalfAwayFromZero,
    /// `PHP_ROUND_HALF_DOWN` (2), `RoundingMode::HalfTowardsZero`.
    HalfTowardsZero,
    /// `PHP_ROUND_HALF_EVEN` (3), `RoundingMode::HalfEven`.
    HalfEven,
    /// `PHP_ROUND_HALF_ODD` (4), `RoundingMode::HalfOdd`.
    HalfOdd,
    /// `PHP_ROUND_CEILING` (5), `RoundingMode::PositiveInfinity`.
    PositiveInfinity,
    /// `PHP_ROUND_FLOOR` (6), `RoundingMode::NegativeInfinity`.
    NegativeInfinity,
    /// `PHP_ROUND_TOWARD_ZERO` (7), `RoundingMode::TowardsZero`.
    TowardsZero,
    /// `PHP_ROUND_AWAY_FROM_ZERO` (8), `RoundingMode::AwayFromZero`.
    AwayFromZero,
}

impl RoundingMode {
    /// The mode for a `PHP_ROUND_*` value (1-8).
    pub fn from_php(mode: i64) -> Option<RoundingMode> {
        Some(match mode {
            1 => RoundingMode::HalfAwayFromZero,
            2 => RoundingMode::HalfTowardsZero,
            3 => RoundingMode::HalfEven,
            4 => RoundingMode::HalfOdd,
            5 => RoundingMode::PositiveInfinity,
            6 => RoundingMode::NegativeInfinity,
            7 => RoundingMode::TowardsZero,
            8 => RoundingMode::AwayFromZero,
            _ => return None,
        })
    }

    /// The `PHP_ROUND_*` value.
    pub fn to_php(self) -> i64 {
        match self {
            RoundingMode::HalfAwayFromZero => 1,
            RoundingMode::HalfTowardsZero => 2,
            RoundingMode::HalfEven => 3,
            RoundingMode::HalfOdd => 4,
            RoundingMode::PositiveInfinity => 5,
            RoundingMode::NegativeInfinity => 6,
            RoundingMode::TowardsZero => 7,
            RoundingMode::AwayFromZero => 8,
        }
    }

    /// Port of `php_math_round_mode_from_enum()` (PHPSRC/ext/standard/math.c): the mode for a
    /// `RoundingMode` case name, chosen like upstream by `name[0] + name[4]`.
    pub fn from_case_name(name: &str) -> Option<RoundingMode> {
        let b = name.as_bytes();
        if b.len() < 5 {
            return None;
        }
        Some(match (b[0], b[4]) {
            (b'H', b'A') => RoundingMode::HalfAwayFromZero,
            (b'H', b'T') => RoundingMode::HalfTowardsZero,
            (b'H', b'E') => RoundingMode::HalfEven,
            (b'H', b'O') => RoundingMode::HalfOdd,
            (b'T', b'r') => RoundingMode::TowardsZero,
            (b'A', b'F') => RoundingMode::AwayFromZero,
            (b'N', b't') => RoundingMode::NegativeInfinity,
            (b'P', b't') => RoundingMode::PositiveInfinity,
            _ => return None,
        })
    }
}

static ROUNDING_MODE: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::enumeration("RoundingMode", None)
        .case("HalfAwayFromZero", None)
        .case("HalfTowardsZero", None)
        .case("HalfEven", None)
        .case("HalfOdd", None)
        .case("TowardsZero", None)
        .case("AwayFromZero", None)
        .case("NegativeInfinity", None)
        .case("PositiveInfinity", None)
        .build()
});

/// The built-in `enum RoundingMode` (PHP 8.4, PHPSRC/ext/standard/basic_functions.stub.php) with
/// its eight cases in declaration order; [`round`] accepts its cases as `$mode`. Register it in
/// the host's class table to make `enum('RoundingMode')` & co. resolve.
pub fn rounding_mode_class() -> ClassRef {
    ROUNDING_MODE.clone()
}

/// Port of `php_intpow10()`: exact powers of ten from a table up to 1e22, `pow(10.0, power)`
/// beyond (glibc's `pow`, [`crate::math::pow`]).
fn intpow10(power: i32) -> f64 {
    const POWERS: [f64; 23] = [
        1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
        1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
    ];
    // Not in lookup table
    if !(0..=22).contains(&power) {
        return cmath::pow(10.0, f64::from(power));
    }
    POWERS[power as usize]
}

/// `php_round_get_basic_edge_case()`
fn basic_edge_case(integral: f64, exponent: f64, places: i32) -> f64 {
    if places > 0 {
        ((integral + 0.5f64.copysign(integral)) / exponent).abs()
    } else {
        ((integral + 0.5f64.copysign(integral)) * exponent).abs()
    }
}

/// `php_round_get_zero_edge_case()`
fn zero_edge_case(integral: f64, exponent: f64, places: i32) -> f64 {
    if places > 0 {
        (integral / exponent).abs()
    } else {
        (integral * exponent).abs()
    }
}

/// Port of `php_round_helper()` (PHPSRC/ext/standard/math.c): rounds the pre-rounded integral
/// part by comparing `value` with the edge between it and its neighbour.
fn round_helper(integral: f64, value: f64, exponent: f64, places: i32, mode: RoundingMode) -> f64 {
    let value_abs = value.abs();
    match mode {
        RoundingMode::HalfAwayFromZero => {
            let edge_case = basic_edge_case(integral, exponent, places);
            if value_abs >= edge_case {
                // We must increase the magnitude of the integral part (rounding up / towards
                // infinity).
                return integral + 1.0f64.copysign(integral);
            }
            integral
        }
        RoundingMode::HalfTowardsZero => {
            let edge_case = basic_edge_case(integral, exponent, places);
            if value_abs > edge_case {
                return integral + 1.0f64.copysign(integral);
            }
            integral
        }
        RoundingMode::PositiveInfinity => {
            let edge_case = zero_edge_case(integral, exponent, places);
            if value > 0.0 && value_abs > edge_case {
                return integral + 1.0;
            }
            integral
        }
        RoundingMode::NegativeInfinity => {
            let edge_case = zero_edge_case(integral, exponent, places);
            if value < 0.0 && value_abs > edge_case {
                return integral - 1.0;
            }
            integral
        }
        RoundingMode::TowardsZero => integral,
        RoundingMode::AwayFromZero => {
            let edge_case = zero_edge_case(integral, exponent, places);
            if value_abs > edge_case {
                return integral + 1.0f64.copysign(integral);
            }
            integral
        }
        RoundingMode::HalfEven => {
            let edge_case = basic_edge_case(integral, exponent, places);
            if value_abs > edge_case {
                return integral + 1.0f64.copysign(integral);
            } else if value_abs == edge_case {
                let even = cmath::fmod(integral, 2.0) == 0.0;
                // If the integral part is not even we can make it even by adding one in the
                // direction of the existing sign.
                if !even {
                    return integral + 1.0f64.copysign(integral);
                }
            }
            integral
        }
        RoundingMode::HalfOdd => {
            let edge_case = basic_edge_case(integral, exponent, places);
            if value_abs > edge_case {
                return integral + 1.0f64.copysign(integral);
            } else if value_abs == edge_case {
                let even = cmath::fmod(integral, 2.0) == 0.0;
                if even {
                    return integral + 1.0f64.copysign(integral);
                }
            }
            integral
        }
    }
}

/// Port of `_php_math_round()` (PHPSRC/ext/standard/math.c, PHP 8.4+ algorithm): rounds `value`
/// to `places` decimal places (negative: to tens, hundreds, ...) in `mode`.
///
/// Non-finite values and zero are returned as is, as are values whose scaled magnitude reaches
/// 1e16. The integral part is "pre-rounded" (`floor`/`ceil` of `value * 10^places`, corrected
/// when the next integer converts back to exactly `value`), then [`RoundingMode`] decides with
/// an edge comparison. Up to 22 places the result is a division/multiplication by an exact power
/// of ten; beyond, upstream prints `"%15fe%d"` and parses it back with `zend_strtod()`, which
/// is reproduced here (including its sign loss for negative zero).
pub fn math_round(value: f64, places: i32, mode: RoundingMode) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }

    if places == 0 && value == value.trunc() {
        return value;
    }

    let places = places.max(i32::MIN + 1);

    let exponent = intpow10(places.abs());

    // When extracting the integer part, the result may be incorrect as a decimal number due to
    // floating point errors (0.285 * 10000000000 => 2849999999.9999995): add 1 to the absolute
    // value, scale it back, and use it when it equals the value.
    let scaled = if places > 0 {
        value * exponent
    } else {
        value / exponent
    };
    let (mut tmp_value, tmp_value2) = if value >= 0.0 {
        let t = cmath::floor(scaled);
        (t, t + 1.0)
    } else {
        let t = cmath::ceil(scaled);
        (t, t - 1.0)
    };

    if (if places > 0 {
        tmp_value2 / exponent
    } else {
        tmp_value2 * exponent
    }) == value
    {
        tmp_value = tmp_value2;
    }

    // This value is beyond our precision, so rounding it is pointless
    if tmp_value.abs() >= 1e16 {
        return value;
    }

    // round the temp value
    tmp_value = round_helper(tmp_value, value, exponent, places, mode);

    // see if it makes sense to use simple division to round the value
    if places.abs() < 23 {
        if places > 0 {
            tmp_value /= exponent;
        } else {
            tmp_value *= exponent;
        }
    } else {
        // Simple division can't be used since that will cause wrong results. Instead, the number
        // is converted to a string and back again using strtod(): snprintf(buf, 39, "%15fe%d",
        // tmp_value, -places).
        let mut buf = conv::format_f(tmp_value, 6, 15, true);
        buf.push(b'e');
        buf.extend_from_slice((-places).to_string().as_bytes());
        buf.truncate(38);
        tmp_value = numeric::strtod(&buf).0;
        // couldn't convert to string and back
        if !tmp_value.is_finite() {
            tmp_value = value;
        }
    }
    tmp_value
}

/// Port of `PHP_FUNCTION(round)` (`round(int|float $num, int $precision = 0, int|RoundingMode
/// $mode = RoundingMode::HalfAwayFromZero): float`). `None` is an omitted argument.
///
/// `$mode` accepts a `RoundingMode` case or an int (`must be of type RoundingMode|int`); ints
/// outside 1-8 throw `ValueError: round(): Argument #3 ($mode) must be a valid rounding mode
/// (RoundingMode::*)`. Ints with a non-negative precision are returned as floats unchanged.
pub fn round(
    num: &Value,
    precision: Option<&Value>,
    mode: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<f64> {
    let value = param::parse_number(num, &Arg::new("round", 1, "num"), diag)?;
    let precision = match precision {
        Some(v) => param::parse_long(v, &Arg::new("round", 2, "precision"), diag)?,
        None => 0,
    };
    let mode_arg = Arg::new("round", 3, "mode").declared("RoundingMode|int");
    let mode = match mode {
        None => Some(RoundingMode::HalfAwayFromZero),
        // Z_PARAM_OBJ_OF_CLASS_OR_LONG
        Some(Value::Object(o)) if o.instance_of("RoundingMode") => {
            o.enum_case_name().and_then(RoundingMode::from_case_name)
        }
        Some(v) => {
            let l = if param::accepts_weak(v, crate::value::TypeTag::Long) {
                param::parse_long(v, &mode_arg, diag)?
            } else {
                return Err(mode_arg.type_error("of type RoundingMode|int", v));
            };
            RoundingMode::from_php(l)
        }
    };
    let Some(mode) = mode else {
        return Err(mode_arg.value_error("must be a valid rounding mode (RoundingMode::*)"));
    };
    // ZEND_NUM_ARGS() >= 2: clamp the precision to the C int range.
    let places = precision.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;

    Ok(match value {
        // Simple case - long that doesn't need to be rounded.
        Number::Int(l) if places >= 0 => l as f64,
        n => math_round(n.to_f64(), places, mode),
    })
}

/// [`round`] called with an argument list: one to three arguments.
pub fn round_call(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    check_arg_count("round", args, 1, Some(3))?;
    round(&args[0], args.get(1), args.get(2), diag).map(Value::Float)
}

/// Port of `PHP_FUNCTION(floor)` (`floor(int|float $num): float`): ints are converted to float,
/// floats rounded down with the C library's `floor()`.
pub fn floor(num: &Value, diag: &mut dyn Diagnostics) -> PhpResult<f64> {
    Ok(
        match param::parse_number(num, &Arg::new("floor", 1, "num"), diag)? {
            Number::Int(l) => l as f64,
            Number::Float(d) => cmath::floor(d),
        },
    )
}

/// [`floor`] called with an argument list: exactly one argument.
pub fn floor_call(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    check_arg_count("floor", args, 1, Some(1))?;
    floor(&args[0], diag).map(Value::Float)
}

/// Port of `PHP_FUNCTION(ceil)` (`ceil(int|float $num): float`): ints are converted to float,
/// floats rounded up with the C library's `ceil()` (`ceil(-0.5)` is `-0.0`).
pub fn ceil(num: &Value, diag: &mut dyn Diagnostics) -> PhpResult<f64> {
    Ok(
        match param::parse_number(num, &Arg::new("ceil", 1, "num"), diag)? {
            Number::Int(l) => l as f64,
            Number::Float(d) => cmath::ceil(d),
        },
    )
}

/// [`ceil`] called with an argument list: exactly one argument.
pub fn ceil_call(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    check_arg_count("ceil", args, 1, Some(1))?;
    ceil(&args[0], diag).map(Value::Float)
}

/// Port of `PHP_FUNCTION(abs)` (`abs(int|float $num): int|float`, the `abs` filter): ints stay
/// ints except `PHP_INT_MIN`, which becomes the float `9.2233720368547758E+18`; floats lose their
/// sign bit.
pub fn abs(num: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Number> {
    Ok(
        match param::parse_number(num, &Arg::new("abs", 1, "num"), diag)? {
            Number::Int(i64::MIN) => Number::Float(-(i64::MIN as f64)),
            Number::Int(l) => Number::Int(l.abs()),
            Number::Float(d) => Number::Float(d.abs()),
        },
    )
}

/// [`abs`] called with an argument list (the `abs` filter is the bare PHP function, so extra
/// filter arguments reach it): exactly one argument, else `ArgumentCountError: abs() expects
/// exactly 1 argument, 2 given`.
pub fn abs_call(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    check_arg_count("abs", args, 1, Some(1))?;
    abs(&args[0], diag).map(Value::from)
}

/// Twig: `CoreExtension::round($value, $precision = 0, $method = 'common')`
/// (TWIG/src/Extension/CoreExtension.php), the `round` filter.
///
/// `$value` is cast with `(float)`; `'common'` calls PHP's `round($value, $precision)`;
/// `'ceil'`/`'floor'` compute `$method($value * 10 ** $precision) / 10 ** $precision` with PHP's
/// operators (so `$precision` follows the `**` operand rules); any other method (compared with
/// `===`) throws `Twig\Error\RuntimeError: The "round" filter only supports the "common",
/// "ceil", and "floor" methods.`
pub fn twig_round(
    value: &Value,
    precision: &Value,
    method: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let value = Value::Float(convert::to_double(value, diag)?);
    let method = match method {
        Value::String(s) => s.as_bytes(),
        _ => b"",
    };

    if method == b"common" {
        return round(&value, Some(precision), None, diag).map(Value::Float);
    }

    if method != b"ceil" && method != b"floor" {
        return Err(twig_runtime_error(
            "The \"round\" filter only supports the \"common\", \"ceil\", and \"floor\" methods.",
        ));
    }

    let scale = ops::pow(&Value::Int(10), precision, diag)?;
    let scaled = ops::mul(&value, &scale, diag)?;
    let rounded = if method == b"ceil" {
        ceil(&scaled, diag)?
    } else {
        floor(&scaled, diag)?
    };
    let scale = ops::pow(&Value::Int(10), precision, diag)?;
    ops::div(&Value::Float(rounded), &scale, diag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Diagnostic;

    fn r(value: f64, places: i32) -> f64 {
        math_round(value, places, RoundingMode::HalfAwayFromZero)
    }

    #[test]
    fn half_up() {
        assert_eq!(r(2.5, 0), 3.0);
        assert_eq!(r(-2.5, 0), -3.0);
        assert_eq!(r(0.285, 2), 0.29);
        assert_eq!(r(1.955, 2), 1.96);
        assert_eq!(r(5.045, 2), 5.05);
        assert_eq!(r(5.055, 2), 5.06);
        assert_eq!(r(1.45, 1), 1.5);
        assert_eq!(r(1234567.891, -3), 1235000.0);
        assert_eq!(r(0.1 + 0.2, 15), 0.3);
        assert_eq!(r(1.23456789, 25), 1.23456789);
        assert_eq!(r(123456.0, -30), 0.0);
        assert_eq!(r(1.5e-20, 25), 1.5e-20);
        assert!(r(-1.0, -400).is_sign_positive());
        assert!(r(-1.0, -30).is_sign_positive());
        assert!(r(-1.0, -10).is_sign_negative());
    }

    #[test]
    fn modes() {
        let m = |v: f64, mode| math_round(v, 0, mode);
        assert_eq!(m(2.5, RoundingMode::HalfEven), 2.0);
        assert_eq!(m(3.5, RoundingMode::HalfEven), 4.0);
        assert_eq!(m(2.5, RoundingMode::HalfOdd), 3.0);
        assert_eq!(m(2.5, RoundingMode::HalfTowardsZero), 2.0);
        assert_eq!(m(-2.1, RoundingMode::NegativeInfinity), -3.0);
        assert_eq!(m(-2.9, RoundingMode::PositiveInfinity), -2.0);
        assert_eq!(m(2.1, RoundingMode::AwayFromZero), 3.0);
        assert_eq!(m(2.9, RoundingMode::TowardsZero), 2.0);
        assert_eq!(
            RoundingMode::from_case_name("NegativeInfinity"),
            Some(RoundingMode::NegativeInfinity)
        );
        assert_eq!(RoundingMode::from_php(9), None);
    }

    #[test]
    fn functions() {
        let mut d: Vec<Diagnostic> = Vec::new();
        let int = Value::Int;
        assert_eq!(round(&int(35), Some(&int(-1)), None, &mut d).unwrap(), 40.0);
        assert_eq!(round_call(&[int(42)], &mut d).unwrap(), Value::Float(42.0));
        let e = round(
            &Value::Float(1.5),
            Some(&int(0)),
            Some(&Value::from("x")),
            &mut d,
        )
        .unwrap_err();
        assert_eq!(
            e.message(),
            "round(): Argument #3 ($mode) must be of type RoundingMode|int, string given"
        );
        let e = round(&Value::Float(1.5), Some(&int(0)), Some(&int(9)), &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "round(): Argument #3 ($mode) must be a valid rounding mode (RoundingMode::*)"
        );
        let half_even = Value::Object(rounding_mode_class().enum_case("HalfEven").unwrap());
        assert_eq!(
            round(&Value::Float(2.5), Some(&int(0)), Some(&half_even), &mut d).unwrap(),
            2.0
        );
        assert_eq!(
            abs(&int(i64::MIN), &mut d).unwrap(),
            Number::Float(2f64.powi(63))
        );
        assert_eq!(abs(&Value::from("-3"), &mut d).unwrap(), Number::Int(3));
        assert!(
            ceil(&Value::Float(-0.5), &mut d)
                .unwrap()
                .is_sign_negative()
        );
        let e = abs_call(&[int(1), int(2)], &mut d).unwrap_err();
        assert_eq!(e.message(), "abs() expects exactly 1 argument, 2 given");
        assert!(d.is_empty());
    }

    #[test]
    fn twig_filter() {
        let mut d: Vec<Diagnostic> = Vec::new();
        let t = |v: f64, p: i64, m: &str, d: &mut Vec<Diagnostic>| {
            twig_round(&Value::Float(v), &Value::Int(p), &Value::from(m), d).unwrap()
        };
        assert_eq!(t(1.255, 2, "common", &mut d), Value::Float(1.26));
        assert_eq!(t(1.255, 2, "floor", &mut d), Value::Float(1.25));
        assert_eq!(t(1.255, 2, "ceil", &mut d), Value::Float(1.26));
        let e = twig_round(
            &Value::Float(1.0),
            &Value::Int(0),
            &Value::from("x"),
            &mut d,
        )
        .unwrap_err();
        assert_eq!(e.class().name(), "Twig\\Error\\RuntimeError");
        assert_eq!(
            e.message(),
            "The \"round\" filter only supports the \"common\", \"ceil\", and \"floor\" methods."
        );
    }
}
