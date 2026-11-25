//! `DateTime` and `DateTimeImmutable` (`php_date_obj`): construction (`php_date_initialize()`), the methods of
//! PHPSRC/ext/date/php_date.c and the object handlers (clone, compare, property tables).

use std::sync::{Mutex, MutexGuard};

use twig_timelib::parse_date::{parse_from_format, strtotime};
use twig_timelib::types::{NO_CLONE, OVERRIDE_TIME, ZONETYPE_ABBR, ZONETYPE_ID, ZONETYPE_OFFSET};
use twig_timelib::{RelTime, Time, TzDb, UNSET, dow, interval};
use twig_value::float::zend_gcvt;
use twig_value::object::{
    CallArgs, ClassRef, Object, ObjectRef, Param, PropertyEntry, PropertyPurpose, PropertyStore,
};
use twig_value::param::Arg;
use twig_value::{
    Array, ArrayKey, Diagnostics, Number, PhpError, PhpResult, PhpString, ThrowableClass, Value,
};

use crate::args::{self, Bound};
use crate::classes;
use crate::context::DateContext;
use crate::errors;
use crate::format::{date_format, tz_offset_str};
use crate::interval::{CIVIL, IntervalObject, WALL};
use crate::timezone::{TimeZoneObject, TzValue};
use crate::util;

/// `PHP_DATE_INIT_CTOR`: called from a constructor (parse errors throw).
pub const INIT_CTOR: i32 = 0x01;
/// `PHP_DATE_INIT_FORMAT`: `createFromFormat()` (fill holes with `TIMELIB_OVERRIDE_TIME`).
pub const INIT_FORMAT: i32 = 0x02;

/// How a failing php_date.c helper reports its warning: as a warning of the procedural function (`date_modify():
/// …`), or (methods running under `zend_replace_error_handling(EH_THROW, …)`) as an exception of the given class
/// whose message carries the method name (`DateTime::modify(): …`).
#[derive(Clone, Copy, Debug)]
pub enum WarningMode<'a> {
    /// `E_WARNING` prefixed with `func(): `.
    Warn(&'a str),
    /// The warning converted to an exception of `class`, prefixed with `func(): `.
    Throw(&'a str, fn() -> ThrowableClass),
}

impl WarningMode<'_> {
    fn emit(self, message: &[u8], diag: &mut dyn Diagnostics) -> PhpResult<()> {
        match self {
            WarningMode::Warn(func) => {
                diag.warn(PhpString::concat(&[func.as_bytes(), b"(): ", message]))
            }
            WarningMode::Throw(func, class) => Err(PhpError::new(
                class(),
                PhpString::concat(&[func.as_bytes(), b"(): ", message]),
            )),
        }
    }
}

/// A `DateTime` or `DateTimeImmutable` object (or an object of a class extending one of them).
pub struct DateObject {
    class: ClassRef,
    immutable: bool,
    ctx: DateContext,
    time: Mutex<Option<Time>>,
    props: PropertyStore,
}

/// Port of `get_base_date_class()`: whether `class` descends from `DateTimeImmutable` (else `DateTime`).
fn is_immutable_class(class: &ClassRef) -> bool {
    class.is_subclass_of("DateTimeImmutable")
}

impl DateObject {
    /// An object of `class` whose constructor has not run (`date_object_new_date()`): every method throws
    /// `DateObjectError` until it is initialised.
    pub fn new_uninitialized(ctx: &DateContext, class: ClassRef) -> DateObject {
        let props = PropertyStore::new(&class);
        DateObject {
            immutable: is_immutable_class(&class),
            class,
            ctx: ctx.clone(),
            time: Mutex::new(None),
            props,
        }
    }

    /// An object of `class` holding `time`.
    pub fn from_time(ctx: &DateContext, class: ClassRef, time: Time) -> ObjectRef {
        let obj = DateObject::new_uninitialized(ctx, class);
        obj.set_time(Some(time));
        ObjectRef::new(obj)
    }

    fn lock(&self) -> MutexGuard<'_, Option<Time>> {
        self.time.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A copy of the broken-down time (`None` when the object is not initialised).
    pub fn time(&self) -> Option<Time> {
        self.lock().clone()
    }

    /// A copy of the time, or `DateObjectError` (`DATE_CHECK_INITIALIZED`).
    pub fn initialized_time(&self) -> PhpResult<Time> {
        self.time()
            .ok_or_else(|| errors::uninitialized(&self.class))
    }

    fn set_time(&self, t: Option<Time>) {
        *self.lock() = t;
    }

    /// Runs `f` on the initialised time in place.
    fn with_time<R>(&self, f: impl FnOnce(&mut Time) -> R) -> PhpResult<R> {
        let mut g = self.lock();
        match g.as_mut() {
            Some(t) => Ok(f(t)),
            None => Err(errors::uninitialized(&self.class)),
        }
    }

    /// Whether this is a `DateTimeImmutable`.
    pub fn is_immutable(&self) -> bool {
        self.immutable
    }

    /// The context this object was created in.
    pub fn context(&self) -> &DateContext {
        &self.ctx
    }

