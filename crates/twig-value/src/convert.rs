//! Type conversions with PHP 8.5 semantics and diagnostics.
//!
//! reference/php/engine-semantics.md §3 (casts), §9 (string conversion), §3.2.4 (implicit int
//! contexts), §6.1 (arithmetic operand conversion). Each function names the php-src routine it
//! ports; the explicit casts are `(bool)` [`to_bool`], `(int)` [`to_long`], `(float)` [`to_double`],
//! `(string)` [`to_string`], `(array)` [`to_array`], `(object)` [`to_object`].

use crate::array::Array;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::float;
use crate::numeric::{self, NumericString};
use crate::object::{CastTarget, ObjectRef, StdClass, property_entries_to_array};
use crate::string::PhpString;
use crate::value::{Number, TypeTag, Value};

/// `ZEND_DOUBLE_FITS_LONG(d)` (64-bit): `!(d >= 2^63 || d < -2^63)`; true for NaN.
pub fn double_fits_long(d: f64) -> bool {
    !(d >= 9223372036854775808.0 || d < -9223372036854775808.0)
}

/// `zend_is_long_compatible(d, l)`: `(double) l == d`.
pub fn is_long_compatible(d: f64, l: i64) -> bool {
    l as f64 == d
}

/// Port of `zend_dval_to_lval_slow()` (PHPSRC/Zend/zend_operators.c, 64-bit): the double modulo
/// 2^64, as a two's complement `zend_long`. `d` must be finite.
pub fn dval_to_lval_slow(d: f64) -> i64 {
    let two_pow_64 = 18446744073709551616.0_f64;
    let mut dmod = crate::math::fmod(d, two_pow_64);
    if dmod < 0.0 {
        // no need to call ceil; original double must have had no fractional part, hence dmod
        // does not have one either
        dmod += two_pow_64;
    }
    if dmod >= two_pow_64 {
        // unreachable for doubles (dmod is a multiple of 4096 when |d| >= 2^64); C's
        // (zend_ulong) conversion of 2^64 yields 0 on x86-64
        return 0;
    }
    (dmod as u64) as i64
}

/// `The float %.*H is not representable as an int, cast occurred` (`zend_oob_double_to_long_error`).
fn oob_double_to_long(d: f64, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    diag.warn(format!(
        "The float {} is not representable as an int, cast occurred",
        float::format_serialize(d)
    ))
}

/// Port of `zend_dval_to_lval()` (PHPSRC/Zend/zend_operators.h, PHP 8.5): truncation; NaN and
/// infinities give `0`, out-of-range values wrap modulo 2^64, both with the warning
/// `The float %s is not representable as an int, cast occurred`.
pub fn dval_to_lval(d: f64, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    if !d.is_finite() {
        oob_double_to_long(d, diag)?;
        return Ok(0);
    }
    if !double_fits_long(d) {
        oob_double_to_long(d, diag)?;
        return Ok(dval_to_lval_slow(d));
    }
    Ok(d as i64)
}

/// Port of `zend_dval_to_lval_silent()`: [`dval_to_lval`] without the warning.
pub fn dval_to_lval_silent(d: f64) -> i64 {
    if !d.is_finite() {
        0
    } else if !double_fits_long(d) {
        dval_to_lval_slow(d)
    } else {
        d as i64
    }
}

/// Port of `zend_dval_to_lval_cap()`: saturating conversion used for float-like numeric strings
/// (`(int) "1e100"` is `PHP_INT_MAX`); NaN/infinity give `0`. No diagnostics.
pub fn dval_to_lval_cap(d: f64) -> i64 {
    if !d.is_finite() {
        0
    } else if !double_fits_long(d) {
        if d > 0.0 { i64::MAX } else { i64::MIN }
    } else {
        d as i64
    }
}

/// `Implicit conversion from float %.*H to int loses precision` (`zend_incompatible_double_to_long_error`).
pub fn incompatible_double_to_long(d: f64, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    diag.deprecated(format!(
        "Implicit conversion from float {} to int loses precision",
        float::format_serialize(d)
    ))
}

/// `Implicit conversion from float-string "%s" to int loses precision`
/// (`zend_incompatible_string_to_long_error`; the C `%s` stops at a NUL byte).
pub fn incompatible_string_to_long(s: &PhpString, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    diag.deprecated(PhpString::concat(&[
        b"Implicit conversion from float-string \"",
        s.c_str(),
        b"\" to int loses precision",
    ]))
}

