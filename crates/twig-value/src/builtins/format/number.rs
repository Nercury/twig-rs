//! `number_format()`: ports of `PHP_FUNCTION(number_format)`, `_php_math_number_format_ex()` and
//! `_php_math_number_format_long()` (PHPSRC/ext/standard/math.c), reference/php/stdlib.md §5.2.

use super::math::{RoundingMode, math_round};
use super::{check_alloc, check_arg_count, conv};
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::{Number, Value};

/// Port of `PHP_FUNCTION(number_format)` (`number_format(float $num, int $decimals = 0,
/// ?string $decimal_separator = ".", ?string $thousands_separator = ","): string`). `None` is an
/// omitted argument.
///
/// `$num` is parsed as `int|float` (`Z_PARAM_NUMBER`; the `null` deprecation names the stub type
/// `float`), `$decimals` as `int`, the separators as `?string` (`null` = default). Ints and
/// floats of magnitude >= 2^52 that fit an int take the exact integer path
/// ([`math_number_format_long`]), other floats [`math_number_format`]. Twig's `number_format`
/// filter is `number_format((float) $number, $decimal, $decimalPoint, $thousandSep)` with the
/// environment's defaults filled in.
pub fn number_format(
    num: &Value,
    decimals: Option<&Value>,
    decimal_separator: Option<&Value>,
    thousands_separator: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let num = param::parse_number(
        num,
        &Arg::new("number_format", 1, "num").declared("float"),
        diag,
    )?;
    let dec = match decimals {
        Some(v) => param::parse_long(v, &Arg::new("number_format", 2, "decimals"), diag)?,
        None => 0,
    };
    let dec_point = match decimal_separator {
        Some(v) => {
            param::parse_str_or_null(v, &Arg::new("number_format", 3, "decimal_separator"), diag)?
        }
        None => None,
    };
    let thousand_sep = match thousands_separator {
        Some(v) => param::parse_str_or_null(
            v,
            &Arg::new("number_format", 4, "thousands_separator"),
            diag,
        )?,
        None => None,
    };
    let dec_point: &[u8] = dec_point.as_deref().unwrap_or(b".");
    let thousand_sep: &[u8] = thousand_sep.as_deref().unwrap_or(b",");

    match num {
        Number::Int(l) => math_number_format_long(l, dec, dec_point, thousand_sep),
        Number::Float(d) => {
            // double values of >= 2^52 can not have fractional digits anymore
            // Casting to long on 64bit will not loose precision on rounding
            if (d >= 4503599627370496.0 || d <= -4503599627370496.0) && convert::double_fits_long(d)
            {
                math_number_format_long(d as i64, dec, dec_point, thousand_sep)
            } else {
                let dec_int = dec.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                math_number_format(d, dec_int, dec_point, thousand_sep)
            }
        }
    }
}

/// [`number_format`] called with an argument list: one to four arguments.
pub fn number_format_call(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    check_arg_count("number_format", args, 1, Some(4))?;
    number_format(&args[0], args.get(1), args.get(2), args.get(3), diag).map(Value::String)
}

/// Copies `digits` (the integer part) into `out` with `thousand_sep` before every group of three
/// digits counted from the right: the backwards copy loop of upstream, run forwards.
fn push_grouped(out: &mut Vec<u8>, digits: &[u8], thousand_sep: &[u8]) {
    let n = digits.len();
    for (i, &c) in digits.iter().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.extend_from_slice(thousand_sep);
        }
        out.push(c);
    }
}

/// Port of `_php_math_number_format_ex()` (PHPSRC/ext/standard/math.c).
///
/// The absolute value is rounded half up to `dec` places ([`math_round`], negative `dec` rounds
/// to tens, hundreds, ...), printed with `"%.*F"` (PHP's own printf: correctly rounded digits,
/// at most 318 of them, the rest padded with zeros), grouped by `thousand_sep` and joined with
/// `dec_point` (both may be empty or multi-byte). A value that rounds to zero loses its sign
/// (never `-0`); NaN and infinities print as `nan`/`inf` (no sign).
pub fn math_number_format(
    d: f64,
    dec: i32,
    dec_point: &[u8],
    thousand_sep: &[u8],
) -> PhpResult<PhpString> {
    let mut d = d;
    let mut is_negative = false;
    if d < 0.0 {
        is_negative = true;
        d = -d;
    }

    d = math_round(d, dec, RoundingMode::HalfAwayFromZero);
    let dec = dec.max(0);
    let tmpbuf = conv::format_f(d, dec, 0, false);
    if !tmpbuf.first().is_some_and(u8::is_ascii_digit) {
        return Ok(PhpString::from(tmpbuf));
    }

    // Check if the number is no longer negative after rounding
    if is_negative && d == 0.0 {
        is_negative = false;
    }

    // find decimal point, if expected
    let dp = if dec != 0 {
        tmpbuf.iter().position(|&c| c == b'.' || c == b',')
    } else {
        None
    };

    // calculate the length of the return buffer
    let integral = dp.unwrap_or(tmpbuf.len());
    let mut reslen = integral + (integral - 1) / 3 * thousand_sep.len();
    if dec != 0 {
        reslen += dec as usize + dec_point.len();
    }
    if is_negative {
        reslen += 1;
    }
    check_alloc(reslen)?;

    let mut res = Vec::with_capacity(reslen);
    // and a minus sign, if needed
    if is_negative {
        res.push(b'-');
    }
    // copy the numbers before the decimal point, adding thousand separator every three digits
    push_grouped(&mut res, &tmpbuf[..integral], thousand_sep);
    // copy the decimal places. Take care, as the sprintf implementation may return less places
    // than we requested due to internal buffer limitations
    if dec != 0 {
        res.extend_from_slice(dec_point);
        let decimals = match dp {
            Some(dp) => &tmpbuf[dp + 1..],
            None => &[][..],
        };
        res.extend_from_slice(decimals);
        // pad with '0's
        let topad = (dec as usize).saturating_sub(decimals.len());
        res.extend(std::iter::repeat_n(b'0', topad));
    }
    Ok(PhpString::from(res))
}

