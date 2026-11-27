//! POSIX TZ strings: the footer of a (slim) tzfile that describes transitions after the last stored one
//! (port of `PHPSRC/ext/date/lib/parse_posix.c`).

use crate::types::{DAYS_PER_YEAR, SECS_PER_DAY, Time, UNSET, is_leap};
use crate::tz::TzInfo;

/// `TIMELIB_POSIX_TRANS_TYPE_JULIAN_NO_FEB29`: `Jn`.
pub const TRANS_TYPE_JULIAN_NO_FEB29: i32 = 1;
/// `TIMELIB_POSIX_TRANS_TYPE_JULIAN_FEB29`: `n`.
pub const TRANS_TYPE_JULIAN_FEB29: i32 = 2;
/// `TIMELIB_POSIX_TRANS_TYPE_MWD`: `Mm.w.d`.
pub const TRANS_TYPE_MWD: i32 = 3;

/// `timelib_posix_trans_info`: one `start[/time]` or `end[/time]` rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PosixTransInfo {
    /// One of the `TRANS_TYPE_*` constants.
    pub kind: i32,
    /// Day number for the Julian forms (the C union member `days`).
    pub days: i32,
    /// Month for `Mm.w.d`.
    pub month: i32,
    /// Week for `Mm.w.d`.
    pub week: i32,
    /// Day of week for `Mm.w.d`.
    pub dow: i32,
    /// Transition time of day in seconds (default 02:00).
    pub hour: i32,
}

impl PosixTransInfo {
    /// `timelib_posix_trans_info_ctor()`.
    fn new() -> Self {
        PosixTransInfo {
            kind: TRANS_TYPE_JULIAN_FEB29,
            days: 0,
            month: 0,
            week: 0,
            dow: 0,
            hour: 2 * 3600,
        }
    }
}

/// `timelib_posix_str`: a parsed POSIX TZ string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PosixStr {
    /// Standard time abbreviation.
    pub std: String,
    /// Standard time UTC offset (seconds east).
    pub std_offset: i64,
    /// DST abbreviation, if the zone has DST.
    pub dst: Option<String>,
    /// DST UTC offset (seconds east).
    pub dst_offset: i64,
    /// Start of DST.
    pub dst_begin: Option<PosixTransInfo>,
    /// End of DST.
    pub dst_end: Option<PosixTransInfo>,
    /// Index into the zone's types for standard time.
    pub type_index_std_type: i32,
    /// Index into the zone's types for DST.
    pub type_index_dst_type: i32,
}

/// `timelib_posix_transitions`: transitions computed for up to three years.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PosixTransitions {
    /// Number of valid entries.
    pub count: usize,
    /// Transition instants.
    pub times: [i64; 6],
    /// Type indexes in effect from the matching instant on.
    pub types: [i64; 6],
}

/// A NUL-terminated byte cursor (the C code walks `char *` pointers).
struct Cursor<'a> {
    s: &'a [u8],
    p: usize,
}

impl Cursor<'_> {
    fn cur(&self) -> u8 {
        self.s.get(self.p).copied().unwrap_or(0)
    }
}

/// `read_description_numeric_abbr()`: `"<" [+-]? .+? ">"`.
fn read_description_numeric_abbr(c: &mut Cursor<'_>) -> Option<String> {
    let begin = c.p + 1;
    // skip '<'
    c.p += 1;
    while c.cur() != 0 && c.cur() != b'>' {
        c.p += 1;
    }
    if c.cur() == 0 {
        return None;
    }
    if c.cur() == b'>' {
        c.p += 1;
    }
    // Abbreviation may not be empty
    if (c.p as isize) - (begin as isize) - 1 < 1 {
        return None;
    }
    Some(String::from_utf8_lossy(&c.s[begin..c.p - 1]).into_owned())
}

/// `read_description_abbr()`: `[A-Za-z]+`.
fn read_description_abbr(c: &mut Cursor<'_>) -> Option<String> {
    let begin = c.p;
    while c.cur().is_ascii_alphabetic() {
        c.p += 1;
    }
    if c.p - begin < 1 {
        return None;
    }
    Some(String::from_utf8_lossy(&c.s[begin..c.p]).into_owned())
}

/// `read_description()`.
fn read_description(c: &mut Cursor<'_>) -> Option<String> {
    if c.cur() == b'<' {
        read_description_numeric_abbr(c)
    } else {
        read_description_abbr(c)
    }
}

/// `read_sign()`.
fn read_sign(c: &mut Cursor<'_>) -> i32 {
    let mut bias = 1;
    if c.cur() == b'+' {
        c.p += 1;
    } else if c.cur() == b'-' {
        bias = -1;
        c.p += 1;
    }
    bias
}

