//! `DateTimeZone` (`php_timezone_obj`) and the timezone functions of PHPSRC/ext/date/php_date.c.

use std::sync::{Arc, RwLock};

use twig_timelib::abbr::{timezone_abbreviations_list as tl_abbreviations, timezone_id_from_abbr};
use twig_timelib::parse_date::parse_zone;
use twig_timelib::posix::{PosixTransitions, get_transitions_for_year};
use twig_timelib::types::{ZONETYPE_ABBR, ZONETYPE_ID, ZONETYPE_OFFSET};
use twig_timelib::unixtime2tm::unixtime2date;
use twig_timelib::{AbbrInfo, Time, TzDb, TzInfo};
use twig_value::object::{
    CallArgs, ClassRef, FetchMode, MethodParams, Object, ObjectRef, Param, PropertyCheck,
    PropertyEntry, PropertyPurpose, PropertyStore,
};
use twig_value::param::Arg;
use twig_value::{Array, Diagnostics, PhpError, PhpResult, PhpString, Value};

use crate::args::{self, Bound};
use crate::classes::{self, TIMEZONE_GROUP_ALL, TIMEZONE_GROUP_ALL_W_BC, TIMEZONE_PER_COUNTRY};
use crate::context::{DateContext, c_str};
use crate::datetime::DateObject;
use crate::errors;
use crate::format::{date_format, tz_offset_str};
use crate::util;

/// The zone of an initialised `DateTimeZone` (`php_timezone_obj.tzi` with its `type`).
#[derive(Clone, Debug)]
pub enum TzValue {
    /// `TIMELIB_ZONETYPE_ID`: a database zone.
    Id(Arc<TzInfo>),
    /// `TIMELIB_ZONETYPE_OFFSET`: a fixed UTC offset in seconds.
    Offset(i64),
    /// `TIMELIB_ZONETYPE_ABBR`: an abbreviation with its offset and DST flag.
    Abbr(AbbrInfo),
}

impl TzValue {
    /// The `TIMELIB_ZONETYPE_*` constant (`timezone_type`: 1 offset, 2 abbreviation, 3 identifier).
    pub fn zone_type(&self) -> u32 {
        match self {
            TzValue::Id(_) => ZONETYPE_ID,
            TzValue::Offset(_) => ZONETYPE_OFFSET,
            TzValue::Abbr(_) => ZONETYPE_ABBR,
        }
    }

    /// Port of `php_timezone_to_string()` (PHPSRC/ext/date/php_date.c): the identifier as written, `+05:00`, or
    /// the abbreviation (`DateTimeZone::getName()`).
    pub fn name(&self) -> PhpString {
        match self {
            TzValue::Id(tz) => PhpString::from(tz.name.as_str()),
            TzValue::Offset(o) => PhpString::from(tz_offset_str(*o)),
            TzValue::Abbr(a) => PhpString::from(a.abbr.as_str()),
        }
    }

    /// Port of `set_timezone_from_timelib_time()` (PHPSRC/ext/date/php_date.c): the zone of a parsed or local
    /// time (`DateTime::getTimezone()`).
    pub fn from_time(t: &Time) -> Option<TzValue> {
        match t.zone_type {
            ZONETYPE_ID => t.tz_info.clone().map(TzValue::Id),
            ZONETYPE_OFFSET => Some(TzValue::Offset(i64::from(t.z))),
            ZONETYPE_ABBR => Some(TzValue::Abbr(AbbrInfo {
                utc_offset: i64::from(t.z),
                abbr: t.tz_abbr.clone().unwrap_or_default(),
                dst: t.dst,
            })),
            _ => None,
        }
    }
}

