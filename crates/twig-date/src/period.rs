//! `DatePeriod` (`php_period_obj`): construction from start/interval/end-or-recurrences or an ISO 8601 period,
//! iteration (`date_period_it_*`), the read-only virtual properties and (un)serialisation
//! (PHPSRC/ext/date/php_date.c). `getIterator()` returns an `InternalIterator` (PHPSRC/Zend/zend_interfaces.c).

use std::sync::{LazyLock, Mutex, MutexGuard};

use twig_timelib::parse_iso_intervals::strtointerval;
use twig_timelib::{RelTime, Time};
use twig_value::object::builtin::iterator;
use twig_value::object::{
    CallArgs, ClassBuilder, ClassRef, FetchMode, MethodParams, Object, ObjectIterator, ObjectRef,
    Param, PropertyCheck, PropertyEntry, PropertyPurpose, PropertyStore, Visibility,
};
use twig_value::param::{self, Arg};
use twig_value::{Array, Diagnostics, PhpError, PhpResult, PhpString, Value, convert};

use crate::args::{self, Bound};
use crate::classes::{self, PERIOD_EXCLUDE_START_DATE, PERIOD_INCLUDE_END_DATE};
use crate::context::{DateContext, c_str};
use crate::datetime::DateObject;
use crate::errors;
use crate::interval::IntervalObject;
use crate::util;

#[derive(Clone, Default)]
struct State {
    start: Option<Time>,
    start_ce: Option<ClassRef>,
    current: Option<Time>,
    end: Option<Time>,
    interval: Option<RelTime>,
    recurrences: i32,
    initialized: bool,
    include_start_date: bool,
    include_end_date: bool,
}

/// A `DatePeriod` object.
pub struct DatePeriodObject {
    class: ClassRef,
    ctx: DateContext,
    state: Mutex<State>,
    props: PropertyStore,
}

/// `date_period_is_internal_property()`.
fn is_internal_property(name: &[u8]) -> bool {
    matches!(
        name,
        b"start"
            | b"current"
            | b"end"
            | b"interval"
            | b"recurrences"
            | b"include_start_date"
            | b"include_end_date"
    )
}

/// Port of `get_base_date_class()`: `DateTime` or `DateTimeImmutable` for a class extending one of them.
fn base_date_class(ce: &ClassRef) -> ClassRef {
    if ce.is_subclass_of("DateTimeImmutable") {
        classes::datetime_immutable()
    } else {
        classes::datetime()
    }
}

/// Port of `date_period_advance()`.
fn advance(it_time: &mut Time, interval: &RelTime) {
    it_time.have_relative = 1;
    it_time.relative = *interval;
    it_time.sse_uptodate = 0;
    it_time.update_ts(None);
    it_time.update_from_sse();
}

/// Port of `create_date_period_datetime()`: a new object of `ce` holding a copy of `datetime`, or `null`.
fn period_datetime(ctx: &DateContext, datetime: Option<&Time>, ce: Option<&ClassRef>) -> Value {
    match (datetime, ce) {
        (Some(t), Some(ce)) => Value::Object(DateObject::from_time(ctx, ce.clone(), t.clone())),
        (Some(t), None) => {
            Value::Object(DateObject::from_time(ctx, classes::datetime(), t.clone()))
        }
        _ => Value::Null,
    }
}

/// Port of `create_date_period_interval()`: a new `DateInterval` (with the zero arithmetic mode of a fresh
/// object), or `null`.
fn period_interval(ctx: &DateContext, interval: Option<&RelTime>) -> Value {
    match interval {
        Some(rt) => Value::Object(IntervalObject::from_reltime(ctx, *rt, 0)),
        None => Value::Null,
    }
}

impl DatePeriodObject {
    /// An uninitialised object of `class` (`date_object_new_period()`).
    pub fn new_uninitialized(ctx: &DateContext, class: ClassRef) -> DatePeriodObject {
        let props = PropertyStore::new(&class);
        DatePeriodObject {
            class,
            ctx: ctx.clone(),
            state: Mutex::new(State::default()),
            props,
        }
    }

