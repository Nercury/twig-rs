//! The objects `twig-test-support`'s converter leaves to its caller, created on the case's
//! environment as the oracle's data code created them: `Twig\Markup` (the runtime's [`Markup`]),
//! `Twig\TemplateWrapper` (`$twig->load()`), `DateTime`, `DateTimeImmutable`, `DateTimeZone`,
//! `DateInterval` and `DatePeriod` (`twig-date`, on the environment's date context). Resources are
//! declined: their ids are process state.
//!
//! For the date classes the oracle also records `get_object_vars()` (`props`, research/php-oracle
//! lib.php): the properties PHP's handlers write into the object's table, plus any dynamic property
//! the data code added, in table order. The factory recreates the dynamic properties in their
//! places (`replay_properties()`) and rebuilds `createFromDateString()` intervals from their
//! `date_string`. Trees recorded before `props` existed rebuild as before. A `DatePeriod` is
//! rebuilt from its `props` the way `var_export()` round-trips it (`__set_state()`).
//!
//! An interval made by `new DateInterval()` adds and subtracts its time part in wall-clock time
//! (`civil_or_wall`, PHPSRC/ext/date/php_date.c), every other one in civil time, and PHP shows the
//! mode nowhere; the oracle probes it and marks such intervals `"wall": true` in the tree, which
//! [`Factory::with_tree`] collects ([`wall_intervals`]).
//!
//! [`apply_state`] applies the PHP state the oracle recorded after the data phase (the default
//! timezone and the `CoreExtension` settings the data code changed).

use std::collections::HashSet;

use twig::runtime::{Environment, Markup};
use twig::value::{Array, ArrayKey, Diagnostic, Number, PhpError, PhpResult, PhpString, Value};
use twig_date::{DateObject, DatePeriodObject, IntervalObject, TimeZoneObject, classes};
use twig_test_support::{
    DateIntervalSpec, FactoryRequest, Item, Key, ObjectFactory, ObjectNode, PhpState, RequestKind,
    TypedNode,
};
use twig_value::object::{CallArgs, ObjectRef, PropertyPurpose};

use super::json::Json;

/// `PHP_DATE_WALL`, the `civil_or_wall` mode of `new DateInterval()` (`__set_state()` reads it from
/// the key of the same name).
const PHP_DATE_WALL: i64 = 2;

/// Creates the objects the converter asks for, on `env`.
pub struct Factory<'a> {
    env: &'a Environment,
    /// The tree ids of the intervals the oracle marked `"wall": true`.
    wall: HashSet<i64>,
}

impl<'a> Factory<'a> {
    /// The factory of `env`.
    pub fn new(env: &'a Environment) -> Self {
        Factory {
            env,
            wall: HashSet::new(),
        }
    }

    /// The factory of `env` for the typed tree `tree` (the JSON the oracle wrote): what the
    /// converter's model of the tree leaves out ([`wall_intervals`]).
    pub fn with_tree(env: &'a Environment, tree: &Json) -> Self {
        Factory {
            env,
            wall: wall_intervals(tree),
        }
    }

    /// Whether the oracle marked the interval node `node` as one with wall-clock arithmetic.
    fn is_wall(&self, node: &TypedNode) -> bool {
        node.as_object().is_some_and(|o| self.wall.contains(&o.id))
    }
}

/// The ids of the `DateInterval` nodes of a typed tree with `"wall": true`: intervals the data code
/// made with `new DateInterval()` (see the module documentation).
pub fn wall_intervals(tree: &Json) -> HashSet<i64> {
    fn walk(json: &Json, out: &mut HashSet<i64>) {
        match json {
            Json::Object(fields) => {
                if json.get("t").and_then(Json::as_str) == Some("object")
                    && json.get("class").and_then(Json::as_str) == Some("DateInterval")
                    && json.get("wall").is_some_and(Json::truthy)
                    && let Some(id) = json.get("id").and_then(Json::as_i64)
                {
                    out.insert(id);
                }
                for (_, value) in fields {
                    walk(value, out);
                }
            }
            Json::Array(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }
    let mut out = HashSet::new();
    walk(tree, &mut out);
    out
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
            } => date_time(self.env, *immutable, datetime, timezone, *timestamp)
                .and_then(|object| with_properties(object, request.node)),
            // `new \DateTimeZone($name)`
            RequestKind::DateTimeZone { name } => TimeZoneObject::construct(ctx, name.as_bytes())
                .and_then(|object| with_properties(object, request.node)),
            RequestKind::DateInterval { spec } => date_interval(
                self.env,
                spec,
                recorded_properties(request.node),
                self.is_wall(request.node),
            )
            .and_then(|object| with_properties(object, request.node)),
            RequestKind::Other(o) if o.class == "DatePeriod" => self.date_period(o),
            RequestKind::Resource { .. } | RequestKind::Other(_) => return None,
        })
    }
}