/// Port of `_php_math_number_format_long()` (PHPSRC/ext/standard/math.c): exact integer
/// formatting. A negative `dec` rounds the magnitude half up to a multiple of `10^-dec` (to 0
/// below -19), never giving `-0`; a positive `dec` appends `dec_point` and `dec` zeros.
pub fn math_number_format_long(
    num: i64,
    dec: i64,
    dec_point: &[u8],
    thousand_sep: &[u8],
) -> PhpResult<PhpString> {
    const POWERS: [u64; 20] = [
        1,
        10,
        100,
        1000,
        10000,
        100000,
        1000000,
        10000000,
        100000000,
        1000000000,
        10000000000,
        100000000000,
        1000000000000,
        10000000000000,
        100000000000000,
        1000000000000000,
        10000000000000000,
        100000000000000000,
        1000000000000000000,
        10000000000000000000,
    ];

    // unsigned absolute number and memorize negative sign
    let mut is_negative = num < 0;
    let mut tmpnum = num.unsigned_abs();

    // rounding the number
    if dec < 0 {
        // Check rounding to more negative places than possible
        if dec < -(POWERS.len() as i64 - 1) {
            tmpnum = 0;
        } else {
            let power = POWERS[dec.unsigned_abs() as usize];
            let power_half = power / 2;
            let rest = tmpnum % power;
            tmpnum /= power;
            tmpnum = if rest >= power_half {
                tmpnum.wrapping_mul(power).wrapping_add(power)
            } else {
                tmpnum.wrapping_mul(power)
            };
        }
        // prevent resulting in negative zero
        if tmpnum == 0 {
            is_negative = false;
        }
    }

    let tmpbuf = tmpnum.to_string();
    let digits = tmpbuf.as_bytes();
    let mut reslen = digits.len() + (digits.len() - 1) / 3 * thousand_sep.len();
    reslen += usize::from(is_negative);
    if dec > 0 {
        reslen = reslen
            .saturating_add(usize::try_from(dec).unwrap_or(usize::MAX))
            .saturating_add(dec_point.len());
    }
    check_alloc(reslen)?;

    let mut res = Vec::with_capacity(reslen);
    if is_negative {
        res.push(b'-');
    }
    push_grouped(&mut res, digits, thousand_sep);
    if dec > 0 {
        res.extend_from_slice(dec_point);
        res.extend(std::iter::repeat_n(b'0', dec as usize));
    }
    Ok(PhpString::from(res))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Diagnostic;

    fn nf(d: f64, dec: i32, dp: &str, ts: &str) -> String {
        math_number_format(d, dec, dp.as_bytes(), ts.as_bytes())
            .unwrap()
            .to_string()
    }

    fn call(args: &[Value]) -> String {
        let mut d: Vec<Diagnostic> = Vec::new();
        match number_format_call(args, &mut d) {
            Ok(v) => v.as_str().unwrap().to_string(),
            Err(e) => format!("{}: {}", e.class(), e.message()),
        }
    }

    #[test]
    fn floats() {
        assert_eq!(nf(1234.5, 0, ".", ","), "1,235");
        assert_eq!(nf(1234.5, 2, ".", ","), "1,234.50");
        assert_eq!(nf(0.5, 0, ".", ","), "1");
        assert_eq!(nf(1.005, 2, ".", ","), "1.01");
        assert_eq!(nf(-0.4, 0, ".", ","), "0");
        assert_eq!(nf(-0.01, 1, ".", ","), "0.0");
        assert_eq!(nf(-1234.567, 2, ",", "."), "-1.234,57");
        assert_eq!(nf(1234567.891, -3, ".", ","), "1,235,000");
        assert_eq!(nf(f64::INFINITY, 0, ".", ","), "inf");
        assert_eq!(nf(f64::NEG_INFINITY, 2, ".", ","), "inf");
        assert_eq!(nf(f64::NAN, 0, ".", ","), "nan");
        assert_eq!(nf(0.123456789, 20, ".", ","), "0.12345678899999999734");
        assert_eq!(nf(1234567.891, 2, "dp", "ts"), "1ts234ts567dp89");
        assert_eq!(nf(123456789.0, 0, "", ""), "123456789");
    }

    #[test]
    fn function() {
        assert_eq!(call(&[Value::Float(1e20)]), "100,000,000,000,000,000,000");
        assert_eq!(
            call(&[Value::Float(9007199254740993.0), Value::Int(2)]),
            "9,007,199,254,740,992.00"
        );
        assert_eq!(call(&[Value::Int(-1234), Value::Int(-2)]), "-1,200");
        assert_eq!(call(&[Value::Int(-49), Value::Int(-2)]), "0");
        assert_eq!(
            call(&[Value::Float(1234.5), Value::from("x")]),
            "TypeError: number_format(): Argument #2 ($decimals) must be of type int, string given"
        );
        assert_eq!(
            call(&[Value::from("abc")]),
            "TypeError: number_format(): Argument #1 ($num) must be of type int|float, string given"
        );
    }
}