    /// `DatePeriod::__set_state($array)` (`php_date_period_initialize_from_hash()`); invalid data throws
    /// `Error: Invalid serialization data for DatePeriod object`.
    pub fn set_state(ctx: &DateContext, data: &Array) -> PhpResult<ObjectRef> {
        let obj = DatePeriodObject::new_uninitialized(ctx, classes::dateperiod());
        if !obj.initialize_from_hash(data) {
            return Err(errors::invalid_serialization("DatePeriod"));
        }
        Ok(ObjectRef::new(obj))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn state(&self) -> State {
        self.lock().clone()
    }

    /// `new DatePeriod($start, $interval, $recurrences, $options)`: `start` must be a date object and
    /// `interval` a `DateInterval`.
    pub fn with_recurrences(
        ctx: &DateContext,
        start: &ObjectRef,
        interval: &ObjectRef,
        recurrences: i64,
        options: i64,
    ) -> PhpResult<ObjectRef> {
        let obj = DatePeriodObject::new_uninitialized(ctx, classes::dateperiod());
        obj.construct_from(start, interval, None, recurrences, options)?;
        Ok(ObjectRef::new(obj))
    }

    /// `new DatePeriod($start, $interval, $end, $options)`.
    pub fn with_end(
        ctx: &DateContext,
        start: &ObjectRef,
        interval: &ObjectRef,
        end: &ObjectRef,
        options: i64,
    ) -> PhpResult<ObjectRef> {
        let obj = DatePeriodObject::new_uninitialized(ctx, classes::dateperiod());
        obj.construct_from(start, interval, Some(end), 0, options)?;
        Ok(ObjectRef::new(obj))
    }

    /// `DatePeriod::createFromISO8601String($specification, $options)`.
    pub fn create_from_iso8601_string(
        ctx: &DateContext,
        specification: &[u8],
        options: i64,
    ) -> PhpResult<ObjectRef> {
        let obj = DatePeriodObject::new_uninitialized(ctx, classes::dateperiod());
        let func = "DatePeriod::createFromISO8601String";
        let recurrences =
            obj.init_iso8601_string(classes::datetime_immutable(), specification, func)?;
        obj.init_finish(options, recurrences, func)?;
        Ok(ObjectRef::new(obj))
    }

    /// The `(DateTimeInterface, DateInterval, int|DateTimeInterface [, int])` forms of the constructor.
    fn construct_from(
        &self,
        start: &ObjectRef,
        interval: &ObjectRef,
        end: Option<&ObjectRef>,
        recurrences: i64,
        options: i64,
    ) -> PhpResult<()> {
        self.reset();
        // check initialisation
        let interface = classes::datetime_interface();
        let start_time = start
            .downcast_ref::<DateObject>()
            .and_then(DateObject::time)
            .ok_or_else(|| errors::uninitialized(&interface))?;
        let end_time = match end {
            Some(e) => Some(
                e.downcast_ref::<DateObject>()
                    .and_then(DateObject::time)
                    .ok_or_else(|| errors::uninitialized(&interface))?,
            ),
            None => None,
        };
        let diff = interval
            .downcast_ref::<IntervalObject>()
            .and_then(IntervalObject::diff)
            .ok_or_else(|| errors::uninitialized(interval.class()))?;
        {
            let mut st = self.lock();
            st.start = Some(start_time);
            st.start_ce = Some(start.class().clone());
            st.interval = Some(diff);
            st.end = end_time;
        }
        self.init_finish(options, recurrences, "DatePeriod::__construct")
    }

    /// `date_period_reset()`.
    fn reset(&self) {
        *self.lock() = State::default();
    }

    /// Port of `date_period_initialize()` + `date_period_init_iso8601_string()`; returns the recurrence count.
    fn init_iso8601_string(&self, base_ce: ClassRef, isostr: &[u8], func: &str) -> PhpResult<i64> {
        let r = strtointerval(isostr);
        let shown = c_str(isostr);
        if r.errors.error_count() > 0 {
            return Err(errors::throw(
                errors::date_malformed_period_string_exception(),
                PhpString::concat(&[b"Unknown or bad format (", shown, b")"]),
            ));
        }
        let recurrences = i64::from(r.recurrences.unwrap_or(0));
        let fail = |what: &str| {
            errors::throw(
                errors::date_malformed_period_string_exception(),
                PhpString::concat(&[
                    format!("{func}(): ISO interval must contain {what}, \"").as_bytes(),
                    shown,
                    b"\" given",
                ]),
            )
        };
        let mut st = self.lock();
        st.start = r.begin;
        st.end = r.end;
        st.interval = r.period;
        if st.start.is_none() {
            return Err(fail("a start date"));
        }
        if st.interval.is_none() {
            return Err(fail("an interval"));
        }
        if st.end.is_none() && recurrences == 0 {
            return Err(fail("an end date or a recurrence count"));
        }
        if let Some(s) = st.start.as_mut() {
            s.update_ts(None);
        }
        if let Some(e) = st.end.as_mut() {
            e.update_ts(None);
        }
        st.start_ce = Some(base_ce);
        Ok(recurrences)
    }

    /// Port of `date_period_init_finish()`.
    fn init_finish(&self, options: i64, recurrences: i64, func: &str) -> PhpResult<()> {
        let max_recurrences = i64::from(i32::MAX - 8);
        let mut st = self.lock();
        if st.end.is_none() && (recurrences < 1 || recurrences > max_recurrences) {
            return Err(errors::throw(
                errors::date_malformed_period_string_exception(),
                format!(
                    "{func}(): Recurrence count must be greater or equal to 1 and lower than {}",
                    max_recurrences + 1
                ),
            ));
        }

        // options
        st.include_start_date = options & PERIOD_EXCLUDE_START_DATE == 0;
        st.include_end_date = options & PERIOD_INCLUDE_END_DATE != 0;

        // recurrences
        let recurrences =
            recurrences + i64::from(st.include_start_date) + i64::from(st.include_end_date);

        if recurrences > max_recurrences {
            return Err(errors::throw(
                errors::date_malformed_string_exception(),
                format!(
                    "{func}(): Recurrence count must be greater or equal to 1 and lower than {} (including options)",
                    max_recurrences + 1
                ),
            ));
        }

        st.recurrences = recurrences as i32;
        st.initialized = true;
        Ok(())
    }

    /// `getRecurrences()`: the count without the start/end inclusions, `None` when it is 0.
    pub fn recurrences(&self) -> Option<i64> {
        let st = self.lock();
        let n = i64::from(st.recurrences)
            - i64::from(st.include_start_date)
            - i64::from(st.include_end_date);
        (n != 0).then_some(n)
    }

    /// Port of `date_period_object_to_hash()`.
    fn object_to_hash(&self, st: &State, props: &mut Vec<PropertyEntry>) {
        let ce = st.start_ce.as_ref();
        util::update_prop(
            props,
            "start",
            period_datetime(&self.ctx, st.start.as_ref(), ce),
        );
        util::update_prop(
            props,
            "current",
            period_datetime(&self.ctx, st.current.as_ref(), ce),
        );
        util::update_prop(
            props,
            "end",
            period_datetime(&self.ctx, st.end.as_ref(), ce),
        );
        util::update_prop(
            props,
            "interval",
            period_interval(&self.ctx, st.interval.as_ref()),
        );
        util::update_prop(props, "recurrences", Value::Int(i64::from(st.recurrences)));
        util::update_prop(
            props,
            "include_start_date",
            Value::Bool(st.include_start_date),
        );
        util::update_prop(props, "include_end_date", Value::Bool(st.include_end_date));
    }

    /// The value of an internal property (`date_period_read_property()` for `BP_VAR_R` / `BP_VAR_IS`).
    fn internal_property(&self, name: &[u8]) -> Value {
        let st = self.state();
        let ce = st.start_ce.as_ref();
        match name {
            b"start" => period_datetime(&self.ctx, st.start.as_ref(), ce),
            b"current" => period_datetime(&self.ctx, st.current.as_ref(), ce),
            b"end" => period_datetime(&self.ctx, st.end.as_ref(), ce),
            b"interval" => period_interval(&self.ctx, st.interval.as_ref()),
            b"recurrences" => Value::Int(i64::from(st.recurrences)),
            b"include_start_date" => Value::Bool(st.include_start_date),
            _ => Value::Bool(st.include_end_date),
        }
    }

    /// Port of `php_date_period_initialize_from_hash()` (no rollback on error, as in PHP).
    fn initialize_from_hash(&self, data: &Array) -> bool {
        let date_time = |v: &Value| -> Option<Option<(Time, ClassRef)>> {
            match v {
                Value::Object(o) if o.instance_of("DateTimeInterface") => {
                    let t = o.downcast_ref::<DateObject>().and_then(DateObject::time)?;
                    Some(Some((t, o.class().clone())))
                }
                Value::Null => Some(None),
                _ => None,
            }
        };
        let mut st = self.lock();
        let Some(v) = data.get_str(b"start") else {
            return false;
        };
        match date_time(v) {
            Some(Some((t, ce))) => {
                st.start = Some(t);
                st.start_ce = Some(ce);
            }
            Some(None) => {}
            None => return false,
        }
        for key in [b"end".as_slice(), b"current".as_slice()] {
            let Some(v) = data.get_str(key) else {
                return false;
            };
            match date_time(v) {
                Some(Some((t, _))) => {
                    if st.start_ce.is_none() {
                        return false;
                    }
                    if key == b"end" {
                        st.end = Some(t);
                    } else {
                        st.current = Some(t);
                    }
                }
                Some(None) => {}
                None => return false,
            }
        }
        match data.get_str(b"interval") {
            Some(Value::Object(o)) if o.class().name() == "DateInterval" => {
                match o
                    .downcast_ref::<IntervalObject>()
                    .and_then(IntervalObject::diff)
                {
                    Some(d) => st.interval = Some(d),
                    None => return false,
                }
            }
            // interval is required
            _ => return false,
        }
        match data.get_str(b"recurrences") {
            Some(Value::Int(r)) if (0..=i64::from(i32::MAX)).contains(r) => {
                st.recurrences = *r as i32;
            }
            _ => return false,
        }
        match data.get_str(b"include_start_date") {
            Some(Value::Bool(b)) => st.include_start_date = *b,
            _ => return false,
        }
        match data.get_str(b"include_end_date") {
            Some(Value::Bool(b)) => st.include_end_date = *b,
            _ => return false,
        }
        st.initialized = true;
        true
    }

    /// The `foreach` iterator (`date_object_period_get_iterator()`).
    fn iterator(this: &ObjectRef) -> PeriodIterator {
        PeriodIterator {
            object: this.clone(),
            current_index: 0,
            current: None,
        }
    }

    /// Resolves the `__construct()` overloads (`zend_parse_parameters_ex(ZEND_PARSE_PARAMS_QUIET, …)` with
    /// `"OOl|l"`, `"OOO|l"`, `"s|l"`).
    fn construct_call(&self, args: &CallArgs, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        let mut vals: Vec<Option<Value>> = args.positional.iter().cloned().map(Some).collect();
        for (name, v) in &args.named {
            let Some(pos) = CONSTRUCT_PARAMS
                .iter()
                .position(|n| n.as_bytes() == name.as_bytes())
            else {
                return Err(PhpError::error(PhpString::concat(&[
                    b"Unknown named parameter $",
                    name.as_bytes(),
                ])));
            };
            if vals.len() <= pos {
                vals.resize(pos + 1, None);
            }
            if vals[pos].is_some() {
                return Err(PhpError::error(PhpString::concat(&[
                    b"Named parameter $",
                    name.as_bytes(),
                    b" overwrites previous argument",
                ])));
            }
            vals[pos] = Some(v.clone());
        }
        if let Some(i) = vals.iter().position(Option::is_none) {
            return Err(PhpError::argument_count_error(format!(
                "DatePeriod::__construct(): Argument #{} (${}) must be passed explicitly, because the default value is not known",
                i + 1,
                CONSTRUCT_PARAMS[i]
            )));
        }
        let vals: Vec<Value> = vals.into_iter().flatten().collect();
        let func = "DatePeriod::__construct";
        let quiet_long = |v: &Value, i: u32, diag: &mut dyn Diagnostics| {
            let mut sink: Vec<twig_value::Diagnostic> = Vec::new();
            match param::parse_long(
                v,
                &Arg::new(func, i, CONSTRUCT_PARAMS[i as usize - 1]),
                &mut sink,
            ) {
                Ok(l) => {
                    for d in sink {
                        diag.report(d)?;
                    }
                    Ok(Some(l))
                }
                Err(_) => Ok::<_, PhpError>(None),
            }
        };
        let obj = |v: &Value, class: &str| match v {
            Value::Object(o) if o.instance_of(class) => Some(o.clone()),
            _ => None,
        };
        let n = vals.len();
        if (3..=4).contains(&n)
            && let (Some(start), Some(interval)) = (
                obj(&vals[0], "DateTimeInterface"),
                obj(&vals[1], "DateInterval"),
            )
        {
            // "OOl|l"
            if let Some(recurrences) = quiet_long(&vals[2], 3, diag)? {
                let options = match vals.get(3) {
                    Some(v) => quiet_long(v, 4, diag)?,
                    None => Some(0),
                };
                if let Some(options) = options {
                    return self.construct_from(&start, &interval, None, recurrences, options);
                }
            }
            // "OOO|l"
            if let Some(end) = obj(&vals[2], "DateTimeInterface") {
                let options = match vals.get(3) {
                    Some(v) => quiet_long(v, 4, diag)?,
                    None => Some(0),
                };
                if let Some(options) = options {
                    return self.construct_from(&start, &interval, Some(&end), 0, options);
                }
            }
        }
        if (1..=2).contains(&n) && !matches!(vals[0], Value::Array(_)) {
            // "s|l"
            let mut sink: Vec<twig_value::Diagnostic> = Vec::new();
            if let Ok(isostr) = param::parse_str(&vals[0], &Arg::new(func, 1, "start"), &mut sink) {
                let options = match vals.get(1) {
                    Some(v) => quiet_long(v, 2, diag)?,
                    None => Some(0),
                };
                if let Some(options) = options {
                    for d in sink {
                        diag.report(d)?;
                    }
                    self.reset();
                    diag.deprecated(
                        "Calling DatePeriod::__construct(string $isostr, int $options = 0) is deprecated, use DatePeriod::createFromISO8601String() instead",
                    )?;
                    let recurrences =
                        self.init_iso8601_string(classes::datetime(), isostr.as_bytes(), func)?;
                    return self.init_finish(options, recurrences, func);
                }
            }
        }
        Err(PhpError::type_error(
            "DatePeriod::__construct() accepts (DateTimeInterface, DateInterval, int [, int]), or (DateTimeInterface, DateInterval, DateTime [, int]), or (string [, int]) as arguments",
        ))
    }
}

/// The `foreach` iterator over a `DatePeriod` (`date_period_it`).
pub struct PeriodIterator {
    object: ObjectRef,
    current_index: i64,
    current: Option<Value>,
}

impl PeriodIterator {
    fn period(&self) -> &DatePeriodObject {
        self.object
            .downcast_ref::<DatePeriodObject>()
            .expect("DatePeriod iterator over a DatePeriod")
    }
}

impl ObjectIterator for PeriodIterator {
    /// Port of `date_period_it_rewind()`.
    fn rewind(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.current_index = 0;
        let p = self.period();
        let mut st = p.lock();
        st.current = None;
        let Some(start) = st.start.clone() else {
            return Err(errors::uninitialized(&classes::dateperiod()));
        };
        let mut current = start;
        if !st.include_start_date
            && let Some(interval) = st.interval
        {
            advance(&mut current, &interval);
        }
        st.current = Some(current);
        drop(st);
        self.current = None;
        Ok(())
    }

