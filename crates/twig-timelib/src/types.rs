//! Core data structures of timelib (`PHPSRC/ext/date/lib/timelib.h`), mirrored field for field.
//!
//! Field names, widths and signedness follow the C structs so that ported code reads like the original and
//! overflow behaviour can be reproduced. Integer arithmetic on these fields uses wrapping operations wherever
//! user input can drive values to the edges (C uses plain signed arithmetic there; see the crate docs).

use std::sync::Arc;

use crate::tz::TzInfo;

/// `TIMELIB_UNSET`: marker for fields the parser did not fill.
pub const UNSET: i64 = -9_999_999;

/// `TIMELIB_ZONETYPE_NONE`.
pub const ZONETYPE_NONE: u32 = 0;
/// `TIMELIB_ZONETYPE_OFFSET`: a fixed UTC offset (`+02:00`).
pub const ZONETYPE_OFFSET: u32 = 1;
/// `TIMELIB_ZONETYPE_ABBR`: a zone abbreviation with a DST flag (`CEST`).
pub const ZONETYPE_ABBR: u32 = 2;
/// `TIMELIB_ZONETYPE_ID`: a tz database identifier (`Europe/Paris`).
pub const ZONETYPE_ID: u32 = 3;

/// `TIMELIB_NONE` option for [`Time::fill_holes`].
pub const NONE: i32 = 0x00;
/// `TIMELIB_OVERRIDE_TIME` option for [`Time::fill_holes`].
pub const OVERRIDE_TIME: i32 = 0x01;
/// `TIMELIB_NO_CLONE` option for [`Time::fill_holes`] (zone data is shared through `Arc` either way).
pub const NO_CLONE: i32 = 0x02;

/// `TIMELIB_SPECIAL_WEEKDAY` (`relative.special.type`).
pub const SPECIAL_WEEKDAY: u32 = 0x01;
/// `TIMELIB_SPECIAL_DAY_OF_WEEK_IN_MONTH` (`relative.special.type`).
pub const SPECIAL_DAY_OF_WEEK_IN_MONTH: u32 = 0x02;
/// `TIMELIB_SPECIAL_LAST_DAY_OF_WEEK_IN_MONTH` (`relative.special.type`).
pub const SPECIAL_LAST_DAY_OF_WEEK_IN_MONTH: u32 = 0x03;

/// `TIMELIB_SPECIAL_FIRST_DAY_OF_MONTH` (`relative.first_last_day_of`).
pub const SPECIAL_FIRST_DAY_OF_MONTH: i32 = 0x01;
/// `TIMELIB_SPECIAL_LAST_DAY_OF_MONTH` (`relative.first_last_day_of`).
pub const SPECIAL_LAST_DAY_OF_MONTH: i32 = 0x02;

pub(crate) const SECS_PER_DAY: i64 = 86_400;
pub(crate) const SECS_PER_HOUR: i64 = 3_600;
pub(crate) const USECS_PER_HOUR: i64 = 3_600_000_000;
pub(crate) const MINS_PER_HOUR: i64 = 60;
pub(crate) const DAYS_PER_YEAR: i64 = 365;
pub(crate) const DAYS_PER_ERA: i64 = 146_097;
pub(crate) const YEARS_PER_ERA: i64 = 400;
pub(crate) const HINNANT_EPOCH_SHIFT: i64 = 719_468;

/// `timelib_is_leap()` (macro in `timelib_private.h`).
#[inline]
pub fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// The `special` member of `timelib_rel_time`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Special {
    /// One of the `SPECIAL_*` weekday constants, or 0.
    pub kind: u32,
    /// Amount for `SPECIAL_WEEKDAY` ("+3 weekdays").
    pub amount: i64,
}

