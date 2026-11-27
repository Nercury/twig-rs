//! The date callables of `CoreExtension`: the `date` filter (`formatDate()`), the `date_modify` filter
//! (`modifyDate()`) and the `date` function (`convertDate()`, which the two filters use).
//!
//! Dates are the `twig-date` objects (`DateTime`, `DateTimeImmutable`, `DateTimeZone`, `DateInterval`) of the
//! environment's [`DateContext`]: its default timezone is PHP's `date_default_timezone_get()`, distinct from the
//! timezone of the `CoreExtension` ([`CoreSettings::timezone`]), which defaults to it. Methods are called through the
//! object protocol, so the objects' own argument parsing, messages and immutability rules apply.

use std::sync::{Arc, LazyLock};

use twig_date::{DateContext, DateObject, TimeZoneObject, WarningMode, classes};
use twig_value::object::{CallArgs, ClassRef, FunctionKind, ObjectRef, Param};
use twig_value::{Diagnostics, Value, convert, ops};

use super::settings::{CoreSettings, core_settings, new_timezone};
use super::{bind, call_method_on, return_string};
use crate::extension::runtime::{Call, CallResult};

/// Twig: `CoreExtension::formatDate($date, $format = null, $timezone = null): string`, the `date` filter.
pub(super) fn format_date_runtime(call: &mut Call<'_>) -> CallResult {
    let args = bind(
        call,
        "Twig\\Extension\\CoreExtension::formatDate",
        &[
            Param::required("date"),
            Param::optional("format", Value::Null),
            Param::optional("timezone", Value::Null),
        ],
        FunctionKind::User,
    )?;
    let settings = core_settings(call.env());
    let ctx = call.env().date_context().clone();
    format_date(
        &settings,
        &ctx,
        &args[0],
        &args[1],
        &args[2],
        call.diagnostics(),
    )
}

/// Twig: `CoreExtension::modifyDate($date, $modifier)`, the `date_modify` filter.
pub(super) fn modify_date_runtime(call: &mut Call<'_>) -> CallResult {
    let args = bind(
        call,
        "Twig\\Extension\\CoreExtension::modifyDate",
        &[Param::required("date"), Param::required("modifier")],
        FunctionKind::User,
    )?;
    let settings = core_settings(call.env());
    let ctx = call.env().date_context().clone();
    modify_date(&settings, &ctx, &args[0], &args[1], call.diagnostics())
}

/// Twig: `CoreExtension::convertDate($date = null, $timezone = null)`, the `date` function.
pub(super) fn convert_date_runtime(call: &mut Call<'_>) -> CallResult {
    let args = bind(
        call,
        "Twig\\Extension\\CoreExtension::convertDate",
        &[
            Param::optional("date", Value::Null),
            Param::optional("timezone", Value::Null),
        ],
        FunctionKind::User,
    )?;
    let settings = core_settings(call.env());
    let ctx = call.env().date_context().clone();
    convert_date(&settings, &ctx, &args[0], &args[1], call.diagnostics())
}

/// Twig: `CoreExtension::formatDate()`: the default format of the settings for a `null` format (the interval
/// format for a `DateInterval`), `$date->format($format)` for an interval (the timezone is ignored), else
/// `convertDate($date, $timezone)->format($format)`.
///
/// # Errors
///
/// The exceptions of [`convert_date`] and of the `format()` methods.
pub(super) fn format_date(
    settings: &CoreSettings,
    ctx: &DateContext,
    date: &Value,
    format: &Value,
    timezone: &Value,
    diag: &mut dyn Diagnostics,
) -> CallResult {
    let interval = matches!(date, Value::Object(o) if o.instance_of("DateInterval"));
    let format = if format.is_null() {
        let [date_format, interval_format] = settings.date_format();
        if interval {
            interval_format
        } else {
            date_format
        }
    } else {
        format.clone()
    };
    let formatted = if interval {
        call_method_on(date, "format", vec![format], diag)?
    } else {
        let date = convert_date(settings, ctx, date, timezone, diag)?;
        format_value(&date, format, diag)?
    };
    return_string(
        "Twig\\Extension\\CoreExtension::formatDate",
        formatted,
        diag,
    )
}