    /// Port of `date_period_it_has_more()`.
    fn valid(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        let index = self.current_index;
        let st = self.period().lock();
        let Some(current) = st.current.as_ref() else {
            return Ok(false);
        };
        Ok(match st.end.as_ref() {
            Some(end) => {
                if current.sse == end.sse {
                    if st.include_end_date {
                        current.us <= end.us
                    } else {
                        current.us < end.us
                    }
                } else {
                    current.sse < end.sse
                }
            }
            None => index < i64::from(st.recurrences),
        })
    }

    /// Port of `date_period_it_current_data()`: a new `DateTime` / `DateTimeImmutable` (the base class of the
    /// start date's class).
    fn current(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        let p = self.period();
        let st = p.lock();
        let ce = st
            .start_ce
            .as_ref()
            .map_or_else(classes::datetime, base_date_class);
        let v = match st.current.clone() {
            Some(t) => Value::Object(DateObject::from_time(&p.ctx, ce, t)),
            None => Value::Null,
        };
        drop(st);
        self.current = Some(v.clone());
        Ok(v)
    }

    /// Port of `date_period_it_current_key()`.
    fn key(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        Ok(Value::Int(self.current_index))
    }

    /// Port of `date_period_it_move_forward()`.
    fn next(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<()> {
        {
            let p = self.period();
            let mut st = p.lock();
            let interval = st.interval;
            if let (Some(current), Some(interval)) = (st.current.as_mut(), interval) {
                advance(current, &interval);
            }
        }
        self.current_index += 1;
        self.current = None;
        Ok(())
    }
}

/// The parameters of `DatePeriod::__construct($start, $interval = UNKNOWN, $end = UNKNOWN, $options = UNKNOWN)`.
const CONSTRUCT_PARAMS: [&str; 4] = ["start", "interval", "end", "options"];

/// The parameters of the methods of `DatePeriod` (by lowercase name), as PHPSRC/ext/date/php_date.stub.php declares
/// them: what [`DatePeriodObject`] reports and binds its arguments to (`args::ParamTable`).
///
/// The constructor's `$interval`, `$end` and `$options` are optional without a known default (`UNKNOWN`), which a
/// [`Param`] cannot express, so they are listed without a default: the engine only uses the names (it checks named
/// arguments against them as they are sent), and `construct_call()` binds the constructor's arguments itself.
fn period_params(method: &str) -> Option<Vec<Param>> {
    Some(match method {
        // static createFromISO8601String(string $specification, int $options = 0)
        "createfromiso8601string" => vec![
            Param::required("specification"),
            Param::optional("options", 0),
        ],
        "__construct" => CONSTRUCT_PARAMS.into_iter().map(Param::required).collect(),
        // __unserialize(array $data)
        "__unserialize" => vec![Param::required("data")],
        // static __set_state(array $array)
        "__set_state" => vec![Param::required("array")],
        "getstartdate" | "getenddate" | "getdateinterval" | "getrecurrences" | "__serialize"
        | "__wakeup" | "getiterator" => Vec::new(),
        _ => return None,
    })
}

impl Object for DatePeriodObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    /// The parameters of the method as php_date.stub.php declares them (`period_params`).
    fn method_params(&self, method: &str) -> Option<MethodParams> {
        args::method_params(period_params, method)
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "createfromiso8601string" => {
                let func = "DatePeriod::createFromISO8601String";
                let a = Bound::method(func, method, period_params, &args)?;
                let spec = a.string(0, diag)?;
                let options = a.long(1, diag)?;
                // object_init_ex(return_value, execute_data->This.value.ce): the called class.
                let obj = DatePeriodObject::new_uninitialized(&self.ctx, self.class.clone());
                let recurrences =
                    obj.init_iso8601_string(classes::datetime_immutable(), spec.as_bytes(), func)?;
                obj.init_finish(options, recurrences, func)?;
                Ok(Value::Object(ObjectRef::new(obj)))
            }
            "__construct" => {
                self.construct_call(&args, diag)?;
                Ok(Value::Null)
            }
            "getstartdate" => {
                Bound::method("DatePeriod::getStartDate", method, period_params, &args)?;
                let st = self.state();
                let Some(start) = st.start else {
                    return Err(errors::uninitialized(&self.class));
                };
                let ce = st.start_ce.unwrap_or_else(classes::datetime);
                Ok(Value::Object(DateObject::from_time(&self.ctx, ce, start)))
            }
            "getenddate" => {
                Bound::method("DatePeriod::getEndDate", method, period_params, &args)?;
                let st = self.state();
                Ok(match st.end {
                    Some(end) => {
                        let ce = st.start_ce.unwrap_or_else(classes::datetime);
                        Value::Object(DateObject::from_time(&self.ctx, ce, end))
                    }
                    None => Value::Null,
                })
            }
            "getdateinterval" => {
                Bound::method("DatePeriod::getDateInterval", method, period_params, &args)?;
                match self.state().interval {
                    Some(rt) => Ok(Value::Object(IntervalObject::from_reltime(
                        &self.ctx, rt, 0,
                    ))),
                    None => Err(errors::uninitialized(&self.class)),
                }
            }
            "getrecurrences" => {
                Bound::method("DatePeriod::getRecurrences", method, period_params, &args)?;
                Ok(self.recurrences().map_or(Value::Null, Value::Int))
            }
            "getiterator" => {
                Bound::method("DatePeriod::getIterator", method, period_params, &args)?;
                Ok(Value::Object(InternalIterator::wrap(Box::new(
                    DatePeriodObject::iterator(this),
                ))))
            }
            "__serialize" => {
                Bound::method("DatePeriod::__serialize", method, period_params, &args)?;
                let st = self.state();
                if st.start.is_none() {
                    return Err(errors::uninitialized(&self.class));
                }
                let mut props = Vec::new();
                self.object_to_hash(&st, &mut props);
                let mut out = util::assoc(
                    props
                        .into_iter()
                        .filter_map(|e| e.value.map(|v| (e.key, v))),
                );
                util::add_common_properties(&mut out, this, diag)?;
                Ok(Value::Array(out))
            }
            "__unserialize" => {
                let a = Bound::method("DatePeriod::__unserialize", method, period_params, &args)?;
                let data = a.array(0)?;
                if !self.initialize_from_hash(&data) {
                    return Err(errors::invalid_serialization("DatePeriod"));
                }
                util::restore_custom_properties(this, &data, is_internal_property, diag)?;
                Ok(Value::Null)
            }
            "__wakeup" => {
                util::wakeup_deprecated("DatePeriod", diag)?;
                Bound::method("DatePeriod::__wakeup", method, period_params, &args)?;
                let table = util::assoc(
                    util::std_properties(this, diag)?
                        .into_iter()
                        .filter_map(|e| e.value.map(|v| (e.key, v))),
                );
                if !self.initialize_from_hash(&table) {
                    return Err(errors::invalid_serialization("DatePeriod"));
                }
                util::restore_custom_properties(this, &table, is_internal_property, diag)?;
                Ok(Value::Null)
            }
            "__set_state" => {
                let a = Bound::method("DatePeriod::__set_state", method, period_params, &args)?;
                let data = a.array(0)?;
                Ok(Value::Object(DatePeriodObject::set_state(
                    &self.ctx, &data,
                )?))
            }
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }

