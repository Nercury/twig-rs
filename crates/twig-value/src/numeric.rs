//! Numeric strings (`_is_numeric_string_ex`) and the float parser (`zend_strtod`).
//!
//! reference/php/engine-semantics.md §4. Both functions treat the input like the NUL-terminated C
//! string PHP passes around: reading past the end yields `0`, and an embedded NUL byte stops
//! scanning.

use crate::value::Number;

/// `MAX_LENGTH_OF_LONG` on 64-bit builds (PHPSRC/Zend/zend_long.h).
const MAX_LENGTH_OF_LONG: usize = 20;
/// `long_min_digits` (PHPSRC/Zend/zend_operators.c): the digits of `|ZEND_LONG_MIN|`.
const LONG_MIN_DIGITS: &[u8] = b"9223372036854775808";

/// The whitespace `_is_numeric_string_ex` and `zend_strtod` skip: space, `\t`, `\n`, `\r`,
/// `\v`, `\f`.
pub fn is_numeric_whitespace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

#[inline]
fn byte(s: &[u8], i: usize) -> u8 {
    if i < s.len() { s[i] } else { 0 }
}

/// Result of [`is_numeric_string_ex`] for a numeric (or leading-numeric) string.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumericString {
    /// The value: `IS_LONG` or `IS_DOUBLE`.
    pub number: Number,
    /// `oflow_info`: `1`/`-1` when an integer-looking string overflowed `zend_long` upwards /
    /// downwards (and was returned as a float), else `0`. Used by string comparison.
    pub overflow: i8,
    /// `trailing_data`: only a prefix was numeric (possible only with `allow_errors`).
    pub trailing_data: bool,
}

/// Port of `_is_numeric_string_ex()` and its inline pre-check `is_numeric_string_ex()`
/// (PHPSRC/Zend/zend_operators.c, PHPSRC/Zend/zend_operators.h).
///
/// Returns `None` when the string is not numeric. With `allow_errors == false` the whole string must
/// be numeric (leading and trailing whitespace allowed); with `allow_errors == true` a numeric prefix
/// is accepted and [`NumericString::trailing_data`] is set. The port always computes the double
/// value (`dval != NULL`), which every relevant engine call site does; the classification is the
/// same either way.
pub fn is_numeric_string_ex(s: &[u8], allow_errors: bool) -> Option<NumericString> {
    // is_numeric_string_ex(): `if (*str > '9') return 0;` (char is signed: bytes >= 0x80 pass on).
    let first = byte(s, 0);
    if first > b'9' && first < 0x80 {
        return None;
    }
    if s.is_empty() {
        return None;
    }
    let mut start = 0;
    while is_numeric_whitespace(byte(s, start)) {
        start += 1;
    }
    let mut ptr = start;
    let mut neg = false;
    if byte(s, ptr) == b'-' {
        neg = true;
        ptr += 1;
    } else if byte(s, ptr) == b'+' {
        ptr += 1;
    }

    let mut digits = 0usize;
    let mut tmp_lval: u64 = 0;
    let mut overflow: i8 = 0;
    let mut dval = 0.0;

    let is_double = if byte(s, ptr).is_ascii_digit() {
        while byte(s, ptr) == b'0' {
            ptr += 1;
        }
        let mut process_double = false;
        // for (type = IS_LONG; !(digits >= MAX_LENGTH_OF_LONG && (dval || allow_errors)); digits++, ptr++)
        loop {
            if digits >= MAX_LENGTH_OF_LONG {
                break;
            }
            let c = byte(s, ptr);
            if c.is_ascii_digit() {
                tmp_lval = tmp_lval.wrapping_mul(10).wrapping_add(u64::from(c - b'0'));
                digits += 1;
                ptr += 1;
                continue;
            } else if c == b'.' {
                // The C conditions `dp_or_e < 1` (here) and `dp_or_e < 2` (exponent) always hold:
                // `dp_or_e` only changes in the `dval == NULL` variant, which this port replaces by
                // always parsing with zend_strtod (same classification).
                process_double = true;
                break;
            } else if c == b'e' || c == b'E' {
                let mut e = ptr + 1;
                if byte(s, e) == b'-' || byte(s, e) == b'+' {
                    // `ptr = e++;` — the pointer moves onto the sign even if no digit follows
                    // (engine-semantics.md §4.5).
                    ptr = e;
                    e += 1;
                }
                if byte(s, e).is_ascii_digit() {
                    process_double = true;
                    break;
                }
            }
            break;
        }
        if !process_double && digits >= MAX_LENGTH_OF_LONG {
            // `*oflow_info = *str == '-' ? -1 : 1; goto process_double;`
            overflow = if byte(s, start) == b'-' { -1 } else { 1 };
            process_double = true;
        }
        process_double
    } else if byte(s, ptr) == b'.' && byte(s, ptr + 1).is_ascii_digit() {
        true
    } else {
        return None;
    };

    if is_double {
        // local_dval = zend_strtod(str, &ptr);
        let (v, consumed) = strtod(&s[start..]);
        dval = v;
        ptr = start + consumed;
    }

    let mut trailing_data = false;
    if ptr != s.len() {
        let mut end = ptr;
        while is_numeric_whitespace(byte(s, end)) {
            end += 1;
        }
        if end != s.len() {
            if !allow_errors {
                return None;
            }
            trailing_data = true;
        }
    }

    if !is_double {
        if digits == MAX_LENGTH_OF_LONG - 1 {
            // strcmp(&ptr[-digits], long_min_digits): compares up to the first NUL / end.
            let cmp = c_strcmp(&s[ptr - digits..], LONG_MIN_DIGITS);
            if !(cmp < 0 || (cmp == 0 && byte(s, start) == b'-')) {
                let (d, _) = strtod(&s[start..]);
                return Some(NumericString {
                    number: Number::Float(d),
                    overflow: if byte(s, start) == b'-' { -1 } else { 1 },
                    trailing_data,
                });
            }
        }
        let v = if neg {
            tmp_lval.wrapping_neg()
        } else {
            tmp_lval
        };
        return Some(NumericString {
            number: Number::Int(v as i64),
            overflow: 0,
            trailing_data,
        });
    }
    Some(NumericString {
        number: Number::Float(dval),
        overflow,
        trailing_data,
    })
}