    /// `date_object_clone_date()`: a copy of the object (class, time and properties).
    fn clone_inner(&self) -> DateObject {
        DateObject {
            class: self.class.clone(),
            immutable: self.immutable,
            ctx: self.ctx.clone(),
            time: Mutex::new(self.time()),
            props: self.props.clone(),
        }
    }

    // ---------------------------------------------------------------------------------------------------------
    // Construction

    /// `new DateTime($datetime, $timezone)` / `new DateTimeImmutable(…)` for `class`: parse errors throw
    /// `DateMalformedStringException` (`Failed to parse time string (%s) at position %d (%c): %s`). `timezone`
    /// must be a `DateTimeZone` object.
    pub fn construct(
        ctx: &DateContext,
        class: ClassRef,
        datetime: &[u8],
        timezone: Option<&ObjectRef>,
    ) -> PhpResult<ObjectRef> {
        let obj = DateObject::new_uninitialized(ctx, class);
        let tz = timezone_arg(timezone, &obj.class, "__construct")?;
        obj.initialize(datetime, None, tz, INIT_CTOR)?;
        Ok(ObjectRef::new(obj))
    }

    /// `date_create()` / `date_create_immutable()`: like [`DateObject::construct`] but a parse error returns
    /// `None` (`false`).
    pub fn create(
        ctx: &DateContext,
        class: ClassRef,
        datetime: &[u8],
        timezone: Option<&ObjectRef>,
    ) -> PhpResult<Option<ObjectRef>> {
        let obj = DateObject::new_uninitialized(ctx, class);
        let tz = timezone_arg(timezone, &obj.class, "__construct")?;
        Ok(obj
            .initialize(datetime, None, tz, 0)?
            .then(|| ObjectRef::new(obj)))
    }

    /// `DateTime::createFromFormat()` / `date_create_from_format()` for `class`: `None` (`false`) when the
    /// string does not match the format (see [`DateContext::last_errors`]).
    pub fn create_from_format(
        ctx: &DateContext,
        class: ClassRef,
        format: &[u8],
        datetime: &[u8],
        timezone: Option<&ObjectRef>,
    ) -> PhpResult<Option<ObjectRef>> {
        let obj = DateObject::new_uninitialized(ctx, class);
        let tz = timezone_arg(timezone, &obj.class, "createFromFormat")?;
        Ok(obj
            .initialize(datetime, Some(format), tz, INIT_FORMAT)?
            .then(|| ObjectRef::new(obj)))
    }

    /// `DateTime::createFromTimestamp()` / `DateTimeImmutable::createFromTimestamp()` for `class`.
    pub fn create_from_timestamp(
        ctx: &DateContext,
        class: ClassRef,
        timestamp: Number,
    ) -> PhpResult<ObjectRef> {
        let func = format!("{}::createFromTimestamp", base_class_name(&class));
        let time = match timestamp {
            Number::Int(sec) => initialize_from_ts_long(sec, 0),
            Number::Float(d) => initialize_from_ts_double(d, &func)?,
        };
        Ok(DateObject::from_time(ctx, class, time))
    }

    /// `DateTime::__set_state($array)` / `DateTimeImmutable::__set_state($array)` (`immutable`): a new object from
    /// the `date`, `timezone_type` and `timezone` entries written by `var_export()` / `__serialize()`; anything
    /// else throws `Error: Invalid serialization data for DateTime object`.
    pub fn set_state(ctx: &DateContext, immutable: bool, data: &Array) -> PhpResult<ObjectRef> {
        let (class, name) = if immutable {
            (classes::datetime_immutable(), "DateTimeImmutable")
        } else {
            (classes::datetime(), "DateTime")
        };
        let obj = DateObject::new_uninitialized(ctx, class);
        if !obj.initialize_from_hash(data)? {
            return Err(errors::invalid_serialization(name));
        }
        Ok(ObjectRef::new(obj))
    }