/// `timelib_rel_time`: a relative time ("+1 month", "next monday", a `DateInterval`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelTime {
    /// Years.
    pub y: i64,
    /// Months.
    pub m: i64,
    /// Days.
    pub d: i64,
    /// Hours.
    pub h: i64,
    /// Minutes.
    pub i: i64,
    /// Seconds.
    pub s: i64,
    /// Microseconds.
    pub us: i64,
    /// Stores the day in 'next monday' (0 = Sunday .. 6 = Saturday, negative after "ago").
    pub weekday: i32,
    /// 0: the current day should not be counted when advancing forwards; 1: it should; 2: "this week" logic.
    pub weekday_behavior: i32,
    /// `SPECIAL_FIRST_DAY_OF_MONTH`, `SPECIAL_LAST_DAY_OF_MONTH` or 0.
    pub first_last_day_of: i32,
    /// Whether the difference is inverted (`DateInterval::$invert`).
    pub invert: i32,
    /// Number of whole days (`DateInterval::$days`), or [`UNSET`].
    pub days: i64,
    /// "weekday" / "x y of z month" specials.
    pub special: Special,
    /// Set when `weekday` is meaningful.
    pub have_weekday_relative: u32,
    /// Set when `special` is meaningful.
    pub have_special_relative: u32,
}

/// `timelib_time_offset`: the result of a zone lookup for one instant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeOffset {
    /// UTC offset in seconds east of UTC.
    pub offset: i32,
    /// Leap seconds in effect (always 0 with PHP's database).
    pub leap_secs: u32,
    /// 1 when DST is in effect.
    pub is_dst: u32,
    /// Zone abbreviation in effect (as stored in the database, not upper-cased).
    pub abbr: String,
    /// Time of the transition that started this period (`i64::MIN` before the first one).
    pub transition_time: i64,
}

/// `timelib_abbr_info`: a zone abbreviation with its offset, as stored by `DateTimeZone('CEST')`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbbrInfo {
    /// UTC offset in seconds (without the DST hour).
    pub utc_offset: i64,
    /// The abbreviation (upper case).
    pub abbr: String,
    /// 1 for a DST abbreviation.
    pub dst: i32,
}

/// `timelib_time`: a broken-down date/time with zone information, parse flags and a pending relative part.
#[derive(Clone, Debug, PartialEq)]
pub struct Time {
    /// Year.
    pub y: i64,
    /// Month.
    pub m: i64,
    /// Day.
    pub d: i64,
    /// Hour.
    pub h: i64,
    /// Minute.
    pub i: i64,
    /// Second.
    pub s: i64,
    /// Microseconds.
    pub us: i64,
    /// UTC offset in seconds (for offset and abbreviation zones; cached offset for identifier zones).
    pub z: i32,
    /// Zone abbreviation (display only).
    pub tz_abbr: Option<String>,
    /// Zone rules for identifier zones.
    pub tz_info: Option<Arc<TzInfo>>,
    /// DST flag.
    pub dst: i32,
    /// Pending relative part.
    pub relative: RelTime,
    /// Seconds since the Unix epoch.
    pub sse: i64,
    /// Parser flag: a time was found (a counter in C: `gnunocolon` increments it).
    pub have_time: u32,
    /// Parser flag: a date was found.
    pub have_date: u32,
    /// Parser flag: a zone was found (a counter: a second zone is a warning, a third an error).
    pub have_zone: u32,
    /// A relative part is pending.
    pub have_relative: u32,
    /// Unused by timelib 2022.17 (kept for layout parity).
    pub have_weeknr_day: u32,
    /// `sse` is up to date with the broken-down fields.
    pub sse_uptodate: u32,
    /// The broken-down fields are up to date with `sse`.
    pub tim_uptodate: u32,
    /// 1 if the struct represents local time, 0 if it is in GMT.
    pub is_localtime: u32,
    /// One of the `ZONETYPE_*` constants.
    pub zone_type: u32,
}

impl Default for Time {
    fn default() -> Self {
        Self::new()
    }
}

impl Time {
    /// `timelib_time_ctor()`: an all-zero time (calloc).
    pub fn new() -> Self {
        Time {
            y: 0,
            m: 0,
            d: 0,
            h: 0,
            i: 0,
            s: 0,
            us: 0,
            z: 0,
            tz_abbr: None,
            tz_info: None,
            dst: 0,
            relative: RelTime::default(),
            sse: 0,
            have_time: 0,
            have_date: 0,
            have_zone: 0,
            have_relative: 0,
            have_weeknr_day: 0,
            sse_uptodate: 0,
            tim_uptodate: 0,
            is_localtime: 0,
            zone_type: 0,
        }
    }

    /// Port of `timelib_time_compare()` (PHPSRC/ext/date/lib/timelib.c): orders by `sse`, then `us`.
    pub fn compare(&self, other: &Time) -> i32 {
        if self.sse == other.sse {
            if self.us == other.us {
                return 0;
            }
            return if self.us < other.us { -1 } else { 1 };
        }
        if self.sse < other.sse { -1 } else { 1 }
    }

