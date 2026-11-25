//! `DateInterval` (`php_interval_obj`): ISO 8601 construction, `createFromDateString()`, the property handlers
//! that expose `y m d h i s f invert days`, `format()` and (un)serialisation (PHPSRC/ext/date/php_date.c).

use std::sync::{Mutex, MutexGuard};

use twig_timelib::parse_date::strtotime;
use twig_timelib::parse_iso_intervals::strtointerval;
use twig_timelib::{RelTime, TzDb, UNSET, interval};
use twig_value::object::{
    CallArgs, ClassRef, FetchMode, Object, ObjectRef, Param, PropertyCheck, PropertyEntry,
    PropertyPurpose, PropertyStore,
};
use twig_value::{Array, Diagnostics, PhpError, PhpResult, PhpString, Value, convert};

use crate::args::{self, Bound};
use crate::classes;
use crate::context::{DateContext, c_str};
use crate::errors;
use crate::format::date_interval_format;
use crate::util;

/// `PHP_DATE_CIVIL`: civil-time arithmetic (`diff()` results, `createFromDateString()`).
pub const CIVIL: i32 = 1;
/// `PHP_DATE_WALL`: wall-clock arithmetic (intervals built from an ISO 8601 string).
pub const WALL: i32 = 2;

#[derive(Clone, Debug, Default)]
struct State {
    diff: Option<RelTime>,
    civil_or_wall: i32,
    initialized: bool,
    from_string: bool,
    date_string: Option<PhpString>,
}

/// A `DateInterval` object.
pub struct IntervalObject {
    class: ClassRef,
    ctx: DateContext,
    state: Mutex<State>,
    props: PropertyStore,
}

/// Port of `date_interval_initialize()` (PHPSRC/ext/date/php_date.c): an ISO 8601 duration (`P1Y2M3DT4H5M6S`,
/// `P0001-02-03T04:05:06`) or a pair of dates. Errors throw `DateMalformedIntervalStringException`
/// (`Unknown or bad format (%s)`, `Failed to parse interval (%s)`).
pub fn date_interval_initialize(format: &[u8]) -> PhpResult<RelTime> {
    let r = strtointerval(format);
    if r.errors.error_count() > 0 {
        return Err(errors::throw(
            errors::date_malformed_interval_string_exception(),
            PhpString::concat(&[b"Unknown or bad format (", c_str(format), b")"]),
        ));
    }
    if let Some(p) = r.period {
        return Ok(p);
    }
    match (r.begin, r.end) {
        (Some(mut b), Some(mut e)) => {
            b.update_ts(None);
            e.update_ts(None);
            Ok(interval::diff(&b, &e))
        }
        _ => Err(errors::throw(
            errors::date_malformed_interval_string_exception(),
            PhpString::concat(&[b"Failed to parse interval (", c_str(format), b")"]),
        )),
    }
}

/// `strtoll(s, NULL, 10)` (`DATE_A64I`): optional leading white space and sign, then decimal digits; saturates
/// on overflow.
fn strtoll(s: &[u8]) -> i64 {
    let s = c_str(s);
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut acc: i128 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        acc = (acc * 10 + i128::from(s[i] - b'0')).min(i128::from(i64::MAX) + 1);
        i += 1;
    }
    if neg {
        (-acc).max(i128::from(i64::MIN)) as i64
    } else {
        acc.min(i128::from(i64::MAX)) as i64
    }
}

/// `restore_custom_dateinterval_properties()`'s internal keys (`date_interval_is_internal_property()`).
fn is_internal_property(name: &[u8]) -> bool {
    matches!(
        name,
        b"date_string"
            | b"from_string"
            | b"y"
            | b"m"
            | b"d"
            | b"h"
            | b"i"
            | b"s"
            | b"f"
            | b"invert"
            | b"days"
    )
}

impl IntervalObject {
    /// An uninitialised object of `class` (`date_object_new_interval()`).
    pub fn new_uninitialized(ctx: &DateContext, class: ClassRef) -> IntervalObject {
        let props = PropertyStore::new(&class);
        IntervalObject {
            class,
            ctx: ctx.clone(),
            state: Mutex::new(State::default()),
            props,
        }
    }