    /// Port of `date_period_read_property()`.
    fn read_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        if is_internal_property(name) {
            return Ok(self.internal_property(name));
        }
        util::std_read_property(this, name, mode, diag)
    }

    /// Port of `date_period_has_property()`.
    fn has_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        check: PropertyCheck,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        if !is_internal_property(name) {
            return util::std_has_property(this, name, check, diag);
        }
        if check == PropertyCheck::Exists {
            return Ok(true);
        }
        if !self.lock().initialized {
            return Ok(false);
        }
        let v = self.internal_property(name);
        match check {
            PropertyCheck::NotEmpty => convert::to_bool(&v, diag),
            _ => Ok(!v.is_null()),
        }
    }

    /// Port of `date_period_write_property()`: the internal properties are read-only.
    fn write_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        if is_internal_property(name) {
            return Err(PhpError::error(PhpString::concat(&[
                b"Cannot modify readonly property DatePeriod::$",
                name,
            ])));
        }
        util::std_write_property(this, name, value, diag)
    }

    /// Port of `date_period_unset_property()`.
    fn unset_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        if is_internal_property(name) {
            return Err(PhpError::error(PhpString::concat(&[
                b"Cannot unset ",
                self.class.name().as_bytes(),
                b"::$",
                name,
            ])));
        }
        STD_UNSET.unset_property(this, name, diag)
    }

    /// Port of `date_period_get_properties_for()`.
    fn properties_for(
        &self,
        this: &ObjectRef,
        _purpose: PropertyPurpose,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        let mut props = util::std_properties(this, diag)?;
        let st = self.state();
        if st.initialized {
            self.object_to_hash(&st, &mut props);
        }
        Ok(props)
    }

    /// `foreach ($period as $k => $date)`.
    fn get_iterator(
        &self,
        this: &ObjectRef,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Box<dyn ObjectIterator>>> {
        Ok(Some(Box::new(DatePeriodObject::iterator(this))))
    }

    /// Port of `date_object_clone_period()`.
    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(DatePeriodObject {
            class: self.class.clone(),
            ctx: self.ctx.clone(),
            state: Mutex::new(self.state()),
            props: self.props.clone(),
        }))
    }
}