    /// Port of `timelib_time_tz_abbr_update()` (PHPSRC/ext/date/lib/timelib.c): stores an upper-cased copy.
    pub fn tz_abbr_update(&mut self, tz_abbr: &str) {
        self.tz_abbr = Some(tz_abbr.to_ascii_uppercase());
    }

    /// Port of `timelib_date_to_int()` (PHPSRC/ext/date/lib/timelib.c): `sse` as a 64-bit PHP integer.
    ///
    /// Always `Some` on 64-bit `timelib_long` platforms, which is the configuration PHP 8.5 uses here.
    pub fn date_to_int(&self) -> Option<i64> {
        Some(self.sse)
    }
}

/// `timelib_error_message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorMessage {
    /// One of the `ERR_*` / `WARN_*` codes.
    pub error_code: i32,
    /// Byte position in the (trimmed) input.
    pub position: i32,
    /// The byte at `position` (0 at the end of the input).
    pub character: u8,
    /// Verbatim timelib message.
    pub message: &'static str,
}

/// `TIMELIB_MAX_ERRORS`: at most this many errors and warnings are recorded.
pub const MAX_ERRORS: usize = 1000;

/// `timelib_error_container`: errors and warnings collected by the parsers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErrorContainer {
    /// Errors (`error_count` is `error_messages.len()`).
    pub error_messages: Vec<ErrorMessage>,
    /// Warnings (`warning_count` is `warning_messages.len()`).
    pub warning_messages: Vec<ErrorMessage>,
}

impl ErrorContainer {
    /// `error_count`.
    pub fn error_count(&self) -> usize {
        self.error_messages.len()
    }

    /// `warning_count`.
    pub fn warning_count(&self) -> usize {
        self.warning_messages.len()
    }
}

/// `TIMELIB_WARN_DOUBLE_TZ`.
pub const WARN_DOUBLE_TZ: i32 = 0x101;
/// `TIMELIB_WARN_INVALID_TIME`.
pub const WARN_INVALID_TIME: i32 = 0x102;
/// `TIMELIB_WARN_INVALID_DATE`.
pub const WARN_INVALID_DATE: i32 = 0x103;
/// `TIMELIB_WARN_TRAILING_DATA`.
pub const WARN_TRAILING_DATA: i32 = 0x11a;

