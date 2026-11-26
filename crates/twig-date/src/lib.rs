//! PHP 8.5.13's `ext/date` object and function layer (port of `PHPSRC/ext/date/php_date.c` and its stubs) on top
//! of [`twig_timelib`], with the date objects exposed as [`twig_value`] objects.
//!
//! # Map of the crate
//!
//! | item | PHP |
//! |---|---|
//! | [`DateContext`], [`Clock`], [`SystemClock`], [`FixedClock`] | the `DATEG()` module globals (default timezone, `date.timezone`, last parse errors, zone cache) and `gettimeofday()` |
//! | [`DateObject`] | `DateTime`, `DateTimeImmutable` (`php_date_obj`) |
//! | [`TimeZoneObject`], [`TzValue`] | `DateTimeZone` (`php_timezone_obj`) |
//! | [`IntervalObject`] | `DateInterval` (`php_interval_obj`) |
//! | [`DatePeriodObject`] | `DatePeriod` (`php_period_obj`) and its iterator |
//! | [`classes`] | the class entries (`DateTimeInterface` constants, `DateTimeZone::ALL`, …) and the `DATE_*` constants |
//! | [`errors`] | `DateError`, `DateObjectError`, `DateRangeError`, `DateException` and its subclasses |
//! | [`format`](mod@format) | `date_format()` (every `date()` letter) and `date_interval_format()` |
//! | [`functions`] | `date()`, `gmdate()`, `idate()`, `mktime()`, `gmmktime()`, `checkdate()`, `strtotime()`, `time()`, `getdate()`, `localtime()`, `date_parse()`, `date_create*()`, `date_modify()`, `date_sub()`, … |
//! | [`timezone`] | `timezone_open()`, `timezone_identifiers_list()`, `timezone_abbreviations_list()`, `timezone_name_from_abbr()`, transitions, location |
//!
//! # State and determinism
//!
//! PHP keeps the default timezone, the last parse errors and a zone cache in request globals and reads "now" from
//! the system clock. Here all of that lives in a [`DateContext`] owned by the caller (a Twig environment holds
//! one); clone it to share. Date objects keep the context that created them, which is how static methods called
//! through an instance (`$d->createFromFormat()`, `$d->getLastErrors()`) find it. A [`FixedClock`] makes every
//! "now" deterministic.
//!
//! ```
//! use twig_date::{DateContext, DateObject, FixedClock, TimeZoneObject, classes};
//!
//! let ctx = DateContext::with_clock(FixedClock::new(1_700_000_000, 250_000));
//! let mut diags = Vec::new();
//! ctx.default_timezone_set(b"Europe/Vilnius", &mut diags).unwrap();
//!
//! // new DateTime('+1 day')->format('Y-m-d H:i:s.u T')
//! let d = DateObject::construct(&ctx, classes::datetime(), b"+1 day", None).unwrap();
//! let d = d.downcast_ref::<DateObject>().unwrap();
//! assert_eq!(d.format(b"Y-m-d H:i:s.u T").unwrap(), "2023-11-16 00:13:20.250000 EET");
//!
//! // Twig: convertDate($date, 'Asia/Tokyo')->format('c')
//! let tz = TimeZoneObject::construct(&ctx, b"Asia/Tokyo").unwrap();
//! let tz = tz.downcast_ref::<TimeZoneObject>().unwrap().value();
//! d.set_timezone_value(tz.as_ref()).unwrap();
//! assert_eq!(d.format(b"c").unwrap(), "2023-11-16T07:13:20+09:00");
//! ```
//!
//! # Objects
//!
//! The objects implement [`twig_value::object::Object`]: methods are dispatched by name through
//! [`twig_value::ObjectRef::call_method`] with PHP's argument parsing and verbatim errors (`DateTime::setTime():
//! Argument #1 ($hour) must be of type int, string given`), each method binds its arguments to the parameters of
//! PHPSRC/ext/date/php_date.stub.php and reports them through
//! [`Object::method_params`](twig_value::object::Object::method_params) (so that a caller unpacking `...$args`
//! checks named arguments as PHP sends them), mutators of `DateTimeImmutable` return modified
//! clones, `clone` copies, and `==`/`<`/`<=>` between date objects compare instants. The synthetic properties
//! follow `get_properties_for`: `var_dump()`, `(array)`, `var_export()` and `json_encode()` see `date`,
//! `timezone_type` and `timezone` (`timezone_type`/`timezone` for `DateTimeZone`), while `get_object_vars()`,
//! `foreach` and `$d->date` do not; `DateInterval` exposes `y m d h i s f invert days` through its property
//! handlers.
//!
//! Typed Rust entry points exist for what Twig's `CoreExtension` needs (`convertDate()`, `formatDate()`,
//! `modifyDate()`): [`DateObject::construct`], [`DateObject::format`], [`DateObject::modify`],
//! [`DateObject::set_timezone_value`], [`TimeZoneObject::construct`], [`IntervalObject::format`] and
//! [`DateContext::default_timezone_get`].
//!
//! # Errors and diagnostics
//!
//! Exceptions are `Err(PhpError)` with the PHP class ([`errors`]) and the verbatim message; warnings, notices and
//! deprecations go to the `&mut dyn Diagnostics` sink of the call. Methods that PHP runs under
//! `zend_replace_error_handling(EH_THROW, …)` (`modify()`, `sub()`) throw the converted warning with its
//! `DateTime::modify(): ` prefix, while the procedural `date_modify()` / `date_sub()` warn.

#![warn(missing_docs)]

mod args;
pub mod classes;
pub mod context;
pub mod datetime;
pub mod errors;
pub mod format;
pub mod functions;
pub mod interval;
pub mod period;
pub mod timezone;
mod util;

pub use context::{Clock, DateContext, FixedClock, SystemClock};
pub use datetime::{DateObject, WarningMode};
pub use interval::IntervalObject;
pub use period::DatePeriodObject;
pub use timezone::{TimeZoneObject, TzValue};

/// The example of `README.md`, compiled and run as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;