/// Twig: `CoreExtension::modifyDate()`: `convertDate($date, false)->modify($modifier)` (the date keeps its timezone;
/// a `DateTime` is cloned first, a `DateTimeImmutable` returns a new object).
///
/// # Errors
///
/// The exceptions of [`convert_date`] and `DateMalformedStringException` (`DateTime::modify(): Failed to parse time
/// string (%s) at position %d (%c): %s`).
pub(super) fn modify_date(
    settings: &CoreSettings,
    ctx: &DateContext,
    date: &Value,
    modifier: &Value,
    diag: &mut dyn Diagnostics,
) -> CallResult {
    let date = convert_date(settings, ctx, date, &Value::Bool(false), diag)?;
    // `DateTime::modify(string $modifier)` with a string modifies the object itself and returns it
    if let (Value::Object(object), Value::String(text)) = (&date, modifier)
        && Arc::ptr_eq(object.class(), &DATETIME)
        && let Some(date_object) = object.downcast_ref::<DateObject>()
    {
        date_object.modify(
            text.as_bytes(),
            WarningMode::Throw(
                "DateTime::modify",
                twig_date::errors::date_malformed_string_exception,
            ),
            diag,
        )?;
        return Ok(date);
    }
    call_method_on(&date, "modify", vec![modifier.clone()], diag)
}

/// Twig: `CoreExtension::convertDate($date = null, $timezone = null)` (TWIG/src/Extension/CoreExtension.php).
///
/// The timezone: `false` leaves the date's zone alone, `null` is the settings' timezone, a `DateTimeZone` is used as
/// it is, anything else goes to `new \DateTimeZone()`. A `DateTimeImmutable` gets `setTimezone()` (a new object), a
/// `DateTime` is cloned first; `null` and `'now'` are the current time in the timezone; a string (after `(string)`)
/// of digits with an optional leading `-` is a Unix timestamp (`new \DateTime('@'.$date)`, zone `+00:00`); anything
/// else is parsed by `new \DateTime($date)` in PHP's default timezone, then converted to the timezone.
///
/// # Errors
///
/// `DateTimeZone::__construct(): Unknown or bad timezone (%s)`, `DateMalformedStringException` (`Failed to parse time
/// string (%s) at position %d (%c): %s`), the `TypeError` of a constructor argument (`DateTime::__construct():
/// Argument #1 ($datetime) must be of type string, array given`), `Object of class %s could not be converted to
/// string`.
pub(super) fn convert_date(
    settings: &CoreSettings,
    ctx: &DateContext,
    date: &Value,
    timezone: &Value,
    diag: &mut dyn Diagnostics,
) -> CallResult {
    // determine the timezone (`None`: `false`)
    let timezone = match timezone {
        Value::Bool(false) => None,
        Value::Null => Some(settings.timezone(ctx)?),
        Value::Object(object) if object.instance_of("DateTimeZone") => Some(object.clone()),
        other => Some(new_timezone(ctx, other, diag)?),
    };

    if let Value::Object(object) = date {
        // immutable dates
        if object.instance_of("DateTimeImmutable") {
            return match timezone {
                Some(timezone) => set_timezone(date, timezone, diag),
                None => Ok(date.clone()),
            };
        }
        if object.instance_of("DateTime") {
            let date = Value::Object(object.clone_object(diag)?);
            if let Some(timezone) = timezone {
                set_timezone(&date, timezone, diag)?;
            }
            return Ok(date);
        }
    }

    if date.is_null() || matches!(date, Value::String(s) if s.as_bytes() == b"now") {
        let timezone = match timezone {
            Some(timezone) => timezone,
            None => settings.timezone(ctx)?,
        };
        return Ok(Value::Object(DateObject::construct(
            ctx,
            classes::datetime(),
            b"now",
            Some(&timezone),
        )?));
    }

    // an int is the timestamp `new \DateTime('@'.$date)` gets, written without building its string
    if let Value::Int(seconds) = date {
        let mut buf = [0u8; 24];
        let at = at_timestamp(*seconds, &mut buf);
        let date = Value::Object(DateObject::construct(ctx, DATETIME.clone(), at, None)?);
        if let Some(timezone) = timezone {
            set_timezone(&date, timezone, diag)?;
        }
        return Ok(date);
    }
    let as_string = convert::to_string(date, diag)?;
    let bytes = as_string.as_bytes();
    let timestamp =
        ctype_digit(bytes) || (bytes.first() == Some(&b'-') && ctype_digit(&bytes[1..]));
    let date = if timestamp {
        let at = ops::concat(&Value::from("@"), date, diag)?;
        new_datetime(ctx, Value::String(at), diag)?
    } else {
        new_datetime(ctx, date.clone(), diag)?
    };
    if let Some(timezone) = timezone {
        set_timezone(&date, timezone, diag)?;
    }
    Ok(date)
}

