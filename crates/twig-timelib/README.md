# twig-timelib

A pure-Rust port of timelib 2022.17, the date and time library behind PHP 8.5.13's `ext/date`: the `strtotime()`
scanner and parser, `parse_from_format()` (`DateTime::createFromFormat()`), ISO 8601 durations and periods, the
normalisation and relative arithmetic of `+1 month` and `last day of next month`, intervals between dates, and time
zones with PHP's embedded timezonedb 2026.6 (598 entries) and its POSIX TZ rules. Every function mirrors a timelib
function, keeping its algorithm, evaluation order and messages; the scanners are generated from timelib's own re2c
sources by re2c's Rust backend. Results are checked against PHP and against the C library.

It is part of [twig-rs](https://github.com/Nercury/twig-rs), a Rust implementation of Twig that renders templates
exactly like Twig 3.31.1 on PHP 8.5.13: [`twig-date`](https://crates.io/crates/twig-date) builds PHP's `DateTime`
classes on it, for the `date` and `date_modify` filters and the `date` function of the
[`twig`](https://crates.io/crates/twig) crate.

`new DateTime('2021-01-31 +1 month')`, as `php_date_initialize()` computes it: parse, fill the missing fields from
"now", compute the timestamp and recompute the fields.

```rust
use twig_timelib::parse_date::strtotime;
use twig_timelib::types::{NO_CLONE, ZONETYPE_ID};
use twig_timelib::{Time, TzCache, TzDb};

fn main() {
    let cache = TzCache::builtin();
    let (mut time, errors) =
        strtotime(b"2021-01-31 +1 month", cache.tzdb(), &mut |name: &str, _: &TzDb| cache.get(name));
    assert_eq!(errors.error_count(), 0);

    let utc = cache.get("UTC").unwrap();
    let mut now = Time::new();
    now.zone_type = ZONETYPE_ID;
    now.tz_info = Some(utc.clone());
    now.unixtime2local(1_700_000_000);

    time.fill_holes(&now, NO_CLONE);
    time.update_ts(Some(&utc));
    time.update_from_sse();
    // January 31 plus one month is "February 31", which overflows into March
    assert_eq!((time.y, time.m, time.d), (2021, 3, 3));
}
```

The [API documentation](https://docs.rs/twig-timelib) maps each module to its timelib source file.

## Licence

MIT, like timelib itself (Derick Rethans, MongoDB, Inc.): the `LICENSE-timelib` file that comes with the crate holds
the licence text. The zone data comes from the IANA tz database (public domain).