    /// Port of `php_date_initialize()` (PHPSRC/ext/date/php_date.c): parses `time_str` (with `format` for
    /// `createFromFormat()`), fills the missing fields from "now" in the requested zone and computes the
    /// timestamp. `Ok(false)` is a parse failure that did not throw (the object is left uninitialised).
    pub fn initialize(
        &self,
        time_str: &[u8],
        format: Option<&[u8]>,
        timezone_object: Option<&TimeZoneObject>,
        flags: i32,
    ) -> PhpResult<bool> {
        let ctx = &self.ctx;
        self.set_time(None);
        let cache = ctx.tz_cache();
        let mut wrapper = |name: &str, _db: &TzDb| cache.get(name);
        let (time_str, parsed, err) = match format {
            Some(f) => {
                let (t, e) = parse_from_format(f, time_str, ctx.tzdb(), &mut wrapper);
                (time_str, t, e)
            }
            None => {
                let s: &[u8] = if time_str.is_empty() {
                    b"now"
                } else {
                    time_str
                };
                let (t, e) = strtotime(s, ctx.tzdb(), &mut wrapper);
                (s, t, e)
            }
        };

        // update last errors and warnings
        let failed = err.error_count() > 0;
        let message = failed.then(|| errors::parse_failure(time_str, &err));
        ctx.update_errors_warnings(Some(err));

        // If called from a constructor throw an exception
        if let Some(message) = message {
            if flags & INIT_CTOR != 0 {
                return Err(PhpError::new(
                    errors::date_malformed_string_exception(),
                    message,
                ));
            }
            return Ok(false);
        }
        let mut time = parsed;

        let mut zone_type = ZONETYPE_ID;
        let mut new_offset: i64 = 0;
        let mut new_dst = 0;
        let mut new_abbr = None;
        let tzi = if let Some(tzobj) = timezone_object {
            match tzobj.value() {
                Some(TzValue::Id(z)) => Some(z),
                Some(TzValue::Offset(o)) => {
                    zone_type = ZONETYPE_OFFSET;
                    new_offset = o;
                    None
                }
                Some(TzValue::Abbr(a)) => {
                    zone_type = ZONETYPE_ABBR;
                    new_offset = a.utc_offset;
                    new_dst = a.dst;
                    new_abbr = Some(a.abbr);
                    None
                }
                None => {
                    self.set_time(Some(time));
                    return Err(PhpError::error(
                        "The DateTimeZone object has not been correctly initialized by its constructor",
                    ));
                }
            }
        } else if let Some(z) = time.tz_info.clone() {
            Some(z)
        } else {
            match ctx.timezone_info() {
                Ok(z) => Some(z),
                Err(e) => {
                    self.set_time(Some(time));
                    return Err(e);
                }
            }
        };

        let mut now = Time::new();
        now.zone_type = zone_type;
        match zone_type {
            ZONETYPE_ID => now.tz_info = tzi.clone(),
            ZONETYPE_OFFSET => now.z = new_offset as i32,
            _ => {
                now.z = new_offset as i32;
                now.dst = new_dst;
                now.tz_abbr = new_abbr;
            }
        }
        let (sec, usec) = ctx.current_time();
        now.unixtime2local(sec);
        // php_date_set_time_fraction(now, usec): an `int` parameter.
        now.us = i64::from(usec as i32);

        if format.is_none() && time_str == b"now" {
            self.set_time(Some(now));
            return Ok(true);
        }

        let mut options = NO_CLONE;
        if flags & INIT_FORMAT != 0 {
            options |= OVERRIDE_TIME;
        }
        time.fill_holes(&now, options);

        time.update_ts(tzi.as_ref());
        time.update_from_sse();

        time.have_relative = 0;

        self.set_time(Some(time));
        Ok(true)
    }

    /// Port of `php_date_initialize_from_hash()`: `date` (string), `timezone_type` (int) and `timezone` (string)
    /// as written by `__serialize()` / `var_export()`.
    fn initialize_from_hash(&self, data: &Array) -> PhpResult<bool> {
        let Some(Value::String(z_date)) = data.get_str(b"date") else {
            return Ok(false);
        };
        let Some(Value::Int(z_timezone_type)) = data.get_str(b"timezone_type") else {
            return Ok(false);
        };
        let Some(Value::String(z_timezone)) = data.get_str(b"timezone") else {
            return Ok(false);
        };
        match u32::try_from(*z_timezone_type) {
            Ok(ZONETYPE_OFFSET) | Ok(ZONETYPE_ABBR) => {
                let tmp = PhpString::concat(&[z_date.as_bytes(), b" ", z_timezone.as_bytes()]);
                self.initialize(tmp.as_bytes(), None, None, 0)
            }
            Ok(ZONETYPE_ID) => {
                let Some(tzi) = self.ctx.parse_tzfile(z_timezone.as_bytes()) else {
                    return Ok(false);
                };
                let tzobj = TimeZoneObject::new_uninitialized(&self.ctx, classes::datetimezone());
                tzobj.set_value(Some(TzValue::Id(tzi)));
                self.initialize(z_date.as_bytes(), None, Some(&tzobj), 0)
            }
            _ => Ok(false),
        }
    }

    // ---------------------------------------------------------------------------------------------------------
    // Methods (typed API)

    /// `DateTimeInterface::format()` (`date_format()` with the object's `is_localtime`).
    pub fn format(&self, format: &[u8]) -> PhpResult<PhpString> {
        let t = self.initialized_time()?;
        Ok(PhpString::from(date_format(
            format,
            &t,
            t.is_localtime != 0,
        )))
    }

    /// Port of `PHP_FUNCTION(date_timestamp_get)`: `getTimestamp()` (recomputes a stale timestamp in place).
    pub fn timestamp(&self) -> PhpResult<i64> {
        self.with_time(|t| {
            if t.sse_uptodate == 0 {
                t.update_ts(None);
            }
            t.sse
        })
    }

    /// `getMicrosecond()`.
    pub fn microsecond(&self) -> PhpResult<i64> {
        Ok(self.initialized_time()?.us)
    }

    /// Port of `PHP_FUNCTION(date_offset_get)`: `getOffset()`.
    pub fn offset(&self) -> PhpResult<Value> {
        let t = self.initialized_time()?;
        if t.is_localtime == 0 {
            return Ok(Value::Int(0));
        }
        Ok(match t.zone_type {
            ZONETYPE_ID => Value::Int(i64::from(
                t.tz_info
                    .as_ref()
                    .map_or(0, |z| z.get_time_zone_info(t.sse).offset),
            )),
            ZONETYPE_OFFSET => Value::Int(i64::from(t.z)),
            ZONETYPE_ABBR => Value::Int(i64::from(t.z.wrapping_add(3600i32.wrapping_mul(t.dst)))),
            _ => Value::Null,
        })
    }