/// Port of `timezone_initialize()` (PHPSRC/ext/date/php_date.c): parses a timezone string (offset,
/// abbreviation or identifier) like `new DateTimeZone()`. The error is the message without the
/// `DateTimeZone::__construct(): ` prefix (`Unknown or bad timezone (%s)`, `Timezone offset is out of range
/// (%s)`).
pub fn timezone_initialize(ctx: &DateContext, tz: &[u8]) -> Result<TzValue, PhpString> {
    let shown = c_str(tz);
    // the NUL-terminated copy the parser reads (a zone name fits on the stack)
    let mut stack = [0u8; 64];
    let mut heap = Vec::new();
    let b: &[u8] = if shown.len() < stack.len() {
        stack[..shown.len()].copy_from_slice(shown);
        &stack[..=shown.len()]
    } else {
        heap.extend_from_slice(shown);
        heap.push(0);
        &heap
    };
    let mut dummy_t = Time::new();
    let mut ptr = 0usize;
    let mut dst = 0i32;
    let mut not_found = false;
    let cache = ctx.tz_cache();
    let z = parse_zone(
        b,
        &mut ptr,
        &mut dst,
        &mut dummy_t,
        &mut not_found,
        ctx.tzdb(),
        &mut |name: &str, _db: &TzDb| cache.get(name),
    );
    dummy_t.z = z as i32;
    if dummy_t.z >= 100 * 60 * 60 || dummy_t.z <= -100 * 60 * 60 {
        return Err(PhpString::concat(&[
            b"Timezone offset is out of range (",
            shown,
            b")",
        ]));
    }
    dummy_t.dst = dst;
    if (!not_found && b[ptr] != 0) || not_found {
        return Err(PhpString::concat(&[
            b"Unknown or bad timezone (",
            shown,
            b")",
        ]));
    }
    TzValue::from_time(&dummy_t)
        .ok_or_else(|| PhpString::concat(&[b"Unknown or bad timezone (", shown, b")"]))
}

/// A `DateTimeZone` object.
pub struct TimeZoneObject {
    class: ClassRef,
    ctx: DateContext,
    tz: RwLock<Option<TzValue>>,
    props: PropertyStore,
}

impl TimeZoneObject {
    /// An uninitialised object of `class` (`date_object_new_timezone()`); its constructor has not run.
    pub fn new_uninitialized(ctx: &DateContext, class: ClassRef) -> TimeZoneObject {
        let props = PropertyStore::new(&class);
        TimeZoneObject {
            class,
            ctx: ctx.clone(),
            tz: RwLock::new(None),
            props,
        }
    }

    /// A `DateTimeZone` holding `tz`.
    pub fn from_value(ctx: &DateContext, tz: TzValue) -> ObjectRef {
        let obj = TimeZoneObject::new_uninitialized(ctx, classes::datetimezone());
        *obj.tz.write().unwrap_or_else(|e| e.into_inner()) = Some(tz);
        ObjectRef::new(obj)
    }

    /// `new DateTimeZone($timezone)` (`DateTimeZone::__construct()`): throws `DateInvalidTimeZoneException`
    /// (`DateTimeZone::__construct(): Unknown or bad timezone (%s)`), or `ValueError` for NUL bytes.
    pub fn construct(ctx: &DateContext, timezone: &[u8]) -> PhpResult<ObjectRef> {
        let obj = TimeZoneObject::new_uninitialized(ctx, classes::datetimezone());
        obj.construct_in_place(timezone)?;
        Ok(ObjectRef::new(obj))
    }

    /// `DateTimeZone::__set_state($array)`: a new object from `timezone_type` and `timezone`; anything else throws
    /// `Error: Invalid serialization data for DateTimeZone object`.
    pub fn set_state(ctx: &DateContext, data: &Array) -> PhpResult<ObjectRef> {
        match initialize_from_hash(ctx, data) {
            Some(tz) => Ok(TimeZoneObject::from_value(ctx, tz)),
            None => Err(errors::invalid_serialization("DateTimeZone")),
        }
    }