/// The properties `get_object_vars()` gives a `DatePeriod`, in its order
/// (`date_period_object_to_hash()`).
const PERIOD_PROPERTIES: &[&str] = &[
    "start",
    "current",
    "end",
    "interval",
    "recurrences",
    "include_start_date",
    "include_end_date",
];

impl Factory<'_> {
    /// A `DatePeriod`: `DatePeriod::__set_state()` of its recorded `get_object_vars()` (the date
    /// objects and the interval rebuilt like the others), which is how `var_export()` round-trips
    /// a period (`php_date_period_initialize_from_hash()`). A period with dynamic properties is
    /// declined.
    fn date_period(&self, o: &ObjectNode) -> PhpResult<Value> {
        let props = o
            .props
            .as_deref()
            .ok_or_else(|| PhpError::error("a DatePeriod node without props"))?;
        let keys: Vec<&[u8]> = props
            .iter()
            .map(|item| match &item.key {
                Key::Str(s) => s.as_bytes(),
                Key::Int(_) => b"".as_slice(),
            })
            .collect();
        if keys
            .iter()
            .copied()
            .ne(PERIOD_PROPERTIES.iter().map(|p| p.as_bytes()))
        {
            return Err(PhpError::error(format!(
                "a DatePeriod with the properties {keys:?}"
            )));
        }
        let mut hash = Array::new();
        for (item, name) in props.iter().zip(PERIOD_PROPERTIES) {
            let value = match &item.value {
                TypedNode::Null => Value::Null,
                TypedNode::Bool(b) => Value::Bool(*b),
                TypedNode::Int(i) => Value::Int(*i),
                TypedNode::Object(node) => match (node.class.as_str(), &node.spec) {
                    ("DateTime" | "DateTimeImmutable", _) => {
                        let (Some(datetime), Some(timezone), Some(timestamp)) =
                            (&node.datetime, &node.timezone, node.timestamp)
                        else {
                            return Err(PhpError::error("a DatePeriod date without its fields"));
                        };
                        let date = date_time(
                            self.env,
                            node.class == "DateTimeImmutable",
                            datetime,
                            timezone,
                            timestamp,
                        )?;
                        with_properties(date, &item.value)?
                    }
                    ("DateInterval", Some(spec)) => {
                        let interval = date_interval(
                            self.env,
                            spec,
                            node.props.as_deref(),
                            self.is_wall(&item.value),
                        )?;
                        with_properties(interval, &item.value)?
                    }
                    (class, _) => {
                        return Err(PhpError::error(format!(
                            "a DatePeriod::${name} of class {class}"
                        )));
                    }
                },
                other => {
                    return Err(PhpError::error(format!(
                        "a DatePeriod::${name} of type {}",
                        other.tag()
                    )));
                }
            };
            hash.insert(*name, value);
        }
        DatePeriodObject::set_state(self.env.date_context(), &hash).map(Value::Object)
    }
}