    /// Port of `PHP_FUNCTION(date_timezone_get)`: `getTimezone()`, `None` (`false`) for a time without zone.
    pub fn timezone(&self) -> PhpResult<Option<ObjectRef>> {
        let t = self.initialized_time()?;
        if t.is_localtime == 0 {
            return Ok(None);
        }
        Ok(TzValue::from_time(&t).map(|tz| TimeZoneObject::from_value(&self.ctx, tz)))
    }

    /// Port of `php_date_timezone_set()`: converts the time to `tz` (same instant). An uninitialised
    /// `DateTimeZone` leaves the zone unchanged, as in PHP.
    pub fn set_timezone_value(&self, tz: Option<&TzValue>) -> PhpResult<()> {
        self.with_time(|t| {
            match tz {
                Some(TzValue::Offset(o)) => t.set_timezone_from_offset(*o),
                Some(TzValue::Abbr(a)) => t.set_timezone_from_abbr(a),
                Some(TzValue::Id(z)) => t.set_timezone(z),
                None => {}
            }
            t.unixtime2local(t.sse);
        })
    }

    /// Port of `php_date_modify()`: applies a relative/absolute time string. Parse errors are reported through
    /// `report` (a warning, or the `DateMalformedStringException` of the `modify()` methods) and leave the time
    /// unchanged (`Ok(false)`).
    pub fn modify(
        &self,
        modify: &[u8],
        report: WarningMode<'_>,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        if self.time().is_none() {
            return Err(errors::uninitialized(&self.class));
        }
        let cache = self.ctx.tz_cache();
        let (tmp_time, err) = strtotime(modify, self.ctx.tzdb(), &mut |name: &str, _db: &TzDb| {
            cache.get(name)
        });

        // update last errors and warnings
        let failed = err.error_count() > 0;
        let message = failed.then(|| errors::parse_failure(modify, &err));
        self.ctx.update_errors_warnings(Some(err));

        if let Some(message) = message {
            // spit out the first library error message, at least
            report.emit(&message, diag)?;
            return Ok(false);
        }