    /// `PHP_METHOD(DateTimeZone, __construct)` on this object.
    fn construct_in_place(&self, timezone: &[u8]) -> PhpResult<()> {
        if timezone.contains(&0) {
            return Err(Arg::new("DateTimeZone::__construct", 1, "timezone")
                .value_error("must not contain any null bytes"));
        }
        match timezone_initialize(&self.ctx, timezone) {
            Ok(tz) => {
                self.set_value(Some(tz));
                Ok(())
            }
            Err(msg) => Err(errors::throw(
                errors::date_invalid_timezone_exception(),
                PhpString::concat(&[b"DateTimeZone::__construct(): ", msg.as_bytes()]),
            )),
        }
    }

    /// The zone, or `None` when the object is not initialised.
    pub fn value(&self) -> Option<TzValue> {
        self.tz.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn set_value(&self, tz: Option<TzValue>) {
        *self.tz.write().unwrap_or_else(|e| e.into_inner()) = tz;
    }

    /// The context this object was created in.
    pub fn context(&self) -> &DateContext {
        &self.ctx
    }

    fn initialized(&self) -> PhpResult<TzValue> {
        self.value()
            .ok_or_else(|| errors::uninitialized(&self.class))
    }

    /// `DateTimeZone::getName()`.
    pub fn name(&self) -> PhpResult<PhpString> {
        Ok(self.initialized()?.name())
    }

    /// Port of `PHP_FUNCTION(timezone_offset_get)`: the offset of this zone at the instant of `datetime`.
    pub fn offset_at(&self, datetime: &DateObject) -> PhpResult<i64> {
        let tz = self.initialized()?;
        let t = datetime.initialized_time()?;
        Ok(match tz {
            TzValue::Id(z) => i64::from(z.get_time_zone_info(t.sse).offset),
            TzValue::Offset(o) => o,
            TzValue::Abbr(a) => a.utc_offset + i64::from(a.dst) * 3600,
        })
    }
}

/// Port of `date_timezone_object_to_hash()`.
fn timezone_object_to_hash(tz: &TzValue, props: &mut Vec<PropertyEntry>) {
    util::update_prop(
        props,
        "timezone_type",
        Value::Int(i64::from(tz.zone_type())),
    );
    util::update_prop(props, "timezone", Value::String(tz.name()));
}

/// Port of `php_date_timezone_initialize_from_hash()`: `timezone_type` (an int 1..3) and `timezone` (a string
/// without NUL bytes) parsed with [`timezone_initialize`].
fn initialize_from_hash(ctx: &DateContext, data: &Array) -> Option<TzValue> {
    let z_timezone_type = data.get_str(b"timezone_type")?;
    let z_timezone = data.get_str(b"timezone")?;
    let Value::Int(tt) = z_timezone_type else {
        return None;
    };
    if *tt < i64::from(ZONETYPE_OFFSET) || *tt > i64::from(ZONETYPE_ID) {
        return None;
    }
    let Value::String(name) = z_timezone else {
        return None;
    };
    if name.as_bytes().contains(&0) {
        return None;
    }
    timezone_initialize(ctx, name.as_bytes()).ok()
}

/// `restore_custom_datetimezone_properties()`'s internal keys.
fn is_internal_property(name: &[u8]) -> bool {
    name == b"timezone_type" || name == b"timezone"
}

/// The parameters of the methods of `DateTimeZone` (by lowercase name), as PHPSRC/ext/date/php_date.stub.php
/// declares them: what [`TimeZoneObject`] reports and binds its arguments to (`args::ParamTable`).
fn timezone_params(method: &str) -> Option<Vec<Param>> {
    Some(match method {
        // __construct(string $timezone)
        "__construct" => vec![Param::required("timezone")],
        // getOffset(DateTimeInterface $datetime)
        "getoffset" => vec![Param::required("datetime")],
        // getTransitions(int $timestampBegin = PHP_INT_MIN, int $timestampEnd = 2147483647)
        "gettransitions" => vec![
            Param::optional("timestampBegin", i64::MIN),
            Param::optional("timestampEnd", i64::from(i32::MAX)),
        ],
        // static listIdentifiers(int $timezoneGroup = DateTimeZone::ALL, ?string $countryCode = null)
        "listidentifiers" => vec![
            Param::optional("timezoneGroup", TIMEZONE_GROUP_ALL),
            Param::optional("countryCode", Value::Null),
        ],
        // __unserialize(array $data)
        "__unserialize" => vec![Param::required("data")],
        // static __set_state(array $array)
        "__set_state" => vec![Param::required("array")],
        "getname" | "getlocation" | "listabbreviations" | "__serialize" | "__wakeup" => Vec::new(),
        _ => return None,
    })
}

/// The `DateTimeZone` methods (`Class::method` for messages is the declaring class).
impl Object for TimeZoneObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    /// The parameters of the method as php_date.stub.php declares them (`timezone_params`).
    fn method_params(&self, method: &str) -> Option<MethodParams> {
        args::method_params(timezone_params, method)
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
                let a = Bound::method("DateTimeZone::__construct", method, timezone_params, &args)?;
                let tz = a.path(0, diag)?;
                self.construct_in_place(tz.as_bytes())?;
                Ok(Value::Null)
            }
            "getname" => {
                Bound::method("DateTimeZone::getName", method, timezone_params, &args)?;
                Ok(Value::String(self.name()?))
            }
            "getoffset" => {
                let a = Bound::method("DateTimeZone::getOffset", method, timezone_params, &args)?;
                let d = a.object_of(0, "DateTimeInterface")?;
                self.initialized()?;
                let date = d
                    .downcast_ref::<DateObject>()
                    .ok_or_else(|| errors::uninitialized(d.class()))?;
                Ok(Value::Int(self.offset_at(date)?))
            }
            "gettransitions" => {
                let a = Bound::method(
                    "DateTimeZone::getTransitions",
                    method,
                    timezone_params,
                    &args,
                )?;
                let begin = a.long(0, diag)?;
                let end = a.long(1, diag)?;
                Ok(match self.initialized()? {
                    TzValue::Id(tz) => Value::Array(transitions(&tz, begin, end)),
                    _ => Value::Bool(false),
                })
            }
            "getlocation" => {
                Bound::method("DateTimeZone::getLocation", method, timezone_params, &args)?;
                Ok(match self.initialized()? {
                    TzValue::Id(tz) => Value::Array(location(&tz)),
                    _ => Value::Bool(false),
                })
            }
            "listabbreviations" => {
                Bound::method(
                    "DateTimeZone::listAbbreviations",
                    method,
                    timezone_params,
                    &args,
                )?;
                Ok(Value::Array(timezone_abbreviations_list()))
            }
            "listidentifiers" => {
                let a = Bound::method(
                    "DateTimeZone::listIdentifiers",
                    method,
                    timezone_params,
                    &args,
                )?;
                let what = a.long(0, diag)?;
                let country = a.string_or_null(1, diag)?;
                timezone_identifiers_list(
                    self.ctx.tzdb(),
                    "DateTimeZone::listIdentifiers",
                    what,
                    country.as_ref().map(|s| s.as_bytes()),
                )
                .map(Value::Array)
            }
            "__serialize" => {
                Bound::method("DateTimeZone::__serialize", method, timezone_params, &args)?;
                let tz = self.initialized()?;
                let mut props = Vec::new();
                timezone_object_to_hash(&tz, &mut props);
                let mut out = util::assoc(
                    props
                        .into_iter()
                        .filter_map(|e| e.value.map(|v| (e.key, v))),
                );
                util::add_common_properties(&mut out, this, diag)?;
                Ok(Value::Array(out))
            }
            "__unserialize" => {
                let a = Bound::method(
                    "DateTimeZone::__unserialize",
                    method,
                    timezone_params,
                    &args,
                )?;
                let data = a.array(0)?;
                match initialize_from_hash(&self.ctx, &data) {
                    Some(tz) => self.set_value(Some(tz)),
                    None => return Err(errors::invalid_serialization("DateTimeZone")),
                }
                util::restore_custom_properties(this, &data, is_internal_property, diag)?;
                Ok(Value::Null)
            }
            "__wakeup" => {
                util::wakeup_deprecated("DateTimeZone", diag)?;
                Bound::method("DateTimeZone::__wakeup", method, timezone_params, &args)?;
                let table = util::assoc(
                    util::std_properties(this, diag)?
                        .into_iter()
                        .filter_map(|e| e.value.map(|v| (e.key, v))),
                );
                match initialize_from_hash(&self.ctx, &table) {
                    Some(tz) => self.set_value(Some(tz)),
                    None => return Err(errors::invalid_serialization("DateTimeZone")),
                }
                Ok(Value::Null)
            }
            "__set_state" => {
                let a = Bound::method("DateTimeZone::__set_state", method, timezone_params, &args)?;
                let data = a.array(0)?;
                Ok(Value::Object(TimeZoneObject::set_state(&self.ctx, &data)?))
            }
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }

    fn read_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        util::std_read_property(this, name, mode, diag)
    }

    fn has_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        check: PropertyCheck,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        util::std_has_property(this, name, check, diag)
    }

    /// Port of `date_object_get_properties_for_timezone()`.
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
        if let Some(tz) = self.value() {
            timezone_object_to_hash(&tz, &mut props);
        }
        Ok(props)
    }

    /// Port of `date_object_clone_timezone()`.
    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(TimeZoneObject {
            class: self.class.clone(),
            ctx: self.ctx.clone(),
            tz: RwLock::new(self.value()),
            props: self.props.clone(),
        }))
    }

    /// Port of `date_object_compare_timezone()`: equal (0) or not (1); different zone kinds throw.
    fn compare(
        &self,
        _this: &ObjectRef,
        other: &Value,
        _this_is_left: bool,
        _compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        // ZEND_COMPARE_OBJECTS_FALLBACK()
        let Value::Object(o) = other else {
            return None;
        };
        let other = o.downcast_ref::<TimeZoneObject>()?;
        let (Some(a), Some(b)) = (self.value(), other.value()) else {
            return Some(Err(PhpError::new(
                errors::date_object_error(),
                "Trying to compare uninitialized DateTimeZone objects",
            )));
        };
        if a.zone_type() != b.zone_type() {
            return Some(Err(PhpError::new(
                errors::date_exception(),
                "Cannot compare two different kinds of DateTimeZone objects",
            )));
        }
        Some(Ok(match (a, b) {
            (TzValue::Offset(x), TzValue::Offset(y)) => i32::from(x != y),
            (TzValue::Abbr(x), TzValue::Abbr(y)) => i32::from(x.abbr != y.abbr),
            (TzValue::Id(x), TzValue::Id(y)) => i32::from(x.name != y.name),
            _ => 1,
        }))
    }
}

