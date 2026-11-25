//! `ArrayObject` and `ArrayIterator` (PHPSRC/ext/spl/spl_array.c), array-backed.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::builtin;
use super::class::{ClassRef, Visibility};
use super::props::{PropertyEntry, PropertyStore, mangle_property_name};
use super::{
    CallArgs, FetchMode, FunctionKind, Object, ObjectIterator, ObjectRef, Param, PropertyCheck,
    PropertyPurpose,
};
use crate::array::Array;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult, ThrowableClass};
use crate::key::{ArrayKey, KeyContext, illegal_offset};
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::Value;

/// `SPL_ARRAY_STD_PROP_LIST`.
const STD_PROP_LIST: i64 = 1;
/// `SPL_ARRAY_ARRAY_AS_PROPS`.
const ARRAY_AS_PROPS: i64 = 2;
/// `SPL_ARRAY_INT_MASK`.
const INT_MASK: i64 = 0xFFFF_0000;

type Storage = Arc<Mutex<Array>>;

fn lock(storage: &Storage) -> MutexGuard<'_, Array> {
    storage.lock().unwrap_or_else(|e| e.into_inner())
}

/// Port of `get_hash_key()`: `None` is `FAILURE` (array/object offsets). `null` emits the PHP 8.5
/// deprecation in every context (also `unset`, unlike plain arrays).
fn hash_key(offset: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Option<ArrayKey>> {
    Ok(match offset {
        Value::Null => {
            diag.deprecated(
                "Using null as an array offset is deprecated, use an empty string instead",
            )?;
            Some(ArrayKey::Str(PhpString::empty()))
        }
        Value::String(s) => Some(ArrayKey::from_string(s.clone())),
        Value::Float(d) => Some(ArrayKey::Int(convert::dval_to_lval_safe(*d, diag)?)),
        Value::Bool(b) => Some(ArrayKey::Int(i64::from(*b))),
        Value::Int(i) => Some(ArrayKey::Int(*i)),
        Value::Array(_) | Value::Object(_) => None,
    })
}

/// Shared implementation of the array-backed handlers and methods.
struct Spl<'a> {
    class: &'a ClassRef,
    storage: &'a Storage,
    flags: &'a AtomicI64,
}