/// C `strcmp` on NUL-terminated views of two byte slices (sign only).
fn c_strcmp(a: &[u8], b: &[u8]) -> i32 {
    let mut i = 0;
    loop {
        let ca = byte(a, i);
        let cb = byte(b, i);
        if ca != cb {
            return if ca < cb { -1 } else { 1 };
        }
        if ca == 0 {
            return 0;
        }
        i += 1;
    }
}

/// `is_numeric_string()`: [`is_numeric_string_ex`] without the overflow/trailing information.
pub fn is_numeric_string(s: &[u8], allow_errors: bool) -> Option<Number> {
    is_numeric_string_ex(s, allow_errors).map(|n| n.number)
}

/// Port of `is_numeric_str_function()` (PHPSRC/Zend/zend_operators.c): the whole string must be
/// numeric (`allow_errors = false`). Used by weak-mode int/float parameters and `is_numeric()`.
pub fn is_numeric_str_function(s: &[u8]) -> Option<Number> {
    is_numeric_string(s, false)
}

/// Port of `is_numeric($value)` (PHPSRC/ext/standard/type.c): ints and floats (NaN and infinities
/// included), and strings that are numeric as a whole (surrounding whitespace allowed).
pub fn is_numeric(v: &crate::value::Value) -> bool {
    use crate::value::Value;
    match v {
        Value::Int(_) | Value::Float(_) => true,
        Value::String(s) => is_numeric_string(s, false).is_some(),
        _ => false,
    }
}

