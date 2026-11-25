//! Rust port of timelib 2022.17, the date/time library behind PHP 8.5.13's `ext/date`.
//!
//! The goal is bit-for-bit parity with PHP: every function here mirrors a timelib function from
//! `PHPSRC/ext/date/lib` (named in its doc comment), keeping the C algorithm, evaluation order and messages. The
//! timezone database is PHP's embedded `timezonedb.h` (version 2026.6, 598 entries).
//!
//! Layout (one module per timelib source file):
//!
//! | module | timelib source | contents |
//! |---|---|---|
//! | [`types`] | `timelib.h` | [`Time`], [`RelTime`], [`TimeOffset`], error containers, constants |
//! | [`dow`] | `dow.c` | day of week/year, ISO weeks, validity checks |
//! | [`unixtime2tm`] | `unixtime2tm.c` | timestamp → broken-down time, zone assignment ([`Time::unixtime2local`], ...) |
//! | [`tm2unixtime`] | `tm2unixtime.c` | normalisation and relative arithmetic ([`Time::update_ts`], ...) |
//! | [`interval`] | `interval.c` | [`interval::diff`], [`interval::add`], [`interval::sub`], wall-clock variants |
//! | [`tz`] | `parse_tz.c` | the embedded database, [`tz::parse_tzfile`], [`tz::TzInfo`] lookups |
//! | [`posix`] | `parse_posix.c` | POSIX TZ footer strings and their yearly transitions |
//! | [`abbr`] | `timezonemap.h`, `fallbackmap.h` | zone abbreviation tables |
//! | [`parse_date`] | `parse_date.re` | `strtotime()` scanner and parser, `parse_from_format()`, `fill_holes()` |
//! | [`parse_iso_intervals`] | `parse_iso_intervals.re` | ISO 8601 durations/periods ([`parse_iso_intervals::strtointerval`]) |
//! | [`timelib`] | `timelib.c` | string and decimal-hour helpers |
//!
//! Typical use, as `new DateTime('2021-01-31 +1 month')` does it (`php_date_initialize()`): parse, fill the
//! missing fields from "now", compute the timestamp and recompute the fields.
//!
//! ```
//! use twig_timelib::parse_date::strtotime;
//! use twig_timelib::types::{NO_CLONE, ZONETYPE_ID};
//! use twig_timelib::{Time, TzCache, TzDb};
//!
//! let cache = TzCache::builtin();
//! let (mut t, errors) =
//!     strtotime(b"2021-01-31 +1 month", cache.tzdb(), &mut |name: &str, _: &TzDb| cache.get(name));
//! assert_eq!(errors.error_count(), 0);
//!
//! let utc = cache.get("UTC").unwrap();
//! let mut now = Time::new();
//! now.zone_type = ZONETYPE_ID;
//! now.tz_info = Some(utc.clone());
//! now.unixtime2local(1_700_000_000);
//!
//! t.fill_holes(&now, NO_CLONE);
//! t.update_ts(Some(&utc));
//! t.update_from_sse();
//! assert_eq!((t.y, t.m, t.d, t.h), (2021, 3, 3, 0));
//! ```
//!
//! Integer arithmetic on user-controlled fields uses wrapping operations: PHP compiles timelib with `-fwrapv`
//! (`PHPSRC/ext/date/config0.m4`), so signed overflow wraps there too, and the port stays panic-free for any
//! input. Where C computes in `int` (for example `z + dst * 3600`), the port does the same in `i32`.
//!
//! Licence: timelib is MIT licensed (Derick Rethans, MongoDB, Inc.); the zone data is from the IANA tz database
//! (public domain).

#![warn(missing_docs)]

pub mod abbr;
pub mod dow;
pub mod interval;
pub mod parse_date;
pub mod parse_iso_intervals;
pub mod posix;
pub mod timelib;
pub mod tm2unixtime;
pub mod types;
pub mod tz;
pub mod unixtime2tm;

pub use types::{
    AbbrInfo, ErrorContainer, ErrorMessage, RelTime, Special, Time, TimeOffset, UNSET,
};
pub use tz::{TzCache, TzDb, TzInfo, builtin_db};

/// `TIMELIB_VERSION`.
pub const VERSION: i32 = 202217;
/// `TIMELIB_EXTENDED_VERSION`.
pub const EXTENDED_VERSION: i32 = 20221701;
/// `TIMELIB_ASCII_VERSION`.
pub const ASCII_VERSION: &str = "2022.17";
