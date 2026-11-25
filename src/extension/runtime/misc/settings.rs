//! The configuration of `Twig\Extension\CoreExtension` that the date and number filters read: the default date and
//! interval formats, the default timezone and the defaults of `number_format`.

use std::fmt;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use twig_date::{DateContext, TimeZoneObject, classes};
use twig_value::object::{CallArgs, ObjectRef};
use twig_value::{Diagnostics, PhpResult, Value};

use crate::runtime::Environment;

struct Inner {
    /// Twig's `$dateFormats`: `['F j, Y H:i', '%d days']`.
    date_formats: RwLock<[Value; 2]>,
    /// Twig's `$numberFormat`: `[0, '.', ',']`.
    number_format: RwLock<[Value; 3]>,
    /// Twig's `$timezone`: a `DateTimeZone`, set by `setTimezone()` or on first use.
    timezone: RwLock<Option<ObjectRef>>,
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

/// The date and number defaults of Twig's `CoreExtension` (TWIG/src/Extension/CoreExtension.php): what
/// `setDateFormat()`, `setTimezone()` and `setNumberFormat()` configure and the `date`, `date_modify` and
/// `number_format` filters and the `date` function read.
///
/// The values are PHP values, untyped like Twig's properties (`setNumberFormat(2, '!', '=')` stores an int and two
/// strings; the filters coerce them when they use them). Clones share the settings.
#[derive(Clone)]
pub struct CoreSettings {
    inner: Arc<Inner>,
}

impl fmt::Debug for CoreSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoreSettings")
            .field("date_formats", &*read(&self.inner.date_formats))
            .field("number_format", &*read(&self.inner.number_format))
            .field("timezone_set", &read(&self.inner.timezone).is_some())
            .finish()
    }
}

impl Default for CoreSettings {
    fn default() -> Self {
        CoreSettings::new()
    }
}

impl CoreSettings {
    /// Twig's defaults: date format `F j, Y H:i`, interval format `%d days`, no timezone yet (PHP's default timezone
    /// on first use), number format `0`, `.`, `,`.
    pub fn new() -> Self {
        CoreSettings {
            inner: Arc::new(Inner {
                date_formats: RwLock::new([Value::from("F j, Y H:i"), Value::from("%d days")]),
                number_format: RwLock::new([Value::Int(0), Value::from("."), Value::from(",")]),
                timezone: RwLock::new(None),
            }),
        }
    }

    /// Sets the default formats of the `date` filter; `None` keeps the current one.
    ///
    /// Twig: `CoreExtension::setDateFormat($format = null, $dateIntervalFormat = null)`.
    pub fn set_date_format(&self, format: Option<Value>, interval_format: Option<Value>) {
        let mut formats = write(&self.inner.date_formats);
        // `null !== $format`
        if let Some(format) = format.filter(|f| !f.is_null()) {
            formats[0] = format;
        }
        if let Some(format) = interval_format.filter(|f| !f.is_null()) {
            formats[1] = format;
        }
    }

    /// The default date format and the default interval format. Twig: `CoreExtension::getDateFormat()`.
    pub fn date_format(&self) -> [Value; 2] {
        read(&self.inner.date_formats).clone()
    }

    /// Sets the default timezone of the date filters and function: a `DateTimeZone` object is kept, anything else
    /// goes to `new \DateTimeZone($timezone)` in `ctx`.
    ///
    /// Twig: `CoreExtension::setTimezone($timezone)`.
    ///
    /// # Errors
    ///
    /// The exceptions of `DateTimeZone::__construct()` (`DateTimeZone::__construct(): Unknown or bad timezone
    /// (Nowhere/City)`, the `TypeError` of a non-string).
    pub fn set_timezone(
        &self,
        ctx: &DateContext,
        timezone: &Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        let timezone = match timezone {
            Value::Object(object) if object.instance_of("DateTimeZone") => object.clone(),
            other => new_timezone(ctx, other, diag)?,
        };
        *write(&self.inner.timezone) = Some(timezone);
        Ok(())
    }

    /// The default timezone: the one set with [`set_timezone`](Self::set_timezone), else (remembered from then on)
    /// `new \DateTimeZone(date_default_timezone_get())` in `ctx`.
    ///
    /// Twig: `CoreExtension::getTimezone()`.
    ///
    /// # Errors
    ///
    /// The error of a corrupt timezone database (`date_default_timezone_get()`).
    pub fn timezone(&self, ctx: &DateContext) -> PhpResult<ObjectRef> {
        if let Some(timezone) = read(&self.inner.timezone).as_ref() {
            return Ok(timezone.clone());
        }
        let name = ctx.default_timezone_get()?;
        let timezone = TimeZoneObject::construct(ctx, name.as_bytes())?;
        Ok(write(&self.inner.timezone).get_or_insert(timezone).clone())
    }

    /// Sets the defaults of the `number_format` filter: decimals, decimal point, thousands separator.
    ///
    /// Twig: `CoreExtension::setNumberFormat($decimal, $decimalPoint, $thousandSep)`.
    pub fn set_number_format(&self, decimal: Value, decimal_point: Value, thousand_sep: Value) {
        *write(&self.inner.number_format) = [decimal, decimal_point, thousand_sep];
    }

    /// The defaults of the `number_format` filter. Twig: `CoreExtension::getNumberFormat()`.
    pub fn number_format(&self) -> [Value; 3] {
        read(&self.inner.number_format).clone()
    }
}

/// `new \DateTimeZone($timezone)`: the constructor parses its `string` parameter (coercion, `TypeError`) and throws
/// `DateTimeZone::__construct(): Unknown or bad timezone (%s)`.
pub(super) fn new_timezone(
    ctx: &DateContext,
    timezone: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<ObjectRef> {
    let object = ObjectRef::new(TimeZoneObject::new_uninitialized(
        ctx,
        classes::datetimezone(),
    ));
    object.invoke_method("__construct", CallArgs::from(vec![timezone.clone()]), diag)?;
    Ok(object)
}

/// The settings of the environment's `CoreExtension` (Twig: `$env->getExtension(CoreExtension::class)`).
pub(super) fn core_settings(env: &Environment) -> CoreSettings {
    env.core_settings()
}
