//! Oracle vector tests: twig-timelib against PHP 8.5.13 (timelib 2022.17, timezonedb 2026.6).
//!
//! The vectors in `tests/vectors/*.json` are produced by the `*.gen.php` scripts next to them; regenerate with
//! `crates/twig-timelib/tests/vectors/regen.sh` (needs the `twig-oracle:3.31.1` Docker image from
//! `research/php-oracle`). The PHP layer each vector goes through is emulated in `tests/common/mod.rs`.

mod common;

use common::*;
use serde_json::{Value, json};
use twig_timelib::abbr::{timezone_abbreviations_list, timezone_id_from_abbr};
use twig_timelib::parse_date::parse_from_format;
use twig_timelib::posix::{PosixTransitions, get_transitions_for_year};
use twig_timelib::tz::{TzInfo, parse_tzfile};
use twig_timelib::unixtime2tm::unixtime2date;
use twig_timelib::{RelTime, UNSET, builtin_db, interval};

fn s(v: &Value) -> &str {
    v.as_str().expect("string")
}

fn i(v: &Value) -> i64 {
    v.as_i64().expect("integer")
}

/// Collects mismatches and fails with the first few once the whole vector set has been checked.
struct Report {
    name: &'static str,
    total: usize,
    failures: Vec<String>,
}

impl Report {
    fn new(name: &'static str) -> Self {
        Report {
            name,
            total: 0,
            failures: Vec::new(),
        }
    }

    fn check(&mut self, ok: bool, describe: impl FnOnce() -> String) {
        self.total += 1;
        if !ok {
            self.failures.push(describe());
        }
    }

    fn finish(self) {
        assert!(self.total > 0, "{}: no vectors", self.name);
        if !self.failures.is_empty() {
            let shown: Vec<_> = self.failures.iter().take(10).cloned().collect();
            panic!(
                "{}: {} of {} cases differ:\n{}",
                self.name,
                self.failures.len(),
                self.total,
                shown.join("\n")
            );
        }
    }
}

#[test]
fn date_parse_vectors() {
    let v = load("parse");
    assert_eq!(s(&v["php"]), "8.5.13");
    let mut r = Report::new("date_parse");
    for case in v["parse"].as_array().unwrap() {
        let input = s(&case["in"]);
        let (t, e) = strtotime(input.as_bytes());
        let got = compact_parse(input, &t, &e);
        r.check(&got == case, || {
            format!("  input {input:?}\n    php  {case}\n    rust {got}")
        });
    }
    r.finish();
}

#[test]
fn date_parse_from_format_vectors() {
    let v = load("parse");
    let mut r = Report::new("date_parse_from_format");
    for case in v["format"].as_array().unwrap() {
        let fmt = s(&case["fmt"]);
        let input = s(&case["in"]);
        let (t, e) = parse_from_format(
            fmt.as_bytes(),
            input.as_bytes(),
            cache().tzdb(),
            &mut wrapper(),
        );
        let mut got = compact_parse(input, &t, &e);
        let mut expected = case.clone();
        expected.as_object_mut().unwrap().remove("fmt");
        got.as_object_mut().unwrap().remove("fmt");
        r.check(got == expected, || {
            format!("  format {fmt:?} input {input:?}\n    php  {case}\n    rust {got}")
        });
    }
    r.finish();
}

#[test]
fn strtotime_vectors() {
    let v = load("strtotime");
    let mut r = Report::new("strtotime");
    for case in v["strtotime"].as_array().unwrap() {
        let (zone, base, input) = (s(&case[0]), i(&case[1]), s(&case[2]));
        let expected = case[3].as_i64();
        let got = php_strtotime(input.as_bytes(), base, zone);
        r.check(got == expected, || {
            format!("  strtotime({input:?}, {base}) in {zone}: php {expected:?} rust {got:?}")
        });
    }
    r.finish();
}