/// `php_format_date(DATE_FORMAT_ISO8601_LARGE_YEAR, 13, ts, 0)`: the `time` of a transition.
fn transition_time(ts: i64) -> PhpString {
    let mut t = Time::new();
    t.unixtime2gmt(ts);
    PhpString::from(date_format(
        classes::DATE_FORMAT_ISO8601_LARGE_YEAR.as_bytes(),
        &t,
        false,
    ))
}

/// One transition entry: `ts`, `time`, `offset`, `isdst`, `abbr`.
fn transition_entry(ts: i64, offset: i32, isdst: bool, abbr: &str) -> Value {
    Value::Array(util::assoc([
        ("ts", Value::Int(ts)),
        ("time", Value::String(transition_time(ts))),
        ("offset", Value::Int(i64::from(offset))),
        ("isdst", Value::Bool(isdst)),
        ("abbr", Value::from(abbr)),
    ]))
}

/// Port of `PHP_FUNCTION(timezone_transitions_get)` (PHPSRC/ext/date/php_date.c) for an identifier zone:
/// the type in effect at `begin`, then every stored transition before `end`, then the transitions the POSIX
/// rules generate up to `end`.
pub fn transitions(tz: &TzInfo, begin: i64, end: i64) -> Array {
    let mut out = Array::new();
    let by_type = |out: &mut Array, ts: i64, ty: usize| {
        let t = tz.types.get(ty).copied().unwrap_or_default();
        let _ = out.push(transition_entry(
            ts,
            t.offset,
            t.isdst != 0,
            tz.abbr_at(t.abbr_idx as usize),
        ));
    };
    let by_trans = |out: &mut Array, i: usize, ts: i64| {
        by_type(out, ts, usize::from(tz.trans_idx[i]));
    };
    let timecnt = tz.bit64.timecnt as usize;
    let mut start = 0usize;
    let mut found = false;
    if begin == i64::MIN {
        by_type(&mut out, begin, 0); // add_nominal()
        found = true;
    } else if timecnt > 0 {
        while start < timecnt {
            if tz.trans[start] > begin {
                if start > 0 {
                    by_trans(&mut out, start - 1, begin);
                } else {
                    by_type(&mut out, begin, 0);
                }
                found = true;
                break;
            }
            start += 1;
        }
    }
    let has_dst_rules = tz.posix_info.as_ref().is_some_and(|p| p.dst_end.is_some());
    if !found {
        if timecnt > 0 {
            if has_dst_rules {
                let to = tz.get_time_zone_info(begin);
                let _ = out.push(transition_entry(begin, to.offset, to.is_dst != 0, &to.abbr));
            } else {
                by_trans(&mut out, timecnt - 1, begin); // add_last()
            }
        } else {
            by_type(&mut out, begin, 0);
        }
    } else {
        for (i, &ts) in tz.trans.iter().enumerate().take(timecnt).skip(start) {
            if ts < end {
                by_trans(&mut out, i, ts);
            } else {
                return out;
            }
        }
    }
    if has_dst_rules && timecnt > 0 {
        let last_transition_ts = tz.trans[timecnt - 1];
        // Find out year for last transition
        let (start_y, _, _) = unixtime2date(last_transition_ts);
        // Find out year for final boundary timestamp
        let (end_y, _, _) = unixtime2date(end);
        let mut y = start_y;
        while y <= end_y {
            let mut tr = PosixTransitions::default();
            get_transitions_for_year(tz, y, &mut tr);
            for j in 0..tr.count {
                if tr.times[j] <= last_transition_ts || tr.times[j] < begin {
                    continue;
                }
                if tr.times[j] > end {
                    return out;
                }
                by_type(&mut out, tr.times[j], tr.types[j] as usize);
            }
            y += 1;
        }
    }
    out
}

