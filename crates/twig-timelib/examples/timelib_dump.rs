//! Differential-testing tool: reads harness commands on stdin and prints the same canonical dump as the C
//! harness in `research/timelib-port/harness/harness.c` (which links PHP's timelib), so the two outputs can be
//! compared byte for byte.
//!
//! ```text
//! cargo run --release -p twig-timelib --example timelib_dump < corpus.txt > rust.txt
//! ```
//!
//! The commands are documented at the top of `harness.c` (`parse`, `init`, `modify`, `format`, `local`,
//! `fmtinit`, `zone`, `diff`, `add`, `mk`, `info`, `tzfile`, `dow`, `abbr`, `ival`, `raw`, `posix`, `hms`,
//! `formatx`); strings are hex encoded.

use std::fmt::Write as _;
use std::io::{self, BufRead, BufWriter, Write};
use std::sync::Arc;

use twig_timelib::abbr::timezone_id_from_abbr;
use twig_timelib::dow;
use twig_timelib::interval;
use twig_timelib::parse_date::{
    DEFAULT_FORMAT_MAP, FormatConfig, FormatSpecifierCode, parse_from_format,
    parse_from_format_with_map, parse_zone, strtotime,
};
use twig_timelib::parse_iso_intervals::strtointerval;
use twig_timelib::posix::{
    PosixTransInfo, PosixTransitions, TRANS_TYPE_MWD, get_transitions_for_year, parse_posix_str,
};
use twig_timelib::timelib::{
    decimal_hour_to_hms, hms_to_decimal_hour, hms_to_seconds, hmsf_to_decimal_hour, strncasecmp,
};
use twig_timelib::types::{NO_CLONE, OVERRIDE_TIME, ZONETYPE_ID, ZONETYPE_OFFSET, is_leap};
use twig_timelib::tz::{get_error_message, parse_tzfile, timezone_id_is_valid};
use twig_timelib::unixtime2tm::{date_from_epoch_days, unixtime2date};
use twig_timelib::{AbbrInfo, ErrorContainer, RelTime, Time, TzCache, TzDb, TzInfo, UNSET};

fn unhex(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        let (Some(h), Some(l)) = ((b[i] as char).to_digit(16), (b[i + 1] as char).to_digit(16))
        else {
            break;
        };
        out.push((h * 16 + l) as u8);
        i += 2;
    }
    out
}

/// The bytes up to the first NUL, as C sees a `char *` (the C harness passes decoded strings as C strings).
fn c_string(mut v: Vec<u8>) -> Vec<u8> {
    if let Some(n) = v.iter().position(|&c| c == 0) {
        v.truncate(n);
    }
    v
}