#[test]
fn modify_vectors() {
    let v = load("strtotime");
    let mut r = Report::new("DateTime::modify");
    for case in v["modify"].as_array().unwrap() {
        let (zone, base, input, expected) = (s(&case[0]), i(&case[1]), s(&case[2]), s(&case[3]));
        let tz = tz_new(zone).unwrap();
        let mut t = from_ts(base, 0, &tz);
        let got = match modify(&mut t, input.as_bytes()) {
            Ok(()) => show(&t),
            Err(e) => e,
        };
        r.check(got == expected, || {
            format!("  @{base} {zone} modify({input:?}):\n    php  {expected}\n    rust {got}")
        });
    }
    r.finish();
}

#[test]
fn construct_vectors() {
    let v = load("strtotime");
    let mut r = Report::new("DateTime::__construct");
    for case in v["construct"].as_array().unwrap() {
        let (zone, input, expected) = (s(&case[0]), s(&case[1]), s(&case[2]));
        let tz = tz_new(zone).unwrap();
        let got = match construct(input.as_bytes(), &tz, 1_764_000_000, 0) {
            Ok(t) => show(&t),
            Err(e) => e,
        };
        r.check(got == expected, || {
            format!("  new DateTime({input:?}, {zone}):\n    php  {expected}\n    rust {got}")
        });
    }
    r.finish();
}

#[test]
fn modify_month_overflow() {
    // The canonical example from the task: timelib's month overflow.
    let tz = tz_new("UTC").unwrap();
    let mut t = construct(b"2021-01-31", &tz, 0, 0).unwrap();
    modify(&mut t, b"+1 month").unwrap();
    assert_eq!(format(&t, "Y-m-d"), "2021-03-03");
}

#[test]
fn diff_vectors() {
    let v = load("interval");
    let mut r = Report::new("DateTime::diff");
    for case in v["diff"].as_array().unwrap() {
        let a = from_ts(i(&case[1]), i(&case[2]), &tz_new(s(&case[0])).unwrap());
        let b = from_ts(i(&case[4]), i(&case[5]), &tz_new(s(&case[3])).unwrap());
        let rt = interval::diff(&a, &b);
        let days = if rt.days == UNSET {
            json!(false)
        } else {
            json!(rt.days)
        };
        let got = json!([rt.y, rt.m, rt.d, rt.h, rt.i, rt.s, rt.us, rt.invert, days]);
        r.check(got == case[6], || format!("  {case}\n    rust {got}"));
    }
    r.finish();
}

#[test]
fn add_sub_vectors() {
    let v = load("interval");
    let mut r = Report::new("DateTime::add/sub");
    for case in v["add"].as_array().unwrap() {
        let tz = tz_new(s(&case[0])).unwrap();
        let t = from_ts(i(&case[1]), i(&case[2]), &tz);
        let wall = s(&case[3]) == "wall";
        let rel = if wall {
            let f: Vec<i64> = case[6].as_array().unwrap().iter().map(i).collect();
            RelTime {
                y: f[0],
                m: f[1],
                d: f[2],
                h: f[3],
                i: f[4],
                s: f[5],
                us: f[6],
                invert: f[7] as i32,
                days: UNSET,
                ..RelTime::default()
            }
        } else {
            strtotime(s(&case[4]).as_bytes()).0.relative
        };
        let got = match add_sub(&t, &rel, wall, s(&case[5]) == "sub") {
            Ok(t) => show(&t),
            Err(e) => e,
        };
        r.check(got == s(&case[7]), || format!("  {case}\n    rust {got}"));
    }
    r.finish();
}

#[test]
fn setter_vectors() {
    let v = load("setters");
    assert_eq!(s(&v["php"]), "8.5.13");
    let mut r = Report::new("DateTime::setDate/setTime/setISODate/setTimestamp");
    for case in v["set"].as_array().unwrap() {
        let (zone, ts, op) = (s(&case[0]), i(&case[1]), s(&case[2]));
        let a: Vec<i64> = case[3].as_array().unwrap().iter().map(i).collect();
        let mut t = from_ts(ts, 0, &tz_new(zone).unwrap());
        match op {
            "date" => set_date(&mut t, a[0], a[1], a[2]),
            "time" => set_time(&mut t, a[0], a[1], a[2], a[3]),
            "iso" => set_isodate(&mut t, a[0], a[1], a[2]),
            _ => set_timestamp(&mut t, a[0]),
        }
        let got = show(&t);
        r.check(got == s(&case[4]), || format!("  {case}\n    rust {got}"));
    }
    r.finish();
}