impl Spl<'_> {
    fn flags(&self) -> i64 {
        self.flags.load(Ordering::Relaxed)
    }

    fn snapshot(&self) -> Array {
        lock(self.storage).clone()
    }

    /// `spl_array_get_dimension_ptr()` for `BP_VAR_R` / `BP_VAR_IS`.
    fn get(&self, offset: &Value, mode: FetchMode, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        let ctx = if mode == FetchMode::Isset {
            KeyContext::Isset
        } else {
            KeyContext::Read
        };
        let key = hash_key(offset, diag)?
            .ok_or_else(|| illegal_offset(offset, ctx, self.class.name().as_bytes()))?;
        let arr = self.snapshot();
        match arr.get(&key) {
            Some(v) => Ok(v.clone()),
            None => {
                if mode == FetchMode::Read {
                    crate::dim::undefined_key(&key, diag)?;
                }
                Ok(Value::Null)
            }
        }
    }

    /// `spl_array_has_dimension_ex(check_inherited = false, ...)`: `check_empty` 0 = isset,
    /// 1 = empty, 2 = exists (offsetExists).
    fn has(&self, offset: &Value, check_empty: u8, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        let key = hash_key(offset, diag)?.ok_or_else(|| {
            illegal_offset(offset, KeyContext::Isset, self.class.name().as_bytes())
        })?;
        let arr = self.snapshot();
        match arr.get(&key) {
            None => Ok(false),
            Some(_) if check_empty == 2 => Ok(true),
            Some(v) if check_empty == 1 => convert::to_bool(v, diag),
            Some(v) => Ok(!v.is_null()),
        }
    }

    /// `spl_array_write_dimension_ex()`: a missing or `null` offset appends (a failing append is
    /// silently ignored, like upstream ignores `zend_hash_next_index_insert()`'s result).
    fn set(
        &self,
        offset: Option<&Value>,
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        match offset {
            None | Some(Value::Null) => {
                let _ = lock(self.storage).push(value);
                Ok(())
            }
            Some(o) => {
                let key = hash_key(o, diag)?.ok_or_else(|| {
                    illegal_offset(o, KeyContext::Write, self.class.name().as_bytes())
                })?;
                lock(self.storage).insert(key, value);
                Ok(())
            }
        }
    }

    /// `spl_array_unset_dimension_ex()`.
    fn unset(&self, offset: &Value, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        let key = hash_key(offset, diag)?.ok_or_else(|| {
            illegal_offset(offset, KeyContext::Unset, self.class.name().as_bytes())
        })?;
        lock(self.storage).remove(&key);
        Ok(())
    }

    /// `spl_array_set_array()` for array input (object storage is not supported).
    fn set_array(&self, array: &Value, flags: i64, fname: &str) -> PhpResult<()> {
        match array {
            Value::Array(a) => {
                *lock(self.storage) = a.clone();
                self.flags.fetch_or(flags, Ordering::Relaxed);
                Ok(())
            }
            other => Err(Arg::new(fname, 1, "array").type_error("of type array", other)),
        }
    }

    /// Methods shared by both classes; `None` when `method` is not one of them.
    fn call_common(
        &self,
        this: &ObjectRef,
        method: &str,
        args: &CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> Option<PhpResult<Value>> {
        const COMMON: &[&str] = &[
            "offsetexists",
            "offsetget",
            "offsetset",
            "offsetunset",
            "append",
            "getarraycopy",
            "count",
            "getflags",
            "setflags",
            "__debuginfo",
            "asort",
            "ksort",
            "uasort",
            "uksort",
            "natsort",
            "natcasesort",
            "serialize",
            "unserialize",
            "__serialize",
            "__unserialize",
        ];
        if !COMMON.contains(&method) {
            return None;
        }
        Some(self.call_common_inner(this, method, args, diag))
    }

    fn call_common_inner(
        &self,
        this: &ObjectRef,
        method: &str,
        args: &CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let class = self.class.name();
        let fname = |m: &str| format!("{class}::{m}");
        let bind = |m: &str, params: &[Param]| args.bind(&fname(m), params, FunctionKind::Internal);
        {
            match method {
                "offsetexists" => {
                    let a = bind("offsetExists", &[Param::required("key")])?;
                    Ok(Value::Bool(self.has(&a[0], 2, diag)?))
                }
                "offsetget" => {
                    let a = bind("offsetGet", &[Param::required("key")])?;
                    self.get(&a[0], FetchMode::Read, diag)
                }
                "offsetset" => {
                    let a = bind(
                        "offsetSet",
                        &[Param::required("key"), Param::required("value")],
                    )?;
                    self.set(Some(&a[0]), a[1].clone(), diag)?;
                    Ok(Value::Null)
                }
                "offsetunset" => {
                    let a = bind("offsetUnset", &[Param::required("key")])?;
                    self.unset(&a[0], diag)?;
                    Ok(Value::Null)
                }
                "append" => {
                    let a = bind("append", &[Param::required("value")])?;
                    self.set(None, a[0].clone(), diag)?;
                    Ok(Value::Null)
                }
                "getarraycopy" => {
                    bind("getArrayCopy", &[])?;
                    Ok(Value::Array(self.snapshot()))
                }
                "count" => {
                    bind("count", &[])?;
                    Ok(Value::Int(self.snapshot().len() as i64))
                }
                "getflags" => {
                    bind("getFlags", &[])?;
                    Ok(Value::Int(self.flags() & !INT_MASK))
                }
                "setflags" => {
                    let a = bind("setFlags", &[Param::required("flags")])?;
                    let name = fname("setFlags");
                    let flags = param::parse_long(&a[0], &Arg::new(&name, 1, "flags"), diag)?;
                    let old = self.flags();
                    self.flags
                        .store((old & INT_MASK) | (flags & !INT_MASK), Ordering::Relaxed);
                    Ok(Value::Null)
                }
                "__debuginfo" => {
                    bind("__debugInfo", &[])?;
                    let entries = self.debug_entries(this);
                    Ok(Value::Array(super::property_entries_to_array(entries)))
                }
                _ => {
                    // asort, ksort, uasort, uksort, natsort, natcasesort, (un)serialize
                    let declared = self
                        .class
                        .method(method)
                        .map(|m| m.name.to_string())
                        .unwrap_or_default();
                    Err(PhpError::error(format!(
                        "{class}::{declared}() is not supported yet"
                    )))
                }
            }
        }
    }

    /// `spl_array_get_debug_info()`: the standard properties plus the private `storage`.
    fn debug_entries(&self, this: &ObjectRef) -> Vec<PropertyEntry> {
        let mut entries = super::props::std_properties(this);
        let base = if self.class.is_subclass_of("ArrayIterator") {
            "ArrayIterator"
        } else {
            "ArrayObject"
        };
        entries.push(PropertyEntry {
            key: ArrayKey::Str(mangle_property_name(Visibility::Private, base, b"storage")),
            value: Some(Value::Array(self.snapshot())),
            type_name: None,
        });
        entries
    }

    /// `spl_array_get_properties_for()`.
    fn properties_for(&self, this: &ObjectRef, purpose: PropertyPurpose) -> Vec<PropertyEntry> {
        if self.flags() & STD_PROP_LIST != 0 {
            return super::props::std_properties(this);
        }
        match purpose {
            PropertyPurpose::ArrayCast | PropertyPurpose::VarExport | PropertyPurpose::Json => self
                .snapshot()
                .iter()
                .map(|(k, v)| PropertyEntry {
                    key: k.clone(),
                    value: Some(v.clone()),
                    type_name: None,
                })
                .collect(),
            PropertyPurpose::Debug => self.debug_entries(this),
            _ => super::props::std_properties(this),
        }
    }

    fn array_as_props(
        &self,
        this: &ObjectRef,
        name: &[u8],
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        Ok(self.flags() & ARRAY_AS_PROPS != 0
            && !super::props::std_has_property(this, name, PropertyCheck::Exists, diag)?)
    }

    /// `spl_array_compare_objects()`: storages first (`zend_compare_symbol_tables`), then the
    /// standard comparison when equal.
    fn compare(
        &self,
        other: &Value,
        this_is_left: bool,
        cmp: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        let other_storage = match other {
            Value::Object(o) => {
                if let Some(ao) = o.downcast_ref::<ArrayObject>() {
                    lock(&ao.storage).clone()
                } else {
                    let it = o.downcast_ref::<ArrayIterator>()?;
                    lock(&it.storage).clone()
                }
            }
            _ => return None,
        };
        let mine = Value::Array(self.snapshot());
        let theirs = Value::Array(other_storage);
        let r = if this_is_left {
            cmp(&mine, &theirs)
        } else {
            cmp(&theirs, &mine)
        };
        match r {
            Ok(0) => None,
            r => Some(r),
        }
    }
}

