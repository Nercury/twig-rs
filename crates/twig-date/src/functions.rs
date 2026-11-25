//! The procedural date functions of PHPSRC/ext/date/php_date.c: `date()`, `gmdate()`, `idate()`, `mktime()`,
//! `gmmktime()`, `checkdate()`, `strtotime()`, `time()`, `getdate()`, `localtime()`, `date_parse()`,
//! `date_parse_from_format()`, the `date_create*()` / `timezone_*()` helpers and `date_modify()` /
//! `date_sub()` (whose failures warn instead of throwing).
//!
//! Arguments are already typed (`?int` parameters are `Option<i64>`); "now" and the default timezone come from
//! the [`DateContext`].

use twig_timelib::parse_date::{parse_from_format, strtotime as tl_strtotime};
use twig_timelib::types::{
    NO_CLONE, SPECIAL_FIRST_DAY_OF_MONTH, SPECIAL_WEEKDAY, ZONETYPE_ABBR, ZONETYPE_ID,
    ZONETYPE_OFFSET, is_leap,
};
use twig_timelib::{ErrorContainer, Time, TzDb, UNSET, dow};
use twig_value::object::{ClassRef, ObjectRef};
use twig_value::param::Arg;
use twig_value::{Array, Diagnostics, PhpResult, PhpString, Value};

use crate::context::DateContext;
use crate::datetime::{DateObject, WarningMode};
use crate::format::{MON_FULL_NAMES, date_format, full_day_name, gmt_offset_abbr, time_offset};
use crate::interval::IntervalObject;
use crate::util;

/// Port of `php_format_date()` (PHPSRC/ext/date/php_date.c): `date($format, $ts)` (`localtime`) or
/// `gmdate($format, $ts)`.
pub fn format_date(
    ctx: &DateContext,
    format: &[u8],
    ts: i64,
    localtime: bool,
) -> PhpResult<PhpString> {
    let mut t = Time::new();
    if localtime {
        let tzi = ctx.timezone_info()?;
        t.tz_info = Some(tzi);
        t.zone_type = ZONETYPE_ID;
        t.unixtime2local(ts);
    } else {
        t.unixtime2gmt(ts);
    }
    Ok(PhpString::from(date_format(format, &t, localtime)))
}

/// `date(string $format, ?int $timestamp = null)`: local time in the default timezone.
pub fn date(ctx: &DateContext, format: &[u8], timestamp: Option<i64>) -> PhpResult<PhpString> {
    let ts = timestamp.unwrap_or_else(|| ctx.time());
    format_date(ctx, format, ts, true)
}

/// `gmdate(string $format, ?int $timestamp = null)`: UTC.
pub fn gmdate(ctx: &DateContext, format: &[u8], timestamp: Option<i64>) -> PhpResult<PhpString> {
    let ts = timestamp.unwrap_or_else(|| ctx.time());
    format_date(ctx, format, ts, false)
}

