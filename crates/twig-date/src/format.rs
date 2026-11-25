//! `DateTime::format()` / `date()` and `DateInterval::format()`: ports of `date_format()` and
//! `date_interval_format()` (PHPSRC/ext/date/php_date.c).
//!
//! PHP builds each specifier with `slprintf()` into a 97-byte (`date_format`) or 33-byte (`date_interval_format`)
//! buffer; pieces longer than the buffer are truncated the same way here. Field values are cast to C `int` where
//! upstream does (`(int) t->d`).

use std::io::Write;

use twig_timelib::types::{ZONETYPE_ABBR, ZONETYPE_ID, ZONETYPE_OFFSET, is_leap};
use twig_timelib::{RelTime, Time, TimeOffset, UNSET, dow};

/// `mon_full_names`.
pub const MON_FULL_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `mon_short_names`.
pub const MON_SHORT_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `day_full_names`.
pub const DAY_FULL_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// `day_short_names`.
pub const DAY_SHORT_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Port of `english_suffix()`.
pub fn english_suffix(number: i64) -> &'static str {
    if (10..=19).contains(&number) {
        return "th";
    }
    match number % 10 {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    }
}

/// Port of `php_date_full_day_name()`.
pub fn full_day_name(y: i64, m: i64, d: i64) -> &'static str {
    let day_of_week = dow::day_of_week(y, m, d);
    if day_of_week < 0 {
        return "Unknown";
    }
    DAY_FULL_NAMES[day_of_week as usize % 7]
}

/// Port of `php_date_short_day_name()`.
pub fn short_day_name(y: i64, m: i64, d: i64) -> &'static str {
    let day_of_week = dow::day_of_week(y, m, d);
    if day_of_week < 0 {
        return "Unknown";
    }
    DAY_SHORT_NAMES[day_of_week as usize % 7]
}

/// `mon_full_names[m - 1]` (C reads out of bounds for months outside 1..12, which normalised times never have).
fn month_name(names: &'static [&'static str; 12], m: i64) -> &'static str {
    usize::try_from(m.wrapping_sub(1))
        .ok()
        .and_then(|i| names.get(i))
        .copied()
        .unwrap_or("")
}

/// `abs()` on a C `int` (wrapping for `INT_MIN`, like the machine instruction).
fn iabs(v: i32) -> i32 {
    v.wrapping_abs()
}

/// `snprintf(buf, size, ...)` into a fixed buffer whose length is then taken from the return value: the text is
/// cut to `size - 1` bytes, and when it was exactly one byte longer the terminating NUL becomes part of the
/// string (`date_create_tz_offset_str()` sets `ZSTR_LEN` from `snprintf()`'s return value).
fn snprintf_len(text: String, size: usize) -> Vec<u8> {
    let mut bytes = text.into_bytes();
    let full = bytes.len();
    if full > size - 1 {
        bytes.truncate(size - 1);
        // Bytes beyond the allocation are undefined in C; offsets never need more than one.
        bytes.resize(full, 0);
    }
    bytes
}

/// Port of `date_create_tz_offset_str()`: `+05:00`, or `+05:00:01` with seconds.
pub fn tz_offset_str(offset: i64) -> Vec<u8> {
    let seconds = (offset % 60) as i32;
    let sign = if offset < 0 { '-' } else { '+' };
    let hours = iabs((offset / 3600) as i32);
    let minutes = iabs((offset % 3600) as i32 / 60);
    if seconds == 0 {
        snprintf_len(format!("{sign}{hours:02}:{minutes:02}"), "+05:00".len() + 1)
    } else {
        snprintf_len(
            format!("{sign}{hours:02}:{minutes:02}:{:02}", iabs(seconds)),
            "+05:00:01".len() + 1,
        )
    }
}

/// The `GMT+0100` abbreviation `date_format()` and `php_idate()` build for offset zones
/// (`snprintf(offset->abbr, 9, "GMT%c%02d%02d", ...)`).
pub(crate) fn gmt_offset_abbr(offset: i32) -> Vec<u8> {
    let s = format!(
        "GMT{}{:02}{:02}",
        if offset < 0 { '-' } else { '+' },
        iabs(offset / 3600),
        iabs((offset % 3600) / 60)
    );
    let mut b = s.into_bytes();
    b.truncate(8);
    b
}