    /// A `DateInterval` holding `rt` with the given arithmetic mode ([`CIVIL`] or [`WALL`]).
    pub fn from_reltime(ctx: &DateContext, rt: RelTime, civil_or_wall: i32) -> ObjectRef {
        let obj = IntervalObject::new_uninitialized(ctx, classes::dateinterval());
        {
            let mut st = obj.lock();
            st.diff = Some(rt);
            st.initialized = true;
            st.civil_or_wall = civil_or_wall;
        }
        ObjectRef::new(obj)
    }

    /// `new DateInterval($duration)`.
    pub fn construct(ctx: &DateContext, duration: &[u8]) -> PhpResult<ObjectRef> {
        let rt = date_interval_initialize(duration)?;
        Ok(IntervalObject::from_reltime(ctx, rt, WALL))
    }

    /// `DateInterval::createFromDateString()`: the relative part of a time string; throws
    /// `DateMalformedIntervalStringException` for parse errors (`Unknown or bad format (%s) at position %d (%c):
    /// %s`) and for strings with a date, time or zone (`String '%s' contains non-relative elements`).
    pub fn create_from_date_string(ctx: &DateContext, datetime: &[u8]) -> PhpResult<ObjectRef> {
        from_date_string(ctx, datetime)
            .map_err(|msg| PhpError::new(errors::date_malformed_interval_string_exception(), msg))
    }