/// C `atoll()` (glibc: `strtoll()` without error reporting).
fn atoll(s: &str) -> i64 {
    let b = s.as_bytes();
    let mut p = 0;
    while p < b.len() && matches!(b[p], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        p += 1;
    }
    let neg = p < b.len() && b[p] == b'-';
    if p < b.len() && (b[p] == b'-' || b[p] == b'+') {
        p += 1;
    }
    let mut acc: i128 = 0;
    while p < b.len() && b[p].is_ascii_digit() {
        acc = (acc * 10 + i128::from(b[p] - b'0')).min(i128::from(i64::MAX) + 1);
        p += 1;
    }
    let v = if neg { -acc } else { acc };
    v.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn put_str(out: &mut String, s: Option<&[u8]>) {
    let Some(s) = s else {
        out.push('-');
        return;
    };
    for &c in s {
        if c == 0 {
            break;
        }
        if c > 32 && c < 127 && c != b'\\' {
            out.push(c as char);
        } else {
            let _ = write!(out, "\\x{c:02x}");
        }
    }
}

/// `with_code = false`: `parse_iso_intervals.re`'s `add_error()` leaves the C error code uninitialised.
fn dump_errors_ex(out: &mut String, e: &ErrorContainer, with_code: bool) {
    let _ = write!(out, " | E{}", e.error_count());
    for m in &e.error_messages {
        if with_code {
            let _ = write!(out, " {}", m.error_code);
        } else {
            out.push_str(" ?");
        }
        let _ = write!(out, "@{}:{:02x}:", m.position, m.character);
        put_str(out, Some(m.message.as_bytes()));
    }
    let _ = write!(out, " | W{}", e.warning_count());
    for m in &e.warning_messages {
        let _ = write!(out, " {}@{}:{:02x}:", m.error_code, m.position, m.character);
        put_str(out, Some(m.message.as_bytes()));
    }
}

fn dump_errors(out: &mut String, e: &ErrorContainer) {
    dump_errors_ex(out, e, true);
}

fn dump_rel(out: &mut String, r: &RelTime) {
    let _ = write!(
        out,
        "ry={} rm={} rd={} rh={} ri={} rs={} rus={} wd={} wb={} fl={} inv={} days={} st={} sa={} hwr={} hsr={}",
        r.y,
        r.m,
        r.d,
        r.h,
        r.i,
        r.s,
        r.us,
        r.weekday,
        r.weekday_behavior,
        r.first_last_day_of,
        r.invert,
        r.days,
        r.special.kind,
        r.special.amount,
        r.have_weekday_relative,
        r.have_special_relative
    );
}

fn dump_time(out: &mut String, t: &Time) {
    let _ = write!(
        out,
        "y={} m={} d={} h={} i={} s={} us={} z={} dst={} abbr=",
        t.y, t.m, t.d, t.h, t.i, t.s, t.us, t.z, t.dst
    );
    put_str(out, t.tz_abbr.as_deref().map(str::as_bytes));
    out.push_str(" tz=");
    put_str(out, t.tz_info.as_ref().map(|tz| tz.name.as_bytes()));
    let _ = write!(
        out,
        " sse={} lt={} zt={} ht={} hd={} hz={} hr={} hw={}",
        t.sse,
        t.is_localtime,
        t.zone_type,
        t.have_time,
        t.have_date,
        t.have_zone,
        t.have_relative,
        t.have_weeknr_day
    );
    out.push_str(" | ");
    dump_rel(out, &t.relative);
}

struct Ctx {
    cache: &'static TzCache,
}

impl Ctx {
    fn db(&self) -> &'static TzDb {
        self.cache.tzdb()
    }

    fn wrapper(&self) -> impl FnMut(&str, &TzDb) -> Option<Arc<TzInfo>> + '_ {
        |name: &str, _db: &TzDb| self.cache.get(name)
    }
}

/// A `DateTimeZone` object (`php_timezone_obj`).
enum Zone {
    Id(Arc<TzInfo>),
    Offset(i64),
    Abbr(AbbrInfo),
}

/// `timezone_initialize()` (PHPSRC/ext/date/php_date.c), as in the C harness.
fn zone_init(ctx: &Ctx, name: &[u8], verbose: Option<&mut String>) -> Result<Zone, i32> {
    let mut b = c_string(name.to_vec());
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
        ctx.db(),
        &mut ctx.wrapper(),
    );
    t.z = z as i32;
    let status = if t.z >= 100 * 60 * 60 || t.z <= -100 * 60 * 60 {
        1
    } else {
        t.dst = dst;
        if !not_found && b[ptr] != 0 {
            2
        } else if not_found {
            3
        } else {
            0
        }
    };
    if let Some(out) = verbose {
        let _ = write!(
            out,
            "st={status} used={ptr} nf={} dst={dst} | ",
            i32::from(not_found)
        );
        dump_time(out, &t);
    }
    if status != 0 {
        return Err(status);
    }
    Ok(match t.zone_type {
        ZONETYPE_ID => Zone::Id(t.tz_info.clone().expect("zone data")),
        ZONETYPE_OFFSET => Zone::Offset(i64::from(t.z)),
        _ => Zone::Abbr(AbbrInfo {
            utc_offset: i64::from(t.z),
            abbr: t.tz_abbr.clone().unwrap_or_default(),
            dst: t.dst,
        }),
    })
}

/// `(new DateTime('@ts'))->setTimezone(zone)` with microseconds.
fn mk_time(z: &Zone, ts: i64, us: i64) -> Time {
    let mut t = Time::new();
    t.unixtime2gmt(ts);
    t.zone_type = ZONETYPE_OFFSET;
    t.is_localtime = 1;
    t.have_zone = 1;
    t.us = us;
    match z {
        Zone::Offset(o) => t.set_timezone_from_offset(*o),
        Zone::Abbr(a) => t.set_timezone_from_abbr(a),
        Zone::Id(tz) => t.set_timezone(tz),
    }
    t.unixtime2local(t.sse);
    t
}