/// `ArrayObject`: an object wrapping an array (`ArrayAccess`, `Countable`, `IteratorAggregate`).
///
/// Supported: array storage, all `ArrayAccess` methods and handlers with PHP's key rules and
/// diagnostics, `count`, `append`, `getArrayCopy`, `exchangeArray`, flags (`STD_PROP_LIST`,
/// `ARRAY_AS_PROPS`), `getIterator()` (an `ArrayIterator` sharing the storage), `(array)`,
/// `json_encode`/`var_export` tables, the `var_dump` storage entry, cloning (copies the storage) and
/// storage comparison. Not supported: object storage (deprecated in PHP 8.5), custom iterator
/// classes, and the sort/serialize methods (they throw `Error` until the sort and serialize
/// builtins exist).
pub struct ArrayObject {
    class: ClassRef,
    storage: Storage,
    flags: AtomicI64,
    iterator_class: Mutex<Arc<str>>,
    props: PropertyStore,
}

impl ArrayObject {
    /// `new ArrayObject($array)`.
    pub fn new(array: Array) -> ArrayObject {
        ArrayObject::with_flags(array, 0)
    }

    /// `new ArrayObject($array, $flags)`.
    pub fn with_flags(array: Array, flags: i64) -> ArrayObject {
        let class = builtin::array_object();
        let props = PropertyStore::new(&class);
        ArrayObject {
            class,
            storage: Arc::new(Mutex::new(array)),
            flags: AtomicI64::new(flags & !INT_MASK),
            iterator_class: Mutex::new(Arc::from("ArrayIterator")),
            props,
        }
    }

    /// Wraps into a handle.
    pub fn into_ref(self) -> ObjectRef {
        ObjectRef::new(self)
    }

    /// A copy of the storage (`getArrayCopy()`).
    pub fn array(&self) -> Array {
        lock(&self.storage).clone()
    }

