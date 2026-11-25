//! The `ext/date` exception hierarchy (PHP 8.3+) and the shared error messages of `php_date.c`.
//!
//! ```text
//! Error
//! └── DateError
//!     ├── DateObjectError
//!     └── DateRangeError
//! Exception
//! └── DateException
//!     ├── DateInvalidTimeZoneException
//!     ├── DateInvalidOperationException
//!     ├── DateMalformedStringException
//!     ├── DateMalformedIntervalStringException
//!     └── DateMalformedPeriodStringException
//! ```

use std::sync::LazyLock;

use twig_timelib::ErrorContainer;
use twig_value::object::ClassEntry;
use twig_value::{PhpError, PhpString, ThrowableClass};

use crate::context::c_str;

macro_rules! throwable {
    ($(#[$doc:meta])* $fn_name:ident, $static_name:ident, $name:literal, $parent:expr) => {
        static $static_name: LazyLock<ThrowableClass> =
            LazyLock::new(|| ThrowableClass::other($name, $parent));
        $(#[$doc])*
        pub fn $fn_name() -> ThrowableClass {
            $static_name.clone()
        }
    };
}

throwable!(
    /// `class DateError extends Error`.
    date_error,
    DATE_ERROR,
    "DateError",
    ThrowableClass::Error
);
throwable!(
    /// `class DateObjectError extends DateError`.
    date_object_error,
    DATE_OBJECT_ERROR,
    "DateObjectError",
    date_error()
);
throwable!(
    /// `class DateRangeError extends DateError`.
    date_range_error,
    DATE_RANGE_ERROR,
    "DateRangeError",
    date_error()
);
throwable!(
    /// `class DateException extends Exception`.
    date_exception,
    DATE_EXCEPTION,
    "DateException",
    ThrowableClass::Exception
);
throwable!(
    /// `class DateInvalidTimeZoneException extends DateException`.
    date_invalid_timezone_exception,
    DATE_INVALID_TIMEZONE_EXCEPTION,
    "DateInvalidTimeZoneException",
    date_exception()
);
throwable!(
    /// `class DateInvalidOperationException extends DateException`.
    date_invalid_operation_exception,
    DATE_INVALID_OPERATION_EXCEPTION,
    "DateInvalidOperationException",
    date_exception()
);
throwable!(
    /// `class DateMalformedStringException extends DateException`.
    date_malformed_string_exception,
    DATE_MALFORMED_STRING_EXCEPTION,
    "DateMalformedStringException",
    date_exception()
);
throwable!(
    /// `class DateMalformedIntervalStringException extends DateException`.
    date_malformed_interval_string_exception,
    DATE_MALFORMED_INTERVAL_STRING_EXCEPTION,
    "DateMalformedIntervalStringException",
    date_exception()
);
throwable!(
    /// `class DateMalformedPeriodStringException extends DateException`.
    date_malformed_period_string_exception,
    DATE_MALFORMED_PERIOD_STRING_EXCEPTION,
    "DateMalformedPeriodStringException",
    date_exception()
);

/// The nine `ext/date` throwable classes, in registration order (`date_register_classes()`).
pub fn all() -> [ThrowableClass; 9] {
    [
        date_error(),
        date_object_error(),
        date_range_error(),
        date_exception(),
        date_invalid_timezone_exception(),
        date_invalid_operation_exception(),
        date_malformed_string_exception(),
        date_malformed_interval_string_exception(),
        date_malformed_period_string_exception(),
    ]
}

/// The first error of a timelib error container as `php_date.c` prints it:
/// `%s at position %d (%c): %s` where `%s` is `input` as a C string and a NUL character prints as a space.
///
/// `lead` is the text before the input (`"Failed to parse time string ("` or `"Unknown or bad format ("`),
/// `tail` the text after the message (`""` or `" while unserializing"`... callers append their own).
pub(crate) fn first_error_message(lead: &[u8], input: &[u8], errors: &ErrorContainer) -> Vec<u8> {
    let m = &errors.error_messages[0];
    let mut out = lead.to_vec();
    out.extend_from_slice(c_str(input));
    out.extend_from_slice(format!(") at position {} (", m.position).as_bytes());
    out.push(if m.character != 0 { m.character } else { b' ' });
    out.extend_from_slice(b"): ");
    out.extend_from_slice(m.message.as_bytes());
    out
}

/// `Failed to parse time string (%s) at position %d (%c): %s` (`php_date_initialize()`, `php_date_modify()`).
pub(crate) fn parse_failure(input: &[u8], errors: &ErrorContainer) -> Vec<u8> {
    first_error_message(b"Failed to parse time string (", input, errors)
}

/// The names of the built-in date classes (for [`uninitialized`]).
const INTERNAL_CLASSES: [&str; 6] = [
    "DateTimeInterface",
    "DateTime",
    "DateTimeImmutable",
    "DateTimeZone",
    "DateInterval",
    "DatePeriod",
];

/// Port of `date_throw_uninitialized_error()` (PHPSRC/ext/date/php_date.c): `DateObjectError` for an object whose
/// constructor did not run (or failed). Classes other than the built-in ones count as user classes and name
/// their nearest built-in ancestor.
pub fn uninitialized(class: &ClassEntry) -> PhpError {
    let is_internal = |name: &str| {
        INTERNAL_CLASSES
            .iter()
            .any(|c| c.eq_ignore_ascii_case(name))
    };
    if is_internal(class.name()) {
        return PhpError::new(
            date_object_error(),
            format!(
                "Object of type {} has not been correctly initialized by calling parent::__construct() in its constructor",
                class.display_name()
            ),
        );
    }
    let mut ce = class.parent().cloned();
    while let Some(c) = ce.clone() {
        if is_internal(c.name()) {
            break;
        }
        ce = c.parent().cloned();
    }
    match ce {
        Some(internal) => PhpError::new(
            date_object_error(),
            format!(
                "Object of type {} (inheriting {}) has not been correctly initialized by calling parent::__construct() in its constructor",
                class.display_name(),
                internal.display_name()
            ),
        ),
        None => PhpError::new(
            date_object_error(),
            format!(
                "Object of type {} not been correctly initialized by calling parent::__construct() in its constructor",
                class.display_name()
            ),
        ),
    }
}

/// `Invalid serialization data for %s object` (`Error`, from `__set_state()`, `__unserialize()`, `__wakeup()`).
pub(crate) fn invalid_serialization(class: &str) -> PhpError {
    PhpError::error(format!("Invalid serialization data for {class} object"))
}

/// A `PhpError` of `class` with a byte message.
pub(crate) fn throw(class: ThrowableClass, message: impl Into<PhpString>) -> PhpError {
    PhpError::new(class, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy() {
        assert!(date_malformed_string_exception().is_a(&date_exception()));
        assert!(date_malformed_string_exception().is_a(&ThrowableClass::Exception));
        assert!(!date_malformed_string_exception().is_a(&ThrowableClass::Error));
        assert!(date_range_error().is_a(&date_error()));
        assert!(date_object_error().is_a(&ThrowableClass::Error));
        assert_eq!(all().len(), 9);
        assert_eq!(
            date_invalid_timezone_exception().name(),
            "DateInvalidTimeZoneException"
        );
    }
}