/// The `timelib_time_offset` `date_format()` computes for a local time: UTC offset, DST flag and abbreviation.
pub(crate) fn time_offset(t: &Time) -> TimeOffset {
    match t.zone_type {
        ZONETYPE_ABBR => TimeOffset {
            offset: t.z.wrapping_add(t.dst.wrapping_mul(3600)),
            leap_secs: 0,
            is_dst: t.dst as u32,
            abbr: t.tz_abbr.clone().unwrap_or_default(),
            transition_time: 0,
        },
        ZONETYPE_OFFSET => TimeOffset {
            offset: t.z,
            leap_secs: 0,
            is_dst: 0,
            abbr: String::from_utf8_lossy(&gmt_offset_abbr(t.z)).into_owned(),
            transition_time: 0,
        },
        ZONETYPE_ID => match &t.tz_info {
            Some(tz) => tz.get_time_zone_info(t.sse),
            None => TimeOffset {
                offset: 0,
                leap_secs: 0,
                is_dst: 0,
                abbr: "GMT".to_owned(),
                transition_time: 0,
            },
        },
        // Shouldn't happen, but code defensively (timelib_time_offset_ctor(): all zero).
        _ => TimeOffset {
            offset: 0,
            leap_secs: 0,
            is_dst: 0,
            abbr: String::new(),
            transition_time: 0,
        },
    }
}