/// Port of `zend_dval_to_lval_safe()`: [`dval_to_lval`] plus the deprecation
/// `Implicit conversion from float %s to int loses precision` when the value is fractional (or
/// NaN) but in range. Used by implicit int contexts and array keys.
pub fn dval_to_lval_safe(d: f64, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    let l = dval_to_lval(d, diag)?;
    if !is_long_compatible(d, l) && double_fits_long(d) {
        incompatible_double_to_long(d, diag)?;
    }
    Ok(l)
}

/// `unexpected NAN value was coerced to %s` (`zend_nan_coerced_to_type_warning`, PHP 8.5).
pub fn nan_coerced(tag: TypeTag, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    diag.warn(format!(
        "unexpected NAN value was coerced to {}",
        tag.name()
    ))
}

/// `W: Object of class %s could not be converted to %s` (`convert_object_to_type`).
fn object_not_convertible(
    obj: &ObjectRef,
    target: CastTarget,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    diag.warn(format!(
        "Object of class {} could not be converted to {}",
        obj.class().display_name(),
        target.type_name()
    ))
}

/// Port of `i_zend_is_true()` (PHPSRC/Zend/zend_operators.h): PHP truthiness (`if`, `!`, `&&`,
/// `(bool)`). `""` and `"0"` are the only falsy strings; NaN is truthy and warns
/// `unexpected NAN value was coerced to bool`; objects are true unless their `cast_object`
/// handler says otherwise.
pub fn to_bool(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    Ok(match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(d) => {
            if d.is_nan() {
                nan_coerced(TypeTag::True, diag)?;
            }
            *d != 0.0
        }
        Value::String(s) => !(s.is_empty() || s.as_bytes() == b"0"),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => match o.cast(CastTarget::Bool, diag)? {
            Some(Value::Bool(b)) => b,
            Some(_) => true,
            // zend_object_is_true(): E_RECOVERABLE_ERROR "Object of class %s could not be
            // converted to bool"; recoverable errors are fatal by default.
            None => {
                return Err(PhpError::error(format!(
                    "Object of class {} could not be converted to bool",
                    o.class().display_name()
                )));
            }
        },
    })
}

/// Port of `zval_get_long()` = `zval_get_long_func(op, false)` (PHPSRC/Zend/zend_operators.c): the
/// `(int)` cast and `intval()`.
///
/// Floats use [`dval_to_lval`] (warning for NaN/inf/out of range), strings are parsed silently as a
/// numeric prefix with saturation, arrays give 0/1, objects without an int cast give 1 with
/// `Object of class X could not be converted to int`.
pub fn to_long(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    to_long_ex(v, false, diag)
}

/// Port of `zval_get_long_func(op, is_strict)`: with `is_strict` fractional floats and float strings
/// additionally emit the "loses precision" deprecations.
pub fn to_long_ex(v: &Value, is_strict: bool, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    Ok(match v {
        Value::Null | Value::Bool(false) => 0,
        Value::Bool(true) => 1,
        Value::Int(i) => *i,
        Value::Float(d) => {
            let l = dval_to_lval(*d, diag)?;
            if is_strict && !is_long_compatible(*d, l) {
                incompatible_double_to_long(*d, diag)?;
            }
            l
        }
        Value::String(s) => match numeric::is_numeric_string(s, true) {
            None => 0,
            Some(Number::Int(i)) => i,
            Some(Number::Float(d)) => {
                let l = dval_to_lval_cap(d);
                if is_strict && !is_long_compatible(d, l) {
                    incompatible_string_to_long(s, diag)?;
                }
                l
            }
        },
        Value::Array(a) => i64::from(!a.is_empty()),
        // convert_object_to_type(): the warning only when the handler fails.
        Value::Object(o) => match o.cast(CastTarget::Long, diag)? {
            Some(Value::Int(i)) => i,
            Some(_) => 1,
            None => {
                object_not_convertible(o, CastTarget::Long, diag)?;
                1
            }
        },
    })
}

