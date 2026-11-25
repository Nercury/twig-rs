//! PHP's own `printf` float conversions (PHPSRC/main/snprintf.c): `php_conv_fp()` with its digit
//! generators `php_fcvt()`/`php_ecvt()` (`__cvt()` on top of `zend_dtoa()` modes 3 and 2), used
//! by `sprintf('%e')`/`sprintf('%f')`, by the `"%.*F"` of `number_format()` and by the `"%15f"`
//! of `round()` beyond 22 places.

use crate::float;

/// `NDIG` of PHPSRC/main/snprintf.c: precision is capped at `NDIG - 2` digits.
const NDIG: i32 = 320;

/// `zend_dtoa(value, 3, ndigits)` for a finite non-zero `value`: the decimal digits of `value`
/// correctly rounded to `ndigits` places after the decimal point (ties to even on the exact
/// binary value), without leading and trailing zeros, and dtoa's decimal exponent (value =
/// 0.d1d2… × 10^decpt). When nothing is left after rounding the digit string is empty and
/// `decpt = -ndigits` (dtoa's `no_digits` exit). Returns `(digits, decpt, sign)`.
///
/// Rust's exact fixed-point formatter produces the same correctly rounded digit string; only
/// dtoa's bookkeeping is reproduced here (verified against PHP by the sprintf and
/// number_format vectors).
fn dtoa_fixed(value: f64, ndigits: i32) -> (Vec<u8>, i32, bool) {
    let sign = value.is_sign_negative();
    let text = format!("{:.*}", ndigits.max(0) as usize, value.abs());
    let (int_part, frac_part) = text.split_once('.').unwrap_or((text.as_str(), ""));
    let mut digits: Vec<u8> = int_part.bytes().chain(frac_part.bytes()).collect();
    let mut decpt = int_part.len() as i32;
    let lead = digits.iter().take_while(|&&c| c == b'0').count();
    if lead == digits.len() {
        return (Vec::new(), -ndigits, sign);
    }
    digits.drain(..lead);
    decpt -= lead as i32;
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    (digits, decpt, sign)
}

/// Port of `__cvt()` (PHPSRC/main/snprintf.c): `ndigit` significant digits (`fmode == false`,
/// dtoa mode 2, `php_ecvt`) or `ndigit` digits after the decimal point (`fmode == true`, dtoa
/// mode 3, `php_fcvt`), padded with trailing zeros when `pad`. Returns `(digits, decpt, sign)`;
/// zero has no sign (`*sign = 0`), so `-0.0` formats like `0.0`.
fn cvt(value: f64, ndigit: i32, fmode: bool, pad: bool) -> (Vec<u8>, i32, bool) {
    let mut siz: i64 = i64::from(ndigit.unsigned_abs()) + 1;
    let decpt;
    let sign;
    let mut s: Vec<u8>;
    if value == 0.0 {
        // __dtoa() doesn't allocate space for 0 so we do it by hand
        decpt = 1 - i32::from(fmode); // 1 for 'e', 0 for 'f'
        sign = false;
        s = vec![b'0'];
        if ndigit == 0 {
            return (s, decpt, sign);
        }
    } else {
        let (p, d, sg) = if fmode {
            dtoa_fixed(value, ndigit)
        } else {
            let r = float::zend_dtoa(value, 2, ndigit);
            (r.digits, r.decpt, r.negative)
        };
        if d == 9999 {
            // Infinity or Nan, convert to inf or nan like printf
            let text: &[u8] = if p.first() == Some(&b'I') {
                b"INF"
            } else {
                b"NAN"
            };
            return (text.to_vec(), 0, sg);
        }
        decpt = d;
        sign = sg;
        // Make a local copy and adjust rve to be in terms of s
        if pad && fmode {
            siz += i64::from(decpt);
        }
        // strlcpy(s, p, siz): at most siz - 1 bytes
        let keep = p.len().min((siz - 1).max(0) as usize);
        s = p[..keep].to_vec();
    }
    // Add trailing zeros
    if pad {
        siz -= s.len() as i64;
        while {
            siz -= 1;
            siz > 0
        } {
            s.push(b'0');
        }
    }
    (s, decpt, sign)
}