        self.with_time(|t| {
            t.relative = tmp_time.relative;
            t.have_relative = tmp_time.have_relative;
            t.sse_uptodate = 0;

            if tmp_time.y != UNSET {
                t.y = tmp_time.y;
            }
            if tmp_time.m != UNSET {
                t.m = tmp_time.m;
            }
            if tmp_time.d != UNSET {
                t.d = tmp_time.d;
            }

            if tmp_time.h != UNSET {
                t.h = tmp_time.h;
                if tmp_time.i != UNSET {
                    t.i = tmp_time.i;
                    if tmp_time.s != UNSET {
                        t.s = tmp_time.s;
                    } else {
                        t.s = 0;
                    }
                } else {
                    t.i = 0;
                    t.s = 0;
                }
            }

            if tmp_time.us != UNSET {
                t.us = tmp_time.us;
            }

            // Reset timezone to UTC if we detect a "@<ts>" modification
            if tmp_time.y == 1970
                && tmp_time.m == 1
                && tmp_time.d == 1
                && tmp_time.h == 0
                && tmp_time.i == 0
                && tmp_time.s == 0
                && tmp_time.us == 0
                && tmp_time.have_zone != 0
                && tmp_time.zone_type == ZONETYPE_OFFSET
                && tmp_time.z == 0
                && tmp_time.dst == 0
            {
                t.set_timezone_from_offset(0);
            }

            t.update_ts(None);
            t.update_from_sse();
            t.have_relative = 0;
            t.relative = RelTime::default();
        })?;
        Ok(true)
    }

    /// Port of `php_date_add()`: adds a `DateInterval` (wall-clock arithmetic for intervals built from an ISO
    /// string, civil for `diff()` results and `createFromDateString()`).
    pub fn add(&self, interval: &IntervalObject) -> PhpResult<()> {
        let t = self.initialized_time()?;
        let (diff, civil_or_wall) = interval.initialized_diff()?;
        let new_time = if civil_or_wall == WALL {
            interval::add_wall(&t, &diff)
        } else {
            interval::add(&t, &diff)
        };
        self.set_time(Some(new_time));
        Ok(())
    }

    /// Port of `php_date_sub()`: subtracts a `DateInterval`; weekday and other special relative parts are
    /// refused through `report` (`Only non-special relative time specifications are supported for
    /// subtraction`).
    pub fn sub(
        &self,
        interval: &IntervalObject,
        report: WarningMode<'_>,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        let t = self.initialized_time()?;
        let (diff, civil_or_wall) = interval.initialized_diff()?;
        if diff.have_weekday_relative != 0 || diff.have_special_relative != 0 {
            return report.emit(
                b"Only non-special relative time specifications are supported for subtraction",
                diag,
            );
        }
        let new_time = if civil_or_wall == WALL {
            interval::sub_wall(&t, &diff)
        } else {
            interval::sub(&t, &diff)
        };
        self.set_time(Some(new_time));
        Ok(())
    }

    /// Port of `php_date_time_set()`: `setTime()`.
    pub fn set_time_of_day(&self, h: i64, i: i64, s: i64, us: i64) -> PhpResult<()> {
        self.with_time(|t| {
            t.h = h;
            t.i = i;
            t.s = s;
            t.us = us;
            t.update_ts(None);
            t.update_from_sse();
        })
    }

    /// Port of `php_date_date_set()`: `setDate()` (no `timelib_update_from_sse()`, as in PHP).
    pub fn set_date(&self, y: i64, m: i64, d: i64) -> PhpResult<()> {
        self.with_time(|t| {
            t.y = y;
            t.m = m;
            t.d = d;
            t.update_ts(None);
        })
    }

    /// Port of `php_date_isodate_set()`: `setISODate()`.
    pub fn set_isodate(&self, y: i64, w: i64, d: i64) -> PhpResult<()> {
        self.with_time(|t| {
            t.y = y;
            t.m = 1;
            t.d = 1;
            t.relative = RelTime::default();
            t.relative.d = dow::daynr_from_weeknr(y, w, d);
            t.have_relative = 1;
            t.update_ts(None);
        })
    }

    /// Port of `php_date_timestamp_set()`: `setTimestamp()` (microseconds reset to 0).
    pub fn set_timestamp(&self, timestamp: i64) -> PhpResult<()> {
        self.with_time(|t| {
            t.unixtime2local(timestamp);
            t.update_ts(None);
            t.us = 0;
        })
    }

    /// `setMicrosecond()` after its range check (`php_date_set_time_fraction()`, an `int`).
    pub fn set_microsecond(&self, us: i64) -> PhpResult<()> {
        self.with_time(|t| t.us = i64::from(us as i32))
    }

    /// Port of `PHP_FUNCTION(date_diff)`: `diff()` as a `DateInterval`.
    pub fn diff(&self, other: &DateObject, absolute: bool) -> PhpResult<ObjectRef> {
        let t1 = self.initialized_time()?;
        let t2 = other.initialized_time()?;
        let mut rt = interval::diff(&t1, &t2);
        if absolute {
            rt.invert = 0;
        }
        Ok(IntervalObject::from_reltime(&self.ctx, rt, CIVIL))
    }

    /// `__serialize()` (`date_object_to_hash()` plus the object's own properties).
    pub fn serialize(&self, this: &ObjectRef, diag: &mut dyn Diagnostics) -> PhpResult<Array> {
        let t = self.initialized_time()?;
        let mut props = Vec::new();
        date_object_to_hash(&t, &mut props);
        let mut out = util::assoc(
            props
                .into_iter()
                .filter_map(|e| e.value.map(|v| (e.key, v))),
        );
        util::add_common_properties(&mut out, this, diag)?;
        Ok(out)
    }

    // ---------------------------------------------------------------------------------------------------------
    // Method dispatch helpers

    /// The object a mutator works on: `$this` for `DateTime`, a clone for `DateTimeImmutable`
    /// (`date_clone_immutable()`).
    fn target(&self, this: &ObjectRef) -> ObjectRef {
        if self.immutable {
            ObjectRef::new(self.clone_inner())
        } else {
            this.clone()
        }
    }
}

/// `get_active_function_or_method_name()` for a date method: the declaring (built-in) class.
fn base_class_name(class: &ClassRef) -> &'static str {
    if is_immutable_class(class) {
        "DateTimeImmutable"
    } else {
        "DateTime"
    }
}

/// The `?DateTimeZone $timezone` argument of the Rust constructors.
fn timezone_arg<'a>(
    timezone: Option<&'a ObjectRef>,
    class: &ClassRef,
    method: &str,
) -> PhpResult<Option<&'a TimeZoneObject>> {
    match timezone {
        None => Ok(None),
        Some(o) => match o.downcast_ref::<TimeZoneObject>() {
            Some(tz) => Ok(Some(tz)),
            None => Err(Arg::new(
                &format!("{}::{method}", base_class_name(class)),
                2,
                "timezone",
            )
            .type_error("of type ?DateTimeZone", &Value::Object(o.clone()))),
        },
    }
}

/// Port of `php_date_initialize_from_ts_long()`.
pub fn initialize_from_ts_long(sec: i64, usec: i32) -> Time {
    let mut t = Time::new();
    t.zone_type = ZONETYPE_OFFSET;
    t.unixtime2gmt(sec);
    t.update_ts(None);
    t.us = i64::from(usec);
    t
}

/// `must be a finite number between %lld and %lld.999999, %g given` (`DateRangeError`).
fn ts_range_error(func: &str, ts: f64) -> PhpError {
    let shown = if ts.is_nan() {
        "NAN".to_owned()
    } else if ts.is_infinite() {
        if ts > 0.0 { "INF" } else { "-INF" }.to_owned()
    } else {
        zend_gcvt(ts, 6, b'.', b'e')
    };
    PhpError::new(
        errors::date_range_error(),
        format!(
            "{func}(): Argument #1 ($timestamp) must be a finite number between {} and {}.999999, {shown} given",
            i64::MIN,
            i64::MAX
        ),
    )
}