fn do_parse(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let input = unhex(f.get(1).copied().unwrap_or(""));
    let (t, e) = strtotime(&input, ctx.db(), &mut ctx.wrapper());
    dump_time(out, &t);
    dump_errors(out, &e);
}

fn do_init(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let mut input = unhex(f.get(3).copied().unwrap_or(""));
    if input.is_empty() {
        input = b"now".to_vec();
    }
    let now_ts = atoll(f[2]);
    let (mut t, e) = strtotime(&input, ctx.db(), &mut ctx.wrapper());
    if e.error_count() > 0 {
        out.push_str("ERR");
        dump_errors(out, &e);
        return;
    }
    let tzi = if t.tz_info.is_some() {
        t.tz_info.clone()
    } else {
        ctx.cache.get(f[1])
    };
    let mut now = Time::new();
    now.zone_type = ZONETYPE_ID;
    now.tz_info = tzi.clone();
    now.unixtime2local(now_ts);
    now.us = 0;
    if input.len() == 3 && strncasecmp(&input, b"now", 3) == 0 {
        t = now;
    } else {
        t.fill_holes(&now, NO_CLONE);
        t.update_ts(tzi.as_ref());
        t.update_from_sse();
        t.have_relative = 0;
    }
    dump_time(out, &t);
    dump_errors(out, &e);
}

fn do_fmtinit(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let format = unhex(f[4]);
    let input = unhex(f.get(5).copied().unwrap_or(""));
    let (mut t, e) = parse_from_format(&format, &input, ctx.db(), &mut ctx.wrapper());
    if e.error_count() > 0 {
        out.push_str("ERR");
        dump_errors(out, &e);
        return;
    }
    let tzi = if t.tz_info.is_some() {
        t.tz_info.clone()
    } else {
        ctx.cache.get(f[1])
    };
    let mut now = Time::new();
    now.zone_type = ZONETYPE_ID;
    now.tz_info = tzi.clone();
    now.unixtime2local(atoll(f[2]));
    now.us = atoll(f[3]);
    t.fill_holes(&now, NO_CLONE | OVERRIDE_TIME);
    t.update_ts(tzi.as_ref());
    t.update_from_sse();
    t.have_relative = 0;
    dump_time(out, &t);
    dump_errors(out, &e);
}