#[test]
fn date_interval_iso_vectors() {
    let v = load("iso_interval");
    assert_eq!(s(&v["php"]), "8.5.13");
    let mut r = Report::new("new DateInterval");
    for case in v["iso"].as_array().unwrap() {
        let spec = s(&case[0]);
        let got = match date_interval_new(spec.as_bytes()) {
            Ok(rt) => {
                let days = if rt.days == UNSET {
                    json!(false)
                } else {
                    json!(rt.days)
                };
                json!([rt.y, rt.m, rt.d, rt.h, rt.i, rt.s, rt.us, rt.invert, days])
            }
            Err(e) => json!(e),
        };
        r.check(got == case[1], || {
            format!(
                "  new DateInterval({spec:?}):\n    php  {}\n    rust {got}",
                case[1]
            )
        });
    }
    r.finish();
}

#[test]
fn zone_list_and_locations() {
    let v = load("zones");
    assert_eq!(s(&v["timezone_version"]), builtin_db().version);
    let zones = v["zones"].as_array().unwrap();
    assert_eq!(zones.len(), builtin_db().index.len());
    let mut r = Report::new("zones");
    for (case, (name, _)) in zones.iter().zip(builtin_db().index.iter()) {
        r.check(s(&case[0]) == *name, || {
            format!("  order: php {} rust {name}", case[0])
        });
        let (tz, _) = parse_tzfile(name, builtin_db()).unwrap();
        // timezone_identifiers_list() without arguments lists the entries with the BC flag set.
        let allowed = [
            "Africa/",
            "America/",
            "Antarctica/",
            "Arctic/",
            "Asia/",
            "Atlantic/",
            "Australia/",
            "Europe/",
            "Indian/",
            "Pacific/",
            "UTC",
        ]
        .iter()
        .any(|p| name.len() >= p.len() && name[..p.len()].eq_ignore_ascii_case(p));
        r.check(case[1].as_bool() == Some(allowed && tz.bc), || {
            format!(
                "  {name}: in default list php {} rust {}",
                case[1],
                allowed && tz.bc
            )
        });
        match tz_new(name) {
            Ok(PhpTz::Id(z)) => {
                let got = json!([
                    name,
                    case[1],
                    z.location.country_code,
                    z.location.latitude,
                    z.location.longitude,
                    z.location.comments
                ]);
                r.check(&got == case, || format!("  php {case}\n    rust {got}"));
            }
            Ok(_) => r.check(case[2].is_null(), || {
                format!("  {name}: php has a location, rust parsed a non-identifier zone")
            }),
            Err(e) => r.check(false, || format!("  {name}: {e}")),
        }
    }
    r.finish();
}

#[test]
fn unixtime2local_in_every_zone() {
    let v = load("zones");
    let mut r = Report::new("unixtime2local");
    for case in v["local"].as_array().unwrap() {
        let (name, ts) = (s(&case[0]), i(&case[1]));
        let t = from_ts(ts, 0, &tz_new(name).unwrap());
        let f = |c: &str| format(&t, c);
        let n = |c: &str| f(c).parse::<i64>().unwrap();
        let got = json!([
            n("Y"),
            n("n"),
            n("j"),
            n("G"),
            n("i"),
            n("s"),
            n("Z"),
            n("I"),
            f("T"),
            n("N"),
            n("z"),
            n("W"),
            n("o"),
            n("t"),
            n("L")
        ]);
        r.check(got == case[2], || {
            format!("  {name} @{ts}: php {} rust {got}", case[2])
        });
    }
    r.finish();
}