/// `read_number()`: `[0-9]+`, accumulated in a C `int`.
fn read_number(c: &mut Cursor<'_>) -> i64 {
    let begin = c.p;
    let mut acc: i32 = 0;
    // skip leading 0's
    while c.cur() == b'0' {
        c.p += 1;
    }
    while c.cur().is_ascii_digit() {
        acc = acc.wrapping_mul(10);
        acc = acc.wrapping_add(i32::from(c.cur() - b'0'));
        c.p += 1;
    }
    if begin == c.p {
        return UNSET;
    }
    i64::from(acc)
}

/// `read_offset()`: `[+-]? [0-9]+ ( ":" [0-9]+ ( ":" [0-9]+ )? )?`, returned with the POSIX sign flipped.
fn read_offset(c: &mut Cursor<'_>) -> i64 {
    let bias = read_sign(c);
    let begin = c.p;

    // read through to : or non-digit for hours
    let hours = read_number(c) as i32;
    if i64::from(hours) == UNSET {
        return i64::from(hours);
    }
    let mut minutes: i32 = 0;
    let mut seconds: i32 = 0;

    // check for optional minutes
    if c.cur() == b':' {
        c.p += 1;
        minutes = read_number(c) as i32;
        if i64::from(minutes) == UNSET {
            return i64::from(minutes);
        }
    }

    // check for optional seconds
    if c.cur() == b':' {
        c.p += 1;
        seconds = read_number(c) as i32;
        if i64::from(seconds) == UNSET {
            return i64::from(seconds);
        }
    }

    if begin == c.p {
        return UNSET;
    }

    // multiplication with -1, because the offset in the identifier is the 'wrong' way around as for example
    // EST5 is UTC-5 (and not +5)
    i64::from(
        (-1i32).wrapping_mul(bias).wrapping_mul(
            hours
                .wrapping_mul(3600)
                .wrapping_add(minutes.wrapping_mul(60))
                .wrapping_add(seconds),
        ),
    )
}

/// `read_trans_spec_mwd()`: `Mm.w.d`.
fn read_trans_spec_mwd(c: &mut Cursor<'_>) -> Option<PosixTransInfo> {
    let mut tmp = PosixTransInfo::new();
    tmp.kind = TRANS_TYPE_MWD;

    // Skip 'M'
    c.p += 1;

    tmp.month = read_number(c) as i32;
    if i64::from(tmp.month) == UNSET {
        return None;
    }
    if c.cur() != b'.' {
        return None;
    }
    c.p += 1;

    tmp.week = read_number(c) as i32;
    if i64::from(tmp.week) == UNSET {
        return None;
    }
    if c.cur() != b'.' {
        return None;
    }
    c.p += 1;

    tmp.dow = read_number(c) as i32;
    if i64::from(tmp.dow) == UNSET {
        return None;
    }
    Some(tmp)
}

/// `read_transition_spec()`: `(Jn | n | Mw.m.d) ( /time )?`.
fn read_transition_spec(c: &mut Cursor<'_>) -> Option<PosixTransInfo> {
    let mut tmp;
    if c.cur() == b'M' {
        tmp = read_trans_spec_mwd(c)?;
    } else {
        tmp = PosixTransInfo::new();
        if c.cur() == b'J' {
            tmp.kind = TRANS_TYPE_JULIAN_NO_FEB29;
            c.p += 1;
        }
        tmp.days = read_number(c) as i32;
        if i64::from(tmp.days) == UNSET {
            return None;
        }
    }

    // Check for the optional hour
    if c.cur() == b'/' {
        c.p += 1;
        tmp.hour = read_offset(c) as i32;
        if i64::from(tmp.hour) == UNSET {
            return None;
        }
        // as the bias for normal offsets = -1, we need to reverse it here
        tmp.hour = tmp.hour.wrapping_neg();
    }
    Some(tmp)
}

/// Port of `timelib_parse_posix_str()` (PHPSRC/ext/date/lib/parse_posix.c). `None` for an invalid string.
pub fn parse_posix_str(posix: &str) -> Option<PosixStr> {
    let mut c = Cursor {
        s: posix.as_bytes(),
        p: 0,
    };

    // read standard description (ie. EST or <-03>)
    let std = read_description(&mut c)?;

    // read required offset
    let std_offset = read_offset(&mut c);
    if std_offset == UNSET {
        return None;
    }

    let mut tmp = PosixStr {
        std,
        std_offset,
        dst: None,
        dst_offset: 0,
        dst_begin: None,
        dst_end: None,
        type_index_std_type: 0,
        type_index_dst_type: 0,
    };

    // if we're at the end return, otherwise we'll continue to try to parse the dst abbreviation and spec
    if c.cur() == 0 {
        return Some(tmp);
    }

    // assume dst is there, and initialise offset
    tmp.dst_offset = tmp.std_offset.wrapping_add(3600);

    tmp.dst = Some(read_description(&mut c)?);

    // if we have a "," here, then the dst offset is the standard offset + 3600 seconds, otherwise, try to parse
    // the dst offset
    if c.cur() != b',' && c.cur() != 0 {
        tmp.dst_offset = read_offset(&mut c);
        if tmp.dst_offset == UNSET {
            return None;
        }
    }

    // if we *don't* have a "," here, we're missing the dst transitions ,start[/time],end[/time]
    if c.cur() != b',' {
        return None;
    }
    c.p += 1;

    // start[/time]
    tmp.dst_begin = Some(read_transition_spec(&mut c)?);

    // if we *don't* have a "," here, we're missing the dst end transition ,end[/time]
    if c.cur() != b',' {
        return None;
    }
    c.p += 1;

    // end[/time]
    tmp.dst_end = Some(read_transition_spec(&mut c)?);

    // make sure there is no trailing data
    if c.cur() != 0 {
        return None;
    }
    Some(tmp)
}