    /// `DateInterval::__set_state($array)` (`php_date_interval_initialize_from_hash()`): `y m d h i s f invert
    /// days` and the relative-time internals, or a `date_string` re-parsed like `createFromDateString()`.
    pub fn set_state(
        ctx: &DateContext,
        data: &Array,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<ObjectRef> {
        let obj = IntervalObject::new_uninitialized(ctx, classes::dateinterval());
        obj.initialize_from_hash(data, diag)?;
        Ok(ObjectRef::new(obj))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn state(&self) -> State {
        self.lock().clone()
    }

    /// The interval, or `None` when the object is not initialised.
    pub fn diff(&self) -> Option<RelTime> {
        let st = self.lock();
        if st.initialized { st.diff } else { None }
    }

    /// The interval and its arithmetic mode, or `DateObjectError` (`DATE_CHECK_INITIALIZED`).
    pub fn initialized_diff(&self) -> PhpResult<(RelTime, i32)> {
        let st = self.lock();
        match (st.initialized, st.diff) {
            (true, Some(d)) => Ok((d, st.civil_or_wall)),
            _ => Err(errors::uninitialized(&self.class)),
        }
    }

    /// Whether the interval came from `createFromDateString()` (`from_string`).
    pub fn is_from_string(&self) -> bool {
        self.lock().from_string
    }

    /// `DateInterval::format()` (`date_interval_format()`).
    pub fn format(&self, format: &[u8]) -> PhpResult<PhpString> {
        let (diff, _) = self.initialized_diff()?;
        Ok(PhpString::from(date_interval_format(format, &diff)))
    }

    /// The value of one of the struct-backed properties (`date_interval_read_property()`), or `None` for other
    /// names.
    fn struct_property(st: &State, name: &[u8]) -> Option<Value> {
        let diff = st.diff.unwrap_or_default();
        let mut value: i64 = -1;
        let mut fvalue: f64 = -1.0;
        match name {
            b"y" => value = diff.y,
            b"m" => value = diff.m,
            b"d" => value = diff.d,
            b"h" => value = diff.h,
            b"i" => value = diff.i,
            b"s" => value = diff.s,
            b"f" => fvalue = diff.us as f64 / 1_000_000.0,
            b"invert" => value = i64::from(diff.invert),
            b"days" => value = diff.days,
            _ => return None,
        }
        Some(if fvalue != -1.0 {
            Value::Float(fvalue)
        } else if value != UNSET {
            Value::Int(value)
        } else {
            Value::Bool(false)
        })
    }

    /// Port of `php_date_interval_initialize_from_hash()`.
    fn initialize_from_hash(&self, data: &Array, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        // If we have a date_string, use that instead
        if let Some(Value::String(date_str)) = data.get_str(b"date_string") {
            let cache = self.ctx.tz_cache();
            let (time, err) = strtotime(
                date_str.as_bytes(),
                self.ctx.tzdb(),
                &mut |name: &str, _db: &TzDb| cache.get(name),
            );
            if err.error_count() > 0 {
                let mut msg = errors::first_error_message(
                    b"Unknown or bad format (",
                    date_str.as_bytes(),
                    &err,
                );
                // "... at position %d (%c) while unserializing: %s"
                let m = &err.error_messages[0];
                let head_len = msg.len() - m.message.len() - 2;
                msg.truncate(head_len);
                msg.extend_from_slice(b" while unserializing: ");
                msg.extend_from_slice(m.message.as_bytes());
                return Err(PhpError::error(msg));
            }
            let mut st = self.lock();
            st.diff = Some(time.relative);
            st.initialized = true;
            st.civil_or_wall = CIVIL;
            st.from_string = true;
            st.date_string = Some(date_str.clone());
            return Ok(());
        }

        let mut diff = RelTime::default();
        let scalar = |v: &Value| !matches!(v, Value::Array(_) | Value::Object(_));
        let read_long = |name: &[u8], def: i64, diag: &mut dyn Diagnostics| -> PhpResult<i64> {
            match data.get_str(name) {
                Some(v) if scalar(v) => convert::to_long(v, diag),
                _ => Ok(def),
            }
        };
        diff.y = read_long(b"y", -1, diag)?;
        diff.m = read_long(b"m", -1, diag)?;
        diff.d = read_long(b"d", -1, diag)?;
        diff.h = read_long(b"h", -1, diag)?;
        diff.i = read_long(b"i", -1, diag)?;
        diff.s = read_long(b"s", -1, diag)?;
        if let Some(v) = data.get_str(b"f") {
            diff.us = convert::dval_to_lval(convert::to_double(v, diag)? * 1_000_000.0, diag)?;
        }
        diff.weekday = read_long(b"weekday", -1, diag)? as i32;
        diff.weekday_behavior = read_long(b"weekday_behavior", -1, diag)? as i32;
        diff.first_last_day_of = read_long(b"first_last_day_of", -1, diag)? as i32;
        diff.invert = read_long(b"invert", 0, diag)? as i32;
        diff.days = match data.get_str(b"days") {
            Some(Value::Bool(false)) => UNSET,
            Some(v) if scalar(v) => strtoll(convert::to_string(v, diag)?.as_bytes()),
            _ => -1,
        };
        diff.special.kind = read_long(b"special_type", 0, diag)? as u32;
        diff.special.amount = match data.get_str(b"special_amount") {
            Some(v) if scalar(v) => strtoll(convert::to_string(v, diag)?.as_bytes()),
            _ => -1,
        };
        diff.have_weekday_relative = read_long(b"have_weekday_relative", 0, diag)? as u32;
        diff.have_special_relative = read_long(b"have_special_relative", 0, diag)? as u32;
        let civil_or_wall = match data.get_str(b"civil_or_wall") {
            Some(v) => convert::to_long(v, diag)? as i32,
            None => CIVIL,
        };

        let mut st = self.lock();
        st.diff = Some(diff);
        st.civil_or_wall = civil_or_wall;
        st.initialized = true;
        Ok(())
    }
}

/// Port of `date_interval_object_to_hash()`.
fn interval_object_to_hash(st: &State) -> Vec<(&'static str, Value)> {
    // Records whether this is a special relative interval that needs to be recreated from a string
    if st.from_string {
        return vec![
            ("from_string", Value::Bool(true)),
            (
                "date_string",
                Value::String(st.date_string.clone().unwrap_or_default()),
            ),
        ];
    }
    let diff = st.diff.unwrap_or_default();
    vec![
        ("y", Value::Int(diff.y)),
        ("m", Value::Int(diff.m)),
        ("d", Value::Int(diff.d)),
        ("h", Value::Int(diff.h)),
        ("i", Value::Int(diff.i)),
        ("s", Value::Int(diff.s)),
        ("f", Value::Float(diff.us as f64 / 1_000_000.0)),
        ("invert", Value::Int(i64::from(diff.invert))),
        (
            "days",
            if diff.days != UNSET {
                Value::Int(diff.days)
            } else {
                Value::Bool(false)
            },
        ),
        ("from_string", Value::Bool(st.from_string)),
    ]
}

/// The shared part of `DateInterval::createFromDateString()` and `date_interval_create_from_date_string()`:
/// the error message on failure.
fn from_date_string(ctx: &DateContext, time_str: &[u8]) -> Result<ObjectRef, PhpString> {
    let cache = ctx.tz_cache();
    let (time, err) = strtotime(time_str, ctx.tzdb(), &mut |name: &str, _db: &TzDb| {
        cache.get(name)
    });
    if err.error_count() > 0 {
        return Err(PhpString::from(errors::first_error_message(
            b"Unknown or bad format (",
            time_str,
            &err,
        )));
    }
    if time.have_date != 0 || time.have_time != 0 || time.have_zone != 0 {
        return Err(PhpString::concat(&[
            b"String '",
            c_str(time_str),
            b"' contains non-relative elements",
        ]));
    }
    // date_interval_instantiate_from_time()
    let obj = IntervalObject::new_uninitialized(ctx, classes::dateinterval());
    {
        let mut st = obj.lock();
        st.diff = Some(time.relative);
        st.initialized = true;
        st.civil_or_wall = CIVIL;
        st.from_string = true;
        st.date_string = Some(PhpString::from(time_str));
    }
    Ok(ObjectRef::new(obj))
}

/// Port of `PHP_FUNCTION(date_interval_create_from_date_string)`: like
/// [`IntervalObject::create_from_date_string`] but failures warn (`date_interval_create_from_date_string(): …`)
/// and return `None` (`false`).
pub fn date_interval_create_from_date_string(
    ctx: &DateContext,
    datetime: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<ObjectRef>> {
    match from_date_string(ctx, datetime) {
        Ok(o) => Ok(Some(o)),
        Err(msg) => {
            diag.warn(PhpString::concat(&[
                b"date_interval_create_from_date_string(): ",
                msg.as_bytes(),
            ]))?;
            Ok(None)
        }
    }
}

impl Object for IntervalObject {
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
        match method {
            "__construct" => {
                let a = Bound::new(
                    "DateInterval::__construct",
                    &args,
                    &[Param::required("duration")],
                )?;
                let s = a.string(0, diag)?;
                let rt = date_interval_initialize(s.as_bytes())?;
                let mut st = self.lock();
                st.diff = Some(rt);
                st.initialized = true;
                st.civil_or_wall = WALL;
                Ok(Value::Null)
            }
            "createfromdatestring" => {
                let a = Bound::new(
                    "DateInterval::createFromDateString",
                    &args,
                    &[Param::required("datetime")],
                )?;
                let s = a.string(0, diag)?;
                Ok(Value::Object(IntervalObject::create_from_date_string(
                    &self.ctx,
                    s.as_bytes(),
                )?))
            }
            "format" => {
                let a = Bound::new("DateInterval::format", &args, &[Param::required("format")])?;
                let f = a.string(0, diag)?;
                Ok(Value::String(self.format(f.as_bytes())?))
            }
            "__serialize" => {
                args::none("DateInterval::__serialize", &args)?;
                let st = self.state();
                if !st.initialized {
                    return Err(errors::uninitialized(&self.class));
                }
                let mut out = util::assoc(interval_object_to_hash(&st));
                util::add_common_properties(&mut out, this, diag)?;
                Ok(Value::Array(out))
            }
            "__unserialize" => {
                let a = Bound::new(
                    "DateInterval::__unserialize",
                    &args,
                    &[Param::required("data")],
                )?;
                let data = a.array(0)?;
                self.initialize_from_hash(&data, diag)?;
                util::restore_custom_properties(this, &data, is_internal_property, diag)?;
                Ok(Value::Null)
            }
            "__wakeup" => {
                util::wakeup_deprecated("DateInterval", diag)?;
                args::none("DateInterval::__wakeup", &args)?;
                // Z_OBJPROP_P() goes through the get_properties handler, which writes the fields first.
                let table = util::assoc(
                    self.properties_for(this, PropertyPurpose::GetObjectVars, diag)?
                        .into_iter()
                        .filter_map(|e| e.value.map(|v| (e.key, v))),
                );
                self.initialize_from_hash(&table, diag)?;
                Ok(Value::Null)
            }
            "__set_state" => {
                let a = Bound::new(
                    "DateInterval::__set_state",
                    &args,
                    &[Param::required("array")],
                )?;
                let data = a.array(0)?;
                Ok(Value::Object(IntervalObject::set_state(
                    &self.ctx, &data, diag,
                )?))
            }
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }

    /// Port of `date_interval_read_property()`.
    fn read_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let st = self.state();
        if st.initialized
            && let Some(v) = IntervalObject::struct_property(&st, name)
        {
            return Ok(v);
        }
        util::std_read_property(this, name, mode, diag)
    }

    /// Port of `date_interval_has_property()`.
    fn has_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        check: PropertyCheck,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        let st = self.state();
        if st.initialized
            && let Some(v) = IntervalObject::struct_property(&st, name)
        {
            return match check {
                PropertyCheck::Exists => Ok(true),
                PropertyCheck::NotEmpty => convert::to_bool(&v, diag),
                PropertyCheck::Isset => Ok(!v.is_null()),
            };
        }
        util::std_has_property(this, name, check, diag)
    }

    /// Port of `date_interval_write_property()`.
    fn write_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        if !self.lock().initialized {
            return util::std_write_property(this, name, value, diag);
        }
        match name {
            b"y" | b"m" | b"d" | b"h" | b"i" | b"s" | b"invert" => {
                let v = convert::to_long(&value, diag)?;
                let mut st = self.lock();
                let diff = st.diff.get_or_insert_with(RelTime::default);
                match name {
                    b"y" => diff.y = v,
                    b"m" => diff.m = v,
                    b"d" => diff.d = v,
                    b"h" => diff.h = v,
                    b"i" => diff.i = v,
                    b"s" => diff.s = v,
                    _ => diff.invert = v as i32,
                }
                Ok(())
            }
            b"f" => {
                let us =
                    convert::dval_to_lval(convert::to_double(&value, diag)? * 1_000_000.0, diag)?;
                let mut st = self.lock();
                st.diff.get_or_insert_with(RelTime::default).us = us;
                Ok(())
            }
            _ => util::std_write_property(this, name, value, diag),
        }
    }

