//! Small helpers from `PHPSRC/ext/date/lib/timelib.c`.

use crate::types::{MINS_PER_HOUR, SECS_PER_HOUR, USECS_PER_HOUR};

/// Port of `timelib_strcasecmp()` (PHPSRC/ext/date/lib/timelib.c): ASCII case-insensitive comparison of two
/// NUL-terminated strings (bytes after an embedded NUL are ignored), returning the first difference of the
/// lower-cased bytes, or the length difference.
pub fn strcasecmp(s1: &[u8], s2: &[u8]) -> i32 {
    let s1 = c_str(s1);
    let s2 = c_str(s2);
    let len = s1.len().min(s2.len());
    for k in 0..len {
        let c1 = i32::from(s1[k].to_ascii_lowercase());
        let c2 = i32::from(s2[k].to_ascii_lowercase());
        if c1 != c2 {
            return c1 - c2;
        }
    }
    (s1.len() as i64 - s2.len() as i64) as i32
}

/// Port of `timelib_strncasecmp()` (PHPSRC/ext/date/lib/timelib.c).
pub fn strncasecmp(s1: &[u8], s2: &[u8], length: usize) -> i32 {
    let s1 = c_str(s1);
    let s2 = c_str(s2);
    let len = length.min(s1.len().min(s2.len()));
    for k in 0..len {
        let c1 = i32::from(s1[k].to_ascii_lowercase());
        let c2 = i32::from(s2[k].to_ascii_lowercase());
        if c1 != c2 {
            return c1 - c2;
        }
    }
    (length.min(s1.len()) as i64 - length.min(s2.len()) as i64) as i32
}

/// The bytes of `s` up to (not including) the first NUL, like C's view of a `char *`.
pub(crate) fn c_str(s: &[u8]) -> &[u8] {
    match s.iter().position(|&b| b == 0) {
        Some(n) => &s[..n],
        None => s,
    }
}

/// C conversion of a `double` to `int` as x86-64 performs it (`cvttsd2si`): truncation toward zero, and
/// `INT_MIN` for NaN and values out of range (Rust's `as` saturates instead).
pub(crate) fn f64_to_c_int(v: f64) -> i32 {
    if v.is_nan() || v >= 2_147_483_648.0 {
        i32::MIN
    } else {
        v as i32
    }
}

/// Port of `timelib_decimal_hour_to_hms()` (PHPSRC/ext/date/lib/timelib.c).
pub fn decimal_hour_to_hms(h: f64) -> (i32, i32, i32) {
    let mut h = h;
    let mut swap = false;
    if h < 0.0 {
        swap = true;
        h = h.abs();
    }
    let mut hour = f64_to_c_int(h.floor());
    let seconds = f64_to_c_int(((h - f64::from(hour)) * 3600.0).floor());
    let min = seconds / 60;
    let sec = seconds % 60;
    if swap {
        // `0 - *hour` (wraps for INT_MIN: PHP builds timelib with -fwrapv).
        hour = hour.wrapping_neg();
    }
    (hour, min, sec)
}

/// Port of `timelib_hms_to_decimal_hour()` (PHPSRC/ext/date/lib/timelib.c).
pub fn hms_to_decimal_hour(hour: i32, min: i32, sec: i32) -> f64 {
    if hour >= 0 {
        f64::from(hour) + f64::from(min) / 60.0 + f64::from(sec) / 3600.0
    } else {
        f64::from(hour) - f64::from(min) / 60.0 - f64::from(sec) / 3600.0
    }
}

/// Port of `timelib_hmsf_to_decimal_hour()` (PHPSRC/ext/date/lib/timelib.c).
pub fn hmsf_to_decimal_hour(hour: i32, min: i32, sec: i32, us: i32) -> f64 {
    if hour >= 0 {
        (f64::from(hour)
            + f64::from(min) / MINS_PER_HOUR as f64
            + f64::from(sec) / SECS_PER_HOUR as f64)
            + f64::from(us) / USECS_PER_HOUR as f64
    } else {
        (f64::from(hour)
            - f64::from(min) / MINS_PER_HOUR as f64
            - f64::from(sec) / SECS_PER_HOUR as f64)
            - f64::from(us) / USECS_PER_HOUR as f64
    }
}

/// Port of `timelib_hms_to_seconds()` (PHPSRC/ext/date/lib/timelib.c).
pub fn hms_to_seconds(h: i64, m: i64, s: i64) -> i64 {
    h.wrapping_mul(SECS_PER_HOUR)
        .wrapping_add(m.wrapping_mul(60))
        .wrapping_add(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_insensitive_compare() {
        assert_eq!(strcasecmp(b"Europe/Paris", b"europe/paris"), 0);
        assert!(strcasecmp(b"abc", b"abd") < 0);
        assert!(strcasecmp(b"abcd", b"abc") > 0);
        assert_eq!(strncasecmp(b"ndx", b"ND", 2), 0);
        assert_eq!(strncasecmp(b"n", b"nd", 2), -1);
    }

    #[test]
    fn decimal_hours() {
        assert_eq!(decimal_hour_to_hms(-1.5), (-1, 30, 0));
        assert_eq!(hms_to_decimal_hour(1, 30, 0), 1.5);
    }
}
