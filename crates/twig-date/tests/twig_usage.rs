//! The crate driven the way Twig's `CoreExtension::convertDate()` / `formatDate()` / `modifyDate()` use PHP's
//! date classes (TWIG/src/Extension/CoreExtension.php), checked against the expectations of Twig 3.31.1's
//! `filters/date.test`, `filters/date_immutable.test`, `filters/date_modify.test` and
//! `filters/date_interval.test` fixtures.

mod common;

use common::*;
use twig_date::{DateContext, DateObject, IntervalObject, TimeZoneObject, classes, functions};
use twig_value::{ObjectRef, PhpResult, Value, convert};

/// The `$timezone` argument of `convertDate()`.
enum Tz<'a> {
    /// `null`: the default timezone.
    Default,
    /// `false`: keep the date's zone.
    Keep,
    /// A zone name.
    Name(&'a str),
    /// A `DateTimeZone` object.
    Obj(ObjectRef),
}

/// `CoreExtension::getTimezone()`.
fn default_tz(ctx: &DateContext) -> ObjectRef {
    let name = ctx.default_timezone_get().unwrap();
    TimeZoneObject::construct(ctx, name.as_bytes()).unwrap()
}

fn ctype_digit(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(u8::is_ascii_digit)
}

/// `CoreExtension::convertDate()`.
fn convert_date(ctx: &DateContext, date: &Value, timezone: Tz<'_>) -> PhpResult<ObjectRef> {
    let mut diags = Vec::new();
    let tz: Option<ObjectRef> = match timezone {
        Tz::Keep => None,
        Tz::Default => Some(default_tz(ctx)),
        Tz::Name(n) => Some(TimeZoneObject::construct(ctx, n.as_bytes())?),
        Tz::Obj(o) => Some(o),
    };
    if let Value::Object(o) = date {
        if o.instance_of("DateTimeImmutable") {
            return match &tz {
                Some(z) => Ok(obj(call(
                    o,
                    "setTimezone",
                    vec![Value::Object(z.clone())],
                    &mut diags,
                )?)),
                None => Ok(o.clone()),
            };
        }
        if o.instance_of("DateTime") {
            let d = o.clone_object(&mut diags)?;
            if let Some(z) = &tz {
                call(
                    &d,
                    "setTimezone",
                    vec![Value::Object(z.clone())],
                    &mut diags,
                )?;
            }
            return Ok(d);
        }
    }
    if date.is_null() || matches!(date, Value::String(s) if s.as_bytes() == b"now") {
        let z = tz.unwrap_or_else(|| default_tz(ctx));
        return DateObject::construct(ctx, classes::datetime(), b"now", Some(&z));
    }
    let s = convert::to_string(date, &mut diags)?;
    let b = s.as_bytes();
    let d = if ctype_digit(b) || (!b.is_empty() && b[0] == b'-' && ctype_digit(&b[1..])) {
        let mut at = b"@".to_vec();
        at.extend_from_slice(b);
        DateObject::construct(ctx, classes::datetime(), &at, None)?
    } else {
        DateObject::construct(ctx, classes::datetime(), b, None)?
    };
    if let Some(z) = &tz {
        call(
            &d,
            "setTimezone",
            vec![Value::Object(z.clone())],
            &mut diags,
        )?;
    }
    Ok(d)
}