    /// Port of `date_object_get_properties_interval()`: like PHP, the interval fields are written into the
    /// object's own property table (where later plain property reads of `from_string` / `date_string` find
    /// them) and that table is returned for every purpose.
    fn properties_for(
        &self,
        this: &ObjectRef,
        _purpose: PropertyPurpose,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        let st = self.state();
        if st.initialized {
            for (k, v) in interval_object_to_hash(&st) {
                self.props.set(k.as_bytes(), v);
            }
        }
        util::std_properties(this, diag)
    }

    /// Port of `date_object_clone_interval()`.
    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(IntervalObject {
            class: self.class.clone(),
            ctx: self.ctx.clone(),
            state: Mutex::new(self.state()),
            props: self.props.clone(),
        }))
    }

    /// Port of `date_interval_compare_objects()`: intervals are not comparable (`Cannot compare DateInterval
    /// objects`, an `E_WARNING`).
    fn compare(
        &self,
        _this: &ObjectRef,
        other: &Value,
        _this_is_left: bool,
        compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        // ZEND_COMPARE_OBJECTS_FALLBACK()
        let Value::Object(o) = other else {
            return None;
        };
        o.downcast_ref::<IntervalObject>()?;
        Some(
            util::warn_from_compare(compare_values, "Cannot compare DateInterval objects")
                .map(|()| 1),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtoll_like_c() {
        assert_eq!(strtoll(b"  -12abc"), -12);
        assert_eq!(strtoll(b"+7"), 7);
        assert_eq!(strtoll(b"x"), 0);
        assert_eq!(strtoll(b"99999999999999999999"), i64::MAX);
        assert_eq!(strtoll(b"-99999999999999999999"), i64::MIN);
        assert_eq!(strtoll(b"5.5"), 5);
    }

    #[test]
    fn construction_errors() {
        let e = date_interval_initialize(b"P1X").unwrap_err();
        assert_eq!(e.message(), "Unknown or bad format (P1X)");
        assert_eq!(e.class().name(), "DateMalformedIntervalStringException");
        let rt = date_interval_initialize(b"P1Y2M3DT4H5M6S").unwrap();
        assert_eq!((rt.y, rt.m, rt.d, rt.h, rt.i, rt.s), (1, 2, 3, 4, 5, 6));
    }
}