/// Port of `zend_strtod()` (PHPSRC/Zend/zend_strtod.c, David Gay's correctly rounded strtod built
/// with `NO_HEX_FP`, no locale, no `inf`/`nan` words).
///
/// Returns the value and the number of bytes consumed (`*se - s00`). Grammar of the consumed prefix:
/// `ws* sign? ( digits ('.' digits*)? | '.' digits ) ([eE] sign? digits)?`; whitespace is only skipped
/// before the sign. Without any mantissa digit nothing is consumed and the result is `0.0`. The
/// literal exponent is clamped to ±19999 like upstream (more than 8 significant exponent digits or a
/// value above 19999). The digits are then converted with Rust's correctly rounded parser, which
/// matches Gay's algorithm (validated on 59,294 strings, engine-semantics.md §4.4).
pub fn strtod(s: &[u8]) -> (f64, usize) {
    let mut i = 0;
    let mut neg = false;
    // for(s = s00;;s++) switch(*s)
    loop {
        match byte(s, i) {
            b'-' | b'+' => {
                neg = byte(s, i) == b'-';
                i += 1;
                if byte(s, i) != 0 {
                    break;
                }
                return (0.0, 0);
            }
            0 => return (0.0, 0),
            b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ' => {
                i += 1;
            }
            _ => break,
        }
    }
    let signed_zero = if neg { -0.0 } else { 0.0 };
    // break2:
    let mut nz0 = false;
    if byte(s, i) == b'0' {
        nz0 = true;
        i += 1;
        while byte(s, i) == b'0' {
            i += 1;
        }
        if byte(s, i) == 0 {
            return (signed_zero, i);
        }
    }
    let int_start = i;
    while byte(s, i).is_ascii_digit() {
        i += 1;
    }
    let int_digits = &s[int_start..i];
    let mut frac_digits: &[u8] = &[];
    if byte(s, i) == b'.' {
        i += 1;
        let frac_start = i;
        while byte(s, i).is_ascii_digit() {
            i += 1;
        }
        frac_digits = &s[frac_start..i];
    }
    let any_digit = nz0 || !int_digits.is_empty() || !frac_digits.is_empty();
    let mut exponent: i32 = 0;
    let c = byte(s, i);
    if c == b'e' || c == b'E' {
        if !any_digit {
            return (0.0, 0);
        }
        let e_pos = i;
        let mut j = i + 1;
        let mut esign = false;
        match byte(s, j) {
            b'-' => {
                esign = true;
                j += 1;
            }
            b'+' => j += 1,
            _ => {}
        }
        if byte(s, j).is_ascii_digit() {
            while byte(s, j) == b'0' {
                j += 1;
            }
            let first = j;
            let mut l: i64 = 0;
            while byte(s, j).is_ascii_digit() {
                if l <= 1_000_000_000 {
                    l = l * 10 + i64::from(byte(s, j) - b'0');
                }
                j += 1;
            }
            let e = if j - first > 8 || l > 19999 {
                19999
            } else {
                l as i32
            };
            exponent = if esign { -e } else { e };
            i = j;
        } else {
            i = e_pos;
        }
    }
    if !any_digit {
        return (0.0, 0);
    }
    let significant = int_digits.iter().chain(frac_digits).any(|&d| d != b'0');
    if !significant {
        return (signed_zero, i);
    }
    let mut buf = String::with_capacity(int_digits.len() + frac_digits.len() + 16);
    if neg {
        buf.push('-');
    }
    if int_digits.is_empty() {
        buf.push('0');
    } else {
        buf.push_str(std::str::from_utf8(int_digits).unwrap_or("0"));
    }
    if !frac_digits.is_empty() {
        buf.push('.');
        buf.push_str(std::str::from_utf8(frac_digits).unwrap_or("0"));
    }
    if exponent != 0 {
        buf.push('e');
        buf.push_str(&exponent.to_string());
    }
    // The buffer only contains ASCII digits, '.', 'e', '-' and is always a valid literal.
    let v = buf.parse::<f64>().unwrap_or(0.0);
    (v, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(s: &str, allow: bool) -> Option<(Number, i8, bool)> {
        is_numeric_string_ex(s.as_bytes(), allow).map(|n| (n.number, n.overflow, n.trailing_data))
    }

    #[test]
    fn numeric_strings() {
        use Number::*;
        assert_eq!(num("123", false), Some((Int(123), 0, false)));
        assert_eq!(num(" 123 ", false), Some((Int(123), 0, false)));
        assert_eq!(num("\t\n123\r\x0b\x0c", false), Some((Int(123), 0, false)));
        assert_eq!(num("1e3", false), Some((Float(1000.0), 0, false)));
        assert_eq!(num(".5", false), Some((Float(0.5), 0, false)));
        assert_eq!(num("5.", false), Some((Float(5.0), 0, false)));
        assert_eq!(num("-.5", false), Some((Float(-0.5), 0, false)));
        assert_eq!(num("00", false), Some((Int(0), 0, false)));
        assert_eq!(num("-0", false), Some((Int(0), 0, false)));
        assert_eq!(num("1e", false), None);
        assert_eq!(num("1e", true), Some((Int(1), 0, true)));
        assert_eq!(num(".", false), None);
        assert_eq!(num("", true), None);
        assert_eq!(num(" ", true), None);
        assert_eq!(num("0x1A", false), None);
        assert_eq!(num("0x1A", true), Some((Int(0), 0, true)));
        assert_eq!(num("1_000", true), Some((Int(1), 0, true)));
        assert_eq!(num("abc", true), None);
        assert_eq!(
            num("9223372036854775807", false),
            Some((Int(i64::MAX), 0, false))
        );
        assert_eq!(
            num("9223372036854775808", false),
            Some((Float(9.223372036854776e18), 1, false))
        );
        assert_eq!(
            num("-9223372036854775808", false),
            Some((Int(i64::MIN), 0, false))
        );
        assert_eq!(
            num("-9223372036854775808 ", false),
            Some((Float(-9.223372036854776e18), -1, false))
        );
        assert_eq!(
            num("-9223372036854775809", false),
            Some((Float(-9.223372036854776e18), -1, false))
        );
        assert_eq!(
            num("99999999999999999999", false),
            Some((Float(1e20), 1, false))
        );
        // exponent-sign pointer quirk (engine-semantics.md §4.5)
        assert_eq!(
            num("9223372036854775808e+", true),
            Some((Int(i64::MIN), 0, true))
        );
        assert_eq!(
            num("9300000000000000000e-", true),
            Some((Int(-9146744073709551616), 0, true))
        );
        assert_eq!(
            num("-9223372036854775809e+", true),
            Some((Int(i64::MAX), 0, true))
        );
        assert_eq!(
            num("9223372036854775808e", true),
            Some((Float(9.223372036854776e18), 1, true))
        );
        assert_eq!(num("1\0", true), Some((Int(1), 0, true)));
        assert_eq!(num("1\0", false), None);
        assert_eq!(num("\u{661}", true), None);
    }

    #[test]
    fn strtod_prefixes() {
        let t = |s: &str| strtod(s.as_bytes());
        assert_eq!(t("1.5abc"), (1.5, 3));
        assert_eq!(t("  -1.5"), (-1.5, 6));
        assert_eq!(t("- 5"), (0.0, 0));
        assert_eq!(t("1e"), (1.0, 1));
        assert_eq!(t("1e+"), (1.0, 1));
        assert_eq!(t("1e400").0, f64::INFINITY);
        assert_eq!(t("-1e400").0, f64::NEG_INFINITY);
        assert_eq!(t("1e-400"), (0.0, 6));
        assert_eq!(t(".e5"), (0.0, 0));
        assert_eq!(t("0x1A"), (0.0, 1));
        let (z, n) = t("-0");
        assert!(z == 0.0 && z.is_sign_negative() && n == 2);
        let (z, n) = t("-.0");
        assert!(z == 0.0 && z.is_sign_negative() && n == 3);
        assert_eq!(t("0.e5"), (0.0, 4));
        assert_eq!(t("1.e5"), (100000.0, 4));
        assert_eq!(t("00012.5"), (12.5, 7));
        assert_eq!(t("INF"), (0.0, 0));
        assert_eq!(t("1.9999999999999999"), (2.0, 18));
        assert_eq!(t("1e0005"), (100000.0, 6));
        assert_eq!(t("1e99999999999").0, f64::INFINITY);
        assert_eq!(t("1e-99999999999").0, 0.0);
    }
}