/// Port of `php_idate()`: one `date()` field as an int (`-1` for unknown letters). As in PHP, `localtime ==
/// false` means "use the default timezone" (`idate()` passes 0).
pub fn php_idate(ctx: &DateContext, format: u8, ts: i64, localtime: bool) -> PhpResult<i32> {
    let mut t = Time::new();
    if !localtime {
        let tzi = ctx.timezone_info()?;
        t.tz_info = Some(tzi);
        t.zone_type = ZONETYPE_ID;
        t.unixtime2local(ts);
    } else {
        t.unixtime2gmt(ts);
    }

    let offset = if !localtime {
        let mut o = time_offset(&t);
        if t.zone_type == ZONETYPE_OFFSET {
            // php_idate() adds the DST hour for offset zones too.
            o.offset = t.z.wrapping_add(t.dst.wrapping_mul(3600));
            o.is_dst = t.dst as u32;
            o.abbr = String::from_utf8_lossy(&gmt_offset_abbr(o.offset)).into_owned();
        }
        Some(o)
    } else {
        None
    };

    let (isoweek, isoyear) = dow::isoweek_from_date(t.y, t.m, t.d);

    Ok(match format {
        // day
        b'd' | b'j' => t.d as i32,
        b'N' => dow::iso_day_of_week(t.y, t.m, t.d) as i32,
        b'w' => dow::day_of_week(t.y, t.m, t.d) as i32,
        b'z' => dow::day_of_year(t.y, t.m, t.d) as i32,
        // week
        b'W' => isoweek as i32,
        // month
        b'm' | b'n' => t.m as i32,
        b't' => dow::days_in_month(t.y, t.m) as i32,
        // year
        b'L' => i32::from(is_leap(i64::from(t.y as i32))),
        b'y' => (t.y % 100) as i32,
        b'Y' => t.y as i32,
        b'o' => isoyear as i32,
        // Swatch Beat a.k.a. Internet Time
        b'B' => {
            let sse = t.sse;
            let mut retval = (sse.wrapping_sub(sse.wrapping_sub((sse % 86400).wrapping_add(3600))))
                .wrapping_mul(10) as i32;
            if retval < 0 {
                retval = retval.wrapping_add(864000);
            }
            // Make sure to do this on a positive int to avoid rounding errors
            (retval / 864) % 1000
        }
        // time
        b'g' | b'h' => {
            if t.h % 12 != 0 {
                (t.h as i32) % 12
            } else {
                12
            }
        }
        b'H' | b'G' => t.h as i32,
        b'i' => t.i as i32,
        b's' => t.s as i32,
        // timezone
        b'I' => offset.as_ref().map_or(0, |o| o.is_dst as i32),
        b'Z' => offset.as_ref().map_or(0, |o| o.offset),
        b'U' => t.sse as i32,
        _ => -1,
    })
}

/// `idate(string $format, ?int $timestamp = null)`: warns `idate(): idate format is one char` or
/// `idate(): Unrecognized date format token` and returns `false` for bad formats.
pub fn idate(
    ctx: &DateContext,
    format: &[u8],
    timestamp: Option<i64>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    if format.len() != 1 {
        diag.warn("idate(): idate format is one char")?;
        return Ok(Value::Bool(false));
    }
    let ts = timestamp.unwrap_or_else(|| ctx.time());
    let ret = php_idate(ctx, format[0], ts, false)?;
    if ret == -1 {
        diag.warn("idate(): Unrecognized date format token")?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(i64::from(ret)))
}

/// Port of `php_mktime()`: `mktime()` (`gmt == false`, default timezone) / `gmmktime()`. Missing fields come from
/// the current time; years 0-69 and 70-100 map to 2000-2069 and 1970-2000.
#[allow(clippy::too_many_arguments)]
pub fn php_mktime(
    ctx: &DateContext,
    gmt: bool,
    hour: i64,
    minute: Option<i64>,
    second: Option<i64>,
    month: Option<i64>,
    day: Option<i64>,
    year: Option<i64>,
) -> PhpResult<i64> {
    // Initialize structure with current time
    let mut now = Time::new();
    let mut tzi = None;
    if gmt {
        now.unixtime2gmt(ctx.time());
    } else {
        let z = ctx.timezone_info()?;
        now.tz_info = Some(z.clone());
        now.zone_type = ZONETYPE_ID;
        now.unixtime2local(ctx.time());
        tzi = Some(z);
    }

    now.h = hour;
    if let Some(min) = minute {
        now.i = min;
    }
    if let Some(sec) = second {
        now.s = sec;
    }
    if let Some(mon) = month {
        now.m = mon;
    }
    if let Some(d) = day {
        now.d = d;
    }
    if let Some(mut yea) = year {
        if (0..70).contains(&yea) {
            yea += 2000;
        } else if (70..=100).contains(&yea) {
            yea += 1900;
        }
        now.y = yea;
    }

    // Update the timestamp
    now.update_ts(tzi.as_ref());
    // Clean up and return (timelib_date_to_int() always fits on 64-bit platforms)
    Ok(now.sse)
}

/// `mktime(int $hour, ?int $minute = null, ?int $second = null, ?int $month = null, ?int $day = null, ?int $year
/// = null)`.
pub fn mktime(
    ctx: &DateContext,
    hour: i64,
    minute: Option<i64>,
    second: Option<i64>,
    month: Option<i64>,
    day: Option<i64>,
    year: Option<i64>,
) -> PhpResult<i64> {
    php_mktime(ctx, false, hour, minute, second, month, day, year)
}