    fn spl(&self) -> Spl<'_> {
        Spl {
            class: &self.class,
            storage: &self.storage,
            flags: &self.flags,
        }
    }
}

impl Object for ArrayObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let spl = self.spl();
        if let Some(r) = spl.call_common(this, method, &args, diag) {
            return r;
        }
        match method {
            "__construct" => {
                if args.is_empty() {
                    return Ok(Value::Null);
                }
                let a = args.bind(
                    "ArrayObject::__construct",
                    &[
                        Param::optional("array", Value::Array(Array::new())),
                        Param::optional("flags", 0),
                        Param::optional("iteratorClass", "ArrayIterator"),
                    ],
                    FunctionKind::Internal,
                )?;
                let flags = param::parse_long(
                    &a[1],
                    &Arg::new("ArrayObject::__construct", 2, "flags"),
                    diag,
                )?;
                if args.len() > 2 {
                    let name = param::parse_str(
                        &a[2],
                        &Arg::new("ArrayObject::__construct", 3, "iteratorClass"),
                        diag,
                    )?;
                    *self
                        .iterator_class
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) =
                        Arc::from(name.to_string_lossy().as_ref());
                }
                spl.set_array(&a[0], flags & !INT_MASK, "ArrayObject::__construct")?;
                Ok(Value::Null)
            }
            "getiterator" => {
                args.bind("ArrayObject::getIterator", &[], FunctionKind::Internal)?;
                Ok(Value::Object(
                    ArrayIterator::sharing(
                        self.storage.clone(),
                        self.flags.load(Ordering::Relaxed),
                    )
                    .into_ref(),
                ))
            }
            "exchangearray" => {
                let a = args.bind(
                    "ArrayObject::exchangeArray",
                    &[Param::required("array")],
                    FunctionKind::Internal,
                )?;
                let old = spl.snapshot();
                spl.set_array(&a[0], 0, "ArrayObject::exchangeArray")?;
                Ok(Value::Array(old))
            }
            "setiteratorclass" => {
                let a = args.bind(
                    "ArrayObject::setIteratorClass",
                    &[Param::required("iteratorClass")],
                    FunctionKind::Internal,
                )?;
                let name = param::parse_str(
                    &a[0],
                    &Arg::new("ArrayObject::setIteratorClass", 1, "iteratorClass"),
                    diag,
                )?;
                *self
                    .iterator_class
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) =
                    Arc::from(name.to_string_lossy().as_ref());
                Ok(Value::Null)
            }
            "getiteratorclass" => {
                args.bind("ArrayObject::getIteratorClass", &[], FunctionKind::Internal)?;
                Ok(Value::from(
                    &*self
                        .iterator_class
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone(),
                ))
            }
            _ => Err(super::undefined_method(&self.class, method)),
        }
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn read_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let spl = self.spl();
        if spl.array_as_props(this, name, diag)? {
            return spl.get(&Value::from(name), mode, diag);
        }
        super::props::std_read_property(this, name, mode, diag)
    }

    fn has_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        check: PropertyCheck,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        let spl = self.spl();
        if spl.array_as_props(this, name, diag)? {
            let check_empty = match check {
                PropertyCheck::Isset => 0,
                PropertyCheck::NotEmpty => 1,
                PropertyCheck::Exists => 2,
            };
            return spl.has(&Value::from(name), check_empty, diag);
        }
        super::props::std_has_property(this, name, check, diag)
    }

    fn write_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        let spl = self.spl();
        if spl.array_as_props(this, name, diag)? {
            return spl.set(Some(&Value::from(name)), value, diag);
        }
        super::props::std_write_property(this, name, value, diag)
    }

    fn unset_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        let spl = self.spl();
        if spl.array_as_props(this, name, diag)? {
            return spl.unset(&Value::from(name), diag);
        }
        super::props::std_unset_property(this, name, diag)
    }

    fn properties_for(
        &self,
        this: &ObjectRef,
        purpose: PropertyPurpose,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        Ok(self.spl().properties_for(this, purpose))
    }

    fn read_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        self.spl().get(offset, mode, diag)
    }

    fn has_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        check_empty: bool,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        self.spl().has(offset, u8::from(check_empty), diag)
    }

    fn write_dimension(
        &self,
        _this: &ObjectRef,
        offset: Option<&Value>,
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        self.spl().set(offset, value, diag)
    }

    fn unset_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        self.spl().unset(offset, diag)
    }

    fn count_elements(
        &self,
        _this: &ObjectRef,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<i64>> {
        Ok(Some(lock(&self.storage).len() as i64))
    }

    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(ArrayObject {
            class: self.class.clone(),
            storage: Arc::new(Mutex::new(self.array())),
            flags: AtomicI64::new(self.flags.load(Ordering::Relaxed)),
            iterator_class: Mutex::new(
                self.iterator_class
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone(),
            ),
            props: self.props.clone(),
        }))
    }

    fn compare(
        &self,
        _this: &ObjectRef,
        other: &Value,
        this_is_left: bool,
        compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        self.spl().compare(other, this_is_left, compare_values)
    }
}