/// Port of `PHP_FUNCTION(timezone_location_get)` for an identifier zone.
pub fn location(tz: &TzInfo) -> Array {
    util::assoc([
        (
            "country_code",
            Value::from(tz.location.country_code.as_str()),
        ),
        ("latitude", Value::Float(tz.location.latitude)),
        ("longitude", Value::Float(tz.location.longitude)),
        ("comments", Value::from(tz.location.comments.as_str())),
    ])
}

/// Port of `check_id_allowed()`.
fn check_id_allowed(id: &str, what: i64) -> bool {
    const GROUPS: [(i64, &str); 11] = [
        (classes::TIMEZONE_GROUP_AFRICA, "Africa/"),
        (classes::TIMEZONE_GROUP_AMERICA, "America/"),
        (classes::TIMEZONE_GROUP_ANTARCTICA, "Antarctica/"),
        (classes::TIMEZONE_GROUP_ARCTIC, "Arctic/"),
        (classes::TIMEZONE_GROUP_ASIA, "Asia/"),
        (classes::TIMEZONE_GROUP_ATLANTIC, "Atlantic/"),
        (classes::TIMEZONE_GROUP_AUSTRALIA, "Australia/"),
        (classes::TIMEZONE_GROUP_EUROPE, "Europe/"),
        (classes::TIMEZONE_GROUP_INDIAN, "Indian/"),
        (classes::TIMEZONE_GROUP_PACIFIC, "Pacific/"),
        (classes::TIMEZONE_GROUP_UTC, "UTC"),
    ];
    GROUPS.iter().any(|(bit, prefix)| {
        what & bit != 0
            && id.len() >= prefix.len()
            && id.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    })
}