const MONTH_LENGTHS: [[i32; 12]; 2] = [
    [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31], // normal year
    [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31], // leap year
];

/// `calc_transition()`: seconds since the start of `year` (adapted by timelib from tzcode's `transtime()`).
fn calc_transition(psi: &PosixTransInfo, year: i64) -> i64 {
    let leap_year = usize::from(is_leap(year));

    match psi.kind {
        TRANS_TYPE_JULIAN_NO_FEB29 => {
            let mut value = i64::from(psi.days) - 1;
            if leap_year != 0 && psi.days >= 60 {
                value += 1;
            }
            value.wrapping_mul(SECS_PER_DAY)
        }
        // `psi->days * SECS_PER_DAY` is an `int` multiplication in C.
        TRANS_TYPE_JULIAN_FEB29 => i64::from(psi.days.wrapping_mul(SECS_PER_DAY as i32)),
        TRANS_TYPE_MWD => {
            // Mm.n.d - nth "dth day" of month m. The C code computes this in `int`.
            // Use Zeller's Congruence to get day-of-week of first day of month.
            let m1 = (psi.month.wrapping_add(9)) % 12 + 1;
            let yy0: i32 = if psi.month <= 2 {
                year.wrapping_sub(1) as i32
            } else {
                year as i32
            };
            let yy1 = yy0 / 100;
            let yy2 = yy0 % 100;
            let mut dow =
                ((26i32.wrapping_mul(m1) - 2) / 10 + 1 + yy2 + yy2 / 4 + yy1 / 4 - 2 * yy1) % 7;
            if dow < 0 {
                dow += 7;
            }

            // "dow" is the day-of-week of the first day of the month. Get the day-of-month (zero-origin) of the
            // first "dow" day of the month.
            let mut d = psi.dow - dow;
            if d < 0 {
                d += 7;
            }
            let month_len = |m: i32| -> i32 { MONTH_LENGTHS[leap_year][(m.clamp(0, 11)) as usize] };
            let mut i = 1;
            while i < psi.week {
                if d + 7 >= month_len(psi.month - 1) {
                    break;
                }
                d += 7;
                i += 1;
            }

            // "d" is the day-of-month (zero-origin) of the day we want.
            let mut value = i64::from(d) * SECS_PER_DAY;
            let mut i = 0;
            while i < psi.month - 1 {
                value += i64::from(month_len(i)) * SECS_PER_DAY;
                i += 1;
            }
            value
        }
        _ => 0,
    }
}

/// `count_leap_years()`: leap days before Jan 1st of `y`.
fn count_leap_years(y: i64) -> i64 {
    // Because we want this for Jan 1, the leap day hasn't happend yet, so subtract one of year before we
    // calculate
    let y = y.wrapping_sub(1);
    (y / 4) - (y / 100) + (y / 400)
}

/// Port of `timelib_ts_at_start_of_year()` (PHPSRC/ext/date/lib/parse_posix.c).
pub fn ts_at_start_of_year(year: i64) -> i64 {
    let epoch_leap_years = count_leap_years(1970);
    let current_leap_years = count_leap_years(year);
    SECS_PER_DAY.wrapping_mul(
        year.wrapping_sub(1970)
            .wrapping_mul(DAYS_PER_YEAR)
            .wrapping_add(current_leap_years)
            .wrapping_sub(epoch_leap_years),
    )
}

