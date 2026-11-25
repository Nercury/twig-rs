//! The small slice of PHP behaviour the lexer and the token stream depend on.
//!
//! These helpers stay private to the root crate: the canonical PHP value semantics live in `twig-value`, which the
//! root crate must not depend on yet. Once it can, [`float_to_string`] should be replaced by the `twig-value`
//! implementation of the same algorithm.

use std::borrow::Cow;

/// The characters PHP's `trim()`, `ltrim()` and `rtrim()` remove by default: `" \t\n\r\0\x0B"`.
pub(crate) const TRIM_DEFAULT: &[u8] = b" \t\n\r\0\x0B";

/// The characters Twig's `~` trim modifiers remove from the text before a tag: `" \t\0\x0B"`
/// (hard-coded in `Lexer::lexData()` and `Lexer::lexRawData()`, independent of the `whitespace_line_chars` option).
pub(crate) const TRIM_LINE: &[u8] = b" \t\0\x0B";

/// PCRE's `\s` without the `u` flag: `[\t\n\x0B\f\r ]`.
pub(crate) fn is_pcre_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ')
}

/// PHP `rtrim($s, $chars)` for a literal character list.
pub(crate) fn rtrim<'a>(s: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let end = s
        .iter()
        .rposition(|b| !chars.contains(b))
        .map_or(0, |i| i + 1);
    &s[..end]
}

/// PHP `ltrim($s, $chars)` for a literal character list.
pub(crate) fn ltrim<'a>(s: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let start = s.iter().position(|b| !chars.contains(b)).unwrap_or(s.len());
    &s[start..]
}

/// PHP `trim($s, $chars)` for a literal character list.
pub(crate) fn trim<'a>(s: &'a [u8], chars: &[u8]) -> &'a [u8] {
    rtrim(ltrim(s, chars), chars)
}

/// Twig: `Lexer::normalizeNewlines()`, i.e. `str_replace(["\r\n", "\r"], "\n", $text)`.
pub(crate) fn normalize_newlines(text: &[u8]) -> Cow<'_, [u8]> {
    if !text.contains(&b'\r') {
        return Cow::Borrowed(text);
    }
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            b'\r' => {
                out.push(b'\n');
                if text.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    Cow::Owned(out)
}

/// The number of line breaks `Lexer::moveCursor()` counts in a chunk: `"\r\n"`, `"\r"` and `"\n"` each count once.
pub(crate) fn count_newlines(text: &[u8]) -> i64 {
    let mut count = 0;
    for (i, &byte) in text.iter().enumerate() {
        if byte == b'\n' || (byte == b'\r' && text.get(i + 1) != Some(&b'\n')) {
            count += 1;
        }
    }
    count
}

/// PHP truthiness of a string: `''` and `'0'` are false.
pub(crate) fn is_truthy_str(s: &[u8]) -> bool {
    !(s.is_empty() || s == b"0")
}

/// PHP `(int) $digits` for a string of ASCII digits (the `{% line N %}` directive): saturates at `PHP_INT_MAX`.
///
/// Port of the overflow path of `_zend_get_long_func_noisy()` for numeric strings: a digit string that does not
/// fit a `zend_long` becomes a double, which `zend_dval_to_lval_cap()` caps at `ZEND_LONG_MAX`
/// (PHPSRC/Zend/zend_operators.c, PHPSRC/Zend/zend_operators.h).
pub(crate) fn int_from_digits(digits: &[u8]) -> i64 {
    let mut value: i64 = 0;
    for &digit in digits {
        let Some(next) = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(i64::from(digit - b'0')))
        else {
            return i64::MAX;
        };
        value = next;
    }
    value
}

/// A PHP number produced by `0 + $numericString`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Number {
    Int(i64),
    Float(f64),
}

