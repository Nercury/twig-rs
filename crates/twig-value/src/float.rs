//! Float to string: `zend_dtoa` digit generation and the `zend_gcvt` layout.
//!
//! reference/php/engine-semantics.md §5. PHP has two settings:
//!
//! * `precision` = 14: `zend_gcvt(d, 14, '.', 'E')` (mode 2, 14 significant digits). Used by every
//!   string conversion: `echo`, `.`, `(string)`, Twig `{{ }}` ([`double_to_string`]).
//! * `serialize_precision` = -1: `zend_gcvt(d, -1, ...)` (mode 0, shortest round-trip digits). Used by
//!   `var_dump` and engine messages (`%.*H`, [`format_serialize`]), `var_export` (adds `.0`,
//!   [`format_var_export`]) and `json_encode` (lowercase `e`, [`format_json`]).
//!
//! Digit generation follows the validated recipe of engine-semantics.md §5.5: Rust's exact
//! formatter (`{:.*e}`, correctly rounded with ties-to-even) for mode 2 plus dtoa's "small integer"
//! quirk, and Rust's shortest formatter re-rounded with ties-to-even for mode 0. Verified against
//! PHP 8.5.13 by `tests/vectors/float_format.json`.

/// The `precision` ini setting of the target (PHP built-in default).
pub const PRECISION: i32 = 14;
/// The `serialize_precision` ini setting of the target (PHP built-in default).
pub const SERIALIZE_PRECISION: i32 = -1;

/// Output of [`zend_dtoa`]: decimal digits with the decimal point position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dtoa {
    /// ASCII digits; for finite values without trailing zeros (except dtoa's small-integer tie
    /// quirk, engine-semantics.md §5.4). `"Infinity"` / `"NaN"` for non-finite values.
    pub digits: Vec<u8>,
    /// Decimal exponent: value = 0.d1d2… × 10^decpt. `9999` for infinities and NaN.
    pub decpt: i32,
    /// The sign bit of the input (also set for `-0.0` and negative NaNs).
    pub negative: bool,
}

/// Port of `zend_dtoa()` (PHPSRC/Zend/zend_strtod.c) for modes 0 and 2.
///
/// * `mode == 0`: shortest digit string that reads back as `value` (ties to even between equally
///   short candidates). `ndigits` is ignored.
/// * `mode == 2`: `max(1, ndigits)` significant digits, correctly rounded (half-even on the exact
///   binary value), trailing zeros removed.
///
/// Other modes are treated as mode 0, like upstream treats modes outside 0-9.
pub fn zend_dtoa(value: f64, mode: i32, ndigits: i32) -> Dtoa {
    let negative = value.is_sign_negative();
    if value.is_nan() {
        return Dtoa {
            digits: b"NaN".to_vec(),
            decpt: 9999,
            negative,
        };
    }
    if value.is_infinite() {
        return Dtoa {
            digits: b"Infinity".to_vec(),
            decpt: 9999,
            negative,
        };
    }
    if value == 0.0 {
        return Dtoa {
            digits: b"0".to_vec(),
            decpt: 1,
            negative,
        };
    }
    let a = value.abs();
    let (digits, exp10) = if mode == 2 {
        let n = ndigits.max(1);
        let (mut digits, exp10) = split_exp(&format!("{:.*e}", (n - 1) as usize, a));
        if !small_int_tie_keep_zeros(a, n) {
            while digits.len() > 1 && digits.last() == Some(&b'0') {
                digits.pop();
            }
        }
        (digits, exp10)
    } else {
        shortest_even(a)
    };
    Dtoa {
        digits,
        decpt: exp10 + 1,
        negative,
    }
}

/// Splits Rust's `{:e}` output (`d.ddde[-]x`) into its digits and decimal exponent.
fn split_exp(s: &str) -> (Vec<u8>, i32) {
    let (mantissa, exp) = s.split_once('e').unwrap_or((s, "0"));
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    (digits, exp.parse().unwrap_or(0))
}

/// dtoa mode 0: Rust gives the shortest length `n`; dtoa picks the candidate of that length nearest to
/// the exact value with ties to even, which is the correctly rounded `n`-digit string whenever that
/// one round-trips (engine-semantics.md §5.5).
fn shortest_even(a: f64) -> (Vec<u8>, i32) {
    let shortest = format!("{a:e}");
    let (digits, exp10) = split_exp(&shortest);
    let n = digits.len();
    let rounded = format!("{:.*e}", n - 1, a);
    let (mut digits, exp10) = if rounded.parse::<f64>().ok() == Some(a) {
        split_exp(&rounded)
    } else {
        (digits, exp10)
    };
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }
    (digits, exp10)
}