/// `gmmktime(...)`: like [`mktime`] in UTC.
pub fn gmmktime(
    ctx: &DateContext,
    hour: i64,
    minute: Option<i64>,
    second: Option<i64>,
    month: Option<i64>,
    day: Option<i64>,
    year: Option<i64>,
) -> PhpResult<i64> {
    php_mktime(ctx, true, hour, minute, second, month, day, year)
}

/// Port of `PHP_FUNCTION(checkdate)`: a valid Gregorian date with a year in 1..=32767.
pub fn checkdate(month: i64, day: i64, year: i64) -> bool {
    !(!(1..=32767).contains(&year) || !dow::valid_date(year, month, day))
}

/// Port of `PHP_FUNCTION(strtotime)`: the timestamp of a date/time string relative to `base` (default: now) in
/// the default timezone, or `None` (`false`) for an empty or unparsable string.
pub fn strtotime(
    ctx: &DateContext,
    datetime: &[u8],
    base_timestamp: Option<i64>,
) -> PhpResult<Option<i64>> {
    // timelib_strtotime() expects the string to not be empty
    if datetime.is_empty() {
        return Ok(None);
    }
    let tzi = ctx.timezone_info()?;
    let mut now = Time::new();
    now.tz_info = Some(tzi.clone());
    now.zone_type = ZONETYPE_ID;
    now.unixtime2local(base_timestamp.unwrap_or_else(|| ctx.time()));

    let cache = ctx.tz_cache();
    let (mut t, error) = tl_strtotime(datetime, ctx.tzdb(), &mut |name: &str, _db: &TzDb| {
        cache.get(name)
    });
    if error.error_count() > 0 {
        return Ok(None);
    }
    t.fill_holes(&now, NO_CLONE);
    t.update_ts(Some(&tzi));
    Ok(t.date_to_int())
}

/// `time()`.
pub fn time(ctx: &DateContext) -> i64 {
    ctx.time()
}

/// Port of `PHP_FUNCTION(getdate)`: `seconds, minutes, hours, mday, wday, mon, year, yday, weekday, month, 0`.
pub fn getdate(ctx: &DateContext, timestamp: Option<i64>) -> PhpResult<Array> {
    let timestamp = timestamp.unwrap_or_else(|| ctx.time());
    let mut ts = Time::new();
    ts.tz_info = Some(ctx.timezone_info()?);
    ts.zone_type = ZONETYPE_ID;
    ts.unixtime2local(timestamp);

    let mut out = util::assoc([
        ("seconds", Value::Int(ts.s)),
        ("minutes", Value::Int(ts.i)),
        ("hours", Value::Int(ts.h)),
        ("mday", Value::Int(ts.d)),
        ("wday", Value::Int(dow::day_of_week(ts.y, ts.m, ts.d))),
        ("mon", Value::Int(ts.m)),
        ("year", Value::Int(ts.y)),
        ("yday", Value::Int(dow::day_of_year(ts.y, ts.m, ts.d))),
        ("weekday", Value::from(full_day_name(ts.y, ts.m, ts.d))),
        (
            "month",
            Value::from(
                usize::try_from(ts.m - 1)
                    .ok()
                    .and_then(|i| MON_FULL_NAMES.get(i))
                    .copied()
                    .unwrap_or(""),
            ),
        ),
    ]);
    out.insert(0, Value::Int(timestamp));
    Ok(out)
}

/// Port of `PHP_FUNCTION(localtime)`: the C `struct tm` fields as a list, or keyed by `tm_*` names.
pub fn localtime(ctx: &DateContext, timestamp: Option<i64>, associative: bool) -> PhpResult<Array> {
    let timestamp = timestamp.unwrap_or_else(|| ctx.time());
    let mut ts = Time::new();
    ts.tz_info = Some(ctx.timezone_info()?);
    ts.zone_type = ZONETYPE_ID;
    ts.unixtime2local(timestamp);
    let fields = [
        ("tm_sec", ts.s),
        ("tm_min", ts.i),
        ("tm_hour", ts.h),
        ("tm_mday", ts.d),
        ("tm_mon", ts.m - 1),
        ("tm_year", ts.y - 1900),
        ("tm_wday", dow::day_of_week(ts.y, ts.m, ts.d)),
        ("tm_yday", dow::day_of_year(ts.y, ts.m, ts.d)),
        ("tm_isdst", i64::from(ts.dst)),
    ];
    Ok(if associative {
        util::assoc(fields.map(|(k, v)| (k, Value::Int(v))))
    } else {
        Array::from_values(fields.map(|(_, v)| Value::Int(v)))
    })
}