/// Port of `date_format()` (PHPSRC/ext/date/php_date.c): formats `t` with the `date()` letters. `localtime` is
/// `t->is_localtime` for `DateTime::format()`, true for `date()` and false for `gmdate()`.
///
/// A backslash makes the next byte literal; a trailing backslash emits the string's terminating NUL byte.
pub fn date_format(format: &[u8], t: &Time, localtime: bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(format.len() * 2);
    if format.is_empty() {
        return out;
    }
    let offset = if localtime {
        time_offset(t)
    } else {
        TimeOffset {
            offset: 0,
            leap_secs: 0,
            is_dst: 0,
            abbr: String::new(),
            transition_time: 0,
        }
    };
    let mut weekyear: Option<(i64, i64)> = None;
    let mut isoweek =
        |t: &Time| *weekyear.get_or_insert_with(|| dow::isoweek_from_date(t.y, t.m, t.d));
    let off = offset.offset;
    let sign = |local: bool| {
        if local {
            if off < 0 { '-' } else { '+' }
        } else {
            '+'
        }
    };
    let hours = if localtime { iabs(off / 3600) } else { 0 };
    let minutes = if localtime {
        iabs((off % 3600) / 60)
    } else {
        0
    };

    let mut buf: Vec<u8> = Vec::with_capacity(96);
    let mut i = 0usize;
    while i < format.len() {
        buf.clear();
        let mut rfc_colon = false;
        let c = format[i];
        let _ = match c {
            // day
            b'd' => write!(buf, "{:02}", t.d as i32),
            b'D' => write!(buf, "{}", short_day_name(t.y, t.m, t.d)),
            b'j' => write!(buf, "{}", t.d as i32),
            b'l' => write!(buf, "{}", full_day_name(t.y, t.m, t.d)),
            b'S' => write!(buf, "{}", english_suffix(t.d)),
            b'w' => write!(buf, "{}", dow::day_of_week(t.y, t.m, t.d) as i32),
            b'N' => write!(buf, "{}", dow::iso_day_of_week(t.y, t.m, t.d) as i32),
            b'z' => write!(buf, "{}", dow::day_of_year(t.y, t.m, t.d) as i32),

            // week
            b'W' => write!(buf, "{:02}", isoweek(t).0 as i32), // iso weeknr
            b'o' => write!(buf, "{}", isoweek(t).1),           // iso year

            // month
            b'F' => write!(buf, "{}", month_name(&MON_FULL_NAMES, t.m)),
            b'm' => write!(buf, "{:02}", t.m as i32),
            b'M' => write!(buf, "{}", month_name(&MON_SHORT_NAMES, t.m)),
            b'n' => write!(buf, "{}", t.m as i32),
            b't' => write!(buf, "{}", dow::days_in_month(t.y, t.m) as i32),

            // year
            b'L' => write!(buf, "{}", i32::from(is_leap(i64::from(t.y as i32)))),
            b'y' => write!(buf, "{:02}", (t.y % 100) as i32),
            b'Y' => write!(
                buf,
                "{}{:04}",
                if t.y < 0 { "-" } else { "" },
                t.y.unsigned_abs()
            ),
            b'x' => write!(
                buf,
                "{}{:04}",
                if t.y < 0 {
                    "-"
                } else if t.y >= 10000 {
                    "+"
                } else {
                    ""
                },
                t.y.unsigned_abs()
            ),
            b'X' => write!(
                buf,
                "{}{:04}",
                if t.y < 0 { "-" } else { "+" },
                t.y.unsigned_abs()
            ),

            // time
            b'a' => write!(buf, "{}", if t.h >= 12 { "pm" } else { "am" }),
            b'A' => write!(buf, "{}", if t.h >= 12 { "PM" } else { "AM" }),
            b'B' => {
                let sse = t.sse;
                let mut retval = (sse
                    .wrapping_sub(sse.wrapping_sub((sse % 86400).wrapping_add(3600))))
                .wrapping_mul(10) as i32;
                if retval < 0 {
                    retval = retval.wrapping_add(864000);
                }
                // Make sure to do this on a positive int to avoid rounding errors
                retval = (retval / 864) % 1000;
                write!(buf, "{retval:03}")
            }
            b'g' => write!(buf, "{}", twelve_hour(t.h)),
            b'G' => write!(buf, "{}", t.h as i32),
            b'h' => write!(buf, "{:02}", twelve_hour(t.h)),
            b'H' => write!(buf, "{:02}", t.h as i32),
            b'i' => write!(buf, "{:02}", t.i as i32),
            b's' => write!(buf, "{:02}", t.s as i32),
            b'u' => write!(buf, "{:06}", t.us as i32),
            b'v' => write!(buf, "{:03}", (t.us / 1000) as i32),

            // timezone
            b'I' => write!(buf, "{}", if localtime { offset.is_dst as i32 } else { 0 }),
            b'p' | b'P' | b'O' => {
                if c == b'p'
                    && (!localtime
                        || offset.abbr == "UTC"
                        || offset.abbr == "Z"
                        || offset.abbr == "GMT+0000")
                {
                    write!(buf, "Z")
                } else {
                    if c != b'O' {
                        rfc_colon = true;
                    }
                    write!(
                        buf,
                        "{}{:02}{}{:02}",
                        sign(localtime),
                        hours,
                        if rfc_colon { ":" } else { "" },
                        minutes
                    )
                }
            }
            b'T' => {
                if localtime {
                    buf.extend_from_slice(offset.abbr.as_bytes());
                } else {
                    buf.extend_from_slice(b"GMT");
                }
                Ok(())
            }
            b'e' => {
                if !localtime {
                    buf.extend_from_slice(b"UTC");
                } else {
                    match t.zone_type {
                        ZONETYPE_ID => {
                            if let Some(tz) = &t.tz_info {
                                buf.extend_from_slice(tz.name.as_bytes());
                            }
                        }
                        ZONETYPE_ABBR => buf.extend_from_slice(offset.abbr.as_bytes()),
                        ZONETYPE_OFFSET => {
                            let seconds = off % 60;
                            let s = if off < 0 { '-' } else { '+' };
                            let _ = if seconds == 0 {
                                write!(
                                    buf,
                                    "{s}{:02}:{:02}",
                                    iabs(off / 3600),
                                    iabs((off % 3600) / 60)
                                )
                            } else {
                                write!(
                                    buf,
                                    "{s}{:02}:{:02}:{:02}",
                                    iabs(off / 3600),
                                    iabs((off % 3600) / 60),
                                    iabs(seconds)
                                )
                            };
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            b'Z' => write!(buf, "{}", if localtime { off } else { 0 }),

            // full date/time
            b'c' => write!(
                buf,
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                t.y,
                t.m as i32,
                t.d as i32,
                t.h as i32,
                t.i as i32,
                t.s as i32,
                sign(localtime),
                hours,
                minutes
            ),
            b'r' => write!(
                buf,
                "{:>3}, {:02} {:>3} {:04} {:02}:{:02}:{:02} {}{:02}{:02}",
                short_day_name(t.y, t.m, t.d),
                t.d as i32,
                month_name(&MON_SHORT_NAMES, t.m),
                t.y,
                t.h as i32,
                t.i as i32,
                t.s as i32,
                sign(localtime),
                hours,
                minutes
            ),
            b'U' => write!(buf, "{}", t.sse),

            b'\\' => {
                if i < format.len() {
                    i += 1;
                }
                // `format[format_len]` is the zend_string's terminating NUL.
                buf.push(format.get(i).copied().unwrap_or(0));
                Ok(())
            }

            other => {
                buf.push(other);
                Ok(())
            }
        };
        // slprintf() into `char buffer[97]`.
        buf.truncate(96);
        out.extend_from_slice(&buf);
        i += 1;
    }
    out
}

/// `(t->h % 12) ? (int) t->h % 12 : 12`.
fn twelve_hour(h: i64) -> i32 {
    if h % 12 != 0 { (h as i32) % 12 } else { 12 }
}

/// Port of `date_interval_format()` (PHPSRC/ext/date/php_date.c): `%y %Y %m %M %d %D %h %H %i %I %s %S %f %F
/// %a %R %r %%`; unknown specifiers are kept with their `%`, a trailing lone `%` is dropped.
pub fn date_interval_format(format: &[u8], t: &RelTime) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(format.len() * 2);
    if format.is_empty() {
        return out;
    }
    let mut have_format_spec = false;
    let mut buf: Vec<u8> = Vec::with_capacity(32);
    for &c in format {
        if have_format_spec {
            buf.clear();
            let _ = match c {
                b'Y' => write!(buf, "{:02}", t.y as i32),
                b'y' => write!(buf, "{}", t.y as i32),

                b'M' => write!(buf, "{:02}", t.m as i32),
                b'm' => write!(buf, "{}", t.m as i32),

                b'D' => write!(buf, "{:02}", t.d as i32),
                b'd' => write!(buf, "{}", t.d as i32),

                b'H' => write!(buf, "{:02}", t.h as i32),
                b'h' => write!(buf, "{}", t.h as i32),

                b'I' => write!(buf, "{:02}", t.i as i32),
                b'i' => write!(buf, "{}", t.i as i32),

                b'S' => write!(buf, "{:02}", t.s),
                b's' => write!(buf, "{}", t.s),

                b'F' => write!(buf, "{:06}", t.us),
                b'f' => write!(buf, "{}", t.us),

                b'a' => {
                    if t.days as i32 != UNSET as i32 {
                        write!(buf, "{}", t.days as i32)
                    } else {
                        write!(buf, "(unknown)")
                    }
                }
                b'r' => write!(buf, "{}", if t.invert != 0 { "-" } else { "" }),
                b'R' => write!(buf, "{}", if t.invert != 0 { '-' } else { '+' }),

                b'%' => write!(buf, "%"),
                other => {
                    buf.push(b'%');
                    buf.push(other);
                    Ok(())
                }
            };
            // slprintf() into `char buffer[33]`.
            buf.truncate(32);
            out.extend_from_slice(&buf);
            have_format_spec = false;
        } else if c == b'%' {
            have_format_spec = true;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use twig_timelib::TzCache;

    fn utc_time(ts: i64) -> Time {
        let mut t = Time::new();
        t.zone_type = ZONETYPE_ID;
        t.tz_info = TzCache::builtin().get("UTC");
        t.unixtime2local(ts);
        t
    }

    #[test]
    fn letters() {
        // 2024-03-04 05:06:07 UTC (reference/php/stdlib.md §13.3).
        let t = utc_time(1_709_528_767);
        let got = date_format(
            b"d D j l N S w z W F m M n t L o X x Y y a A B g G h H i s",
            &t,
            true,
        );
        assert_eq!(
            String::from_utf8(got).unwrap(),
            "04 Mon 4 Monday 1 th 1 63 10 March 03 Mar 3 31 1 2024 +2024 2024 2024 24 am AM 254 5 5 05 05 06 07"
        );
        assert_eq!(date_format(b"Y\\", &t, true), b"2024\0");
        assert_eq!(date_format(b"\\Y\\m", &t, true), b"Ym");
        assert_eq!(date_format(b"", &t, true), b"");
        assert_eq!(
            date_format(b"c|r|U|e|T|P|p|O|Z|I", &t, true),
            b"2024-03-04T05:06:07+00:00|Mon, 04 Mar 2024 05:06:07 +0000|1709528767|UTC|UTC|+00:00|Z|+0000|0|0"
        );
        assert_eq!(date_format(b"e T p", &t, false), b"UTC GMT Z");
    }

    #[test]
    fn offsets() {
        assert_eq!(tz_offset_str(19800), b"+05:30");
        assert_eq!(tz_offset_str(-3601), b"-01:00:01");
        // 100 h 39 min: snprintf() truncation keeps the NUL inside the string.
        assert_eq!(tz_offset_str(362_340), b"+100:3\0");
        assert_eq!(gmt_offset_abbr(-12600), b"GMT-0330");
    }

    #[test]
    fn interval() {
        let rt = RelTime {
            y: 0,
            m: 2,
            d: 4,
            h: 10,
            i: 20,
            s: 30,
            days: 64,
            ..RelTime::default()
        };
        let got = date_interval_format(
            b"%y %Y %m %M %d %D %h %H %i %I %s %S %f %F %a %R %r %% %q %",
            &rt,
        );
        assert_eq!(
            String::from_utf8(got).unwrap(),
            "0 00 2 02 4 04 10 10 20 20 30 30 0 000000 64 +  % %q "
        );
        let unknown = RelTime {
            days: UNSET,
            invert: 1,
            ..RelTime::default()
        };
        assert_eq!(date_interval_format(b"%a%R%r", &unknown), b"(unknown)--");
    }
}