/// PHP `0 + $s` for the strings `Lexer::lexExpression()` passes (the matched `REGEX_NUMBER` text with the
/// underscores removed): digits, an optional fraction and an optional exponent.
///
/// Port of `_is_numeric_string_ex()` for well-formed input (PHPSRC/Zend/zend_operators.c): a string with `.`, `e`
/// or `E` is a double; an integer string is a `zend_long` unless it overflows, in which case it is a double. Doubles
/// come from `zend_strtod()`, which is correctly rounded like Rust's `f64` parser (`1e1000` is `INF`).
pub(crate) fn numeric_literal(s: &str) -> Number {
    if !s.bytes().any(|b| matches!(b, b'.' | b'e' | b'E'))
        && let Ok(int) = s.parse::<i64>()
    {
        return Number::Int(int);
    }
    // The lexer only passes strings matching `REGEX_NUMBER`, which Rust's float grammar accepts.
    Number::Float(s.parse::<f64>().unwrap_or(f64::NAN))
}

/// PHP's conversion of a float to a string with the default `precision` ini setting (14): what `echo`, `(string)`
/// and `sprintf('%s', ...)` print.
///
/// Port of `zend_gcvt(value, 14, '.', 'E', buf)` (PHPSRC/Zend/zend_strtod.c) with `zend_dtoa()` mode 2 digits,
/// following the validated recipe of `reference/php/engine-semantics.md` §5.5.
pub(crate) fn float_to_string(value: f64) -> String {
    const PRECISION: i32 = 14;
    if value.is_nan() {
        return "NAN".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-INF" } else { "INF" }.to_owned();
    }
    let mut out = String::new();
    if value.is_sign_negative() {
        out.push('-');
    }
    let (digits, decpt) = if value == 0.0 {
        ("0".to_owned(), 1)
    } else {
        dtoa_mode2(value.abs(), PRECISION)
    };
    let digits = digits.as_bytes();
    // zend_gcvt: `(decpt >= 0 && decpt > ndigit) || decpt < -3` (the first test is implied by the second).
    if !(-3..=PRECISION).contains(&decpt) {
        // Exponential format, e.g. 1.0E+25.
        let exponent = decpt - 1;
        out.push(char::from(digits[0]));
        out.push('.');
        if digits.len() == 1 {
            out.push('0');
        } else {
            out.extend(digits[1..].iter().map(|&d| char::from(d)));
        }
        out.push('E');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&exponent.unsigned_abs().to_string());
    } else if decpt < 0 {
        out.push_str("0.");
        for _ in decpt..0 {
            out.push('0');
        }
        out.extend(digits.iter().map(|&d| char::from(d)));
    } else {
        let decpt = decpt as usize;
        for i in 0..decpt {
            out.push(digits.get(i).map_or('0', |&d| char::from(d)));
        }
        if digits.len() > decpt {
            if decpt == 0 {
                out.push('0');
            }
            out.push('.');
            out.extend(digits[decpt..].iter().map(|&d| char::from(d)));
        }
    }
    out
}