/// A `DateTime` or `DateTimeImmutable` with the recorded `format('Y-m-d\TH:i:s.uP')`, timezone
/// name and timestamp: `new $class($localTime, new \DateTimeZone($timezone))`, checked against the
/// record. A local time that names another instant (the second of two equal local times when the
/// clocks go back) or that the parser does not take (years beyond four digits) is rebuilt from the
/// instant instead: `createFromTimestamp($timestamp)`, `setTimezone()`, `setMicrosecond()`.
fn date_time(
    env: &Environment,
    immutable: bool,
    datetime: &str,
    timezone: &str,
    timestamp: i64,
) -> PhpResult<ObjectRef> {
    let ctx = env.date_context();
    let zone = TimeZoneObject::construct(ctx, timezone.as_bytes())?;
    let class = if immutable {
        classes::datetime_immutable()
    } else {
        classes::datetime()
    };
    let matches = |object: &ObjectRef| -> PhpResult<bool> {
        let date = object
            .downcast_ref::<DateObject>()
            .ok_or_else(|| PhpError::error("not a DateObject"))?;
        Ok(
            date.format(b"Y-m-d\\TH:i:s.uP")?.as_bytes() == datetime.as_bytes()
                && date.timestamp()? == timestamp,
        )
    };
    // `Y-m-d\TH:i:s.u` followed by the `P` offset (`+02:00`)
    let local = datetime
        .len()
        .checked_sub(6)
        .and_then(|end| datetime.get(..end))
        .ok_or_else(|| PhpError::error(format!("unexpected datetime {datetime:?}")))?;
    if let Ok(object) = DateObject::construct(ctx, class.clone(), local.as_bytes(), Some(&zone))
        && matches(&object)?
    {
        return Ok(object);
    }
    // from the instant: the microseconds are the `.u` of the record
    let micros: i64 = local
        .rsplit_once('.')
        .and_then(|(_, u)| u.parse().ok())
        .ok_or_else(|| PhpError::error(format!("unexpected datetime {datetime:?}")))?;
    let mut ignored: Vec<Diagnostic> = Vec::new();
    let utc = DateObject::create_from_timestamp(ctx, class, Number::Int(timestamp))?;
    let zoned = match utc.call_method(
        "setTimezone",
        CallArgs::from(vec![Value::Object(zone)]),
        &mut ignored,
    )? {
        Value::Object(object) => object,
        other => {
            return Err(PhpError::error(format!("setTimezone() returned {other:?}")));
        }
    };
    zoned
        .downcast_ref::<DateObject>()
        .ok_or_else(|| PhpError::error("not a DateObject"))?
        .set_microsecond(micros)?;
    if !matches(&zoned)? {
        let date = zoned
            .downcast_ref::<DateObject>()
            .ok_or_else(|| PhpError::error("not a DateObject"))?;
        return Err(PhpError::error(format!(
            "rebuilt the date as {} ({}), the oracle recorded {datetime} ({timestamp})",
            date.format(b"Y-m-d\\TH:i:s.uP")?,
            date.timestamp()?
        )));
    }
    Ok(zoned)
}

/// A `DateInterval` with the recorded public fields: `DateInterval::__set_state()`, or
/// `DateInterval::createFromDateString($date_string)` for an interval PHP built that way (its
/// recorded properties say `from_string` and hold the `date_string`). `__set_state()` gives the
/// civil mode unless the hash says `civil_or_wall`: `wall` (the oracle's probe) asks for
/// `PHP_DATE_WALL`.
///
/// `__set_state()` reads the microseconds as `(int) ($f * 1000000)` (`zend_dval_to_lval()`, like
/// PHP's `php_date_interval_initialize_from_hash()`), which truncates `f` = 0.255505 to 255504
/// microseconds. PHP's own `f` is always `us / 1000000.0` (`date_interval_read_property()`,
/// PHPSRC/ext/date/php_date.c), so the interval had `round(f * 1000000)` microseconds; the factory
/// passes an `f` half a microsecond further from zero, which truncates to exactly that.
fn date_interval(
    env: &Environment,
    spec: &DateIntervalSpec,
    props: Option<&[Item]>,
    wall: bool,
) -> PhpResult<ObjectRef> {
    let from_string = props.and_then(|props| {
        let get = |name: &str| {
            props
                .iter()
                .find(|i| key_is(&i.key, name))
                .map(|i| &i.value)
        };
        match (get("from_string"), get("date_string")) {
            (Some(TypedNode::Bool(true)), Some(TypedNode::String(s))) => Some(s.clone()),
            _ => None,
        }
    });
    if let Some(date_string) = from_string {
        return IntervalObject::create_from_date_string(env.date_context(), date_string.as_bytes());
    }
    let micros = (spec.f * 1_000_000.0).round();
    let f = (micros + 0.5_f64.copysign(micros)) / 1_000_000.0;
    let mut fields = Array::new();
    fields.insert("y", spec.y);
    fields.insert("m", spec.m);
    fields.insert("d", spec.d);
    fields.insert("h", spec.h);
    fields.insert("i", spec.i);
    fields.insert("s", spec.s);
    fields.insert("f", if spec.f == 0.0 { spec.f } else { f });
    fields.insert("invert", spec.invert);
    fields.insert("days", spec.days.map_or(Value::Bool(false), Value::Int));
    if wall {
        fields.insert("civil_or_wall", PHP_DATE_WALL);
    }
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let object = IntervalObject::set_state(env.date_context(), &fields, &mut diagnostics)?;
    if let Some(diagnostic) = diagnostics.first() {
        return Err(PhpError::error(format!(
            "DateInterval::__set_state() raised {diagnostic}"
        )));
    }
    Ok(object)
}

