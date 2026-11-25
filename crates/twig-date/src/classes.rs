//! Class entries of `ext/date` (`date_register_classes()` and the stubs in PHPSRC/ext/date/php_date.stub.php),
//! with methods in arginfo order (`get_class_methods()` order) and the class constants.

use std::sync::LazyLock;

use twig_value::object::builtin::{ClassTable, iterator_aggregate};
use twig_value::object::{ClassBuilder, ClassRef, Visibility};
use twig_value::{PhpResult, Value};

/// `DATE_FORMAT_RFC3339` (`DATE_ATOM`, `DATE_RFC3339`, `DATE_W3C`).
pub const DATE_FORMAT_RFC3339: &str = "Y-m-d\\TH:i:sP";
/// `DATE_FORMAT_COOKIE`.
pub const DATE_FORMAT_COOKIE: &str = "l, d-M-Y H:i:s T";
/// `DATE_FORMAT_ISO8601`.
pub const DATE_FORMAT_ISO8601: &str = "Y-m-d\\TH:i:sO";
/// `DATE_FORMAT_ISO8601_EXPANDED`.
pub const DATE_FORMAT_ISO8601_EXPANDED: &str = "X-m-d\\TH:i:sP";
/// `DATE_FORMAT_ISO8601_LARGE_YEAR` (internal: `DateTimeZone::getTransitions()`).
pub const DATE_FORMAT_ISO8601_LARGE_YEAR: &str = "x-m-d\\TH:i:sP";
/// `DATE_FORMAT_RFC822` (`DATE_RFC822`).
pub const DATE_FORMAT_RFC822: &str = "D, d M y H:i:s O";
/// `DATE_FORMAT_RFC850` (`DATE_RFC850`).
pub const DATE_FORMAT_RFC850: &str = "l, d-M-y H:i:s T";
/// `DATE_FORMAT_RFC1036` (`DATE_RFC1036`).
pub const DATE_FORMAT_RFC1036: &str = "D, d M y H:i:s O";
/// `DATE_FORMAT_RFC1123` (`DATE_RFC1123`, `DATE_RSS`).
pub const DATE_FORMAT_RFC1123: &str = "D, d M Y H:i:s O";
/// `DATE_FORMAT_RFC7231` (`DATE_RFC7231`, deprecated since PHP 8.5).
pub const DATE_FORMAT_RFC7231: &str = "D, d M Y H:i:s \\G\\M\\T";
/// `DATE_FORMAT_RFC2822` (`DATE_RFC2822`).
pub const DATE_FORMAT_RFC2822: &str = "D, d M Y H:i:s O";
/// `DATE_FORMAT_RFC3339_EXTENDED` (`DATE_RFC3339_EXTENDED`).
pub const DATE_FORMAT_RFC3339_EXTENDED: &str = "Y-m-d\\TH:i:s.vP";

/// The deprecation suffix of `DATE_RFC7231` / `DateTimeInterface::RFC7231` (`#[\Deprecated]` in the stub).
pub const RFC7231_DEPRECATION: &str =
    "since 8.5, as this format ignores the associated timezone and always uses GMT";

/// A global constant registered by `ext/date`, with the `#[\Deprecated]` suffix PHP appends to
/// `Constant %s is deprecated` when it has one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalConstant {
    /// Constant name.
    pub name: &'static str,
    /// Value (`DATE_*` are strings, `SUNFUNCS_*` ints).
    pub value: ConstantValue,
    /// `Some(", since …, …")`-style suffix text (without the leading `Constant X is deprecated`).
    pub deprecated: Option<&'static str>,
}

/// The value of a [`GlobalConstant`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstantValue {
    /// A string.
    Str(&'static str),
    /// An int.
    Int(i64),
}

impl ConstantValue {
    /// As a PHP value.
    pub fn to_value(self) -> Value {
        match self {
            ConstantValue::Str(s) => Value::from(s),
            ConstantValue::Int(i) => Value::Int(i),
        }
    }
}

const SUNFUNCS_DEPRECATION: &str =
    "since 8.4, as date_sunrise() and date_sunset() were deprecated in 8.1";