/// `CoreExtension::formatDate()` with the default formats `F j, Y H:i` / `%d days`.
fn format_date(ctx: &DateContext, date: &Value, format: Option<&str>, tz: Tz<'_>) -> String {
    if let Value::Object(o) = date
        && let Some(iv) = o.downcast_ref::<IntervalObject>()
    {
        return iv
            .format(format.unwrap_or("%d days").as_bytes())
            .unwrap()
            .to_string_lossy()
            .into_owned();
    }
    let d = convert_date(ctx, date, tz).unwrap();
    let f = format.unwrap_or("F j, Y H:i");
    d.downcast_ref::<DateObject>()
        .unwrap()
        .format(f.as_bytes())
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn new_date(ctx: &DateContext, s: &str, tz: Option<&str>) -> Value {
    let tz = tz.map(|z| TimeZoneObject::construct(ctx, z.as_bytes()).unwrap());
    Value::Object(
        DateObject::construct(ctx, classes::datetime(), s.as_bytes(), tz.as_ref()).unwrap(),
    )
}

#[test]
fn date_filter_fixture() {
    let c = Ctx::new();
    let ctx = &c.ctx;
    ctx.default_timezone_set(b"Europe/Paris", &mut Vec::new())
        .unwrap();
    let date1 = Value::Int(
        functions::mktime(ctx, 13, Some(45), Some(0), Some(10), Some(4), Some(2010)).unwrap(),
    );
    let date2 = new_date(ctx, "2010-10-04 13:45", None);
    let date3 = Value::from("2010-10-04 13:45");
    let date4 = Value::Int(1286199900);
    let date5 = Value::Int(-189291360);
    let date6 = new_date(ctx, "2010-10-04 13:45", Some("America/New_York"));
    let date7 = Value::from("2010-01-28T15:00:00+04:00");
    let timezone1 = TimeZoneObject::construct(ctx, b"America/New_York").unwrap();

    let f = |d: &Value, fmt: Option<&str>, tz: Tz<'_>| format_date(ctx, d, fmt, tz);
    let got = [
        f(&date1, None, Tz::Default),
        f(&date1, Some("d/m/Y"), Tz::Default),
        f(&date1, Some("d/m/Y H:i:s"), Tz::Name("Asia/Hong_Kong")),
        f(&date1, Some("d/m/Y H:i:s P"), Tz::Name("Asia/Hong_Kong")),
        f(&date1, Some("d/m/Y H:i:s P"), Tz::Name("America/Chicago")),
        f(&date1, Some("e"), Tz::Default),
        f(&date1, Some("d/m/Y H:i:s"), Tz::Default),
        f(&date2, None, Tz::Default),
        f(&date2, Some("d/m/Y"), Tz::Default),
        f(&date2, Some("d/m/Y H:i:s"), Tz::Name("Asia/Hong_Kong")),
        f(&date2, Some("d/m/Y H:i:s"), Tz::Obj(timezone1.clone())),
        f(&date2, Some("d/m/Y H:i:s"), Tz::Default),
        f(&date3, None, Tz::Default),
        f(&date3, Some("d/m/Y"), Tz::Default),
        f(&date4, None, Tz::Default),
        f(&date4, Some("d/m/Y"), Tz::Default),
        f(&date5, None, Tz::Default),
        f(&date5, Some("d/m/Y"), Tz::Default),
        f(&date6, Some("d/m/Y H:i:s P"), Tz::Name("Europe/Paris")),
        f(&date6, Some("d/m/Y H:i:s P"), Tz::Name("Asia/Hong_Kong")),
        f(&date6, Some("d/m/Y H:i:s P"), Tz::Keep),
        f(&date6, Some("e"), Tz::Name("Europe/Paris")),
        f(&date6, Some("e"), Tz::Keep),
        f(&date7, None, Tz::Default),
        f(&date7, None, Tz::Name("Europe/Paris")),
        f(&date7, None, Tz::Name("Asia/Hong_Kong")),
        f(&date7, None, Tz::Keep),
        f(&date7, None, Tz::Name("Indian/Mauritius")),
        f(
            &Value::from("2010-01-28 15:00:00"),
            None,
            Tz::Name("Europe/Paris"),
        ),
        f(
            &Value::from("2010-01-28 15:00:00"),
            None,
            Tz::Name("Asia/Hong_Kong"),
        ),
    ];
    let want = [
        "October 4, 2010 13:45",
        "04/10/2010",
        "04/10/2010 19:45:00",
        "04/10/2010 19:45:00 +08:00",
        "04/10/2010 06:45:00 -05:00",
        "Europe/Paris",
        "04/10/2010 13:45:00",
        "October 4, 2010 13:45",
        "04/10/2010",
        "04/10/2010 19:45:00",
        "04/10/2010 07:45:00",
        "04/10/2010 13:45:00",
        "October 4, 2010 13:45",
        "04/10/2010",
        "October 4, 2010 15:45",
        "04/10/2010",
        "January 2, 1964 04:04",
        "02/01/1964",
        "04/10/2010 19:45:00 +02:00",
        "05/10/2010 01:45:00 +08:00",
        "04/10/2010 13:45:00 -04:00",
        "Europe/Paris",
        "America/New_York",
        "January 28, 2010 12:00",
        "January 28, 2010 12:00",
        "January 28, 2010 19:00",
        "January 28, 2010 15:00",
        "January 28, 2010 15:00",
        "January 28, 2010 15:00",
        "January 28, 2010 22:00",
    ];
    assert_eq!(got.to_vec(), want.map(str::to_owned).to_vec());
}

#[test]
fn immutable_modify_and_interval_fixtures() {
    let c = Ctx::new();
    let ctx = &c.ctx;
    ctx.default_timezone_set(b"Europe/Paris", &mut Vec::new())
        .unwrap();
    let date1 = Value::Object(
        DateObject::construct(
            ctx,
            classes::datetime_immutable(),
            b"2010-10-04 13:45",
            None,
        )
        .unwrap(),
    );
    let ny = TimeZoneObject::construct(ctx, b"America/New_York").unwrap();
    let date2 = Value::Object(
        DateObject::construct(
            ctx,
            classes::datetime_immutable(),
            b"2010-10-04 13:45",
            Some(&ny),
        )
        .unwrap(),
    );
    let f = |d: &Value, fmt: Option<&str>, tz: Tz<'_>| format_date(ctx, d, fmt, tz);
    // modifyDate(): convertDate($date, false)->modify($modifier)
    let modified = {
        let d = convert_date(ctx, &date1, Tz::Keep).unwrap();
        call(&d, "modify", vec![Value::from("+1 hour")], &mut Vec::new()).unwrap()
    };
    let got = [
        f(&date1, None, Tz::Default),
        f(&date1, Some("d/m/Y"), Tz::Default),
        f(&date1, Some("d/m/Y H:i:s"), Tz::Name("Asia/Hong_Kong")),
        f(&date1, Some("d/m/Y H:i:s"), Tz::Obj(ny.clone())),
        f(&date1, Some("d/m/Y H:i:s"), Tz::Default),
        f(&modified, Some("d/m/Y H:i:s"), Tz::Default),
        f(&date2, Some("d/m/Y H:i:s P"), Tz::Name("Europe/Paris")),
        f(&date2, Some("d/m/Y H:i:s P"), Tz::Name("Asia/Hong_Kong")),
        f(&date2, Some("d/m/Y H:i:s P"), Tz::Keep),
        f(&date2, Some("e"), Tz::Name("Europe/Paris")),
        f(&date2, Some("e"), Tz::Keep),
    ];
    let want = [
        "October 4, 2010 13:45",
        "04/10/2010",
        "04/10/2010 19:45:00",
        "04/10/2010 07:45:00",
        "04/10/2010 13:45:00",
        "04/10/2010 14:45:00",
        "04/10/2010 19:45:00 +02:00",
        "05/10/2010 01:45:00 +08:00",
        "04/10/2010 13:45:00 -04:00",
        "Europe/Paris",
        "America/New_York",
    ];
    assert_eq!(got.to_vec(), want.map(str::to_owned).to_vec());

    // filters/date_modify.test (UTC)
    ctx.default_timezone_set(b"UTC", &mut Vec::new()).unwrap();
    for date in [
        Value::from("2010-10-04 13:45"),
        new_date(ctx, "2010-10-04 13:45", None),
    ] {
        let d = convert_date(ctx, &date, Tz::Keep).unwrap();
        let d = obj(call(&d, "modify", vec![Value::from("-1day")], &mut Vec::new()).unwrap());
        assert_eq!(
            f(&Value::Object(d), Some("Y-m-d H:i:s"), Tz::Default),
            "2010-10-03 13:45:00"
        );
    }

    // filters/date_interval.test
    let iv = Value::Object(IntervalObject::construct(ctx, b"P2D").unwrap());
    assert_eq!(f(&iv, None, Tz::Default), "2 days");
    assert_eq!(
        f(&iv, Some("%d days %h hours"), Tz::Default),
        "2 days 0 hours"
    );
    assert_eq!(
        f(&iv, Some("%d days %h hours"), Tz::Obj(ny)),
        "2 days 0 hours"
    );
}

#[test]
fn now_is_deterministic() {
    // functions/date.test: date().format('r') == date('now').format('r'), date() > date('-1day')
    let c = Ctx::new();
    c.now(1_286_199_900, 500_000);
    let ctx = &c.ctx;
    let a = convert_date(ctx, &Value::Null, Tz::Default).unwrap();
    let b = convert_date(ctx, &Value::from("now"), Tz::Default).unwrap();
    let ra = call(&a, "format", vec![Value::from("r")], &mut Vec::new()).unwrap();
    let rb = call(&b, "format", vec![Value::from("r")], &mut Vec::new()).unwrap();
    assert_eq!(ra, rb);
    assert_eq!(str_val(&ra), "Mon, 04 Oct 2010 13:45:00 +0000");
    let yesterday = convert_date(ctx, &Value::from("-1day"), Tz::Default).unwrap();
    assert!(
        twig_value::compare::greater(
            &Value::Object(a),
            &Value::Object(yesterday),
            &mut Vec::new()
        )
        .unwrap()
    );
}