/// Port of `php_date_initialize_from_ts_double()`: seconds and rounded microseconds of a float timestamp.
pub fn initialize_from_ts_double(ts: f64, func: &str) -> PhpResult<Time> {
    let sec_dval = ts.trunc();
    if sec_dval.is_nan() || !twig_value::convert::double_fits_long(sec_dval) {
        return Err(ts_range_error(func, ts));
    }
    let mut sec = sec_dval as i64;
    let mut usec = ((ts % 1.0) * 1_000_000.0).round() as i32;

    if usec.wrapping_abs() == 1_000_000 {
        sec += if usec > 0 { 1 } else { -1 };
        usec = 0;
    }

    if usec < 0 {
        if sec == i64::MIN {
            return Err(ts_range_error(func, ts));
        }
        sec -= 1;
        usec += 1_000_000;
    }

    Ok(initialize_from_ts_long(sec, usec))
}

/// Port of `date_object_to_hash()`: the `date`, `timezone_type` and `timezone` entries.
pub(crate) fn date_object_to_hash(t: &Time, props: &mut Vec<PropertyEntry>) {
    // first we add the date and time in ISO format
    util::update_prop(
        props,
        "date",
        Value::String(PhpString::from(date_format(b"x-m-d H:i:s.u", t, true))),
    );

    // then we add the timezone name (or similar)
    if t.is_localtime != 0 {
        let zone_type = Value::Int(i64::from(t.zone_type));
        util::update_prop(props, "timezone_type", zone_type.clone());
        let zv = match t.zone_type {
            ZONETYPE_ID => Value::from(t.tz_info.as_ref().map_or("", |z| z.name.as_str())),
            ZONETYPE_OFFSET => Value::String(PhpString::from(tz_offset_str(i64::from(t.z)))),
            ZONETYPE_ABBR => Value::from(t.tz_abbr.as_deref().unwrap_or("")),
            // No case matched: the zval still holds the zone type.
            _ => zone_type,
        };
        util::update_prop(props, "timezone", zv);
    }
}

/// `restore_custom_datetime_properties()`'s internal keys (`date_time_is_internal_property()`).
fn is_internal_property(name: &[u8]) -> bool {
    name == b"date" || name == b"timezone_type" || name == b"timezone"
}

/// Port of `PHP_FUNCTION(date_get_last_errors)` (`DateTime::getLastErrors()`): `false` without warnings or
/// errors, else `warning_count`, `warnings`, `error_count`, `errors`.
pub fn last_errors_value(ctx: &DateContext) -> Value {
    match ctx.last_errors() {
        Some(e) => {
            let mut out = Array::new();
            crate::functions::zval_from_error_container(&mut out, &e);
            Value::Array(out)
        }
        None => Value::Bool(false),
    }
}

/// `$o instanceof DateTimeInterface` and a [`DateObject`].
fn date_arg(o: &ObjectRef) -> PhpResult<&DateObject> {
    o.downcast_ref::<DateObject>()
        .ok_or_else(|| errors::uninitialized(o.class()))
}

fn interval_arg(o: &ObjectRef) -> PhpResult<&IntervalObject> {
    o.downcast_ref::<IntervalObject>()
        .ok_or_else(|| errors::uninitialized(o.class()))
}