fn do_modify(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let input = unhex(f.get(3).copied().unwrap_or(""));
    let mut t = Time::new();
    t.zone_type = ZONETYPE_ID;
    t.tz_info = ctx.cache.get(f[1]);
    t.unixtime2local(atoll(f[2]));
    t.us = 0;
    let (tmp, e) = strtotime(&input, ctx.db(), &mut ctx.wrapper());
    if e.error_count() > 0 {
        out.push_str("ERR");
        dump_errors(out, &e);
        return;
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
    dump_time(out, &t);
    dump_errors(out, &e);
}

fn do_format(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let format = unhex(f[1]);
    let input = unhex(f.get(2).copied().unwrap_or(""));
    let (t, e) = parse_from_format(&format, &input, ctx.db(), &mut ctx.wrapper());
    dump_time(out, &t);
    dump_errors(out, &e);
}

fn do_local(ctx: &Ctx, out: &mut String, f: &[&str]) {
    match ctx.cache.get(f[1]) {
        None => out.push_str("NOTZ"),
        Some(tz) => {
            let mut t = Time::new();
            t.zone_type = ZONETYPE_ID;
            t.tz_info = Some(tz);
            t.unixtime2local(atoll(f[2]));
            dump_time(out, &t);
        }
    }
}

fn do_diff(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let z1 = zone_init(ctx, &unhex(f[1]), None);
    let z2 = zone_init(ctx, &unhex(f[4]), None);
    let (Ok(z1), Ok(z2)) = (z1, z2) else {
        out.push_str("BADZONE");
        return;
    };
    let t1 = mk_time(&z1, atoll(f[2]), atoll(f[3]));
    let t2 = mk_time(&z2, atoll(f[5]), atoll(f[6]));
    let dd = interval::diff_days(&t1, &t2);
    let rt = interval::diff(&t1, &t2);
    let _ = write!(out, "dd={dd} | ");
    dump_rel(out, &rt);
    out.push_str(" || ");
    dump_time(out, &t1);
    out.push_str(" || ");
    dump_time(out, &t2);
}

fn do_add(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let Ok(z) = zone_init(ctx, &unhex(f[1]), None) else {
        out.push_str("BADZONE");
        return;
    };
    let iv = match f[5] {
        "rel" => {
            let s = unhex(f.get(6).copied().unwrap_or(""));
            let (p, e) = strtotime(&s, ctx.db(), &mut ctx.wrapper());
            if e.error_count() > 0 {
                out.push_str("ERR");
                dump_errors(out, &e);
                None
            } else {
                Some(p.relative)
            }
        }
        "f" if f.len() >= 14 => Some(RelTime {
            y: atoll(f[6]),
            m: atoll(f[7]),
            d: atoll(f[8]),
            h: atoll(f[9]),
            i: atoll(f[10]),
            s: atoll(f[11]),
            us: atoll(f[12]),
            invert: atoll(f[13]) as i32,
            days: UNSET,
            ..RelTime::default()
        }),
        _ => {
            out.push_str("BADLINE");
            None
        }
    };
    let Some(iv) = iv else { return };
    let t = mk_time(&z, atoll(f[2]), atoll(f[3]));
    let r = match f[4] {
        "add" => interval::add(&t, &iv),
        "sub" => interval::sub(&t, &iv),
        "addwall" => interval::add_wall(&t, &iv),
        "subwall" => interval::sub_wall(&t, &iv),
        _ => {
            out.push_str("BADOP");
            return;
        }
    };
    dump_time(out, &r);
}

fn do_mk(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let Ok(z) = zone_init(ctx, &unhex(f[1]), None) else {
        out.push_str("BADZONE");
        return;
    };
    let mut t = mk_time(&z, atoll(f[2]), 0);
    let (a, b, c, d) = (atoll(f[4]), atoll(f[5]), atoll(f[6]), atoll(f[7]));
    match f[3] {
        "date" => {
            t.y = a;
            t.m = b;
            t.d = c;
            t.update_ts(None);
        }
        "time" => {
            t.h = a;
            t.i = b;
            t.s = c;
            t.us = d;
            t.update_ts(None);
            t.update_from_sse();
        }
        "iso" => {
            t.y = a;
            t.m = 1;
            t.d = 1;
            t.relative = RelTime::default();
            t.relative.d = dow::daynr_from_weeknr(a, b, c);
            t.have_relative = 1;
            t.update_ts(None);
        }
        "ts" => {
            t.unixtime2local(a);
            t.update_ts(None);
            t.us = 0;
        }
        _ => {}
    }
    dump_time(out, &t);
    let _ = write!(out, " | off={}", t.get_current_offset());
}

fn do_info(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let Some(tz) = ctx.cache.get(f[1]) else {
        out.push_str("NOTZ");
        return;
    };
    let ts = atoll(f[2]);
    let o = tz.get_time_zone_info(ts);
    let _ = write!(
        out,
        "o={} ls={} dst={} abbr=",
        o.offset, o.leap_secs, o.is_dst
    );
    put_str(out, Some(o.abbr.as_bytes()));
    let _ = write!(out, " tt={}", o.transition_time);
    let (ok, off, tt, isdst) = match tz.get_time_zone_offset_info(ts) {
        Some((off, tt, isdst)) => (1, off, tt, isdst),
        None => (0, 0, 0, 0),
    };
    let _ = write!(
        out,
        " | ok={ok} off={off} tt={tt} dst={isdst} | in={}",
        tz.timestamp_is_in_dst(ts)
    );
    if tz.posix_info.as_ref().is_some_and(|p| p.dst_end.is_some()) {
        let (y, _, _) = unixtime2date(ts);
        let mut tr = PosixTransitions::default();
        get_transitions_for_year(&tz, y.wrapping_sub(1), &mut tr);
        get_transitions_for_year(&tz, y, &mut tr);
        get_transitions_for_year(&tz, y.wrapping_add(1), &mut tr);
        let _ = write!(out, " | n={}", tr.count);
        for i in 0..tr.count {
            let _ = write!(out, " {}:{}", tr.times[i], tr.types[i]);
        }
    }
}

fn put_trans_info(out: &mut String, p: Option<&PosixTransInfo>) {
    match p {
        None => out.push('-'),
        Some(p) if p.kind == TRANS_TYPE_MWD => {
            let _ = write!(
                out,
                "{}:{}.{}.{}/{}",
                p.kind, p.month, p.week, p.dow, p.hour
            );
        }
        Some(p) => {
            let _ = write!(out, "{}:{}/{}", p.kind, p.days, p.hour);
        }
    }
}

fn do_tzfile(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let name = c_string(unhex(f.get(1).copied().unwrap_or("")));
    let name_str = String::from_utf8_lossy(&name);
    let result = parse_tzfile(&name_str, ctx.db());
    let err = match &result {
        Ok((_, code)) => *code,
        Err(code) => *code,
    };
    let _ = write!(
        out,
        "err={err} valid={}",
        i32::from(timezone_id_is_valid(&name, ctx.db()))
    );
    let Ok((tz, _)) = result else { return };
    out.push_str(" name=");
    put_str(out, Some(tz.name.as_bytes()));
    let _ = write!(out, " bc={} cc=", u8::from(tz.bc));
    put_str(out, Some(tz.location.country_code.as_bytes()));
    let _ = write!(
        out,
        " lat={:016x} lon={:016x} com=",
        tz.location.latitude.to_bits(),
        tz.location.longitude.to_bits()
    );
    put_str(out, Some(tz.location.comments.as_bytes()));
    for (label, c) in [("b32", &tz.bit32), ("b64", &tz.bit64)] {
        let _ = write!(
            out,
            " {label}={},{},{},{},{},{}",
            c.ttisgmtcnt, c.ttisstdcnt, c.leapcnt, c.timecnt, c.typecnt, c.charcnt
        );
    }
    out.push_str(" trans=");
    for i in 0..tz.bit64.timecnt as usize {
        let _ = write!(out, "{}:{},", tz.trans[i], tz.trans_idx[i]);
    }
    out.push_str(" types=");
    for t in tz.types.iter().take(tz.bit64.typecnt as usize) {
        let _ = write!(
            out,
            "{}/{}/{}/{}/{},",
            t.offset, t.isdst, t.abbr_idx, t.isstdcnt, t.isgmtcnt
        );
    }
    out.push_str(" abbrs=");
    for c in tz.timezone_abbr.iter().take(tz.bit64.charcnt as usize) {
        let _ = write!(out, "{c:02x}");
    }
    out.push_str(" posix=");
    put_str(out, tz.posix_string.as_deref().map(str::as_bytes));
    if let Some(p) = &tz.posix_info {
        out.push_str(" std=");
        put_str(out, Some(p.std.as_bytes()));
        let _ = write!(out, " so={} dst=", p.std_offset);
        put_str(out, p.dst.as_deref().map(str::as_bytes));
        let _ = write!(out, " do={} b=", p.dst_offset);
        put_trans_info(out, p.dst_begin.as_ref());
        out.push_str(" e=");
        put_trans_info(out, p.dst_end.as_ref());
        let _ = write!(
            out,
            " ti={},{}",
            p.type_index_std_type, p.type_index_dst_type
        );
    }
}

fn do_dow(out: &mut String, f: &[&str]) {
    let (y, m, d) = (atoll(f[1]), atoll(f[2]), atoll(f[3]));
    let _ = write!(
        out,
        "vd={} vt={} leap={} dn={}",
        i32::from(dow::valid_date(y, m, d)),
        i32::from(dow::valid_time(y, m, d)),
        i32::from(is_leap(y)),
        dow::daynr_from_weeknr(y, m, d)
    );
    let mut tmp = Time::new();
    (tmp.y, tmp.m, tmp.d) = (y, m, d);
    let _ = write!(out, " ed={}", tmp.epoch_days_from_time());
    let (a, b, c) = date_from_epoch_days(y);
    let _ = write!(out, " fe={a}-{b}-{c}");
    let (a, b, c) = unixtime2date(y);
    let _ = write!(out, " ud={a}-{b}-{c}");
    if (0..=12).contains(&m) {
        let _ = write!(
            out,
            " dow={} idow={} doy={} dim={}",
            dow::day_of_week(y, m, d),
            dow::iso_day_of_week(y, m, d),
            dow::day_of_year(y, m, d),
            dow::days_in_month(y, m)
        );
        let (iw, iy) = dow::isoweek_from_date(y, m, d);
        let _ = write!(out, " iw={iw} iy={iy}");
        let (a, b, c) = dow::isodate_from_date(y, m, d);
        let _ = write!(out, " isod={a}-{b}-{c}");
    }
    if (-60..=60).contains(&m) && (-60..=60).contains(&d) {
        let (a, b, c) = dow::date_from_isodate(y, m, d);
        let _ = write!(out, " fromiso={a}-{b}-{c}");
    }
}

fn do_abbr(out: &mut String, f: &[&str]) {
    let s = c_string(unhex(f[1]));
    let r = timezone_id_from_abbr(&s, atoll(f[2]), atoll(f[3]) as i32).flatten();
    put_str(out, r.map(str::as_bytes));
}

fn do_ival(out: &mut String, f: &[&str]) {
    let s = unhex(f.get(1).copied().unwrap_or(""));
    let r = strtointerval(&s);
    let _ = write!(out, "r={} | b=", r.recurrences.unwrap_or(0));
    match &r.begin {
        Some(b) => dump_time(out, b),
        None => out.push('-'),
    }
    out.push_str(" | e=");
    match &r.end {
        Some(e) => dump_time(out, e),
        None => out.push('-'),
    }
    out.push_str(" | p=");
    match &r.period {
        Some(p) => dump_rel(out, p),
        None => out.push('-'),
    }
    dump_errors_ex(out, &r.errors, false);
}

fn do_raw(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let input = unhex(f.get(1).copied().unwrap_or(""));
    let (mut t, e) = strtotime(&input, ctx.db(), &mut ctx.wrapper());
    t.update_ts(None);
    dump_time(out, &t);
    out.push_str(" || ");
    t.update_from_sse();
    dump_time(out, &t);
    out.push_str(" || ");
    if t.zone_type != ZONETYPE_ID || t.tz_info.is_some() {
        t.unixtime2local(t.sse);
        dump_time(out, &t);
    }
    dump_errors(out, &e);
}

fn do_posix(out: &mut String, f: &[&str]) {
    let s = c_string(unhex(f[1]));
    let Some(mut p) = parse_posix_str(&String::from_utf8_lossy(&s)) else {
        out.push_str("NULL");
        return;
    };
    out.push_str("std=");
    put_str(out, Some(p.std.as_bytes()));
    let _ = write!(out, " so={} dst=", p.std_offset);
    put_str(out, p.dst.as_deref().map(str::as_bytes));
    let _ = write!(out, " do={} b=", p.dst_offset);
    put_trans_info(out, p.dst_begin.as_ref());
    out.push_str(" e=");
    put_trans_info(out, p.dst_end.as_ref());
    if p.dst_end.is_some() {
        p.type_index_std_type = 0;
        p.type_index_dst_type = 1;
        let tz = TzInfo {
            name: String::new(),
            bit32: Default::default(),
            bit64: Default::default(),
            trans: Vec::new(),
            trans_idx: Vec::new(),
            types: Vec::new(),
            timezone_abbr: Vec::new(),
            leap_times: Vec::new(),
            bc: false,
            location: Default::default(),
            posix_string: None,
            posix_info: Some(p),
        };
        let mut tr = PosixTransitions::default();
        get_transitions_for_year(&tz, atoll(f[2]), &mut tr);
        let _ = write!(out, " | n={}", tr.count);
        for i in 0..tr.count {
            let _ = write!(out, " {}:{}", tr.times[i], tr.types[i]);
        }
    }
}

fn do_hms(out: &mut String, f: &[&str]) {
    let h = f64::from_bits(u64::from_str_radix(f[1], 16).unwrap_or(0));
    let (hh, mm, ss) = decimal_hour_to_hms(h);
    // `atoi()`: `(int) strtol()`.
    let atoi = |s: &str| atoll(s) as i32;
    let d1 = hms_to_decimal_hour(atoi(f[2]), atoi(f[3]), atoi(f[4]));
    let d2 = hmsf_to_decimal_hour(atoi(f[2]), atoi(f[3]), atoi(f[4]), atoi(f[5]));
    let _ = write!(
        out,
        "{hh} {mm} {ss} | {:016x} {:016x} | {} | ",
        d1.to_bits(),
        d2.to_bits(),
        hms_to_seconds(atoll(f[2]), atoll(f[3]), atoll(f[4]))
    );
    put_str(out, Some(get_error_message(atoi(f[2])).as_bytes()));
}

/// The map of the C harness's `formatx` command: PHP's default map plus the ISO week/year and
/// offset-in-minutes specifiers, which PHP does not map.
fn custom_format_map() -> Vec<(u8, FormatSpecifierCode)> {
    let mut map: Vec<(u8, FormatSpecifierCode)> = DEFAULT_FORMAT_MAP
        .iter()
        .copied()
        .filter(|(c, _)| !b"PpTOe".contains(c))
        .collect();
    map.extend([
        (b'e', FormatSpecifierCode::TimezoneOffset),
        (b'Z', FormatSpecifierCode::TimezoneOffsetMinutes),
        (b'V', FormatSpecifierCode::WeekOfYearIso),
        (b'W', FormatSpecifierCode::WeekOfYear),
        (b'N', FormatSpecifierCode::DayOfWeekIso),
        (b'w', FormatSpecifierCode::DayOfWeek),
        (b'o', FormatSpecifierCode::YearIso),
    ]);
    map
}

fn do_formatx(ctx: &Ctx, out: &mut String, f: &[&str]) {
    let map = custom_format_map();
    let config = FormatConfig {
        format_map: &map,
        prefix_char: if atoll(f[1]) != 0 { b'%' } else { 0 },
    };
    let format = unhex(f[2]);
    let input = unhex(f.get(3).copied().unwrap_or(""));
    let (t, e) = parse_from_format_with_map(&format, &input, ctx.db(), &mut ctx.wrapper(), &config);
    dump_time(out, &t);
    dump_errors(out, &e);
}

fn main() {
    let ctx = Ctx {
        cache: TzCache::builtin(),
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut w = BufWriter::new(stdout.lock());
    for line in stdin.lock().split(b'\n') {
        let Ok(line) = line else { break };
        let line = String::from_utf8_lossy(&line);
        let line = line.trim_end_matches(['\n', '\r']);
        let f: Vec<&str> = line.splitn(16, ' ').collect();
        let n = f.len();
        let mut out = String::new();
        match f[0] {
            "parse" => do_parse(&ctx, &mut out, &f),
            "init" if n >= 3 => do_init(&ctx, &mut out, &f),
            "modify" if n >= 3 => do_modify(&ctx, &mut out, &f),
            "format" if n >= 2 => do_format(&ctx, &mut out, &f),
            "local" if n >= 3 => do_local(&ctx, &mut out, &f),
            "fmtinit" if n >= 5 => do_fmtinit(&ctx, &mut out, &f),
            "zone" => {
                let name = unhex(f.get(1).copied().unwrap_or(""));
                let _ = zone_init(&ctx, &name, Some(&mut out));
            }
            "diff" if n >= 7 => do_diff(&ctx, &mut out, &f),
            "add" if n >= 6 => do_add(&ctx, &mut out, &f),
            "mk" if n >= 8 => do_mk(&ctx, &mut out, &f),
            "info" if n >= 3 => do_info(&ctx, &mut out, &f),
            "tzfile" => do_tzfile(&ctx, &mut out, &f),
            "dow" if n >= 4 => do_dow(&mut out, &f),
            "abbr" if n >= 4 => do_abbr(&mut out, &f),
            "ival" => do_ival(&mut out, &f),
            "raw" => do_raw(&ctx, &mut out, &f),
            "posix" if n >= 3 => do_posix(&mut out, &f),
            "hms" if n >= 6 => do_hms(&mut out, &f),
            "formatx" if n >= 3 => do_formatx(&ctx, &mut out, &f),
            _ => out.push_str("BADLINE"),
        }
        out.push('\n');
        if w.write_all(out.as_bytes()).is_err() {
            break;
        }
    }
    let _ = w.flush();
}