#[derive(Default)]
struct IterState {
    entries: Option<Vec<(ArrayKey, Value)>>,
    pos: usize,
}

/// `ArrayIterator`: an `Iterator` over an array (`SeekableIterator`, `ArrayAccess`, `Countable`).
///
/// The position is shared by `foreach` and the `current()`/`key()`/`next()`/`valid()`/`rewind()`
/// methods, like PHP's internal hash position. Iteration walks a snapshot of the storage taken at
/// `rewind()` (or first use): changes made to the storage while iterating are not seen by the
/// running iteration (PHP's hash iterators would see them).
pub struct ArrayIterator {
    class: ClassRef,
    storage: Storage,
    flags: AtomicI64,
    state: Mutex<IterState>,
    props: PropertyStore,
}

impl ArrayIterator {
    /// `new ArrayIterator($array)`.
    pub fn new(array: Array) -> ArrayIterator {
        ArrayIterator::sharing(Arc::new(Mutex::new(array)), 0)
    }

    fn sharing(storage: Storage, flags: i64) -> ArrayIterator {
        let class = builtin::array_iterator();
        let props = PropertyStore::new(&class);
        ArrayIterator {
            class,
            storage,
            flags: AtomicI64::new(flags & !INT_MASK),
            state: Mutex::new(IterState::default()),
            props,
        }
    }

    /// Wraps into a handle.
    pub fn into_ref(self) -> ObjectRef {
        ObjectRef::new(self)
    }

    /// A copy of the storage (`getArrayCopy()`).
    pub fn array(&self) -> Array {
        lock(&self.storage).clone()
    }

    fn spl(&self) -> Spl<'_> {
        Spl {
            class: &self.class,
            storage: &self.storage,
            flags: &self.flags,
        }
    }

    fn state(&self) -> MutexGuard<'_, IterState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn rewind(&self) {
        let entries: Vec<(ArrayKey, Value)> = lock(&self.storage)
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut st = self.state();
        st.entries = Some(entries);
        st.pos = 0;
    }

    fn ensure(&self) {
        if self.state().entries.is_none() {
            self.rewind();
        }
    }

    fn valid(&self) -> bool {
        self.ensure();
        let st = self.state();
        st.entries.as_ref().is_some_and(|e| st.pos < e.len())
    }

    fn current(&self) -> Value {
        self.ensure();
        let st = self.state();
        st.entries
            .as_ref()
            .and_then(|e| e.get(st.pos))
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Null)
    }

    fn key(&self) -> Value {
        self.ensure();
        let st = self.state();
        st.entries
            .as_ref()
            .and_then(|e| e.get(st.pos))
            .map(|(k, _)| k.to_value())
            .unwrap_or(Value::Null)
    }

    fn next(&self) {
        self.ensure();
        let mut st = self.state();
        let len = st.entries.as_ref().map_or(0, Vec::len);
        if st.pos < len {
            st.pos += 1;
        }
    }
}