impl Object for DateObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let base = base_class_name(&self.class);
        let name = |m: &str| format!("{base}::{m}");
        let ret_this = |o: ObjectRef| Ok(Value::Object(o));
        match method {
            "__construct" => {
                let f = name("__construct");
                let a = Bound::new(
                    &f,
                    &args,
                    &[
                        Param::optional("datetime", "now"),
                        Param::optional("timezone", Value::Null),
                    ],
                )?;
                let time_str = a.string(0, diag)?;
                let tz = a.object_of_or_null(1, "DateTimeZone")?;
                let tz = tz.as_ref().and_then(|o| o.downcast_ref::<TimeZoneObject>());
                self.initialize(time_str.as_bytes(), None, tz, INIT_CTOR)?;
                Ok(Value::Null)
            }
            "__serialize" => {
                args::none(&name("__serialize"), &args)?;
                Ok(Value::Array(self.serialize(this, diag)?))
            }
            "__unserialize" => {
                let f = name("__unserialize");
                let a = Bound::new(&f, &args, &[Param::required("data")])?;
                let data = a.array(0)?;
                if !self.initialize_from_hash(&data)? {
                    return Err(errors::invalid_serialization(base));
                }
                util::restore_custom_properties(this, &data, is_internal_property, diag)?;
                Ok(Value::Null)
            }
            "__wakeup" => {
                util::wakeup_deprecated(base, diag)?;
                args::none(&name("__wakeup"), &args)?;
                let table = util::assoc(
                    util::std_properties(this, diag)?
                        .into_iter()
                        .filter_map(|e| e.value.map(|v| (e.key, v))),
                );
                if !self.initialize_from_hash(&table)? {
                    return Err(errors::invalid_serialization(base));
                }
                Ok(Value::Null)
            }
            "__set_state" => {
                let f = name("__set_state");
                let a = Bound::new(&f, &args, &[Param::required("array")])?;
                let data = a.array(0)?;
                Ok(Value::Object(DateObject::set_state(
                    &self.ctx,
                    self.immutable,
                    &data,
                )?))
            }
            "createfromimmutable" | "createfrommutable" | "createfrominterface" => {
                let (m, param, class) = match method {
                    "createfromimmutable" => ("createFromImmutable", "object", "DateTimeImmutable"),
                    "createfrommutable" => ("createFromMutable", "object", "DateTime"),
                    _ => ("createFromInterface", "object", "DateTimeInterface"),
                };
                let f = name(m);
                let a = Bound::new(&f, &args, &[Param::required(param)])?;
                let src = a.object_of(0, class)?;
                let old = date_arg(&src)?;
                let t = old.initialized_time()?;
                // object_init_ex(return_value, execute_data->This.value.ce): the called class.
                Ok(Value::Object(DateObject::from_time(
                    &self.ctx,
                    self.class.clone(),
                    t,
                )))
            }
            "createfromformat" => {
                let f = name("createFromFormat");
                let a = Bound::new(
                    &f,
                    &args,
                    &[
                        Param::required("format"),
                        Param::required("datetime"),
                        Param::optional("timezone", Value::Null),
                    ],
                )?;
                let format = a.string(0, diag)?;
                let datetime = a.path(1, diag)?;
                let tz = a.object_of_or_null(2, "DateTimeZone")?;
                let obj = DateObject::new_uninitialized(&self.ctx, self.class.clone());
                let tzo = tz.as_ref().and_then(|o| o.downcast_ref::<TimeZoneObject>());
                if obj.initialize(
                    datetime.as_bytes(),
                    Some(format.as_bytes()),
                    tzo,
                    INIT_FORMAT,
                )? {
                    Ok(Value::Object(ObjectRef::new(obj)))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            "createfromtimestamp" => {
                let f = name("createFromTimestamp");
                let a = Bound::new(&f, &args, &[Param::required("timestamp")])?;
                let ts = a.number(0, diag)?;
                let time = match ts {
                    Number::Int(sec) => initialize_from_ts_long(sec, 0),
                    Number::Float(d) => initialize_from_ts_double(d, &f)?,
                };
                Ok(Value::Object(DateObject::from_time(
                    &self.ctx,
                    self.class.clone(),
                    time,
                )))
            }
            "getlasterrors" => {
                args::none(&name("getLastErrors"), &args)?;
                Ok(last_errors_value(&self.ctx))
            }
            "format" => {
                let f = name("format");
                let a = Bound::new(&f, &args, &[Param::required("format")])?;
                let format = a.string(0, diag)?;
                Ok(Value::String(self.format(format.as_bytes())?))
            }
            "modify" => {
                let f = name("modify");
                let a = Bound::new(&f, &args, &[Param::required("modifier")])?;
                let modify = a.string(0, diag)?;
                let target = self.target(this);
                let obj = date_arg(&target)?;
                obj.modify(
                    modify.as_bytes(),
                    WarningMode::Throw(&f, errors::date_malformed_string_exception),
                    diag,
                )?;
                ret_this(target)
            }
            "add" => {
                let f = name("add");
                let a = Bound::new(&f, &args, &[Param::required("interval")])?;
                let iv = a.object_of(0, "DateInterval")?;
                let target = self.target(this);
                date_arg(&target)?.add(interval_arg(&iv)?)?;
                ret_this(target)
            }
            "sub" => {
                let f = name("sub");
                let a = Bound::new(&f, &args, &[Param::required("interval")])?;
                let iv = a.object_of(0, "DateInterval")?;
                let target = self.target(this);
                date_arg(&target)?.sub(
                    interval_arg(&iv)?,
                    WarningMode::Throw(&f, errors::date_invalid_operation_exception),
                    diag,
                )?;
                ret_this(target)
            }
            "gettimezone" => {
                args::none(&name("getTimezone"), &args)?;
                Ok(match self.timezone()? {
                    Some(tz) => Value::Object(tz),
                    None => Value::Bool(false),
                })
            }
            "settimezone" => {
                let f = name("setTimezone");
                let a = Bound::new(&f, &args, &[Param::required("timezone")])?;
                let tz = a.object_of(0, "DateTimeZone")?;
                let target = self.target(this);
                let value = tz.downcast_ref::<TimeZoneObject>().and_then(|t| t.value());
                date_arg(&target)?.set_timezone_value(value.as_ref())?;
                ret_this(target)
            }
            "getoffset" => {
                args::none(&name("getOffset"), &args)?;
                self.offset()
            }
            "getmicrosecond" => {
                args::none(&name("getMicrosecond"), &args)?;
                Ok(Value::Int(self.microsecond()?))
            }
            "settime" => {
                let f = name("setTime");
                let a = Bound::new(
                    &f,
                    &args,
                    &[
                        Param::required("hour"),
                        Param::required("minute"),
                        Param::optional("second", 0),
                        Param::optional("microsecond", 0),
                    ],
                )?;
                let h = a.long(0, diag)?;
                let i = a.long(1, diag)?;
                let s = a.long(2, diag)?;
                let us = a.long(3, diag)?;
                let target = self.target(this);
                date_arg(&target)?.set_time_of_day(h, i, s, us)?;
                ret_this(target)
            }
            "setdate" => {
                let f = name("setDate");
                let a = Bound::new(
                    &f,
                    &args,
                    &[
                        Param::required("year"),
                        Param::required("month"),
                        Param::required("day"),
                    ],
                )?;
                let y = a.long(0, diag)?;
                let m = a.long(1, diag)?;
                let d = a.long(2, diag)?;
                let target = self.target(this);
                date_arg(&target)?.set_date(y, m, d)?;
                ret_this(target)
            }
            "setisodate" => {
                let f = name("setISODate");
                let a = Bound::new(
                    &f,
                    &args,
                    &[
                        Param::required("year"),
                        Param::required("week"),
                        Param::optional("dayOfWeek", 1),
                    ],
                )?;
                let y = a.long(0, diag)?;
                let w = a.long(1, diag)?;
                let d = a.long(2, diag)?;
                let target = self.target(this);
                date_arg(&target)?.set_isodate(y, w, d)?;
                ret_this(target)
            }
            "settimestamp" => {
                let f = name("setTimestamp");
                let a = Bound::new(&f, &args, &[Param::required("timestamp")])?;
                let ts = a.long(0, diag)?;
                let target = self.target(this);
                date_arg(&target)?.set_timestamp(ts)?;
                ret_this(target)
            }
            "setmicrosecond" => {
                let f = name("setMicrosecond");
                let a = Bound::new(&f, &args, &[Param::required("microsecond")])?;
                let us = a.long(0, diag)?;
                if !(0..=999_999).contains(&us) {
                    return Err(PhpError::new(
                        errors::date_range_error(),
                        format!(
                            "{f}(): Argument #1 ($microsecond) must be between 0 and 999999, {us} given"
                        ),
                    ));
                }
                self.initialized_time()?;
                let target = self.target(this);
                date_arg(&target)?.set_microsecond(us)?;
                ret_this(target)
            }
            "gettimestamp" => {
                args::none(&name("getTimestamp"), &args)?;
                Ok(Value::Int(self.timestamp()?))
            }
            "diff" => {
                let f = name("diff");
                let a = Bound::new(
                    &f,
                    &args,
                    &[
                        Param::required("targetObject"),
                        Param::optional("absolute", false),
                    ],
                )?;
                let other = a.object_of(0, "DateTimeInterface")?;
                let absolute = a.bool(1, diag)?;
                let other = date_arg(&other)?;
                Ok(Value::Object(self.diff(other, absolute)?))
            }
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }

    /// Port of `date_object_get_properties_for()`: the object's properties plus `date`, `timezone_type` and
    /// `timezone` for `var_dump()`, `(array)`, `var_export()`, `json_encode()` and `serialize()`;
    /// `get_object_vars()` and `foreach` see only the real properties.
    fn properties_for(
        &self,
        this: &ObjectRef,
        purpose: PropertyPurpose,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        let mut props = util::std_properties(this, diag)?;
        match purpose {
            PropertyPurpose::Serialize
            | PropertyPurpose::VarExport
            | PropertyPurpose::Json
            | PropertyPurpose::ArrayCast
            | PropertyPurpose::Debug => {}
            _ => return Ok(props),
        }
        if let Some(t) = self.time() {
            date_object_to_hash(&t, &mut props);
        }
        Ok(props)
    }

    /// Port of `date_object_clone_date()`.
    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(self.clone_inner()))
    }

    /// Port of `date_object_compare_date()`: any two date objects compare by instant (`timelib_time_compare()`),
    /// other operands use the standard comparison.
    fn compare(
        &self,
        _this: &ObjectRef,
        other: &Value,
        this_is_left: bool,
        _compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        // ZEND_COMPARE_OBJECTS_FALLBACK()
        let Value::Object(o) = other else {
            return None;
        };
        let other = o.downcast_ref::<DateObject>()?;
        let (o1, o2) = if this_is_left {
            (self, other)
        } else {
            (other, self)
        };
        if o1.time().is_none() || o2.time().is_none() {
            return Some(Err(PhpError::new(
                errors::date_object_error(),
                "Trying to compare an incomplete DateTime or DateTimeImmutable object",
            )));
        }
        let refresh = |o: &DateObject| {
            o.with_time(|t| {
                if t.sse_uptodate == 0 {
                    let tz = t.tz_info.clone();
                    t.update_ts(tz.as_ref());
                }
                t.clone()
            })
        };
        let t1 = match refresh(o1) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let t2 = match refresh(o2) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        Some(Ok(t1.compare(&t2)))
    }
}

impl DateObject {
    /// Port of `date_object_to_hash()` as an array: the synthetic `date`, `timezone_type` and `timezone` entries
    /// (what `((array) $date)[$item]` reads for these keys; Twig's `getAttribute()` uses it for
    /// `DateTimeInterface` objects).
    pub fn to_hash(&self) -> PhpResult<Array> {
        let t = self.initialized_time()?;
        let mut props = Vec::new();
        date_object_to_hash(&t, &mut props);
        let mut out = Array::new();
        for e in props {
            if let (ArrayKey::Str(k), Some(v)) = (e.key, e.value) {
                out.insert(ArrayKey::Str(k), v);
            }
        }
        Ok(out)
    }
}