/// Port of `php_conv_fp()` (PHPSRC/main/snprintf.c): formats `num` for `F` (fixed, `precision`
/// digits after `dec_point`) or `e`/`E` (`d.ddd` + exponent with explicit sign and no zero
/// padding, `+0` for a zero exponent). The sign is returned separately (`is_negative`), never
/// written. Infinity and NaN give `INF`/`NAN` (callers check for them first).
pub(super) fn conv_fp(
    format: u8,
    num: f64,
    add_dp: bool,
    precision: i32,
    dec_point: u8,
) -> (Vec<u8>, bool) {
    let precision = if precision >= NDIG - 1 {
        NDIG - 2
    } else {
        precision
    };
    let (p, mut decimal_point, is_negative) = if format == b'F' {
        cvt(num, precision, true, true)
    } else {
        // either e or E format
        cvt(num, precision + 1, false, true)
    };
    // Check for Infinity and NaN
    if p.first().is_some_and(u8::is_ascii_alphabetic) {
        return (p, false);
    }
    let mut s: Vec<u8> = Vec::with_capacity(p.len() + 8);
    let mut pi = 0usize;
    if format == b'F' {
        if decimal_point <= 0 {
            if num != 0.0 || precision > 0 {
                s.push(b'0');
                if precision > 0 {
                    s.push(dec_point);
                    while decimal_point < 0 {
                        decimal_point += 1;
                        s.push(b'0');
                    }
                } else if add_dp {
                    s.push(dec_point);
                }
            }
        } else {
            let mut addz = if decimal_point >= NDIG {
                decimal_point - NDIG + 1
            } else {
                0
            };
            decimal_point -= addz;
            while decimal_point > 0 {
                decimal_point -= 1;
                s.push(p.get(pi).copied().unwrap_or(0));
                pi += 1;
            }
            while addz > 0 {
                addz -= 1;
                s.push(b'0');
            }
            if precision > 0 || add_dp {
                s.push(dec_point);
            }
        }
    } else {
        s.push(p.first().copied().unwrap_or(b'0'));
        pi = 1;
        if precision > 0 || add_dp {
            s.push(b'.');
        }
    }
    // copy the rest of p, the NUL is NOT copied
    if pi < p.len() {
        s.extend_from_slice(&p[pi..]);
    }
    if format != b'F' {
        s.push(format); // either e or E
        decimal_point -= 1;
        if decimal_point != 0 {
            s.push(if decimal_point < 0 { b'-' } else { b'+' });
            s.extend_from_slice(decimal_point.unsigned_abs().to_string().as_bytes());
        } else {
            s.push(b'+');
            s.push(b'0');
        }
    }
    (s, is_negative)
}

/// `%[width].[precision]F` of PHP's `snprintf`/`spprintf` (`format_converter()`,
/// PHPSRC/main/snprintf.c and PHPSRC/main/spprintf.c) for finite and non-finite values without
/// flags: `nan`/`inf` (spprintf) for non-finite values, else [`conv_fp`] with a `-` prefix,
/// right-aligned with spaces to `width`.
pub(super) fn format_f(num: f64, precision: i32, width: usize, upper_special: bool) -> Vec<u8> {
    let mut s = if num.is_nan() {
        if upper_special {
            b"NAN".to_vec()
        } else {
            b"nan".to_vec()
        }
    } else if num.is_infinite() {
        if upper_special {
            b"INF".to_vec()
        } else {
            b"inf".to_vec()
        }
    } else {
        let (digits, is_negative) = conv_fp(b'F', num, false, precision, b'.');
        if is_negative {
            let mut v = Vec::with_capacity(digits.len() + 1);
            v.push(b'-');
            v.extend_from_slice(&digits);
            v
        } else {
            digits
        }
    };
    if s.len() < width {
        let mut padded = vec![b' '; width - s.len()];
        padded.append(&mut s);
        s = padded;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(num: f64, precision: i32) -> String {
        let (s, neg) = conv_fp(b'F', num, false, precision, b'.');
        format!(
            "{}{}",
            if neg { "-" } else { "" },
            String::from_utf8(s).unwrap()
        )
    }

    fn e(num: f64, precision: i32) -> String {
        let (s, neg) = conv_fp(b'e', num, false, precision, b'.');
        format!(
            "{}{}",
            if neg { "-" } else { "" },
            String::from_utf8(s).unwrap()
        )
    }

    #[test]
    fn fixed() {
        assert_eq!(f(1.5, 6), "1.500000");
        assert_eq!(f(0.0, 0), "0");
        assert_eq!(f(0.0, 2), "0.00");
        assert_eq!(f(-0.0, 2), "0.00");
        assert_eq!(f(0.4, 0), "0");
        assert_eq!(f(0.5, 0), "0");
        assert_eq!(f(0.6, 0), "1");
        assert_eq!(f(2.5, 0), "2");
        assert_eq!(f(3.5, 0), "4");
        assert_eq!(f(0.25, 1), "0.2");
        assert_eq!(f(0.04, 1), "0.0");
        assert_eq!(f(0.006, 2), "0.01");
        assert_eq!(f(100.0, 2), "100.00");
        assert_eq!(f(-2.34, 1), "-2.3");
        assert_eq!(
            f(0.1, 50),
            "0.10000000000000000555111512312578270211815834045410"
        );
        assert_eq!(f(1e20, 0), "100000000000000000000");
    }

    #[test]
    fn exponential() {
        assert_eq!(e(1234.5678, 6), "1.234568e+3");
        assert_eq!(e(12345.0, 0), "1e+4");
        assert_eq!(e(0.0, 6), "0.000000e+0");
        assert_eq!(e(-0.0, 1), "0.0e+0");
        assert_eq!(e(1.5e-7, 2), "1.50e-7");
        assert_eq!(e(-1e300, 3), "-1.000e+300");
    }

    #[test]
    fn printf_f() {
        assert_eq!(format_f(150000.0, 6, 15, true), b"  150000.000000");
        assert_eq!(format_f(-3.0, 6, 15, true), b"      -3.000000");
        assert_eq!(format_f(f64::NAN, 2, 0, false), b"nan");
        assert_eq!(format_f(f64::INFINITY, 2, 0, false), b"inf");
    }
}