/// Port of `timelib_get_transitions_for_year()` (PHPSRC/ext/date/lib/parse_posix.c): appends the two DST
/// transitions of `year` to `transitions`. The zone must have a POSIX string with DST rules.
pub fn get_transitions_for_year(tz: &TzInfo, year: i64, transitions: &mut PosixTransitions) {
    let Some(pi) = tz.posix_info.as_ref() else {
        return;
    };
    let (Some(begin), Some(end)) = (pi.dst_begin.as_ref(), pi.dst_end.as_ref()) else {
        return;
    };
    if transitions.count + 2 > transitions.times.len() {
        return;
    }
    let [first, first_type, second, second_type] = tz.posix_cache.year(year, || {
        let year_begin_ts = ts_at_start_of_year(year);

        let mut trans_begin = year_begin_ts;
        trans_begin = trans_begin.wrapping_add(calc_transition(begin, year));
        trans_begin = trans_begin.wrapping_add(i64::from(begin.hour));
        trans_begin = trans_begin.wrapping_sub(pi.std_offset);

        let mut trans_end = year_begin_ts;
        trans_end = trans_end.wrapping_add(calc_transition(end, year));
        trans_end = trans_end.wrapping_add(i64::from(end.hour));
        trans_end = trans_end.wrapping_sub(pi.dst_offset);

        let (dst, std) = (
            i64::from(pi.type_index_dst_type),
            i64::from(pi.type_index_std_type),
        );
        if trans_begin < trans_end {
            [trans_begin, dst, trans_end, std]
        } else {
            [trans_end, std, trans_begin, dst]
        }
    });

    let n = transitions.count;
    transitions.times[n] = first;
    transitions.types[n] = first_type;
    transitions.times[n + 1] = second;
    transitions.types[n + 1] = second_type;
    transitions.count += 2;
}

/// Port of `timelib_fetch_posix_timezone_offset()` (PHPSRC/ext/date/lib/parse_posix.c): the type in effect at
/// `ts` according to the POSIX string, plus the transition time that started it when `want_transition` is set.
pub(crate) fn fetch_posix_timezone_offset(
    tz: &TzInfo,
    ts: i64,
    want_transition: bool,
) -> Option<(usize, i64)> {
    let pi = tz.posix_info.as_ref()?;

    // If there is no second (dst_end) information, the UTC offset is valid for the whole year, so no need to do
    // clever logic
    if pi.dst_end.is_none() {
        let transition_time = if want_transition {
            tz.trans.last().copied().unwrap_or(0)
        } else {
            0
        };
        return Some((pi.type_index_std_type as usize, transition_time));
    }

    // Find 'year' (UTC) for 'ts'
    let mut dummy = Time::new();
    dummy.unixtime2gmt(ts);
    let year = dummy.y;

    // Calculate transition times for 'year-1', 'year', and 'year+1'
    let mut transitions = PosixTransitions::default();
    get_transitions_for_year(tz, year.wrapping_sub(1), &mut transitions);
    get_transitions_for_year(tz, year, &mut transitions);
    get_transitions_for_year(tz, year.wrapping_add(1), &mut transitions);

    // Check where the 'ts' falls in the 4 transitions
    for i in 1..transitions.count {
        if ts < transitions.times[i] {
            return Some((transitions.types[i - 1] as usize, transitions.times[i - 1]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_strings() {
        let p = parse_posix_str("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        assert_eq!(p.std, "CET");
        assert_eq!(p.std_offset, 3600);
        assert_eq!(p.dst.as_deref(), Some("CEST"));
        assert_eq!(p.dst_offset, 7200);
        let b = p.dst_begin.unwrap();
        assert_eq!(
            (b.kind, b.month, b.week, b.dow, b.hour),
            (TRANS_TYPE_MWD, 3, 5, 0, 7200)
        );
        let e = p.dst_end.unwrap();
        assert_eq!((e.month, e.week, e.dow, e.hour), (10, 5, 0, 10800));

        let p = parse_posix_str("<-03>3").unwrap();
        assert_eq!(p.std, "-03");
        assert_eq!(p.std_offset, -10800);
        assert!(p.dst.is_none());

        assert!(parse_posix_str("EST5EDT").is_none());
        assert!(parse_posix_str("").is_none());
        assert!(parse_posix_str("<>1").is_none());
        let p = parse_posix_str("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0").unwrap();
        assert_eq!(p.std_offset, 37800);
        assert_eq!(p.dst_offset, 39600);
        let p = parse_posix_str("XXX3YYY,J60/-1:30,300/25").unwrap();
        assert_eq!(p.dst_begin.unwrap().kind, TRANS_TYPE_JULIAN_NO_FEB29);
        assert_eq!(p.dst_begin.unwrap().hour, -5400);
        assert_eq!(p.dst_end.unwrap().kind, TRANS_TYPE_JULIAN_FEB29);
        assert_eq!(p.dst_end.unwrap().hour, 90000);
    }

    #[test]
    fn start_of_year() {
        assert_eq!(ts_at_start_of_year(1970), 0);
        assert_eq!(ts_at_start_of_year(2024), 1_704_067_200);
        assert_eq!(ts_at_start_of_year(1969), -31_536_000);
    }
}