/// Port of `zval_from_error_container()`: `warning_count`, `warnings` (position => message; a later message at
/// the same position replaces the earlier one), `error_count`, `errors`.
pub fn zval_from_error_container(z: &mut Array, error: &ErrorContainer) {
    let list = |messages: &[twig_timelib::ErrorMessage]| {
        let mut element = Array::with_capacity(messages.len());
        for m in messages {
            element.insert(i64::from(m.position), Value::from(m.message));
        }
        element
    };
    z.insert("warning_count", Value::Int(error.warning_count() as i64));
    z.insert("warnings", Value::Array(list(&error.warning_messages)));
    z.insert("error_count", Value::Int(error.error_count() as i64));
    z.insert("errors", Value::Array(list(&error.error_messages)));
}

/// Port of `php_date_do_return_parsed_time()`: the `date_parse()` array.
fn return_parsed_time(parsed_time: &Time, error: &ErrorContainer) -> Array {
    let mut out = Array::new();
    let element = |v: i64| {
        if v == UNSET {
            Value::Bool(false)
        } else {
            Value::Int(v)
        }
    };
    out.insert("year", element(parsed_time.y));
    out.insert("month", element(parsed_time.m));
    out.insert("day", element(parsed_time.d));
    out.insert("hour", element(parsed_time.h));
    out.insert("minute", element(parsed_time.i));
    out.insert("second", element(parsed_time.s));

    out.insert(
        "fraction",
        if parsed_time.us == UNSET {
            Value::Bool(false)
        } else {
            Value::Float(parsed_time.us as f64 / 1_000_000.0)
        },
    );

    zval_from_error_container(&mut out, error);

    out.insert("is_localtime", Value::Bool(parsed_time.is_localtime != 0));

    if parsed_time.is_localtime != 0 {
        out.insert("zone_type", element(i64::from(parsed_time.zone_type)));
        match parsed_time.zone_type {
            ZONETYPE_OFFSET => {
                out.insert("zone", element(i64::from(parsed_time.z)));
                out.insert("is_dst", Value::Bool(parsed_time.dst != 0));
            }
            ZONETYPE_ID => {
                if let Some(abbr) = &parsed_time.tz_abbr {
                    out.insert("tz_abbr", Value::from(abbr.as_str()));
                }
                if let Some(tz) = &parsed_time.tz_info {
                    out.insert("tz_id", Value::from(tz.name.as_str()));
                }
            }
            ZONETYPE_ABBR => {
                out.insert("zone", element(i64::from(parsed_time.z)));
                out.insert("is_dst", Value::Bool(parsed_time.dst != 0));
                out.insert(
                    "tz_abbr",
                    Value::from(parsed_time.tz_abbr.as_deref().unwrap_or("")),
                );
            }
            _ => {}
        }
    }
    if parsed_time.have_relative != 0 {
        let r = &parsed_time.relative;
        let mut element = util::assoc([
            ("year", Value::Int(r.y)),
            ("month", Value::Int(r.m)),
            ("day", Value::Int(r.d)),
            ("hour", Value::Int(r.h)),
            ("minute", Value::Int(r.i)),
            ("second", Value::Int(r.s)),
        ]);
        if r.have_weekday_relative != 0 {
            element.insert("weekday", Value::Int(i64::from(r.weekday)));
        }
        if r.have_special_relative != 0 && r.special.kind == SPECIAL_WEEKDAY {
            element.insert("weekdays", Value::Int(r.special.amount));
        }
        if r.first_last_day_of != 0 {
            let key = if r.first_last_day_of == SPECIAL_FIRST_DAY_OF_MONTH {
                "first_day_of_month"
            } else {
                "last_day_of_month"
            };
            element.insert(key, Value::Bool(true));
        }
        out.insert("relative", Value::Array(element));
    }
    out
}