/// `zend_dtoa(value, 2, ndigit, ...)` for a finite, positive value: the first `ndigit` significant digits correctly
/// rounded (ties to even on the exact binary value), trailing zeros removed except for the small-integer tie quirk
/// (`reference/php/engine-semantics.md` §5.4). Returns the digits and `decpt` (value = 0.d1d2... × 10^decpt).
fn dtoa_mode2(value: f64, ndigit: i32) -> (String, i32) {
    let formatted = format!("{:.*e}", (ndigit - 1) as usize, value);
    let (mantissa, exponent) = formatted.split_once('e').unwrap_or((&formatted, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let mut digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    if !small_int_tie_keeps_zeros(value, ndigit) {
        while digits.len() > 1 && digits.ends_with('0') {
            digits.pop();
        }
    }
    (digits, exponent + 1)
}

/// dtoa's "small integer" path breaks out without stripping trailing zeros when an integral value below 1e15 is an
/// exact tie at the requested precision and rounds down to an even digit (`zend_strtod.c`, `goto ret1`).
fn small_int_tie_keeps_zeros(value: f64, ndigit: i32) -> bool {
    if ndigit > 14 || value.fract() != 0.0 || value >= 1e15 {
        return false;
    }
    let ndigit = ndigit as usize;
    let integer = format!("{}", value as u64);
    let bytes = integer.as_bytes();
    if bytes.len() <= ndigit {
        return false;
    }
    let rest = &bytes[ndigit..];
    rest[0] == b'5'
        && rest[1..].iter().all(|&b| b == b'0')
        && (bytes[ndigit - 1] - b'0').is_multiple_of(2)
}

/// Lossy UTF-8 view of PHP bytes, used where Twig interpolates template bytes into a message.
pub(crate) fn lossy(bytes: &[u8]) -> Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

/// A numeric string as classified by `_is_numeric_string_ex()`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum NumericString {
    /// `IS_LONG` with its value.
    Long(i64),
    /// `IS_DOUBLE` with its value and the `oflow_info` result: `1`/`-1` when the string is an integer (or starts
    /// with 20 or more integer digits) beyond `zend_long` on the positive/negative side, else `0`.
    Double(f64, i8),
}

/// The whitespace `_is_numeric_string_ex()` allows before and after the number: `" \t\n\r\v\f"`.
fn is_numeric_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// PHP `is_numeric_string_ex($str, $length, &$lval, &$dval, allow_errors: false, &$oflow_info, NULL)`: whether the
/// whole string (leading and trailing whitespace allowed) is a decimal integer or float, and its value.
///
/// Port of `is_numeric_string_ex()` (PHPSRC/Zend/zend_operators.h) and `_is_numeric_string_ex()`
/// (PHPSRC/Zend/zend_operators.c) for callers that request both values, as `zendi_smart_streq()` and
/// `compare_long_to_string()` do. The C code reads the string's NUL terminator; here a byte past the end reads as
/// NUL. `zend_strtod()` is correctly rounded, like Rust's `f64` parser.
fn numeric_string(s: &[u8]) -> Option<NumericString> {
    /// `MAX_LENGTH_OF_LONG` on 64-bit platforms.
    const MAX_LENGTH_OF_LONG: usize = 20;
    /// `long_min_digits`.
    const LONG_MIN_DIGITS: &[u8] = b"9223372036854775808";

    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    // is_numeric_string_ex(): `if (*str > '9') return 0;`
    if at(0) > b'9' || s.is_empty() {
        return None;
    }
    // Skip any whitespace.
    let mut start = 0;
    while is_numeric_whitespace(at(start)) {
        start += 1;
    }
    let mut ptr = start;
    let negative = at(ptr) == b'-';
    if negative || at(ptr) == b'+' {
        ptr += 1;
    }
    let sign: i8 = if at(start) == b'-' { -1 } else { 1 };

    let mut is_double = false;
    let mut overflow = 0i8;
    let mut digits = 0;
    let mut long_value: u64 = 0;
    if at(ptr).is_ascii_digit() {
        // Skip any leading 0s.
        while at(ptr) == b'0' {
            ptr += 1;
        }
        // Count the digits; a decimal point or an exponent makes it a double. With `dval` requested, counting stops
        // at MAX_LENGTH_OF_LONG digits.
        while digits < MAX_LENGTH_OF_LONG {
            let c = at(ptr);
            if c.is_ascii_digit() {
                // `zend_ulong` arithmetic: the 20th digit may wrap, but then the value is discarded.
                long_value = long_value
                    .wrapping_mul(10)
                    .wrapping_add(u64::from(c - b'0'));
                digits += 1;
                ptr += 1;
                continue;
            }
            if c == b'.' {
                is_double = true;
            } else if c == b'e' || c == b'E' {
                let mut e = ptr + 1;
                if at(e) == b'-' || at(e) == b'+' {
                    // `ptr = e++;`: a failed exponent leaves `ptr` on the sign.
                    ptr = e;
                    e += 1;
                }
                is_double = at(e).is_ascii_digit();
            }
            break;
        }
        if digits >= MAX_LENGTH_OF_LONG {
            overflow = sign;
            is_double = true;
        }
    } else if at(ptr) == b'.' && at(ptr + 1).is_ascii_digit() {
        is_double = true;
    } else {
        return None;
    }

    let mut double_value = 0.0;
    if is_double {
        // process_double: `local_dval = zend_strtod(str, &ptr);`
        let (value, end) = strtod(s, start);
        double_value = value;
        ptr = end;
    }

    // Only whitespace may follow the number (`ptr` never passes the end: it only moves over bytes that exist).
    if !s[ptr..].iter().all(|&b| is_numeric_whitespace(b)) {
        return None;
    }

    if is_double {
        return Some(NumericString::Double(double_value, overflow));
    }
    if digits == MAX_LENGTH_OF_LONG - 1 {
        // `strcmp(&ptr[-digits], long_min_digits)`: the digits plus the trailing whitespace, up to the end.
        let cmp = s[ptr - digits..].cmp(LONG_MIN_DIGITS);
        if !(cmp.is_lt() || (cmp.is_eq() && negative)) {
            return Some(NumericString::Double(strtod(s, start).0, sign));
        }
    }
    // `(zend_long) -tmp_lval` on the unsigned value.
    let value = if negative {
        long_value.wrapping_neg()
    } else {
        long_value
    };
    Some(NumericString::Long(value as i64))
}

/// `zend_strtod(s + start, &end)` for a string that starts (after an optional sign) with a digit or with `.` and a
/// digit: the value of the longest prefix of the form `[+-]?[0-9]*(\.[0-9]*)?([eE][+-]?[0-9]+)?` and its end.
///
/// Port of `zend_strtod()` (PHPSRC/Zend/zend_strtod.c) for that input; the conversion itself is Rust's correctly
/// rounded `f64` parser, which agrees with David Gay's correctly rounded `strtod()`.
fn strtod(s: &[u8], start: usize) -> (f64, usize) {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut end = start;
    if at(end) == b'-' || at(end) == b'+' {
        end += 1;
    }
    while at(end).is_ascii_digit() {
        end += 1;
    }
    if at(end) == b'.' {
        end += 1;
        while at(end).is_ascii_digit() {
            end += 1;
        }
    }
    if at(end) == b'e' || at(end) == b'E' {
        let mut exponent = end + 1;
        if at(exponent) == b'-' || at(exponent) == b'+' {
            exponent += 1;
        }
        if at(exponent).is_ascii_digit() {
            while at(exponent).is_ascii_digit() {
                exponent += 1;
            }
            end = exponent;
        }
    }
    let text = std::str::from_utf8(&s[start..end]).unwrap_or("0");
    // Rust's grammar needs a digit in the mantissa (`1.` and `.5` are accepted, `.` and `.e1` are not); callers only
    // pass input with a digit.
    (text.parse::<f64>().unwrap_or(0.0), end)
}

/// PHP `$a == $b` for two strings: numeric strings compare as numbers, anything else byte for byte.
///
/// Port of `zend_fast_equal_strings()` (PHPSRC/Zend/zend_operators.h) and `zendi_smart_streq()`
/// (PHPSRC/Zend/zend_operators.c), including its fallback to a byte comparison for integers that overflow to the
/// same side and for infinite values.
pub(crate) fn loose_equals_strings(s1: &[u8], s2: &[u8]) -> bool {
    let (Some(n1), Some(n2)) = (numeric_string(s1), numeric_string(s2)) else {
        return s1 == s2;
    };
    match (n1, n2) {
        (NumericString::Long(l1), NumericString::Long(l2)) => l1 == l2,
        (NumericString::Double(d1, o1), NumericString::Double(d2, o2)) => {
            // Both values are integers overflown to the same side: the double comparison may have lost precision.
            if o1 != 0 && o1 == o2 && d1 - d2 == 0.0 {
                return s1 == s2;
            }
            // Both values overflowed to the same infinity: a numeric comparison would be inaccurate.
            if d1 == d2 && !d1.is_finite() {
                return s1 == s2;
            }
            d1 == d2
        }
        (NumericString::Long(l1), NumericString::Double(d2, o2)) => o2 == 0 && l1 as f64 == d2,
        (NumericString::Double(d1, o1), NumericString::Long(l2)) => o1 == 0 && d1 == l2 as f64,
    }
}

/// PHP `$int == $string`.
///
/// Port of `compare_long_to_string()` (PHPSRC/Zend/zend_operators.c), reduced to the equality test of
/// `zend_compare() == 0`.
pub(crate) fn loose_equals_int_string(value: i64, s: &[u8]) -> bool {
    match numeric_string(s) {
        Some(NumericString::Long(long)) => value == long,
        Some(NumericString::Double(double, _)) => value as f64 == double,
        None => value.to_string().as_bytes() == s,
    }
}

/// PHP `$float == $string`.
///
/// Port of the `IS_DOUBLE`/`IS_STRING` case of `zend_compare()` and `compare_double_to_string()`
/// (PHPSRC/Zend/zend_operators.c), reduced to the equality test: NAN equals nothing, and a non-numeric string is
/// compared with the float's string form (`precision` 14).
pub(crate) fn loose_equals_float_string(value: f64, s: &[u8]) -> bool {
    if value.is_nan() {
        return false;
    }
    match numeric_string(s) {
        Some(NumericString::Long(long)) => value == long as f64,
        Some(NumericString::Double(double, _)) => value == double,
        None => float_to_string(value).as_bytes() == s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_like_php() {
        assert_eq!(rtrim(b"a \t\n\r\0\x0B", TRIM_DEFAULT), b"a");
        assert_eq!(rtrim(b"a\x0C", TRIM_DEFAULT), b"a\x0C");
        assert_eq!(rtrim(b"a \n\t\0", TRIM_LINE), b"a \n");
        assert_eq!(rtrim(b"   ", TRIM_DEFAULT), b"");
        assert_eq!(ltrim(b" \x0C}}", TRIM_DEFAULT), b"\x0C}}");
        assert_eq!(trim(b"\n a b \n", TRIM_DEFAULT), b"a b");
    }

    #[test]
    fn normalizes_and_counts_newlines() {
        assert_eq!(
            &*normalize_newlines(b"a\r\nb\rc\nd\r\r\n"),
            b"a\nb\nc\nd\n\n"
        );
        assert_eq!(count_newlines(b"a\r\nb\rc\nd\r\r\n"), 5);
        assert_eq!(count_newlines(b"\r"), 1);
        assert_eq!(count_newlines(b"abc"), 0);
    }

    #[test]
    fn converts_numeric_literals() {
        assert_eq!(numeric_literal("007"), Number::Int(7));
        assert_eq!(
            numeric_literal("9223372036854775807"),
            Number::Int(i64::MAX)
        );
        assert_eq!(
            numeric_literal("9223372036854775808"),
            Number::Float(9_223_372_036_854_775_808.0)
        );
        assert_eq!(numeric_literal("1.0"), Number::Float(1.0));
        assert_eq!(numeric_literal("1e3"), Number::Float(1000.0));
        assert_eq!(numeric_literal("1e1000"), Number::Float(f64::INFINITY));
    }

    #[test]
    fn saturates_line_directives() {
        assert_eq!(int_from_digits(b"0010"), 10);
        assert_eq!(int_from_digits(b"9223372036854775807"), i64::MAX);
        assert_eq!(int_from_digits(b"99999999999999999999"), i64::MAX);
    }

    #[test]
    fn classifies_numeric_strings() {
        use NumericString::{Double, Long};
        assert_eq!(numeric_string(b" 12 "), Some(Long(12)));
        assert_eq!(numeric_string(b"-007"), Some(Long(-7)));
        assert_eq!(numeric_string(b"1.5e3"), Some(Double(1500.0, 0)));
        assert_eq!(numeric_string(b".5"), Some(Double(0.5, 0)));
        assert_eq!(numeric_string(b"1."), Some(Double(1.0, 0)));
        assert_eq!(
            numeric_string(b"-9223372036854775808"),
            Some(Long(i64::MIN))
        );
        // The trailing whitespace takes part in the comparison with "9223372036854775808".
        assert_eq!(
            numeric_string(b"-9223372036854775808 "),
            Some(Double(-9_223_372_036_854_775_808.0, -1))
        );
        assert_eq!(
            numeric_string(b"9223372036854775808"),
            Some(Double(9_223_372_036_854_775_808.0, 1))
        );
        // 20 integer digits overflow even with a fraction.
        assert!(matches!(
            numeric_string(b"12345678901234567890.5"),
            Some(Double(_, 1))
        ));
        for not_numeric in [
            &b""[..],
            b" ",
            b"1e",
            b"1e+",
            b"0x1A",
            b"1_000",
            b"1 2",
            b"1\0",
            b"abc",
            b".",
        ] {
            assert_eq!(numeric_string(not_numeric), None, "{not_numeric:?}");
        }
    }

    #[test]
    fn compares_loosely_like_php() {
        assert!(loose_equals_strings(b"1.0", b"1"));
        assert!(loose_equals_strings(b"1e3", b"1000"));
        assert!(!loose_equals_strings(b"abc", b"ABC"));
        // Integers overflowing to the same side compare as strings; a float string never counts as overflowing.
        assert!(!loose_equals_strings(
            b"9223372036854775808",
            b"9223372036854775809"
        ));
        assert!(loose_equals_strings(
            b"9223372036854775808",
            b"9223372036854775808.0"
        ));
        assert!(loose_equals_int_string(1, b" 1"));
        assert!(loose_equals_int_string(i64::MAX, b"9223372036854775808"));
        assert!(!loose_equals_int_string(1, b"1a"));
        assert!(loose_equals_float_string(1e20, b"1.0E+20"));
        assert!(loose_equals_float_string(f64::INFINITY, b"INF"));
        assert!(!loose_equals_float_string(f64::NAN, b"NAN"));
    }

    #[test]
    fn formats_floats_with_precision_14() {
        // reference/php/engine-semantics.md §5.4 and §5.6 (verified on the oracle).
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (0.1, "0.1"),
            (100.0, "100"),
            (1e14, "1.0E+14"),
            (1e15, "1.0E+15"),
            (1e25, "1.0E+25"),
            (123_456_789_012_345.0, "1.2345678901234E+14"),
            (0.0001, "0.0001"),
            (0.00001, "1.0E-5"),
            (0.000123, "0.000123"),
            (1.5e-7, "1.5E-7"),
            (0.1 + 0.2, "0.3"),
            (1.0 / 3.0, "0.33333333333333"),
            (std::f64::consts::PI, "3.1415926535898"),
            (9_223_372_036_854_775_808.0, "9.2233720368548E+18"),
            (f64::MAX, "1.7976931348623E+308"),
            (5e-324, "4.9406564584125E-324"),
            (99_999_999_999_999.99, "1.0E+14"),
            (100_000_000_000_005.0, "1.0000000000000E+14"),
            (100_000_000_000_015.0, "1.0000000000002E+14"),
            (123_456_789_012_305.0, "1.2345678901230E+14"),
            (999_999_999_999_995.0, "1.0E+15"),
            (1e20, "1.0E+20"),
            (1.5, "1.5"),
            (f64::INFINITY, "INF"),
            (f64::NEG_INFINITY, "-INF"),
            (f64::NAN, "NAN"),
        ];
        for &(value, expected) in cases {
            assert_eq!(float_to_string(value), expected, "{value:e}");
        }
    }
}