/// The global constants of `ext/date` in registration order (`register_php_date_symbols()`).
pub const GLOBAL_CONSTANTS: [GlobalConstant; 17] = {
    const fn s(name: &'static str, v: &'static str) -> GlobalConstant {
        GlobalConstant {
            name,
            value: ConstantValue::Str(v),
            deprecated: None,
        }
    }
    [
        s("DATE_ATOM", DATE_FORMAT_RFC3339),
        s("DATE_COOKIE", DATE_FORMAT_COOKIE),
        s("DATE_ISO8601", DATE_FORMAT_ISO8601),
        s("DATE_ISO8601_EXPANDED", DATE_FORMAT_ISO8601_EXPANDED),
        s("DATE_RFC822", DATE_FORMAT_RFC822),
        s("DATE_RFC850", DATE_FORMAT_RFC850),
        s("DATE_RFC1036", DATE_FORMAT_RFC1036),
        s("DATE_RFC1123", DATE_FORMAT_RFC1123),
        GlobalConstant {
            name: "DATE_RFC7231",
            value: ConstantValue::Str(DATE_FORMAT_RFC7231),
            deprecated: Some(RFC7231_DEPRECATION),
        },
        s("DATE_RFC2822", DATE_FORMAT_RFC2822),
        s("DATE_RFC3339", DATE_FORMAT_RFC3339),
        s("DATE_RFC3339_EXTENDED", DATE_FORMAT_RFC3339_EXTENDED),
        s("DATE_RSS", DATE_FORMAT_RFC1123),
        s("DATE_W3C", DATE_FORMAT_RFC3339),
        GlobalConstant {
            name: "SUNFUNCS_RET_TIMESTAMP",
            value: ConstantValue::Int(0),
            deprecated: Some(SUNFUNCS_DEPRECATION),
        },
        GlobalConstant {
            name: "SUNFUNCS_RET_STRING",
            value: ConstantValue::Int(1),
            deprecated: Some(SUNFUNCS_DEPRECATION),
        },
        GlobalConstant {
            name: "SUNFUNCS_RET_DOUBLE",
            value: ConstantValue::Int(2),
            deprecated: Some(SUNFUNCS_DEPRECATION),
        },
    ]
};

/// `PHP_DATE_TIMEZONE_GROUP_AFRICA` (`DateTimeZone::AFRICA`).
pub const TIMEZONE_GROUP_AFRICA: i64 = 0x0001;
/// `PHP_DATE_TIMEZONE_GROUP_AMERICA`.
pub const TIMEZONE_GROUP_AMERICA: i64 = 0x0002;
/// `PHP_DATE_TIMEZONE_GROUP_ANTARCTICA`.
pub const TIMEZONE_GROUP_ANTARCTICA: i64 = 0x0004;
/// `PHP_DATE_TIMEZONE_GROUP_ARCTIC`.
pub const TIMEZONE_GROUP_ARCTIC: i64 = 0x0008;
/// `PHP_DATE_TIMEZONE_GROUP_ASIA`.
pub const TIMEZONE_GROUP_ASIA: i64 = 0x0010;
/// `PHP_DATE_TIMEZONE_GROUP_ATLANTIC`.
pub const TIMEZONE_GROUP_ATLANTIC: i64 = 0x0020;
/// `PHP_DATE_TIMEZONE_GROUP_AUSTRALIA`.
pub const TIMEZONE_GROUP_AUSTRALIA: i64 = 0x0040;
/// `PHP_DATE_TIMEZONE_GROUP_EUROPE`.
pub const TIMEZONE_GROUP_EUROPE: i64 = 0x0080;
/// `PHP_DATE_TIMEZONE_GROUP_INDIAN`.
pub const TIMEZONE_GROUP_INDIAN: i64 = 0x0100;
/// `PHP_DATE_TIMEZONE_GROUP_PACIFIC`.
pub const TIMEZONE_GROUP_PACIFIC: i64 = 0x0200;
/// `PHP_DATE_TIMEZONE_GROUP_UTC`.
pub const TIMEZONE_GROUP_UTC: i64 = 0x0400;
/// `PHP_DATE_TIMEZONE_GROUP_ALL`.
pub const TIMEZONE_GROUP_ALL: i64 = 0x07FF;
/// `PHP_DATE_TIMEZONE_GROUP_ALL_W_BC`.
pub const TIMEZONE_GROUP_ALL_W_BC: i64 = 0x0FFF;
/// `PHP_DATE_TIMEZONE_PER_COUNTRY`.
pub const TIMEZONE_PER_COUNTRY: i64 = 0x1000;

/// `PHP_DATE_PERIOD_EXCLUDE_START_DATE`.
pub const PERIOD_EXCLUDE_START_DATE: i64 = 0x0001;
/// `PHP_DATE_PERIOD_INCLUDE_END_DATE`.
pub const PERIOD_INCLUDE_END_DATE: i64 = 0x0002;