/// dtoa's "small integer" path keeps trailing zeros when an integral value below 1e15 is an exact
/// tie at the `n`-th digit and rounds down (engine-semantics.md §5.4).
fn small_int_tie_keep_zeros(a: f64, n: i32) -> bool {
    if n > 14 || a.fract() != 0.0 || a >= 1e15 {
        return false;
    }
    let s = format!("{}", a as u64);
    let b = s.as_bytes();
    let n = n as usize;
    if b.len() <= n {
        return false;
    }
    let rest = &b[n..];
    rest[0] == b'5' && rest[1..].iter().all(|&c| c == b'0') && (b[n - 1] - b'0').is_multiple_of(2)
}

/// Port of `zend_gcvt()` (PHPSRC/Zend/zend_strtod.c): `ndigit >= 0` selects mode 2 with that many
/// digits, a negative `ndigit` selects mode 0 (shortest) with an exponential threshold of 17.
///
/// Exponential form (`d.dddE+x`, mantissa always with a `.`, exponent signed and unpadded) is used
/// when the decimal exponent exceeds `ndigit` or is below -3. Infinities and NaN print as
/// `INF`/`-INF`/`NAN`, truncated to `ndigit` characters like upstream's `snprintf(buf, ndigit + 1)`.
pub fn zend_gcvt(value: f64, ndigit: i32, dec_point: u8, exponent: u8) -> String {
    let (mode, ndigit) = if ndigit >= 0 { (2, ndigit) } else { (0, 17) };
    let d = zend_dtoa(value, mode, ndigit);
    if d.decpt == 9999 {
        let inf = d.digits[0] == b'I';
        let s = format!(
            "{}{}",
            if d.negative && inf { "-" } else { "" },
            if inf { "INF" } else { "NAN" }
        );
        let keep = s.len().min(ndigit.max(0) as usize);
        return s[..keep].to_string();
    }
    let digits = &d.digits;
    let mut decpt = d.decpt;
    let mut out: Vec<u8> = Vec::with_capacity(digits.len() + 8);
    if d.negative {
        out.push(b'-');
    }
    if (decpt >= 0 && decpt > ndigit) || decpt < -3 {
        // exponential format (e.g. 1.2345e+13)
        decpt -= 1;
        let esign = if decpt < 0 { b'-' } else { b'+' };
        let e = decpt.unsigned_abs();
        out.push(digits[0]);
        out.push(dec_point);
        if digits.len() == 1 {
            out.push(b'0');
        } else {
            out.extend_from_slice(&digits[1..]);
        }
        out.push(exponent);
        out.push(esign);
        out.extend_from_slice(e.to_string().as_bytes());
    } else if decpt < 0 {
        // standard format 0.
        out.push(b'0');
        out.push(dec_point);
        out.extend(std::iter::repeat_n(b'0', decpt.unsigned_abs() as usize));
        out.extend_from_slice(digits);
    } else {
        // standard format
        let decpt = decpt as usize;
        for i in 0..decpt {
            out.push(if i < digits.len() { digits[i] } else { b'0' });
        }
        if digits.len() > decpt {
            if decpt == 0 {
                out.push(b'0');
            }
            out.push(dec_point);
            out.extend_from_slice(&digits[decpt..]);
        }
    }
    // Only ASCII digits, signs, letters and the given separator bytes are produced.
    String::from_utf8(out).unwrap_or_default()
}

/// Port of `zend_double_to_str()` without its NaN warning (PHPSRC/Zend/zend_operators.c):
/// `zend_gcvt(num, precision ? precision : 1, '.', 'E')` with `precision` = 14.
///
/// This is the float part of every PHP string conversion; use
/// [`convert::double_to_string`](crate::convert::double_to_string) to also get the PHP 8.5
/// `unexpected NAN value was coerced to string` warning.
pub fn double_to_string(d: f64) -> String {
    zend_gcvt(d, PRECISION, b'.', b'E')
}

/// [`double_to_string`] with an explicit `precision` ini value (`0` behaves as `1`).
pub fn double_to_string_with_precision(d: f64, precision: i32) -> String {
    zend_gcvt(d, if precision == 0 { 1 } else { precision }, b'.', b'E')
}

/// The `%.*H` conversion with precision `serialize_precision` (-1) as used by `var_dump()`
/// (`float(%.*H)`) and engine messages such as
/// `The float %.*H is not representable as an int, cast occurred`
/// (PHPSRC/main/spprintf.c): NaN and infinities print as `NAN`, `INF`, `-INF`; otherwise
/// `zend_gcvt(d, -1, '.', 'E')`.
pub fn format_serialize(d: f64) -> String {
    if d.is_nan() {
        "NAN".to_string()
    } else if d.is_infinite() {
        if d > 0.0 {
            "INF".to_string()
        } else {
            "-INF".to_string()
        }
    } else {
        zend_gcvt(d, SERIALIZE_PRECISION, b'.', b'E')
    }
}