/// The classes of the objects the date filters create: their methods are the ones of `twig-date`, called directly
/// when the arguments are what the methods' typed parameters receive as they are.
static DATETIME: LazyLock<ClassRef> = LazyLock::new(classes::datetime);
static DATETIME_IMMUTABLE: LazyLock<ClassRef> = LazyLock::new(classes::datetime_immutable);
static DATETIMEZONE: LazyLock<ClassRef> = LazyLock::new(classes::datetimezone);

/// `$date->format($format)`: `DateTimeInterface::format()` of a `DateTime` or `DateTimeImmutable` with a string
/// format is [`DateObject::format`].
fn format_value(date: &Value, format: Value, diag: &mut dyn Diagnostics) -> CallResult {
    if let (Value::Object(object), Value::String(text)) = (date, &format)
        && (Arc::ptr_eq(object.class(), &DATETIME)
            || Arc::ptr_eq(object.class(), &DATETIME_IMMUTABLE))
        && let Some(date_object) = object.downcast_ref::<DateObject>()
    {
        return Ok(Value::String(date_object.format(text.as_bytes())?));
    }
    call_method_on(date, "format", vec![format], diag)
}

/// `$date->setTimezone($timezone)`. `DateTime::setTimezone()` with a `DateTimeZone` sets the zone of the object
/// itself ([`DateObject::set_timezone_value`]) and returns it.
fn set_timezone(date: &Value, timezone: ObjectRef, diag: &mut dyn Diagnostics) -> CallResult {
    if let Value::Object(object) = date
        && Arc::ptr_eq(object.class(), &DATETIME)
        && Arc::ptr_eq(timezone.class(), &DATETIMEZONE)
        && let Some(date_object) = object.downcast_ref::<DateObject>()
        && let Some(zone) = timezone.downcast_ref::<TimeZoneObject>()
    {
        date_object.set_timezone_value(zone.value().as_ref())?;
        return Ok(date.clone());
    }
    call_method_on(date, "setTimezone", vec![Value::Object(timezone)], diag)
}

/// `new \DateTime($datetime)`: the constructor parses its `string` parameter (coercion, `TypeError`) in PHP's default
/// timezone; a string is received as it is ([`DateObject::construct`]).
fn new_datetime(ctx: &DateContext, datetime: Value, diag: &mut dyn Diagnostics) -> CallResult {
    if let Value::String(text) = &datetime {
        return Ok(Value::Object(DateObject::construct(
            ctx,
            classes::datetime(),
            text.as_bytes(),
            None,
        )?));
    }
    let object = ObjectRef::new(DateObject::new_uninitialized(ctx, classes::datetime()));
    object.invoke_method("__construct", CallArgs::from(vec![datetime]), diag)?;
    Ok(Value::Object(object))
}

/// `'@'.$seconds`, written at the end of `buf`.
fn at_timestamp(seconds: i64, buf: &mut [u8; 24]) -> &[u8] {
    let mut n = seconds.unsigned_abs();
    let mut at = buf.len();
    loop {
        at -= 1;
        buf[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if seconds < 0 {
        at -= 1;
        buf[at] = b'-';
    }
    at -= 1;
    buf[at] = b'@';
    &buf[at..]
}

/// PHP's `ctype_digit()` on a string (C locale): not empty, only `0`-`9`.
fn ctype_digit(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}