macro_rules! lazy_class {
    ($(#[$doc:meta])* $fn_name:ident, $static_name:ident, $init:expr) => {
        static $static_name: LazyLock<ClassRef> = LazyLock::new(|| $init);
        $(#[$doc])*
        pub fn $fn_name() -> ClassRef {
            $static_name.clone()
        }
    };
}

fn methods(mut b: ClassBuilder, list: &[(&str, bool)]) -> ClassBuilder {
    for (name, is_static) in list {
        b = b.method_with(name, Visibility::Public, *is_static);
    }
    b
}

lazy_class!(
    /// `interface DateTimeInterface` with the `ATOM` … `W3C` format constants.
    datetime_interface,
    DATETIME_INTERFACE,
    {
        let b = ClassBuilder::interface("DateTimeInterface")
            .constant("ATOM", DATE_FORMAT_RFC3339)
            .constant("COOKIE", DATE_FORMAT_COOKIE)
            .constant("ISO8601", DATE_FORMAT_ISO8601)
            .constant("ISO8601_EXPANDED", DATE_FORMAT_ISO8601_EXPANDED)
            .constant("RFC822", DATE_FORMAT_RFC822)
            .constant("RFC850", DATE_FORMAT_RFC850)
            .constant("RFC1036", DATE_FORMAT_RFC1036)
            .constant("RFC1123", DATE_FORMAT_RFC1123)
            .constant("RFC7231", DATE_FORMAT_RFC7231)
            .constant("RFC2822", DATE_FORMAT_RFC2822)
            .constant("RFC3339", DATE_FORMAT_RFC3339)
            .constant("RFC3339_EXTENDED", DATE_FORMAT_RFC3339_EXTENDED)
            .constant("RSS", DATE_FORMAT_RFC1123)
            .constant("W3C", DATE_FORMAT_RFC3339);
        methods(
            b,
            &[
                ("format", false),
                ("getTimezone", false),
                ("getOffset", false),
                ("getTimestamp", false),
                ("getMicrosecond", false),
                ("diff", false),
                ("__wakeup", false),
                ("__serialize", false),
                ("__unserialize", false),
            ],
        )
        .build()
    }
);

lazy_class!(
    /// `class DateTime implements DateTimeInterface`.
    datetime,
    DATETIME,
    methods(
        ClassBuilder::class("DateTime").implements(&datetime_interface()),
        &[
            ("__construct", false),
            ("__serialize", false),
            ("__unserialize", false),
            ("__wakeup", false),
            ("__set_state", true),
            ("createFromImmutable", true),
            ("createFromInterface", true),
            ("createFromFormat", true),
            ("createFromTimestamp", true),
            ("getLastErrors", true),
            ("format", false),
            ("modify", false),
            ("add", false),
            ("sub", false),
            ("getTimezone", false),
            ("setTimezone", false),
            ("getOffset", false),
            ("getMicrosecond", false),
            ("setTime", false),
            ("setDate", false),
            ("setISODate", false),
            ("setTimestamp", false),
            ("setMicrosecond", false),
            ("getTimestamp", false),
            ("diff", false),
        ],
    )
    .build()
);

lazy_class!(
    /// `class DateTimeImmutable implements DateTimeInterface`.
    datetime_immutable,
    DATETIME_IMMUTABLE,
    methods(
        ClassBuilder::class("DateTimeImmutable").implements(&datetime_interface()),
        &[
            ("__construct", false),
            ("__serialize", false),
            ("__unserialize", false),
            ("__wakeup", false),
            ("__set_state", true),
            ("createFromFormat", true),
            ("createFromTimestamp", true),
            ("getLastErrors", true),
            ("format", false),
            ("getTimezone", false),
            ("getOffset", false),
            ("getTimestamp", false),
            ("getMicrosecond", false),
            ("diff", false),
            ("modify", false),
            ("add", false),
            ("sub", false),
            ("setTimezone", false),
            ("setTime", false),
            ("setDate", false),
            ("setISODate", false),
            ("setTimestamp", false),
            ("setMicrosecond", false),
            ("createFromMutable", true),
            ("createFromInterface", true),
        ],
    )
    .build()
);

lazy_class!(
    /// `class DateTimeZone` with the timezone group constants.
    datetimezone,
    DATETIMEZONE,
    methods(
        ClassBuilder::class("DateTimeZone")
            .constant("AFRICA", TIMEZONE_GROUP_AFRICA)
            .constant("AMERICA", TIMEZONE_GROUP_AMERICA)
            .constant("ANTARCTICA", TIMEZONE_GROUP_ANTARCTICA)
            .constant("ARCTIC", TIMEZONE_GROUP_ARCTIC)
            .constant("ASIA", TIMEZONE_GROUP_ASIA)
            .constant("ATLANTIC", TIMEZONE_GROUP_ATLANTIC)
            .constant("AUSTRALIA", TIMEZONE_GROUP_AUSTRALIA)
            .constant("EUROPE", TIMEZONE_GROUP_EUROPE)
            .constant("INDIAN", TIMEZONE_GROUP_INDIAN)
            .constant("PACIFIC", TIMEZONE_GROUP_PACIFIC)
            .constant("UTC", TIMEZONE_GROUP_UTC)
            .constant("ALL", TIMEZONE_GROUP_ALL)
            .constant("ALL_WITH_BC", TIMEZONE_GROUP_ALL_W_BC)
            .constant("PER_COUNTRY", TIMEZONE_PER_COUNTRY),
        &[
            ("__construct", false),
            ("getName", false),
            ("getOffset", false),
            ("getTransitions", false),
            ("getLocation", false),
            ("listAbbreviations", true),
            ("listIdentifiers", true),
            ("__serialize", false),
            ("__unserialize", false),
            ("__wakeup", false),
            ("__set_state", true),
        ],
    )
    .build()
);

lazy_class!(
    /// `class DateInterval`.
    dateinterval,
    DATEINTERVAL,
    methods(
        ClassBuilder::class("DateInterval"),
        &[
            ("__construct", false),
            ("createFromDateString", true),
            ("format", false),
            ("__serialize", false),
            ("__unserialize", false),
            ("__wakeup", false),
            ("__set_state", true),
        ],
    )
    .build()
);

lazy_class!(
    /// `class DatePeriod implements IteratorAggregate` (its virtual properties are served by
    /// [`DatePeriodObject`](crate::DatePeriodObject)).
    dateperiod,
    DATEPERIOD,
    methods(
        ClassBuilder::class("DatePeriod")
            .implements(&iterator_aggregate())
            .constant("EXCLUDE_START_DATE", PERIOD_EXCLUDE_START_DATE)
            .constant("INCLUDE_END_DATE", PERIOD_INCLUDE_END_DATE),
        &[
            ("createFromISO8601String", true),
            ("__construct", false),
            ("getStartDate", false),
            ("getEndDate", false),
            ("getDateInterval", false),
            ("getRecurrences", false),
            ("__serialize", false),
            ("__unserialize", false),
            ("__wakeup", false),
            ("__set_state", true),
            ("getIterator", false),
        ],
    )
    .build()
);

/// The date classes in registration order.
pub fn all() -> [ClassRef; 6] {
    [
        datetime_interface(),
        datetime(),
        datetime_immutable(),
        datetimezone(),
        dateinterval(),
        dateperiod(),
    ]
}

/// Registers the date classes in a class table (`class_exists('DateTime')`, `constant('DateTime::ATOM')`).
pub fn register(table: &mut ClassTable) -> PhpResult<()> {
    for c in all() {
        table.register(c)?;
    }
    Ok(())
}

/// The deprecation suffix of a class constant, when it has one: `DateTimeInterface::RFC7231` (and so
/// `DateTime::RFC7231`/`DateTimeImmutable::RFC7231`). PHP reports `Constant %s::%s is deprecated, %s`.
pub fn class_constant_deprecation(class: &str, name: &str) -> Option<&'static str> {
    let date_class = ["DateTimeInterface", "DateTime", "DateTimeImmutable"]
        .iter()
        .any(|c| c.eq_ignore_ascii_case(class));
    (date_class && name == "RFC7231").then_some(RFC7231_DEPRECATION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_and_constants() {
        let dt = datetime();
        assert!(dt.is_subclass_of("DateTimeInterface"));
        assert!(!dt.is_subclass_of("DateTimeImmutable"));
        assert_eq!(dt.constant("ATOM"), Some(Value::from(DATE_FORMAT_RFC3339)));
        assert_eq!(datetimezone().constant("ALL"), Some(Value::Int(2047)));
        assert!(dateperiod().is_subclass_of("Traversable"));
        let names: Vec<_> = datetime_immutable().public_method_names();
        assert_eq!(names.len(), 25);
        assert_eq!(&*names[0], "__construct");
        let mut t = ClassTable::new();
        register(&mut t).unwrap();
        assert!(t.class_exists("datetime"));
        assert!(t.interface_exists("DateTimeInterface"));
    }
}