/// `timezone_transitions_get()` (PHPSRC/ext/date/php_date.c) without the formatted "time" field.
fn transitions(tz: &TzInfo, begin: i64, end: i64) -> Vec<Value> {
    let mut out = Vec::new();
    let entry = |out: &mut Vec<Value>, ts: i64, ty: usize| {
        let t = tz.types[ty];
        out.push(json!([
            ts,
            t.offset,
            t.isdst != 0,
            tz.abbr_at(t.abbr_idx as usize)
        ]));
    };
    let timecnt = tz.bit64.timecnt as usize;
    let mut begin_idx = 0usize;
    let mut found = false;
    if begin == i64::MIN {
        entry(&mut out, begin, 0);
        found = true;
    } else if timecnt > 0 {
        while begin_idx < timecnt {
            if tz.trans[begin_idx] > begin {
                if begin_idx > 0 {
                    entry(&mut out, begin, usize::from(tz.trans_idx[begin_idx - 1]));
                } else {
                    entry(&mut out, begin, 0);
                }
                found = true;
                break;
            }
            begin_idx += 1;
        }
    }
    let has_dst_rules = tz.posix_info.as_ref().is_some_and(|p| p.dst_end.is_some());
    if !found {
        if timecnt > 0 {
            if has_dst_rules {
                let to = tz.get_time_zone_info(begin);
                out.push(json!([begin, to.offset, to.is_dst != 0, to.abbr]));
            } else {
                entry(&mut out, begin, usize::from(tz.trans_idx[timecnt - 1]));
            }
        } else {
            entry(&mut out, begin, 0);
        }
    } else {
        for k in begin_idx..timecnt {
            if tz.trans[k] < end {
                entry(&mut out, tz.trans[k], usize::from(tz.trans_idx[k]));
            } else {
                return out;
            }
        }
    }
    if has_dst_rules {
        let last = tz.trans[timecnt - 1];
        let (start_y, _, _) = unixtime2date(last);
        let (end_y, _, _) = unixtime2date(end);
        for y in start_y..=end_y {
            let mut tr = PosixTransitions::default();
            get_transitions_for_year(tz, y, &mut tr);
            for j in 0..tr.count {
                if tr.times[j] <= last || tr.times[j] < begin {
                    continue;
                }
                if tr.times[j] > end {
                    return out;
                }
                entry(&mut out, tr.times[j], tr.types[j] as usize);
            }
        }
    }
    out
}

#[test]
fn transition_lists() {
    let v = load("zones");
    let mut r = Report::new("getTransitions");
    for case in v["transitions"].as_array().unwrap() {
        let (name, begin, end) = (s(&case[0]), i(&case[1]), i(&case[2]));
        let (tz, _) = parse_tzfile(name, builtin_db()).unwrap();
        let got = Value::Array(transitions(&tz, begin, end));
        r.check(got == case[3], || {
            format!(
                "  {name} {begin}..{end}: php {} entries, rust {} entries",
                case[3].as_array().unwrap().len(),
                got.as_array().unwrap().len()
            )
        });
    }
    r.finish();
}

#[test]
fn abbreviation_tables() {
    let v = load("zones");
    // timezone_abbreviations_list() groups entries by abbreviation in order of first appearance.
    let mut groups: Vec<(&str, Vec<Value>)> = Vec::new();
    for e in timezone_abbreviations_list() {
        let item = json!([e.name, e.dst != 0, e.gmtoffset, e.full_tz_name]);
        match groups.iter_mut().find(|(n, _)| *n == e.name) {
            Some((_, list)) => list.push(item),
            None => groups.push((e.name, vec![item])),
        }
    }
    let flat: Vec<Value> = groups.into_iter().flat_map(|(_, l)| l).collect();
    assert_eq!(Value::Array(flat), v["abbreviations"]);

    let mut r = Report::new("timezone_name_from_abbr");
    for case in v["from_abbr"].as_array().unwrap() {
        let got = timezone_id_from_abbr(s(&case[0]).as_bytes(), i(&case[1]), i(&case[2]) as i32)
            .flatten();
        let got = got.map_or(json!(false), |z| json!(z));
        r.check(got == case[3], || format!("  {case}: rust {got}"));
    }
    r.finish();
}
