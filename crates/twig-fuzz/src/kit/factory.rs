//! The objects `twig-test-support`'s converter leaves to its caller, created on the case's
//! environment as the oracle's data code created them: `Twig\Markup` (the runtime's [`Markup`]),
//! `Twig\TemplateWrapper` (`$twig->load()`), `DateTime`, `DateTimeImmutable`, `DateTimeZone` and
//! `DateInterval` (`twig-date`, on the environment's date context). Resources are declined: their
//! ids are process state.
//!
//! [`apply_state`] applies the PHP state the oracle recorded after the data phase (the default
//! timezone and the `CoreExtension` settings the data code changed).

use twig::runtime::{Environment, Markup};
use twig::value::{Array, Diagnostic, PhpError, PhpResult, PhpString, Value};
use twig_date::{DateObject, IntervalObject, TimeZoneObject, classes};
use twig_test_support::{DateIntervalSpec, FactoryRequest, ObjectFactory, PhpState, RequestKind};

/// Creates the objects the converter asks for, on `env`.
pub struct Factory<'a> {
    env: &'a Environment,
}

impl<'a> Factory<'a> {
    /// The factory of `env`.
    pub fn new(env: &'a Environment) -> Self {
        Factory { env }
    }
}

impl ObjectFactory for Factory<'_> {
    fn create(&self, request: &FactoryRequest<'_>) -> Option<PhpResult<Value>> {
        let ctx = self.env.date_context();
        Some(match &request.kind {
            // `new \Twig\Markup($content, $charset)`
            RequestKind::Markup {
                content,
                charset,
                safe_strategies,
            } => Ok(Value::Object(Markup::create(
                content.clone(),
                Some(PhpString::from(*charset)),
                safe_strategies.clone(),
            ))),
            // `$twig->load($template)`
            RequestKind::TemplateWrapper { template } => self
                .env
                .load(template)
                .map(|wrapper| wrapper.to_value())
                .map_err(|error| PhpError::error(format!("{}: {error}", error.class_name()))),
            RequestKind::DateTime {
                immutable,
                datetime,
                timezone,
                timestamp,
            } => date_time(self.env, *immutable, datetime, timezone, *timestamp),
            // `new \DateTimeZone($name)`
            RequestKind::DateTimeZone { name } => {
                TimeZoneObject::construct(ctx, name.as_bytes()).map(Value::Object)
            }
            RequestKind::DateInterval { spec } => date_interval(self.env, spec),
            RequestKind::Resource { .. } | RequestKind::Other(_) => return None,
        })
    }
}

/// A `DateTime` or `DateTimeImmutable` with the recorded `format('Y-m-d\TH:i:s.uP')`, timezone
/// name and timestamp: `new $class($localTime, new \DateTimeZone($timezone))`, checked against the
/// record.
fn date_time(
    env: &Environment,
    immutable: bool,
    datetime: &str,
    timezone: &str,
    timestamp: i64,
) -> PhpResult<Value> {
    let ctx = env.date_context();
    // `Y-m-d\TH:i:s.u` followed by the `P` offset (`+02:00`)
    let local = datetime
        .len()
        .checked_sub(6)
        .and_then(|end| datetime.get(..end))
        .ok_or_else(|| PhpError::error(format!("unexpected datetime {datetime:?}")))?;
    let zone = TimeZoneObject::construct(ctx, timezone.as_bytes())?;
    let class = if immutable {
        classes::datetime_immutable()
    } else {
        classes::datetime()
    };
    let object = DateObject::construct(ctx, class, local.as_bytes(), Some(&zone))?;
    let date = object
        .downcast_ref::<DateObject>()
        .ok_or_else(|| PhpError::error("DateObject::construct() made another object"))?;
    let made = date.format(b"Y-m-d\\TH:i:s.uP")?;
    if made.as_bytes() != datetime.as_bytes() || date.timestamp()? != timestamp {
        return Err(PhpError::error(format!(
            "rebuilt the date as {made} ({}), the oracle recorded {datetime} ({timestamp})",
            date.timestamp()?
        )));
    }
    Ok(Value::Object(object))
}

/// A `DateInterval` with the recorded public fields: `DateInterval::__set_state()`.
fn date_interval(env: &Environment, spec: &DateIntervalSpec) -> PhpResult<Value> {
    let mut fields = Array::new();
    fields.insert("y", spec.y);
    fields.insert("m", spec.m);
    fields.insert("d", spec.d);
    fields.insert("h", spec.h);
    fields.insert("i", spec.i);
    fields.insert("s", spec.s);
    fields.insert("f", spec.f);
    fields.insert("invert", spec.invert);
    fields.insert("days", spec.days.map_or(Value::Bool(false), Value::Int));
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let object = IntervalObject::set_state(env.date_context(), &fields, &mut diagnostics)?;
    if let Some(diagnostic) = diagnostics.first() {
        return Err(PhpError::error(format!(
            "DateInterval::__set_state() raised {diagnostic}"
        )));
    }
    Ok(Value::Object(object))
}

/// Twig's defaults of `CoreExtension::$dateFormats` and `$numberFormat`.
const DEFAULT_DATE_FORMAT: (&str, &str) = ("F j, Y H:i", "%d days");

/// Applies the PHP state recorded after the data phase: `date_default_timezone_set()`, then the
/// `CoreExtension` timezone, date formats and number format where they differ from the defaults.
///
/// # Errors
///
/// A timezone twig-rs does not accept.
pub fn apply_state(env: &Environment, state: &PhpState) -> Result<(), String> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let accepted = env
        .date_context()
        .default_timezone_set(state.php_default_timezone.as_bytes(), &mut diagnostics)
        .map_err(|e| format!("date_default_timezone_set(): {e}"))?;
    if !accepted || !diagnostics.is_empty() {
        return Err(format!(
            "date_default_timezone_set({:?}) failed: {diagnostics:?}",
            state.php_default_timezone
        ));
    }
    let settings = env.core_settings();
    if state.core_timezone != state.php_default_timezone {
        settings
            .set_timezone(
                env.date_context(),
                &Value::from(state.core_timezone.as_str()),
                &mut diagnostics,
            )
            .map_err(|e| format!("CoreExtension::setTimezone('{}'): {e}", state.core_timezone))?;
    }
    let (date, interval) = &state.core_date_format;
    if (date.as_str(), interval.as_str()) != DEFAULT_DATE_FORMAT {
        settings.set_date_format(
            Some(Value::from(date.as_str())),
            Some(Value::from(interval.as_str())),
        );
    }
    let (decimals, point, thousands) = &state.core_number_format;
    if (*decimals, point.as_str(), thousands.as_str()) != (0, ".", ",") {
        settings.set_number_format(
            Value::Int(*decimals),
            Value::from(point.as_str()),
            Value::from(thousands.as_str()),
        );
    }
    if !diagnostics.is_empty() {
        return Err(format!(
            "applying the CoreExtension settings raised {diagnostics:?}"
        ));
    }
    Ok(())
}
