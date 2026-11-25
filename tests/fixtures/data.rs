//! The `--DATA--` context of a case and the PHP state it renders with. `twig-test-support` rebuilds the context from
//! the oracle's typed trees (`tests/twig/oracle-corpus.json` and its supplement); the objects that crate leaves to
//! its caller are created here, as the DATA code created them: `Twig\Markup` (the runtime's [`Markup`]),
//! `Twig\TemplateWrapper` (`$twig->load()` on the case's environment, after the load phase), `DateTime`,
//! `DateTimeImmutable`, `DateTimeZone` and `DateInterval` (`twig-date`, on the environment's `DateContext`) and a
//! stand-in for resources, which `twig-value` does not model (`reference/deviations.md`, fixture data).
//!
//! The PHP state recorded right before rendering ([`PhpState`]) is applied to the environment: the PHP default
//! timezone (`date_default_timezone_set()` in DATA) and the `CoreExtension` settings DATA changed
//! (`setTimezone()`, `setDateFormat()`, `setNumberFormat()`).

use std::sync::LazyLock;

use twig::runtime::{Environment, Markup};
use twig::value::object::{CallArgs, ClassBuilder, ClassRef, Object, ObjectRef};
use twig::value::{Array, Diagnostic, Diagnostics, PhpError, PhpResult, PhpString, Value};
use twig_date::{DateObject, IntervalObject, TimeZoneObject, classes};
use twig_test_support::{
    Case, DateIntervalSpec, FactoryRequest, ObjectFactory, PhpState, RequestKind,
};

/// Creates the objects `twig-test-support` asks for, on the environment of the case.
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
                .map_err(|error| {
                    PhpError::error(format!("{}: {error}", error.kind().class_name()))
                }),
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
            // `opendir()`: a directory stream
            RequestKind::Resource { resource_type } if *resource_type == "stream" => {
                Ok(Value::Object(ObjectRef::new(ResourceStandIn)))
            }
            RequestKind::Resource { .. } | RequestKind::Other(_) => return None,
        })
    }
}

/// A `DateTime` or `DateTimeImmutable` with the recorded `format('Y-m-d\TH:i:s.uP')`, timezone name and timestamp:
/// `new $class($localTime, new \DateTimeZone($timezone))`, checked against the record.
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
            "rebuilt {} as {made} ({}), recorded {datetime} ({timestamp})",
            if immutable {
                "DateTimeImmutable"
            } else {
                "DateTime"
            },
            date.timestamp()?
        )));
    }
    Ok(Value::Object(object))
}

/// A `DateInterval` with the recorded public fields: `DateInterval::__set_state()` of `y m d h i s f invert days`.
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

/// The class of [`ResourceStandIn`].
static RESOURCE_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRs\\Fixtures\\ResourceStandIn")
        .method("__toString")
        .build()
});

/// A stand-in for a PHP resource (`opendir()` in `tests/in.test`): truthy, `Resource id #1` as a string, neither a
/// string nor iterable for `in`, and compared strictly by `in_array()` like a resource.
struct ResourceStandIn;

impl Object for ResourceStandIn {
    fn class(&self) -> &ClassRef {
        &RESOURCE_CLASS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__tostring" => Ok(Value::from("Resource id #1")),
            _ => Err(PhpError::error(format!(
                "Call to undefined method TwigRs\\Fixtures\\ResourceStandIn::{method}()"
            ))),
        }
    }
}

/// The DATA context of a render-phase case, or why it cannot be rebuilt.
pub fn context(case: &Case<'_>, env: &Environment) -> Result<Array, String> {
    match case.context(&Factory::new(env)) {
        None => Err(
            "the oracle has no DATA context for this case (it ended in the load phase)".to_owned(),
        ),
        Some(Err(error)) => Err(format!("the DATA context cannot be rebuilt: {error}")),
        Some(Ok(conversion)) => match conversion.value {
            Value::Array(context) => Ok(context),
            other => Err(format!(
                "the DATA context is not an array ({})",
                twig::value::types::zend_zval_type_name(&other)
            )),
        },
    }
}

/// Twig's defaults of `CoreExtension::$dateFormats` and `$numberFormat`.
const DEFAULT_DATE_FORMAT: (&str, &str) = ("F j, Y H:i", "%d days");

/// Applies the PHP state of a case: `date_default_timezone_set()`, then the `CoreExtension` timezone, date formats
/// and number format where DATA changed them (`$twig->getExtension(CoreExtension::class)->setTimezone()`,
/// `setDateFormat()`, `setNumberFormat()`). Returns the parts of the state that could not be applied (none so far).
///
/// # Errors
///
/// The default timezone or the `CoreExtension` timezone is not accepted.
pub fn apply_state(env: &Environment, state: &PhpState) -> Result<Vec<String>, String> {
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
    // `CoreExtension::getTimezone()` defaults to the PHP default timezone of its first call
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
    Ok(Vec::new())
}