/// The recorded `get_object_vars()` of an object node (`props`), if the oracle wrote it.
fn recorded_properties(node: &TypedNode) -> Option<&[Item]> {
    node.as_object()?.props.as_deref()
}

/// Whether a property key is the name `name`.
fn key_is(key: &Key, name: &str) -> bool {
    matches!(key, Key::Str(s) if s.as_bytes() == name.as_bytes())
}

fn array_key(key: &Key) -> ArrayKey {
    match key {
        Key::Int(i) => ArrayKey::Int(*i),
        Key::Str(s) => ArrayKey::Str(s.clone()),
    }
}

/// The object with the dynamic properties of its record ([`replay_properties`]).
fn with_properties(object: ObjectRef, node: &TypedNode) -> PhpResult<Value> {
    if let Some(props) = recorded_properties(node) {
        replay_properties(&object, props)?;
    }
    Ok(Value::Object(object))
}

/// Gives a rebuilt date object the property table PHP's object had (`recorded`: its
/// `get_object_vars()`, in table order). The date classes keep their state outside the table and
/// write it there whenever the table is read (`date_object_get_properties_for()`,
/// `date_object_get_properties_interval()`, PHPSRC/ext/date/php_date.c): a key already in the table
/// keeps its place, the others are appended. So a property the data code wrote before anything read
/// the table comes first (`$i->days = 5` on an interval: a dynamic property, whose value the handler
/// then overwrites, and json_encode() starts with `"days":false`), and one written later comes after
/// the handler's keys. The rebuilt object gets the recorded keys that come before the class's first
/// own key written into its empty table, then its own keys, then the other recorded keys appended;
/// the values are the recorded scalars (a nested array or object is not supported).
fn replay_properties(object: &ObjectRef, recorded: &[Item]) -> PhpResult<()> {
    let mut ignored: Vec<Diagnostic> = Vec::new();
    // the keys the handler writes, in its order (on a copy: the object's own table stays empty)
    let own: Vec<ArrayKey> = object
        .clone_object(&mut ignored)?
        .properties_for(PropertyPurpose::GetObjectVars, &mut ignored)?
        .into_iter()
        .map(|e| e.key)
        .collect();
    let keys: Vec<ArrayKey> = recorded.iter().map(|i| array_key(&i.key)).collect();
    if keys == own {
        return Ok(());
    }
    let write = |item: &Item, ignored: &mut Vec<Diagnostic>| -> PhpResult<()> {
        let value = match &item.value {
            TypedNode::Null => Value::Null,
            TypedNode::Bool(b) => Value::Bool(*b),
            TypedNode::Int(i) => Value::Int(*i),
            TypedNode::Float(f) => Value::Float(*f),
            TypedNode::String(s) => Value::String(s.clone()),
            other => {
                return Err(PhpError::error(format!(
                    "a dynamic {} property of a date object",
                    other.tag()
                )));
            }
        };
        let name = match &item.key {
            Key::Int(i) => PhpString::from(i.to_string()),
            Key::Str(s) => s.clone(),
        };
        object.write_property(name.as_bytes(), value, ignored)
    };
    let first_own = own.first().and_then(|k| keys.iter().position(|r| r == k));
    let (before, after) = recorded.split_at(first_own.unwrap_or(0));
    for item in before {
        write(item, &mut ignored)?;
    }
    let _ = object.properties_for(PropertyPurpose::GetObjectVars, &mut ignored)?;
    for item in after {
        if !own.contains(&array_key(&item.key)) {
            write(item, &mut ignored)?;
        }
    }
    let now: Vec<ArrayKey> = object
        .properties_for(PropertyPurpose::GetObjectVars, &mut ignored)?
        .into_iter()
        .map(|e| e.key)
        .collect();
    if now != keys {
        return Err(PhpError::error(format!(
            "rebuilt the properties of a {} as {now:?}, the oracle recorded {keys:?}",
            object.class_name()
        )));
    }
    Ok(())
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