impl Object for ArrayIterator {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let spl = self.spl();
        if let Some(r) = spl.call_common(this, method, &args, diag) {
            return r;
        }
        let bind0 =
            |m: &str| args.bind(&format!("ArrayIterator::{m}"), &[], FunctionKind::Internal);
        match method {
            "__construct" => {
                if args.is_empty() {
                    return Ok(Value::Null);
                }
                let a = args.bind(
                    "ArrayIterator::__construct",
                    &[
                        Param::optional("array", Value::Array(Array::new())),
                        Param::optional("flags", 0),
                    ],
                    FunctionKind::Internal,
                )?;
                let flags = param::parse_long(
                    &a[1],
                    &Arg::new("ArrayIterator::__construct", 2, "flags"),
                    diag,
                )?;
                spl.set_array(&a[0], flags & !INT_MASK, "ArrayIterator::__construct")?;
                *self.state() = IterState::default();
                Ok(Value::Null)
            }
            "rewind" => {
                bind0("rewind")?;
                self.rewind();
                Ok(Value::Null)
            }
            "valid" => {
                bind0("valid")?;
                Ok(Value::Bool(self.valid()))
            }
            "current" => {
                bind0("current")?;
                Ok(self.current())
            }
            "key" => {
                bind0("key")?;
                Ok(self.key())
            }
            "next" => {
                bind0("next")?;
                self.next();
                Ok(Value::Null)
            }
            "seek" => {
                let a = args.bind(
                    "ArrayIterator::seek",
                    &[Param::required("offset")],
                    FunctionKind::Internal,
                )?;
                let position =
                    param::parse_long(&a[0], &Arg::new("ArrayIterator::seek", 1, "offset"), diag)?;
                if position >= 0 {
                    self.rewind();
                    let mut n = position;
                    while n > 0 && self.valid() {
                        self.next();
                        n -= 1;
                    }
                    if self.valid() {
                        return Ok(Value::Null);
                    }
                }
                Err(PhpError::new(
                    ThrowableClass::OutOfBoundsException,
                    format!("Seek position {position} is out of range"),
                ))
            }
            _ => Err(super::undefined_method(&self.class, method)),
        }
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn read_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let spl = self.spl();
        if spl.array_as_props(this, name, diag)? {
            return spl.get(&Value::from(name), mode, diag);
        }
        super::props::std_read_property(this, name, mode, diag)
    }

    fn has_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        check: PropertyCheck,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        let spl = self.spl();
        if spl.array_as_props(this, name, diag)? {
            let check_empty = match check {
                PropertyCheck::Isset => 0,
                PropertyCheck::NotEmpty => 1,
                PropertyCheck::Exists => 2,
            };
            return spl.has(&Value::from(name), check_empty, diag);
        }
        super::props::std_has_property(this, name, check, diag)
    }

    fn properties_for(
        &self,
        this: &ObjectRef,
        purpose: PropertyPurpose,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        Ok(self.spl().properties_for(this, purpose))
    }

    fn read_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        self.spl().get(offset, mode, diag)
    }

    fn has_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        check_empty: bool,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        self.spl().has(offset, u8::from(check_empty), diag)
    }

    fn write_dimension(
        &self,
        _this: &ObjectRef,
        offset: Option<&Value>,
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        self.spl().set(offset, value, diag)
    }

    fn unset_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        self.spl().unset(offset, diag)
    }

    fn count_elements(
        &self,
        _this: &ObjectRef,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<i64>> {
        Ok(Some(lock(&self.storage).len() as i64))
    }

    fn get_iterator(
        &self,
        this: &ObjectRef,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Box<dyn ObjectIterator>>> {
        Ok(Some(Box::new(SplIterator {
            object: this.clone(),
        })))
    }

    /// `spl_array_object_clone()`: a cloned `ArrayIterator` iterates the original's storage.
    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        let it = ArrayIterator::sharing(self.storage.clone(), self.flags.load(Ordering::Relaxed));
        Ok(ObjectRef::new(ArrayIterator {
            props: self.props.clone(),
            ..it
        }))
    }

    fn compare(
        &self,
        _this: &ObjectRef,
        other: &Value,
        this_is_left: bool,
        compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        self.spl().compare(other, this_is_left, compare_values)
    }
}

/// `spl_array_it_funcs`: drives the `ArrayIterator`'s own position.
struct SplIterator {
    object: ObjectRef,
}

impl SplIterator {
    fn it(&self) -> &ArrayIterator {
        match self.object.downcast_ref::<ArrayIterator>() {
            Some(it) => it,
            None => unreachable!("SplIterator wraps an ArrayIterator"),
        }
    }
}

impl ObjectIterator for SplIterator {
    fn rewind(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.it().rewind();
        Ok(())
    }

    fn valid(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        Ok(self.it().valid())
    }

    fn current(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        Ok(self.it().current())
    }

    fn key(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        Ok(self.it().key())
    }

    fn next(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.it().next();
        Ok(())
    }
}