/// Port of `PHP_FUNCTION(timezone_identifiers_list)` (`DateTimeZone::listIdentifiers()`): `func` names the
/// caller in the `ValueError` for a bad country code.
pub fn timezone_identifiers_list(
    tzdb: &TzDb,
    func: &str,
    what: i64,
    country: Option<&[u8]>,
) -> PhpResult<Array> {
    let option = country.unwrap_or(b"");
    // Extra validation
    if what == TIMEZONE_PER_COUNTRY && option.len() != 2 {
        return Err(Arg::new(func, 2, "countryCode").value_error(
            "must be a two-letter ISO 3166-1 compatible country code when argument #1 ($timezoneGroup) is DateTimeZone::PER_COUNTRY",
        ));
    }
    let mut out = Array::new();
    for (id, pos) in tzdb.index {
        let pos = *pos as usize;
        let byte = |k: usize| tzdb.data.get(pos + k).copied().unwrap_or(0);
        if what == TIMEZONE_PER_COUNTRY {
            if byte(5) == option[0] && byte(6) == option[1] {
                let _ = out.push(Value::from(*id));
            }
        } else if what == TIMEZONE_GROUP_ALL_W_BC || (check_id_allowed(id, what) && byte(4) == 1) {
            let _ = out.push(Value::from(*id));
        }
    }
    Ok(out)
}

