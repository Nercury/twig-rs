//! Test-only emulation of the PHP layer (`PHPSRC/ext/date/php_date.c`) on top of twig-timelib, so oracle
//! vectors produced through PHP functions (`date_parse()`, `strtotime()`, `DateTime::modify()`, ...) can be
//! checked against the port. Each helper mirrors the php_date.c function named in its doc comment, only as far as
//! the vectors need.

#![allow(dead_code)]

use std::sync::Arc;

use serde_json::{Map, Value, json};
use twig_timelib::parse_date::{parse_zone, strtotime as tl_strtotime};
use twig_timelib::parse_iso_intervals::strtointerval;
use twig_timelib::timelib::strncasecmp;
use twig_timelib::types::{
    NO_CLONE, SPECIAL_FIRST_DAY_OF_MONTH, SPECIAL_WEEKDAY, ZONETYPE_ABBR, ZONETYPE_ID,
    ZONETYPE_OFFSET,
};
use twig_timelib::{
    AbbrInfo, ErrorContainer, RelTime, Time, TzCache, TzDb, TzInfo, UNSET, dow, interval,
};

/// Loads `tests/vectors/<topic>.json`.
pub fn load(topic: &str) -> Value {
    let path = format!("{}/tests/vectors/{topic}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub fn cache() -> &'static TzCache {
    TzCache::builtin()
}

pub fn wrapper() -> impl FnMut(&str, &TzDb) -> Option<Arc<TzInfo>> {
    |name: &str, _db: &TzDb| cache().get(name)
}

pub fn strtotime(input: &[u8]) -> (Time, ErrorContainer) {
    tl_strtotime(input, cache().tzdb(), &mut wrapper())
}

/// A `DateTimeZone` object (`php_timezone_obj`).
#[derive(Clone, Debug)]
pub enum PhpTz {
    Id(Arc<TzInfo>),
    Offset(i64),
    Abbr(AbbrInfo),
}

/// `timezone_initialize()`: `new DateTimeZone($name)`.
pub fn tz_new(name: &str) -> Result<PhpTz, String> {
    let mut b = name.as_bytes().to_vec();
    b.push(0);
    let mut t = Time::new();
    let mut ptr = 0;
    let mut dst = 0;
    let mut not_found = false;
    let z = parse_zone(
        &b,
        &mut ptr,
        &mut dst,
        &mut t,
        &mut not_found,
        cache().tzdb(),
        &mut wrapper(),
    );
    t.z = z as i32;
    if t.z >= 100 * 60 * 60 || t.z <= -100 * 60 * 60 {
        return Err(format!("Timezone offset is out of range ({name})"));
    }
    t.dst = dst;
    if !not_found && b[ptr] != 0 {
        return Err(format!("Unknown or bad timezone ({name})"));
    }
    if not_found {
        return Err(format!("Unknown or bad timezone ({name})"));
    }
    Ok(match t.zone_type {
        ZONETYPE_ID => PhpTz::Id(t.tz_info.clone().expect("zone data")),
        ZONETYPE_OFFSET => PhpTz::Offset(i64::from(t.z)),
        _ => PhpTz::Abbr(AbbrInfo {
            utc_offset: i64::from(t.z),
            abbr: t.tz_abbr.clone().unwrap_or_default(),
            dst: t.dst,
        }),
    })
}

/// `php_date_timezone_set()`: `$d->setTimezone($tz)`.
pub fn set_timezone(t: &mut Time, tz: &PhpTz) {
    match tz {
        PhpTz::Offset(o) => t.set_timezone_from_offset(*o),
        PhpTz::Abbr(a) => t.set_timezone_from_abbr(a),
        PhpTz::Id(z) => t.set_timezone(z),
    }
    t.unixtime2local(t.sse);
}

/// `new DateTime('@ts')` (or `createFromFormat('U.u')`) followed by `setTimezone($tz)`.
pub fn from_ts(ts: i64, us: i64, tz: &PhpTz) -> Time {
    let mut t = Time::new();
    t.unixtime2gmt(ts);
    t.zone_type = ZONETYPE_OFFSET;
    t.is_localtime = 1;
    t.have_zone = 1;
    t.us = us;
    set_timezone(&mut t, tz);
    t
}

/// `%s` of a C string: the bytes up to the first NUL.
fn c_str(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// Encodes an error message like the vector generators do (`hex:` for non-UTF-8).
pub fn enc(b: Vec<u8>) -> String {
    match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => format!(
            "hex:{}",
            e.into_bytes()
                .iter()
                .map(|c| format!("{c:02x}"))
                .collect::<String>()
        ),
    }
}

/// `"Failed to parse time string (%s) at position %d (%c): %s"` with the first error.
fn parse_failure(prefix: &str, input: &[u8], e: &ErrorContainer) -> String {
    let m = &e.error_messages[0];
    let mut out = prefix.as_bytes().to_vec();
    out.extend_from_slice(b"Failed to parse time string (");
    out.extend_from_slice(c_str(input));
    out.extend_from_slice(format!(") at position {} (", m.position).as_bytes());
    out.push(if m.character != 0 { m.character } else { b' ' });
    out.extend_from_slice(b"): ");
    out.extend_from_slice(m.message.as_bytes());
    enc(out)
}

/// `php_date_modify()` with `DateTime::modify()`'s exception wrapping.
pub fn modify(t: &mut Time, input: &[u8]) -> Result<(), String> {
    let (tmp, e) = strtotime(input);
    if e.error_count() > 0 {
        return Err(parse_failure(
            "DateMalformedStringException: DateTime::modify(): ",
            input,
            &e,
        ));
    }
    t.relative = tmp.relative;
    t.have_relative = tmp.have_relative;
    t.sse_uptodate = 0;
    if tmp.y != UNSET {
        t.y = tmp.y;
    }
    if tmp.m != UNSET {
        t.m = tmp.m;
    }
    if tmp.d != UNSET {
        t.d = tmp.d;
    }
    if tmp.h != UNSET {
        t.h = tmp.h;
        if tmp.i != UNSET {
            t.i = tmp.i;
            t.s = if tmp.s != UNSET { tmp.s } else { 0 };
        } else {
            t.i = 0;
            t.s = 0;
        }
    }
    if tmp.us != UNSET {
        t.us = tmp.us;
    }
    // Reset timezone to UTC if we detect a "@<ts>" modification
    if tmp.y == 1970
        && tmp.m == 1
        && tmp.d == 1
        && tmp.h == 0
        && tmp.i == 0
        && tmp.s == 0
        && tmp.us == 0
        && tmp.have_zone != 0
        && tmp.zone_type == ZONETYPE_OFFSET
        && tmp.z == 0
        && tmp.dst == 0
    {
        t.set_timezone_from_offset(0);
    }
    t.update_ts(None);
    t.update_from_sse();
    t.have_relative = 0;
    t.relative = RelTime::default();
    Ok(())
}

/// `php_date_initialize()` for `new DateTime($input, $tz)` with a fixed current time.
pub fn construct(input: &[u8], tz: &PhpTz, now_sec: i64, now_usec: i64) -> Result<Time, String> {
    let input: &[u8] = if input.is_empty() { b"now" } else { input };
    let (mut t, e) = strtotime(input);
    if e.error_count() > 0 {
        return Err(parse_failure("DateMalformedStringException: ", input, &e));
    }
    let mut now = Time::new();
    let mut tzi = None;
    match tz {
        PhpTz::Id(z) => {
            now.zone_type = ZONETYPE_ID;
            now.tz_info = Some(Arc::clone(z));
            tzi = Some(Arc::clone(z));
        }
        PhpTz::Offset(o) => {
            now.zone_type = ZONETYPE_OFFSET;
            now.z = *o as i32;
        }
        PhpTz::Abbr(a) => {
            now.zone_type = ZONETYPE_ABBR;
            now.z = a.utc_offset as i32;
            now.dst = a.dst;
            now.tz_abbr = Some(a.abbr.clone());
        }
    }
    now.unixtime2local(now_sec);
    now.us = now_usec;
    if input.len() == 3 && strncasecmp(input, b"now", 3) == 0 {
        return Ok(now);
    }
    t.fill_holes(&now, NO_CLONE);
    t.update_ts(tzi.as_ref());
    t.update_from_sse();
    t.have_relative = 0;
    Ok(t)
}

/// `PHP_FUNCTION(strtotime)` with `date_default_timezone_set($zone)`.
pub fn php_strtotime(input: &[u8], base: i64, zone: &str) -> Option<i64> {
    if input.is_empty() {
        return None;
    }
    let tzi = cache().get(zone)?;
    let mut now = Time::new();
    now.tz_info = Some(Arc::clone(&tzi));
    now.zone_type = ZONETYPE_ID;
    now.unixtime2local(base);
    let (mut t, e) = strtotime(input);
    if e.error_count() > 0 {
        return None;
    }
    t.fill_holes(&now, NO_CLONE);
    t.update_ts(Some(&tzi));
    t.date_to_int()
}

/// The abbreviation/offset that `date_format()` uses for local times.
fn offset_info(t: &Time) -> (i32, u32, String) {
    match t.zone_type {
        ZONETYPE_ABBR => (
            t.z + t.dst * 3600,
            t.dst as u32,
            t.tz_abbr.clone().unwrap_or_default(),
        ),
        ZONETYPE_OFFSET => {
            let o = t.z;
            let abbr = format!(
                "GMT{}{:02}{:02}",
                if o < 0 { '-' } else { '+' },
                (o / 3600).abs(),
                ((o % 3600) / 60).abs()
            );
            (o, 0, abbr)
        }
        ZONETYPE_ID => {
            let off = t
                .tz_info
                .as_ref()
                .expect("zone data")
                .get_time_zone_info(t.sse);
            (off.offset, off.is_dst, off.abbr)
        }
        _ => (0, 0, String::new()),
    }
}

/// `date_format()` for the letters the vectors use.
pub fn format(t: &Time, fmt: &str) -> String {
    let localtime = t.is_localtime != 0;
    let (offset, is_dst, abbr) = if localtime {
        offset_info(t)
    } else {
        (0, 0, "GMT".to_owned())
    };
    let mut out = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        match c {
            'd' => out.push_str(&format!("{:02}", t.d as i32)),
            'j' => out.push_str(&format!("{}", t.d as i32)),
            'N' => out.push_str(&format!("{}", dow::iso_day_of_week(t.y, t.m, t.d) as i32)),
            'z' => out.push_str(&format!("{}", dow::day_of_year(t.y, t.m, t.d) as i32)),
            'W' => out.push_str(&format!(
                "{:02}",
                dow::isoweek_from_date(t.y, t.m, t.d).0 as i32
            )),
            'o' => out.push_str(&format!("{}", dow::isoweek_from_date(t.y, t.m, t.d).1)),
            'm' => out.push_str(&format!("{:02}", t.m as i32)),
            'n' => out.push_str(&format!("{}", t.m as i32)),
            't' => out.push_str(&format!("{}", dow::days_in_month(t.y, t.m) as i32)),
            'L' => out.push_str(if twig_timelib::types::is_leap(i64::from(t.y as i32)) {
                "1"
            } else {
                "0"
            }),
            'Y' => out.push_str(&format!(
                "{}{:04}",
                if t.y < 0 { "-" } else { "" },
                t.y.unsigned_abs()
            )),
            'G' => out.push_str(&format!("{}", t.h as i32)),
            'H' => out.push_str(&format!("{:02}", t.h as i32)),
            'i' => out.push_str(&format!("{:02}", t.i as i32)),
            's' => out.push_str(&format!("{:02}", t.s as i32)),
            'u' => out.push_str(&format!("{:06}", t.us as i32)),
            'I' => out.push_str(&format!("{}", if localtime { is_dst } else { 0 })),
            'P' | 'O' => {
                let colon = if c == 'P' { ":" } else { "" };
                out.push_str(&format!(
                    "{}{:02}{colon}{:02}",
                    if localtime && offset < 0 { '-' } else { '+' },
                    if localtime { (offset / 3600).abs() } else { 0 },
                    if localtime {
                        ((offset % 3600) / 60).abs()
                    } else {
                        0
                    }
                ));
            }
            'T' => out.push_str(&abbr),
            'Z' => out.push_str(&format!("{}", if localtime { offset } else { 0 })),
            'U' => out.push_str(&format!("{}", t.sse)),
            'e' => match t.zone_type {
                ZONETYPE_ID => out.push_str(&t.tz_info.as_ref().expect("zone data").name),
                ZONETYPE_ABBR => out.push_str(&abbr),
                _ => out.push_str(&format!(
                    "{}{:02}:{:02}",
                    if offset < 0 { '-' } else { '+' },
                    (offset / 3600).abs(),
                    ((offset % 3600) / 60).abs()
                )),
            },
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// The generators' `show()`: `format('Y-m-d H:i:s.u T P U')`.
pub fn show(t: &Time) -> String {
    format(t, "Y-m-d H:i:s.u T P U")
}

/// `zval_from_error_container()` positions: later messages at the same position replace earlier ones.
fn positioned(messages: &[twig_timelib::ErrorMessage]) -> Value {
    let mut list: Vec<(i32, &str)> = Vec::new();
    for m in messages {
        if let Some(slot) = list.iter_mut().find(|(p, _)| *p == m.position) {
            slot.1 = m.message;
        } else {
            list.push((m.position, m.message));
        }
    }
    Value::Array(list.into_iter().map(|(p, msg)| json!([p, msg])).collect())
}

/// `php_date_do_return_parsed_time()` in the compact form of `parse.gen.php`.
pub fn compact_parse(input: &str, t: &Time, e: &ErrorContainer) -> Value {
    let mut c = Map::new();
    c.insert("in".into(), json!(input));
    for (v, k) in [
        (t.y, "y"),
        (t.m, "m"),
        (t.d, "d"),
        (t.h, "h"),
        (t.i, "i"),
        (t.s, "s"),
    ] {
        if v != UNSET {
            c.insert(k.into(), json!(v));
        }
    }
    if t.us != UNSET {
        let f = t.us as f64 / 1_000_000.0;
        c.insert(
            "f".into(),
            json!(
                f.to_be_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ),
        );
    }
    if e.warning_count() > 0 {
        c.insert("wc".into(), json!(e.warning_count()));
        c.insert("w".into(), positioned(&e.warning_messages));
    }
    if e.error_count() > 0 {
        c.insert("ec".into(), json!(e.error_count()));
        c.insert("e".into(), positioned(&e.error_messages));
    }
    if t.is_localtime != 0 {
        c.insert("lt".into(), json!(true));
        c.insert("zt".into(), json!(t.zone_type));
        let zone = if i64::from(t.z) == UNSET {
            json!(false)
        } else {
            json!(t.z)
        };
        match t.zone_type {
            ZONETYPE_OFFSET => {
                c.insert("z".into(), zone);
                c.insert("dst".into(), json!(t.dst != 0));
            }
            ZONETYPE_ID => {
                if let Some(a) = &t.tz_abbr {
                    c.insert("abbr".into(), json!(a));
                }
                if let Some(z) = &t.tz_info {
                    c.insert("tzid".into(), json!(z.name));
                }
            }
            ZONETYPE_ABBR => {
                c.insert("z".into(), zone);
                c.insert("dst".into(), json!(t.dst != 0));
                c.insert("abbr".into(), json!(t.tz_abbr.clone().unwrap_or_default()));
            }
            _ => {}
        }
    }
    if t.have_relative != 0 {
        let r = &t.relative;
        c.insert("rel".into(), json!([r.y, r.m, r.d, r.h, r.i, r.s]));
        if r.have_weekday_relative != 0 {
            c.insert("wd".into(), json!(r.weekday));
        }
        if r.have_special_relative != 0 && r.special.kind == SPECIAL_WEEKDAY {
            c.insert("wds".into(), json!(r.special.amount));
        }
        if r.first_last_day_of != 0 {
            let key = if r.first_last_day_of == SPECIAL_FIRST_DAY_OF_MONTH {
                "fdom"
            } else {
                "ldom"
            };
            c.insert(key.into(), json!(true));
        }
    }
    Value::Object(c)
}

/// Adds or subtracts an interval like `php_date_add()` / `php_date_sub()` (with `DateTime::sub()`'s exception).
pub fn add_sub(t: &Time, rel: &RelTime, wall: bool, sub: bool) -> Result<Time, String> {
    if sub {
        if rel.have_weekday_relative != 0 || rel.have_special_relative != 0 {
            return Err("DateInvalidOperationException: DateTime::sub(): Only non-special relative time specifications are supported for subtraction".into());
        }
        Ok(if wall {
            interval::sub_wall(t, rel)
        } else {
            interval::sub(t, rel)
        })
    } else {
        Ok(if wall {
            interval::add_wall(t, rel)
        } else {
            interval::add(t, rel)
        })
    }
}

/// `date_interval_initialize()`: `new DateInterval($spec)` with its exception messages.
pub fn date_interval_new(spec: &[u8]) -> Result<RelTime, String> {
    let r = strtointerval(spec);
    let shown = enc(c_str(spec).to_vec());
    if r.errors.error_count() > 0 {
        return Err(format!(
            "DateMalformedIntervalStringException: Unknown or bad format ({shown})"
        ));
    }
    if let Some(p) = r.period {
        return Ok(p);
    }
    match (r.begin, r.end) {
        (Some(mut b), Some(mut e)) => {
            b.update_ts(None);
            e.update_ts(None);
            Ok(interval::diff(&b, &e))
        }
        _ => Err(format!(
            "DateMalformedIntervalStringException: Failed to parse interval ({shown})"
        )),
    }
}

/// `php_date_date_set()`: `$d->setDate($y, $m, $d)` (no `timelib_update_from_sse()`, as in PHP).
pub fn set_date(t: &mut Time, y: i64, m: i64, d: i64) {
    t.y = y;
    t.m = m;
    t.d = d;
    t.update_ts(None);
}

/// `php_date_time_set()`: `$d->setTime($h, $i, $s, $us)`.
pub fn set_time(t: &mut Time, h: i64, i: i64, s: i64, us: i64) {
    t.h = h;
    t.i = i;
    t.s = s;
    t.us = us;
    t.update_ts(None);
    t.update_from_sse();
}

/// `php_date_isodate_set()`: `$d->setISODate($y, $w, $dow)`.
pub fn set_isodate(t: &mut Time, y: i64, w: i64, d: i64) {
    t.y = y;
    t.m = 1;
    t.d = 1;
    t.relative = RelTime::default();
    t.relative.d = dow::daynr_from_weeknr(y, w, d);
    t.have_relative = 1;
    t.update_ts(None);
}

/// `php_date_timestamp_set()`: `$d->setTimestamp($ts)`.
pub fn set_timestamp(t: &mut Time, ts: i64) {
    t.unixtime2local(ts);
    t.update_ts(None);
    t.us = 0;
}