/// Port of `PHP_FUNCTION(date_parse)`.
pub fn date_parse(ctx: &DateContext, datetime: &[u8]) -> Array {
    let cache = ctx.tz_cache();
    let (t, e) = tl_strtotime(datetime, ctx.tzdb(), &mut |name: &str, _db: &TzDb| {
        cache.get(name)
    });
    return_parsed_time(&t, &e)
}

/// Port of `PHP_FUNCTION(date_parse_from_format)` (`ValueError` for NUL bytes in `datetime`).
pub fn date_parse_from_format(
    ctx: &DateContext,
    format: &[u8],
    datetime: &[u8],
) -> PhpResult<Array> {
    if datetime.contains(&0) {
        return Err(Arg::new("date_parse_from_format", 2, "datetime")
            .value_error("must not contain any null bytes"));
    }
    let cache = ctx.tz_cache();
    let (t, e) = parse_from_format(
        format,
        datetime,
        ctx.tzdb(),
        &mut |name: &str, _db: &TzDb| cache.get(name),
    );
    Ok(return_parsed_time(&t, &e))
}

/// `date_create(string $datetime = "now", ?DateTimeZone $timezone = null)`: `None` (`false`) on parse errors.
pub fn date_create(
    ctx: &DateContext,
    datetime: &[u8],
    timezone: Option<&ObjectRef>,
) -> PhpResult<Option<ObjectRef>> {
    DateObject::create(ctx, crate::classes::datetime(), datetime, timezone)
}

/// `date_create_immutable(...)`.
pub fn date_create_immutable(
    ctx: &DateContext,
    datetime: &[u8],
    timezone: Option<&ObjectRef>,
) -> PhpResult<Option<ObjectRef>> {
    DateObject::create(
        ctx,
        crate::classes::datetime_immutable(),
        datetime,
        timezone,
    )
}

/// `date_create_from_format(string $format, string $datetime, ?DateTimeZone $timezone = null)` (and the
/// immutable variant with `class` = `DateTimeImmutable`).
pub fn date_create_from_format(
    ctx: &DateContext,
    class: ClassRef,
    format: &[u8],
    datetime: &[u8],
    timezone: Option<&ObjectRef>,
) -> PhpResult<Option<ObjectRef>> {
    if datetime.contains(&0) {
        let func = if class.is_subclass_of("DateTimeImmutable") {
            "date_create_immutable_from_format"
        } else {
            "date_create_from_format"
        };
        return Err(Arg::new(func, 2, "datetime").value_error("must not contain any null bytes"));
    }
    DateObject::create_from_format(ctx, class, format, datetime, timezone)
}

/// `date_modify(DateTime $object, string $modifier)`: warns `date_modify(): Failed to parse time string …` and
/// returns `None` (`false`) instead of throwing.
pub fn date_modify(
    object: &ObjectRef,
    modifier: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<ObjectRef>> {
    let Some(d) = object.downcast_ref::<DateObject>() else {
        return Err(crate::errors::uninitialized(object.class()));
    };
    Ok(d.modify(modifier, WarningMode::Warn("date_modify"), diag)?
        .then(|| object.clone()))
}

/// `date_sub(DateTime $object, DateInterval $interval)`: special relative intervals warn
/// (`date_sub(): Only non-special relative time specifications are supported for subtraction`) and leave the
/// object unchanged.
pub fn date_sub(
    object: &ObjectRef,
    interval: &ObjectRef,
    diag: &mut dyn Diagnostics,
) -> PhpResult<ObjectRef> {
    let (Some(d), Some(i)) = (
        object.downcast_ref::<DateObject>(),
        interval.downcast_ref::<IntervalObject>(),
    ) else {
        return Err(crate::errors::uninitialized(object.class()));
    };
    d.sub(i, WarningMode::Warn("date_sub"), diag)?;
    Ok(object.clone())
}

/// `timezone_version_get()`: the database version (`2026.6`).
pub fn timezone_version_get(ctx: &DateContext) -> &'static str {
    ctx.tzdb().version
}

/// `date_default_timezone_get()`.
pub fn date_default_timezone_get(ctx: &DateContext) -> PhpResult<String> {
    ctx.default_timezone_get()
}

/// `date_default_timezone_set(string $timezoneId)`.
pub fn date_default_timezone_set(
    ctx: &DateContext,
    timezone_id: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    ctx.default_timezone_set(timezone_id, diag)
}