/// Port of `PHP_FUNCTION(timezone_abbreviations_list)` (`DateTimeZone::listAbbreviations()`).
pub fn timezone_abbreviations_list() -> Array {
    let mut out = Array::new();
    for entry in tl_abbreviations() {
        let element = util::assoc([
            ("dst", Value::Bool(entry.dst != 0)),
            ("offset", Value::Int(i64::from(entry.gmtoffset))),
            (
                "timezone_id",
                entry.full_tz_name.map_or(Value::Null, Value::from),
            ),
        ]);
        let key = PhpString::from(entry.name);
        let mut list = match out.get_sym(key.as_bytes()) {
            Some(Value::Array(a)) => a.clone(),
            _ => Array::new(),
        };
        let _ = list.push(Value::Array(element));
        out.insert(key, Value::Array(list));
    }
    out
}

/// Port of `PHP_FUNCTION(timezone_name_from_abbr)`: the identifier for an abbreviation (and/or offset and DST
/// flag), or `false`.
pub fn timezone_name_from_abbr(abbr: &[u8], utc_offset: i64, is_dst: i64) -> Value {
    match timezone_id_from_abbr(c_str(abbr), utc_offset, is_dst as i32) {
        Some(Some(id)) => Value::from(id),
        _ => Value::Bool(false),
    }
}

/// Port of `PHP_FUNCTION(timezone_open)`: like `new DateTimeZone()`, but an invalid zone raises the warning
/// `timezone_open(): Unknown or bad timezone (%s)` and returns `None` (`false`).
pub fn timezone_open(
    ctx: &DateContext,
    timezone: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<ObjectRef>> {
    if timezone.contains(&0) {
        return Err(
            Arg::new("timezone_open", 1, "timezone").value_error("must not contain any null bytes")
        );
    }
    match timezone_initialize(ctx, timezone) {
        Ok(tz) => Ok(Some(TimeZoneObject::from_value(ctx, tz))),
        Err(msg) => {
            diag.warn(PhpString::concat(&[b"timezone_open(): ", msg.as_bytes()]))?;
            Ok(None)
        }
    }
}
