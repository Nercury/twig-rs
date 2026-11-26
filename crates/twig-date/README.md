# twig-date

PHP 8.5.13's `ext/date` layer on [`twig-timelib`](https://crates.io/crates/twig-timelib): `DateTime`,
`DateTimeImmutable`, `DateTimeZone`, `DateInterval` and `DatePeriod` as objects of the
[`twig-value`](https://crates.io/crates/twig-value) value model (methods with PHP's argument parsing and verbatim
errors, comparisons, `var_dump()`, casts, `clone`), and the date functions (`date()`, `mktime()`, `strtotime()`,
`date_parse()`, `date_create()`, the `timezone_*()` functions, ...). PHP's request globals (the default timezone, the
last parse errors) and its clock live in a `DateContext` owned by the caller, so a `FixedClock` makes every "now"
deterministic. Results are checked against PHP with oracle-generated test vectors.

It is part of [twig-rs](https://github.com/Nercury/twig-rs), a Rust implementation of Twig that renders templates
exactly like Twig 3.31.1 on PHP 8.5.13: the [`twig`](https://crates.io/crates/twig) crate's `date` and `date_modify`
filters and `date` function use it, and each environment holds a `DateContext` (`Environment::date_context()`).

```rust
use twig_date::{DateContext, DateObject, FixedClock, TimeZoneObject, classes};
use twig_value::object::CallArgs;
use twig_value::{Diagnostic, PhpError, Value};

fn main() -> Result<(), PhpError> {
    // "now" is 2023-11-14 22:13:20.25 UTC
    let context = DateContext::with_clock(FixedClock::new(1_700_000_000, 250_000));
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    context.default_timezone_set(b"Europe/Vilnius", &mut diagnostics)?;

    // $date = new DateTime('+1 day')
    let object = DateObject::construct(&context, classes::datetime(), b"+1 day", None)?;
    let date = object.downcast_ref::<DateObject>().unwrap();
    assert_eq!(date.format(b"Y-m-d H:i:s.u T")?, "2023-11-16 00:13:20.250000 EET");

    // $date->setTimezone(new DateTimeZone('Asia/Tokyo'))->format('c')
    let tokyo = TimeZoneObject::construct(&context, b"Asia/Tokyo")?;
    date.set_timezone_value(tokyo.downcast_ref::<TimeZoneObject>().unwrap().value().as_ref())?;
    assert_eq!(date.format(b"c")?, "2023-11-16T07:13:20+09:00");

    // $date->setTime('noon', 0): methods are called by name, with PHP's argument checks
    let arguments = CallArgs::from(vec![Value::from("noon"), Value::Int(0)]);
    let error = object.call_method("setTime", arguments, &mut diagnostics).unwrap_err();
    assert_eq!(error.message(), "DateTime::setTime(): Argument #1 ($hour) must be of type int, string given");
    Ok(())
}
```

The [API documentation](https://docs.rs/twig-date) maps the crate's items to PHP's classes and functions.

## Licence

`BSD-3-Clause AND PHP-3.01 AND Zend-2.0`: twig-rs is distributed under the BSD 3-Clause licence; the crate ports
php-src's `ext/date` code (PHP License 3.01) and `InternalIterator` from the Zend Engine (Zend Engine License 2.00).
The `LICENSE` and `NOTICE` files that come with the crate hold the details and the licence texts.