/// Port of `zval_get_double()` (PHPSRC/Zend/zend_operators.c): the `(float)` cast and `floatval()`.
/// Strings use `zend_strtod()` on the whole string (longest numeric prefix, never fails, no
/// diagnostics); objects without a float cast give 1.0 with
/// `Object of class X could not be converted to float`.
pub fn to_double(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<f64> {
    Ok(match v {
        Value::Null | Value::Bool(false) => 0.0,
        Value::Bool(true) => 1.0,
        Value::Int(i) => *i as f64,
        Value::Float(d) => *d,
        Value::String(s) => numeric::strtod(s).0,
        Value::Array(a) => {
            if a.is_empty() {
                0.0
            } else {
                1.0
            }
        }
        Value::Object(o) => match o.cast(CastTarget::Double, diag)? {
            Some(Value::Float(d)) => d,
            Some(_) => 1.0,
            None => {
                object_not_convertible(o, CastTarget::Double, diag)?;
                1.0
            }
        },
    })
}

/// Port of `zend_long_to_str()`: decimal representation.
pub fn long_to_string(i: i64) -> PhpString {
    PhpString::from(i.to_string())
}

/// Port of `zend_double_to_str()` (PHPSRC/Zend/zend_operators.c): `precision` = 14 formatting plus
/// the PHP 8.5 warning `unexpected NAN value was coerced to string` for NaN.
pub fn double_to_string(d: f64, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let s = PhpString::from(float::double_to_string(d));
    if d.is_nan() {
        nan_coerced(TypeTag::String, diag)?;
    }
    Ok(s)
}

/// Port of `zval_try_get_string_func()` (PHPSRC/Zend/zend_operators.c): PHP string conversion
/// (`(string)`, `echo`, `.`, Twig `{{ }}`).
///
/// `null`/`false` → `""`, `true` → `"1"`, ints in decimal, floats with precision 14 (NaN warns),
/// arrays → `"Array"` with `Array to string conversion`, objects via `__toString()` else
/// `Error: Object of class X could not be converted to string`.
pub fn to_string(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    match v {
        Value::Null | Value::Bool(false) => Ok(PhpString::empty()),
        Value::Bool(true) => Ok(PhpString::from("1")),
        Value::Int(i) => Ok(long_to_string(*i)),
        Value::Float(d) => double_to_string(*d, diag),
        Value::String(s) => Ok(s.clone()),
        Value::Array(_) => {
            diag.warn("Array to string conversion")?;
            Ok(PhpString::from("Array"))
        }
        Value::Object(o) => o.to_php_string(diag),
    }
}

/// Port of `_zendi_try_convert_scalar_to_number()` (PHPSRC/Zend/zend_operators.c), the operand
/// conversion of `+ - * / **`: `Ok(None)` means failure (the operator then throws
/// `Unsupported operand types: ...`). Leading-numeric strings warn
/// `A non-numeric value encountered`.
pub fn try_to_number(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Option<Number>> {
    Ok(match v {
        Value::Null | Value::Bool(false) => Some(Number::Int(0)),
        Value::Bool(true) => Some(Number::Int(1)),
        Value::Int(i) => Some(Number::Int(*i)),
        Value::Float(d) => Some(Number::Float(*d)),
        Value::String(s) => match numeric::is_numeric_string_ex(s, true) {
            None => None,
            Some(NumericString {
                number,
                trailing_data,
                ..
            }) => {
                if trailing_data {
                    diag.warn("A non-numeric value encountered")?;
                }
                Some(number)
            }
        },
        Value::Array(_) => None,
        Value::Object(o) => match o.cast(CastTarget::Number, diag)? {
            Some(Value::Int(i)) => Some(Number::Int(i)),
            Some(Value::Float(d)) => Some(Number::Float(d)),
            _ => None,
        },
    })
}

/// Port of `zendi_try_get_long()` (PHPSRC/Zend/zend_operators.c), the operand conversion of
/// `% << >>` and of `& | ^` when not both operands are strings. `Ok(None)` means failure
/// (`Unsupported operand types`).
///
/// Floats: [`dval_to_lval_safe`]. Strings: numeric prefix (`A non-numeric value encountered` for
/// trailing data), float strings saturate and warn `Implicit conversion from float-string "%s" to
/// int loses precision` when not integral.
pub fn try_get_long(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Option<i64>> {
    Ok(match v {
        Value::Null | Value::Bool(false) => Some(0),
        Value::Bool(true) => Some(1),
        Value::Int(i) => Some(*i),
        Value::Float(d) => Some(dval_to_lval_safe(*d, diag)?),
        Value::String(s) => match numeric::is_numeric_string_ex(s, true) {
            None => None,
            Some(NumericString {
                number,
                trailing_data,
                ..
            }) => {
                if trailing_data {
                    diag.warn("A non-numeric value encountered")?;
                }
                match number {
                    Number::Int(i) => Some(i),
                    Number::Float(d) => {
                        let l = dval_to_lval_cap(d);
                        if !is_long_compatible(d, l) {
                            incompatible_string_to_long(s, diag)?;
                        }
                        Some(l)
                    }
                }
            }
        },
        Value::Array(_) => None,
        Value::Object(o) => match o.cast(CastTarget::Long, diag)? {
            Some(Value::Int(i)) => Some(i),
            _ => None,
        },
    })
}

/// Port of `_zendi_convert_scalar_to_number_silent()` (PHPSRC/Zend/zend_operators.c), used by
/// comparisons: never fails; non-numeric strings give `0`; arrays are returned unchanged; objects
/// without a numeric cast give `1` (with the `convert_object_to_type` warning).
pub fn to_number_silent(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    Ok(match v {
        Value::Null | Value::Bool(false) => Value::Int(0),
        Value::Bool(true) => Value::Int(1),
        Value::String(s) => match numeric::is_numeric_string(s, true) {
            Some(n) => Value::from(n),
            None => Value::Int(0),
        },
        Value::Object(o) => match o.cast(CastTarget::Number, diag)? {
            Some(n @ (Value::Int(_) | Value::Float(_))) => n,
            Some(_) => Value::Int(1),
            None => {
                object_not_convertible(o, CastTarget::Number, diag)?;
                Value::Int(1)
            }
        },
        Value::Int(_) | Value::Float(_) | Value::Array(_) => v.clone(),
    })
}

/// Port of `convert_scalar_to_number()` (PHPSRC/Zend/zend_operators.c): like
/// [`to_number_silent`] but for scalars only; arrays are not numbers (they give `Number::Int`
/// of their truthiness only through [`to_long`], so this returns `None` for arrays).
pub fn convert_scalar_to_number(
    v: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<Number>> {
    Ok(match to_number_silent(v, diag)? {
        Value::Int(i) => Some(Number::Int(i)),
        Value::Float(d) => Some(Number::Float(d)),
        _ => None,
    })
}

/// Port of `convert_to_array()` (PHPSRC/Zend/zend_operators.c): the `(array)` cast.
///
/// `null` → `[]`; scalars → `[0 => value]` (NaN warns `unexpected NAN value was coerced to
/// array`); objects → their property table for [`PropertyPurpose::ArrayCast`]
/// (mangled private/protected names, numeric names as int keys); `Closure` → `[0 => closure]`.
///
/// [`PropertyPurpose::ArrayCast`]: crate::object::PropertyPurpose::ArrayCast
pub fn to_array(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Array> {
    Ok(match v {
        Value::Array(a) => a.clone(),
        Value::Null => Array::new(),
        Value::Object(o) => {
            if o.downcast_ref::<crate::object::Closure>().is_some() {
                Array::from_values([v.clone()])
            } else {
                property_entries_to_array(
                    o.properties_for(crate::object::PropertyPurpose::ArrayCast, diag)?,
                )
            }
        }
        Value::Float(d) if d.is_nan() => {
            nan_coerced(TypeTag::Array, diag)?;
            Array::from_values([v.clone()])
        }
        _ => Array::from_values([v.clone()]),
    })
}

/// Port of `convert_to_object()` (PHPSRC/Zend/zend_operators.c): the `(object)` cast.
///
/// Objects are returned as is; `null` → empty `stdClass`; arrays → `stdClass` with the elements as
/// properties (int keys become property names `"0"`, `"1"`, ...); scalars → `stdClass` with a
/// `scalar` property (NaN warns `unexpected NAN value was coerced to object`).
pub fn to_object(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
    Ok(match v {
        Value::Object(o) => o.clone(),
        Value::Null => StdClass::new().into_ref(),
        Value::Array(a) => {
            StdClass::with_properties(a.iter().map(|(k, v)| (k.to_php_string(), v.clone())))
                .into_ref()
        }
        Value::Float(d) => {
            if d.is_nan() {
                nan_coerced(TypeTag::Object, diag)?;
            }
            StdClass::with_properties([("scalar", v.clone())]).into_ref()
        }
        _ => StdClass::with_properties([("scalar", v.clone())]).into_ref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{Diagnostic, Level};

    fn d() -> Vec<Diagnostic> {
        Vec::new()
    }

    #[test]
    fn float_to_int() {
        let mut diag = d();
        assert_eq!(dval_to_lval(1e20, &mut diag).unwrap(), 7766279631452241920);
        assert_eq!(
            dval_to_lval(-1e20, &mut diag).unwrap(),
            -7766279631452241920
        );
        assert_eq!(dval_to_lval(f64::NAN, &mut diag).unwrap(), 0);
        assert_eq!(
            dval_to_lval(9.223372036854776e18, &mut diag).unwrap(),
            i64::MIN
        );
        assert_eq!(
            dval_to_lval(-9.223372036854776e18, &mut diag).unwrap(),
            i64::MIN
        );
        assert_eq!(dval_to_lval(1e19, &mut diag).unwrap(), -8446744073709551616);
        assert_eq!(dval_to_lval(18446744073709551616.0, &mut diag).unwrap(), 0);
        assert_eq!(
            dval_to_lval(18446744073709551616.0 + 8192.0, &mut diag).unwrap(),
            8192
        );
        assert_eq!(dval_to_lval(1.5, &mut diag).unwrap(), 1);
        assert_eq!(dval_to_lval(-1.5, &mut diag).unwrap(), -1);
        assert_eq!(
            diag.iter()
                .map(|d| d.message.to_string())
                .collect::<Vec<_>>(),
            vec![
                "The float 1.0E+20 is not representable as an int, cast occurred",
                "The float -1.0E+20 is not representable as an int, cast occurred",
                "The float NAN is not representable as an int, cast occurred",
                "The float 9.223372036854776E+18 is not representable as an int, cast occurred",
                "The float 1.0E+19 is not representable as an int, cast occurred",
                "The float 1.8446744073709552E+19 is not representable as an int, cast occurred",
                "The float 1.844674407370956E+19 is not representable as an int, cast occurred",
            ]
        );
        let mut diag = d();
        assert_eq!(dval_to_lval_safe(f64::NAN, &mut diag).unwrap(), 0);
        assert_eq!(diag.len(), 2);
        assert_eq!(
            diag[1],
            Diagnostic::new(
                Level::Deprecated,
                "Implicit conversion from float NAN to int loses precision"
            )
        );
    }

    #[test]
    fn casts() {
        let mut diag = d();
        let s = |x: &str| Value::from(x);
        for (input, want) in [
            ("123", 123),
            (" 123", 123),
            ("123 ", 123),
            ("12abc", 12),
            ("abc", 0),
            ("", 0),
            ("0x1A", 0),
            ("012", 12),
            ("1e3", 1000),
            ("1.9", 1),
            ("-1.9", -1),
            (".5", 0),
            ("1e100", i64::MAX),
            ("-1e100", i64::MIN),
            ("9223372036854775808", i64::MAX),
            ("1.9999999999999999", 2),
            ("1.5e3abc", 1500),
        ] {
            assert_eq!(to_long(&s(input), &mut diag).unwrap(), want, "{input:?}");
        }
        assert!(diag.is_empty());
        assert_eq!(to_double(&s("1e400"), &mut diag).unwrap(), f64::INFINITY);
        assert!(to_double(&s("-0"), &mut diag).unwrap().is_sign_negative());
        assert!(!to_bool(&s("0"), &mut diag).unwrap());
        assert!(to_bool(&s("0.0"), &mut diag).unwrap());
        assert!(to_bool(&Value::Float(f64::NAN), &mut diag).unwrap());
        assert_eq!(
            diag,
            vec![Diagnostic::new(
                Level::Warning,
                "unexpected NAN value was coerced to bool"
            )]
        );
        let mut diag = d();
        assert_eq!(to_string(&Value::Float(-0.0), &mut diag).unwrap(), "-0");
        assert_eq!(
            to_string(&Value::Array(Array::new()), &mut diag).unwrap(),
            "Array"
        );
        assert_eq!(
            diag,
            vec![Diagnostic::new(
                Level::Warning,
                "Array to string conversion"
            )]
        );
    }

    #[test]
    fn implicit_long() {
        let mut diag = d();
        assert_eq!(
            try_get_long(&Value::from("7.5x"), &mut diag).unwrap(),
            Some(7)
        );
        assert_eq!(
            diag,
            vec![
                Diagnostic::new(Level::Warning, "A non-numeric value encountered"),
                Diagnostic::new(
                    Level::Deprecated,
                    "Implicit conversion from float-string \"7.5x\" to int loses precision"
                ),
            ]
        );
        let mut diag = d();
        assert_eq!(try_get_long(&Value::from("abc"), &mut diag).unwrap(), None);
        assert_eq!(
            try_get_long(&Value::from("9.5e18"), &mut diag).unwrap(),
            Some(i64::MAX)
        );
        assert_eq!(diag.len(), 1);
        assert_eq!(
            try_to_number(&Value::from(" 1 "), &mut diag).unwrap(),
            Some(Number::Int(1))
        );
        assert_eq!(try_to_number(&Value::from("\n"), &mut diag).unwrap(), None);
    }
}