/// `var_export()` of a float: port of `smart_str_append_double(buf, d, serialize_precision, true)`
/// (PHPSRC/Zend/zend_smart_str.c): appends `.0` to finite values whose text has no `.`.
pub fn format_var_export(d: f64) -> String {
    let mut s = zend_gcvt(d, SERIALIZE_PRECISION, b'.', b'E');
    if d.is_finite() && !s.contains('.') {
        s.push_str(".0");
    }
    s
}

/// `json_encode()` of a finite float: port of `php_json_encode_double()`
/// (PHPSRC/ext/json/json_encoder.c), `zend_gcvt(d, serialize_precision, '.', 'e')`, plus `.0` when
/// `preserve_zero_fraction` (`JSON_PRESERVE_ZERO_FRACTION`) is set and the text has no `.`.
///
/// `json_encode` rejects NaN and infinities before formatting ("Inf and NaN cannot be JSON
/// encoded"); for them this returns `zend_gcvt`'s `NAN`/`INF`/`-INF`.
pub fn format_json(d: f64, preserve_zero_fraction: bool) -> String {
    let mut s = zend_gcvt(d, SERIALIZE_PRECISION, b'.', b'e');
    if preserve_zero_fraction && d.is_finite() && !s.contains('.') {
        s.push_str(".0");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precision_14() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (0.1, "0.1"),
            (100.0, "100"),
            (1e14, "1.0E+14"),
            (1e15, "1.0E+15"),
            (0.0001, "0.0001"),
            (0.00001, "1.0E-5"),
            (0.000123, "0.000123"),
            (1.5e-7, "1.5E-7"),
            (0.1 + 0.2, "0.3"),
            (1.0 / 3.0, "0.33333333333333"),
            (10.0 / 3.0, "3.3333333333333"),
            (std::f64::consts::PI, "3.1415926535898"),
            (9.223372036854776e18, "9.2233720368548E+18"),
            (1.7976931348623157e308, "1.7976931348623E+308"),
            (4.9e-324, "4.9406564584125E-324"),
            (99999999999999.99, "1.0E+14"),
            (123456789012345.0, "1.2345678901234E+14"),
            (123456789012305.0, "1.2345678901230E+14"),
            (900000000000005.0, "9.0000000000000E+14"),
            (100000000000001.0, "1.0E+14"),
            (100000000000005.0, "1.0000000000000E+14"),
            (100000000000015.0, "1.0000000000002E+14"),
            (999999999999995.0, "1.0E+15"),
            (99999999999999.0, "99999999999999"),
            (f64::NAN, "NAN"),
            (f64::INFINITY, "INF"),
            (f64::NEG_INFINITY, "-INF"),
            (1e25, "1.0E+25"),
        ];
        for &(v, want) in cases {
            assert_eq!(double_to_string(v), want, "value {v:?}");
        }
        assert_eq!(double_to_string_with_precision(120005.0, 5), "1.2000E+5");
        assert_eq!(double_to_string_with_precision(1.5, 0), "2");
        assert_eq!(double_to_string_with_precision(0.125, 2), "0.12");
        assert_eq!(double_to_string_with_precision(f64::INFINITY, 1), "I");
        assert_eq!(double_to_string_with_precision(f64::NEG_INFINITY, 2), "-I");
    }

    #[test]
    fn serialize_precision() {
        let cases: &[(f64, &str, &str, &str)] = &[
            (0.0, "0.0", "0", "0"),
            (-0.0, "-0.0", "-0", "-0"),
            (1.0, "1.0", "1", "1"),
            (
                1e15,
                "1000000000000000.0",
                "1000000000000000",
                "1000000000000000",
            ),
            (1e17, "1.0E+17", "1.0E+17", "1.0e+17"),
            (
                0.1 + 0.2,
                "0.30000000000000004",
                "0.30000000000000004",
                "0.30000000000000004",
            ),
            (
                12345678901234567.0,
                "12345678901234568.0",
                "12345678901234568",
                "12345678901234568",
            ),
            (4.9e-324, "5.0E-324", "5.0E-324", "5.0e-324"),
            // 662936471232937.25: an exact tie between two shortest candidates (dtoa picks even).
            (
                662_936_471_232_937.0 + 0.25,
                "662936471232937.2",
                "662936471232937.2",
                "662936471232937.2",
            ),
            (f64::NAN, "NAN", "NAN", "NAN"),
            (f64::NEG_INFINITY, "-INF", "-INF", "-INF"),
        ];
        for &(v, export, dump, json) in cases {
            assert_eq!(format_var_export(v), export, "value {v:?}");
            assert_eq!(format_serialize(v), dump, "value {v:?}");
            assert_eq!(format_json(v, false), json, "value {v:?}");
        }
        assert_eq!(format_json(1.0, true), "1.0");
    }
}