/// The standard `unset_property` handler (the default of [`Object`]).
struct StdUnset(ClassRef);

impl Object for StdUnset {
    fn class(&self) -> &ClassRef {
        &self.0
    }
}

static STD_UNSET: LazyLock<StdUnset> =
    LazyLock::new(|| StdUnset(twig_value::object::builtin::std_class()));

/// `final class InternalIterator implements Iterator` (PHPSRC/Zend/zend_interfaces.c): what
/// `IteratorAggregate::getIterator()` of internal classes returns.
pub struct InternalIterator {
    class: ClassRef,
    state: Mutex<InternalState>,
}

struct InternalState {
    iter: Box<dyn ObjectIterator>,
    rewind_called: bool,
    index: i64,
}

static INTERNAL_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("InternalIterator")
        .final_class()
        .implements(&iterator())
        .method_with("__construct", Visibility::Private, false)
        .method("current")
        .method("key")
        .method("next")
        .method("valid")
        .method("rewind")
        .build()
});

/// The `InternalIterator` class entry.
pub fn internal_iterator_class() -> ClassRef {
    INTERNAL_ITERATOR.clone()
}

impl InternalIterator {
    /// Wraps an object iterator into an `InternalIterator` object (`zend_create_internal_iterator_zval()`).
    pub fn wrap(iter: Box<dyn ObjectIterator>) -> ObjectRef {
        ObjectRef::new(InternalIterator {
            class: internal_iterator_class(),
            state: Mutex::new(InternalState {
                iter,
                rewind_called: false,
                index: 0,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, InternalState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `zend_internal_iterator_ensure_rewound()`.
fn ensure_rewound(st: &mut InternalState, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    if !st.rewind_called {
        st.rewind_called = true;
        st.iter.rewind(diag)?;
    }
    Ok(())
}

/// The parameters of the public methods of `InternalIterator` (PHPSRC/Zend/zend_interfaces.stub.php), by lowercase
/// name: `current()`, `key()`, `next()`, `valid()` and `rewind()` take none.
fn internal_iterator_params(method: &str) -> Option<Vec<Param>> {
    matches!(method, "current" | "key" | "next" | "valid" | "rewind").then(Vec::new)
}

impl Object for InternalIterator {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    /// The parameters of the method (`internal_iterator_params`).
    fn method_params(&self, method: &str) -> Option<MethodParams> {
        args::method_params(internal_iterator_params, method)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let name = format!(
            "InternalIterator::{}",
            match method {
                "current" => "current",
                "key" => "key",
                "next" => "next",
                "valid" => "valid",
                "rewind" => "rewind",
                _ => return Err(twig_value::object::undefined_method(&self.class, method)),
            }
        );
        Bound::method(&name, method, internal_iterator_params, &args)?;
        let mut st = self.lock();
        match method {
            "current" => {
                ensure_rewound(&mut st, diag)?;
                st.iter.current(diag)
            }
            "key" => {
                ensure_rewound(&mut st, diag)?;
                st.iter.key(diag)
            }
            "next" => {
                ensure_rewound(&mut st, diag)?;
                // Advance index first to match foreach behavior.
                st.index += 1;
                st.iter.next(diag)?;
                Ok(Value::Null)
            }
            "valid" => {
                ensure_rewound(&mut st, diag)?;
                Ok(Value::Bool(st.iter.valid(diag)?))
            }
            _ => {
                st.rewind_called = true;
                st.iter.rewind(diag)?;
                st.index = 0;
                Ok(Value::Null)
            }
        }
    }
}