/// `TIMELIB_ERR_DOUBLE_TZ`.
pub const ERR_DOUBLE_TZ: i32 = 0x201;
/// `TIMELIB_ERR_TZID_NOT_FOUND`.
pub const ERR_TZID_NOT_FOUND: i32 = 0x202;
/// `TIMELIB_ERR_DOUBLE_TIME`.
pub const ERR_DOUBLE_TIME: i32 = 0x203;
/// `TIMELIB_ERR_DOUBLE_DATE`.
pub const ERR_DOUBLE_DATE: i32 = 0x204;
/// `TIMELIB_ERR_UNEXPECTED_CHARACTER`.
pub const ERR_UNEXPECTED_CHARACTER: i32 = 0x205;
/// `TIMELIB_ERR_EMPTY_STRING`.
pub const ERR_EMPTY_STRING: i32 = 0x206;
/// `TIMELIB_ERR_UNEXPECTED_DATA`.
pub const ERR_UNEXPECTED_DATA: i32 = 0x207;
/// `TIMELIB_ERR_NO_TEXTUAL_DAY`.
pub const ERR_NO_TEXTUAL_DAY: i32 = 0x208;
/// `TIMELIB_ERR_NO_TWO_DIGIT_DAY`.
pub const ERR_NO_TWO_DIGIT_DAY: i32 = 0x209;
/// `TIMELIB_ERR_NO_THREE_DIGIT_DAY_OF_YEAR`.
pub const ERR_NO_THREE_DIGIT_DAY_OF_YEAR: i32 = 0x20a;
/// `TIMELIB_ERR_NO_TWO_DIGIT_MONTH`.
pub const ERR_NO_TWO_DIGIT_MONTH: i32 = 0x20b;
/// `TIMELIB_ERR_NO_TEXTUAL_MONTH`.
pub const ERR_NO_TEXTUAL_MONTH: i32 = 0x20c;
/// `TIMELIB_ERR_NO_TWO_DIGIT_YEAR`.
pub const ERR_NO_TWO_DIGIT_YEAR: i32 = 0x20d;
/// `TIMELIB_ERR_NO_FOUR_DIGIT_YEAR`.
pub const ERR_NO_FOUR_DIGIT_YEAR: i32 = 0x20e;
/// `TIMELIB_ERR_NO_TWO_DIGIT_HOUR`.
pub const ERR_NO_TWO_DIGIT_HOUR: i32 = 0x20f;
/// `TIMELIB_ERR_HOUR_LARGER_THAN_12`.
pub const ERR_HOUR_LARGER_THAN_12: i32 = 0x210;
/// `TIMELIB_ERR_MERIDIAN_BEFORE_HOUR`.
pub const ERR_MERIDIAN_BEFORE_HOUR: i32 = 0x211;
/// `TIMELIB_ERR_NO_MERIDIAN`.
pub const ERR_NO_MERIDIAN: i32 = 0x212;
/// `TIMELIB_ERR_NO_TWO_DIGIT_MINUTE`.
pub const ERR_NO_TWO_DIGIT_MINUTE: i32 = 0x213;
/// `TIMELIB_ERR_NO_TWO_DIGIT_SECOND`.
pub const ERR_NO_TWO_DIGIT_SECOND: i32 = 0x214;
/// `TIMELIB_ERR_NO_SIX_DIGIT_MICROSECOND`.
pub const ERR_NO_SIX_DIGIT_MICROSECOND: i32 = 0x215;
/// `TIMELIB_ERR_NO_SEP_SYMBOL`.
pub const ERR_NO_SEP_SYMBOL: i32 = 0x216;
/// `TIMELIB_ERR_EXPECTED_ESCAPE_CHAR`.
pub const ERR_EXPECTED_ESCAPE_CHAR: i32 = 0x217;
/// `TIMELIB_ERR_NO_ESCAPED_CHAR`.
pub const ERR_NO_ESCAPED_CHAR: i32 = 0x218;
/// `TIMELIB_ERR_WRONG_FORMAT_SEP`.
pub const ERR_WRONG_FORMAT_SEP: i32 = 0x219;
/// `TIMELIB_ERR_TRAILING_DATA`.
pub const ERR_TRAILING_DATA: i32 = 0x21a;
/// `TIMELIB_ERR_DATA_MISSING`.
pub const ERR_DATA_MISSING: i32 = 0x21b;
/// `TIMELIB_ERR_NO_THREE_DIGIT_MILLISECOND`.
pub const ERR_NO_THREE_DIGIT_MILLISECOND: i32 = 0x21c;
/// `TIMELIB_ERR_NO_FOUR_DIGIT_YEAR_ISO`.
pub const ERR_NO_FOUR_DIGIT_YEAR_ISO: i32 = 0x21d;
/// `TIMELIB_ERR_NO_TWO_DIGIT_WEEK`.
pub const ERR_NO_TWO_DIGIT_WEEK: i32 = 0x21e;
/// `TIMELIB_ERR_INVALID_WEEK`.
pub const ERR_INVALID_WEEK: i32 = 0x21f;
/// `TIMELIB_ERR_NO_DAY_OF_WEEK`.
pub const ERR_NO_DAY_OF_WEEK: i32 = 0x220;
/// `TIMELIB_ERR_INVALID_DAY_OF_WEEK`.
pub const ERR_INVALID_DAY_OF_WEEK: i32 = 0x221;
/// `TIMELIB_ERR_INVALID_SPECIFIER`.
pub const ERR_INVALID_SPECIFIER: i32 = 0x222;
/// `TIMELIB_ERR_INVALID_TZ_OFFSET`.
pub const ERR_INVALID_TZ_OFFSET: i32 = 0x223;
/// `TIMELIB_ERR_FORMAT_LITERAL_MISMATCH`.
pub const ERR_FORMAT_LITERAL_MISMATCH: i32 = 0x224;
/// `TIMELIB_ERR_MIX_ISO_WITH_NATURAL`.
pub const ERR_MIX_ISO_WITH_NATURAL: i32 = 0x225;
/// `TIMELIB_ERR_NUMBER_OUT_OF_RANGE`.
pub const ERR_NUMBER_OUT_OF_RANGE: i32 = 0x226;
