//! PHP objects: the [`Object`] protocol, handles ([`ObjectRef`]), classes ([`ClassEntry`]) and the
//! built-in classes (`stdClass`, `Closure`, enums, `ArrayObject`, `ArrayIterator`).
//!
//! # Model
//!
//! PHP splits an object into a class entry (static: name, parents, interfaces, constants, declared
//! properties and methods) and per-object handlers (`zend_object_handlers`). This module mirrors
//! that split:
//!
//! * [`ClassEntry`] (built with [`ClassBuilder`], shared as [`ClassRef`]) is the class descriptor.
//! * [`Object`] is the handler table. Every handler has a default that implements PHP's *standard*
//!   behaviour (`zend_std_*`) in terms of the class entry, an optional [`PropertyStore`] and the
//!   object's methods: `ArrayAccess` dimension access calls `offsetGet`/`offsetExists`/...,
//!   `Countable` calls `count`, `Iterator`/`IteratorAggregate` objects are traversed through their
//!   methods, string conversion calls `__toString`, missing properties fall back to
//!   `__get`/`__isset`/`__set`/`__unset`. A host type implements [`Object::class`] and
//!   [`Object::call`] (a `match` on the lowercase method name) and overrides handlers only where an
//!   internal PHP class has custom handlers (like `ArrayObject`). [`ObjectBuilder`] makes objects of
//!   a class of public properties and closure methods without a hand-written implementation.
//! * [`ObjectRef`] is the handle stored in [`Value::Object`](crate::Value): cheap to clone, shared,
//!   with an identity ([`ObjectRef::id`], [`ObjectRef::ptr_eq`]). Its inherent methods are the
//!   engine-level operations (`$obj->m()`, `$obj->p`, `isset($obj->p)`, `$obj[$k]`, `(string)
//!   $obj`, `clone`, `foreach`) with PHP's lookup rules and verbatim errors.
//!
//! All operations run "from a foreign scope" (the caller is not the object's class), which is how
//! Twig's `CoreExtension` accesses template data: private and protected members are inaccessible.
//!
//! Objects must be `Send + Sync`; implementations use interior mutability for mutable state.

mod args;
mod array_object;
mod builder;
pub mod builtin;
mod class;
mod closure;
mod enum_case;
mod iter;
mod props;
mod std_class;

pub use args::{CallArgs, CalleeParams, FunctionKind, MethodParams, Param};
pub use array_object::{ArrayIterator, ArrayObject};
pub use builder::ObjectBuilder;
pub use class::{
    ClassBuilder, ClassEntry, ClassKind, ClassRef, ConstantInfo, EnumBacking, MethodInfo,
    PropertyInfo, Visibility,
};
pub use closure::{Closure, ClosureFn};
pub use enum_case::{EnumCase, enum_from};
pub use iter::{MethodIterator, ObjectIterator, iterator_count, iterator_to_array, traverse};
pub use props::{PropertyEntry, PropertyStore, mangle_property_name, unmangle_property_name};
pub use std_class::StdClass;

use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::array::Array;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult, ThrowableClass};
use crate::string::PhpString;
use crate::value::Value;

/// Fetch mode of a property or dimension read (`BP_VAR_R` vs `BP_VAR_IS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchMode {
    /// Plain read (`$o->p`, `$o[$k]`): undefined members warn.
    Read,
    /// Quiet read for `isset()`/`??` (`BP_VAR_IS`): no warnings for undefined members.
    Isset,
}

/// What `has_property` checks (`ZEND_PROPERTY_ISSET`, `ZEND_PROPERTY_NOT_EMPTY`,
/// `ZEND_PROPERTY_EXISTS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyCheck {
    /// `isset($o->p)`: exists and is not null.
    Isset,
    /// `!empty($o->p)`: exists and is truthy.
    NotEmpty,
    /// Exists, even when null (never consults `__isset`).
    Exists,
}

/// Target of a `cast_object` handler call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastTarget {
    /// `IS_STRING`
    String,
    /// `_IS_BOOL`
    Bool,
    /// `IS_LONG`
    Long,
    /// `IS_DOUBLE`
    Double,
    /// `_IS_NUMBER` (int or float)
    Number,
}

impl CastTarget {
    /// The type name used in `Object of class %s could not be converted to %s`
    /// (`zend_get_type_by_const`).
    pub fn type_name(self) -> &'static str {
        match self {
            CastTarget::String => "string",
            CastTarget::Bool => "bool",
            CastTarget::Long => "int",
            CastTarget::Double => "float",
            CastTarget::Number => "int|float",
        }
    }
}

/// Why a property table is requested (`zend_prop_purpose`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyPurpose {
    /// `var_dump()`, `print_r()`, `debug_zval_dump()` (uses `get_debug_info`).
    Debug,
    /// `(array)` cast.
    ArrayCast,
    /// `serialize()`.
    Serialize,
    /// `var_export()`.
    VarExport,
    /// `json_encode()`.
    Json,
    /// `get_object_vars()` and `foreach` over plain objects.
    GetObjectVars,
}

/// The PHP object handler table. See the [module documentation](self) for the model.
///
/// Every method receiving `this` gets the object's own handle (needed to return `$this`, to call
/// the object's methods from a default handler, or to create iterators referencing it).
pub trait Object: Any + Send + Sync {
    /// The object's class.
    fn class(&self) -> &ClassRef;

    /// Invokes a method of the object's class. `method` is the **lowercase** method name, already
    /// resolved against the class's method table (visibility checks and the `__call` fallback
    /// happen in [`ObjectRef::call_method`]). Magic methods (`__get`, `__isset`, `__set`,
    /// `__unset`, `__call`, `__toString`, `__invoke`) arrive here too when the class declares them.
    ///
    /// The default throws `Call to undefined method Class::method()`.
    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let _ = (this, args, diag);
        Err(undefined_method(self.class(), method))
    }

    /// The parameters of the method `method` (its **lowercase** name, as [`Object::call`] gets it)
    /// as the class declares them, so that the engine can send the arguments of a call the way PHP
    /// does. PHP checks a named argument against the parameters of the called function as it sends
    /// it (`zend_handle_named_arg()`, PHPSRC/Zend/zend_execute.c): in `$object->method(...$args)`
    /// an unknown name fails at its element of `$args`, before a positional element after it is
    /// looked at (`ZEND_SEND_UNPACK`). [`ObjectRef::method_params`] resolves the method of a call.
    ///
    /// `None` (the default) means unknown: named arguments are then sent unchecked, and the method's
    /// own binding ([`CallArgs::bind`]) reports an unknown name, after the elements PHP would not
    /// have reached. A class whose methods bind their arguments with fixed [`Param`] lists returns
    /// those lists.
    fn method_params(&self, method: &str) -> Option<MethodParams> {
        let _ = method;
        None
    }

    /// Backing store for declared and dynamic properties used by the standard property handlers.
    /// Objects without properties return `None` (the default).
    fn property_store(&self) -> Option<&PropertyStore> {
        None
    }

    /// `read_property` (`$obj->name`). Default: `zend_std_read_property()`.
    fn read_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        props::std_read_property(this, name, mode, diag)
    }

    /// `has_property` (`isset($obj->name)`, `empty()`, exists checks). Default:
    /// `zend_std_has_property()`.
    fn has_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        check: PropertyCheck,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        props::std_has_property(this, name, check, diag)
    }

    /// `write_property` (`$obj->name = value`). Default: `zend_std_write_property()`.
    fn write_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        props::std_write_property(this, name, value, diag)
    }

    /// `unset_property` (`unset($obj->name)`). Default: `zend_std_unset_property()`.
    fn unset_property(
        &self,
        this: &ObjectRef,
        name: &[u8],
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        props::std_unset_property(this, name, diag)
    }

    /// `get_properties_for`: the property table for `purpose`, keyed by mangled names
    /// (`"\0Class\0name"` for private, `"\0*\0name"` for protected) in slot order followed by
    /// dynamic properties. Default: the standard table of the [`PropertyStore`].
    fn properties_for(
        &self,
        this: &ObjectRef,
        purpose: PropertyPurpose,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        let _ = (purpose, diag);
        Ok(props::std_properties(this))
    }

    /// `read_dimension` (`$obj[$offset]`). Default: `zend_std_read_dimension()` (`offsetGet`, with an
    /// `offsetExists` check first in [`FetchMode::Isset`]; `Cannot use object of type X as array`
    /// for classes without `ArrayAccess`).
    fn read_dimension(
        &self,
        this: &ObjectRef,
        offset: &Value,
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        std_read_dimension(this, offset, mode, diag)
    }

    /// `has_dimension` (`isset($obj[$offset])` / `empty()`). Default: `zend_std_has_dimension()`.
    fn has_dimension(
        &self,
        this: &ObjectRef,
        offset: &Value,
        check_empty: bool,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        std_has_dimension(this, offset, check_empty, diag)
    }

    /// `write_dimension` (`$obj[$offset] = v`; `None` for `$obj[] = v`). Default:
    /// `zend_std_write_dimension()` (`offsetSet`).
    fn write_dimension(
        &self,
        this: &ObjectRef,
        offset: Option<&Value>,
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        let class = this.class();
        if !class.is_subclass_of("ArrayAccess") {
            return Err(bad_array_access(class));
        }
        let offset = offset.cloned().unwrap_or(Value::Null);
        this.invoke_method("offsetset", CallArgs::from(vec![offset, value]), diag)
            .map(|_| ())
    }

    /// `unset_dimension` (`unset($obj[$offset])`). Default: `zend_std_unset_dimension()`
    /// (`offsetUnset`).
    fn unset_dimension(
        &self,
        this: &ObjectRef,
        offset: &Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        let class = this.class();
        if !class.is_subclass_of("ArrayAccess") {
            return Err(bad_array_access(class));
        }
        this.invoke_method("offsetunset", CallArgs::from(vec![offset.clone()]), diag)
            .map(|_| ())
    }

    /// `count_elements` handler; `Ok(None)` when the object has none (the default). `count()` then
    /// falls back to the `Countable::count()` method (see [`ObjectRef::count`]).
    fn count_elements(
        &self,
        this: &ObjectRef,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<i64>> {
        let _ = (this, diag);
        Ok(None)
    }

    /// `get_iterator` of the class (`foreach`, `iterator_to_array()`); `Ok(None)` for classes that
    /// are not `Traversable` (then `foreach` walks the visible properties). Default: user
    /// `Iterator` classes are driven through `rewind`/`valid`/`current`/`key`/`next`
    /// (`zend_user_it_get_iterator`), `IteratorAggregate` classes through `getIterator()`
    /// (`zend_user_it_get_new_iterator`).
    fn get_iterator(
        &self,
        this: &ObjectRef,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Box<dyn ObjectIterator>>> {
        iter::std_get_iterator(this, diag)
    }

    /// `cast_object`: `Ok(None)` is `FAILURE` without an exception. Default:
    /// `zend_std_cast_object_tostring()` — `__toString()` for strings, `true` for bool, failure
    /// otherwise.
    fn cast(
        &self,
        this: &ObjectRef,
        target: CastTarget,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Value>> {
        std_cast(this, target, diag)
    }

    /// `clone_obj` (`clone $obj`). Default: `Trying to clone an uncloneable object of class X`.
    fn clone_object(&self, this: &ObjectRef, diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        let _ = diag;
        Err(PhpError::error(format!(
            "Trying to clone an uncloneable object of class {}",
            this.class().display_name()
        )))
    }

    /// Custom `compare` handler for `$this <=> $other` (`this_is_left` tells the operand order;
    /// `zend_compare()` has already handled identity, `null` and `bool` operands). `None` (the
    /// default) selects `zend_std_compare_objects()`. `compare_values` is the engine's
    /// `zend_compare()` for recursive comparisons (it reports its own diagnostics).
    ///
    /// Enums and closures return "uncomparable" (`Some(Ok(1))`); `ArrayObject` compares its storage
    /// first and returns `None` when equal so the standard comparison runs next, like
    /// `spl_array_compare_objects()`.
    fn compare(
        &self,
        this: &ObjectRef,
        other: &Value,
        this_is_left: bool,
        compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        let _ = (this, other, this_is_left, compare_values);
        None
    }
}

/// `Call to undefined method Class::method()` (`zend_undefined_method`).
pub fn undefined_method(class: &ClassEntry, method: &str) -> PhpError {
    PhpError::error(format!(
        "Call to undefined method {}::{}()",
        class.display_name(),
        method
    ))
}

/// `Cannot use object of type X as array` (`zend_bad_array_access`).
pub fn bad_array_access(class: &ClassEntry) -> PhpError {
    PhpError::error(format!(
        "Cannot use object of type {} as array",
        class.display_name()
    ))
}

static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(1);

struct ObjectCell<T: ?Sized> {
    id: u64,
    obj: T,
}

/// A handle to a PHP object (`zend_object *`): cloning shares the object.
#[derive(Clone)]
pub struct ObjectRef(Arc<ObjectCell<dyn Object>>);

impl ObjectRef {
    /// Wraps an object, assigning a fresh object id.
    pub fn new<T: Object>(obj: T) -> ObjectRef {
        let id = NEXT_OBJECT_ID.fetch_add(1, Ordering::Relaxed);
        ObjectRef(Arc::new(ObjectCell { id, obj }))
    }

    /// The object id (`spl_object_id()`, the `#N` of `var_dump()`). Ids are unique per process and
    /// never reused, unlike PHP's per-request handle numbers.
    pub fn id(&self) -> u64 {
        self.0.id
    }

    /// Identity (`===` on objects).
    pub fn ptr_eq(a: &ObjectRef, b: &ObjectRef) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// The handler table.
    pub fn object(&self) -> &dyn Object {
        &self.0.obj
    }

    /// Downcasts to a concrete object type.
    pub fn downcast_ref<T: Object>(&self) -> Option<&T> {
        let any: &dyn Any = &self.0.obj;
        any.downcast_ref::<T>()
    }

    /// The object's class.
    pub fn class(&self) -> &ClassRef {
        self.0.obj.class()
    }

    /// The class name (`get_class()`, `$obj::class`).
    pub fn class_name(&self) -> &str {
        self.class().name()
    }

    /// `$obj instanceof Name` (case-insensitive; parents and interfaces included).
    pub fn instance_of(&self, class_name: &str) -> bool {
        self.class().is_subclass_of(class_name)
    }

    /// The enum case name, when this is an enum case.
    pub fn enum_case_name(&self) -> Option<&str> {
        self.downcast_ref::<EnumCase>().map(EnumCase::name)
    }

    /// The backing value, when this is a case of a backed enum.
    pub fn enum_case_value(&self) -> Option<&Value> {
        self.downcast_ref::<EnumCase>().and_then(EnumCase::value)
    }

    /// Calls a method by lowercase name without visibility checks or `__call` fallback (the
    /// engine-internal `zend_call_known_instance_method`).
    pub fn invoke_method(
        &self,
        lc_method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        self.0.obj.call(self, lc_method, args, diag)
    }

    /// `$obj->name(...args)` from a foreign scope: case-insensitive lookup in the class's method
    /// table; a missing or non-public method falls back to `__call($name, $args)` when the class
    /// has one; otherwise `Error` (`Call to undefined method X::name()` /
    /// `Call to private method X::name() from global scope`). Port of `zend_std_get_method()`; a
    /// `Closure` also answers `__invoke` (`zend_closure_get_method()`).
    pub fn call_method(
        &self,
        name: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        if name.eq_ignore_ascii_case("__invoke")
            && let Some(closure) = self.downcast_ref::<Closure>()
        {
            return closure.invoke(args, diag);
        }
        let class = self.class();
        match class.method_entry(name) {
            Some((lc, m)) if m.visibility == Visibility::Public => {
                self.invoke_method(lc, args, diag)
            }
            found => {
                let found = found.map(|(_, m)| m);
                if class.method("__call").is_some() {
                    let call_args =
                        CallArgs::from(vec![Value::from(name), Value::Array(args.to_array())]);
                    return self.invoke_method("__call", call_args, diag);
                }
                match found {
                    Some(m) => Err(PhpError::error(format!(
                        "Call to {} method {}::{}() from global scope",
                        m.visibility.as_str(),
                        m.declaring_class,
                        name
                    ))),
                    None => Err(undefined_method(class, name)),
                }
            }
        }
    }

    /// The parameters of the function `$obj->name(...)` calls, found like [`ObjectRef::call_method`]
    /// finds it (`zend_std_get_method()`): a public method's [`Object::method_params`]; for a
    /// missing or non-public method of a class with `__call()`, the trampoline
    /// ([`MethodParams::call_trampoline`]); a `Closure`'s `__invoke` answers for itself. `None` when
    /// they are unknown, or when the call fails before any argument is sent (no such method).
    pub fn method_params(&self, name: &str) -> Option<MethodParams> {
        if name.eq_ignore_ascii_case("__invoke") && self.downcast_ref::<Closure>().is_some() {
            return self.0.obj.method_params("__invoke");
        }
        let class = self.class();
        match class.method_entry(name) {
            Some((lc, m)) if m.visibility == Visibility::Public => self.0.obj.method_params(lc),
            _ if class.method("__call").is_some() => Some(MethodParams::call_trampoline()),
            _ => None,
        }
    }

    /// Whether the class has a method (case-insensitive, any visibility; `method_exists()`).
    pub fn has_method(&self, name: &str) -> bool {
        self.class().method(name).is_some()
    }

    /// `$obj->name` (`BP_VAR_R`) or the quiet read of `isset()`/`??` (`BP_VAR_IS`).
    pub fn read_property(
        &self,
        name: &[u8],
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        self.0.obj.read_property(self, name, mode, diag)
    }

    /// `has_property` handler call.
    pub fn has_property(
        &self,
        name: &[u8],
        check: PropertyCheck,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        self.0.obj.has_property(self, name, check, diag)
    }

    /// `$obj->name = value`.
    pub fn write_property(
        &self,
        name: &[u8],
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        self.0.obj.write_property(self, name, value, diag)
    }

    /// `unset($obj->name)`.
    pub fn unset_property(&self, name: &[u8], diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.0.obj.unset_property(self, name, diag)
    }

    /// Port of `property_exists($obj, $name)`: a declared property that is not private to a parent
    /// class, or an existing dynamic property (even when null). Never calls `__isset`.
    pub fn property_exists(&self, name: &[u8], diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        let class = self.class();
        if let Some(p) = class.property(name)
            && (p.visibility != Visibility::Private || *p.declaring_class == *class.name())
        {
            return Ok(true);
        }
        self.has_property(name, PropertyCheck::Exists, diag)
    }

    /// The property table for `purpose` (see [`Object::properties_for`]).
    pub fn properties_for(
        &self,
        purpose: PropertyPurpose,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        self.0.obj.properties_for(self, purpose, diag)
    }

    /// The properties visible from a foreign scope, initialised ones only, in table order: what
    /// `foreach` over a plain object and `get_object_vars()` from outside the class see.
    pub fn visible_properties(
        &self,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<(PhpString, Value)>> {
        let table = self.properties_for(PropertyPurpose::GetObjectVars, diag)?;
        Ok(table
            .into_iter()
            .filter_map(|e| {
                let value = e.value?;
                match &e.key {
                    crate::key::ArrayKey::Int(i) => Some((PhpString::from(i.to_string()), value)),
                    crate::key::ArrayKey::Str(s) => {
                        if s.first() == Some(&0) {
                            None
                        } else {
                            Some((s.clone(), value))
                        }
                    }
                }
            })
            .collect())
    }

    /// `$obj[$offset]`.
    pub fn read_dimension(
        &self,
        offset: &Value,
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        self.0.obj.read_dimension(self, offset, mode, diag)
    }

    /// `isset($obj[$offset])` (`check_empty == false`) / `!empty($obj[$offset])`.
    pub fn has_dimension(
        &self,
        offset: &Value,
        check_empty: bool,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        self.0.obj.has_dimension(self, offset, check_empty, diag)
    }

    /// `$obj[$offset] = value` / `$obj[] = value`.
    pub fn write_dimension(
        &self,
        offset: Option<&Value>,
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        self.0.obj.write_dimension(self, offset, value, diag)
    }

    /// `unset($obj[$offset])`.
    pub fn unset_dimension(&self, offset: &Value, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.0.obj.unset_dimension(self, offset, diag)
    }

    /// The object part of `count()`: the `count_elements` handler, else `Countable::count()`
    /// converted with `zval_get_long()`. `Ok(None)` when the object is not countable (the caller
    /// raises `count(): Argument #1 ($value) must be of type Countable|array, X given`).
    pub fn count(&self, diag: &mut dyn Diagnostics) -> PhpResult<Option<i64>> {
        if let Some(n) = self.0.obj.count_elements(self, diag)? {
            return Ok(Some(n));
        }
        if self.instance_of("Countable") {
            let v = self.invoke_method("count", CallArgs::new(), diag)?;
            return crate::convert::to_long(&v, diag).map(Some);
        }
        Ok(None)
    }

    /// The class's `get_iterator` (see [`Object::get_iterator`]).
    pub fn get_iterator(
        &self,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Box<dyn ObjectIterator>>> {
        self.0.obj.get_iterator(self, diag)
    }

    /// `cast_object` handler call.
    pub fn cast(&self, target: CastTarget, diag: &mut dyn Diagnostics) -> PhpResult<Option<Value>> {
        self.0.obj.cast(self, target, diag)
    }

    /// `(string) $obj`: `cast_object(IS_STRING)` or
    /// `Error: Object of class X could not be converted to string`.
    pub fn to_php_string(&self, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
        match self.cast(CastTarget::String, diag)? {
            Some(Value::String(s)) => Ok(s),
            Some(other) => crate::convert::to_string(&other, diag),
            None => Err(PhpError::error(format!(
                "Object of class {} could not be converted to string",
                self.class().display_name()
            ))),
        }
    }

    /// `clone $obj`.
    pub fn clone_object(&self, diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        self.0.obj.clone_object(self, diag)
    }

    /// `$obj(...args)`: a `Closure`, or an object whose class has `__invoke`; else
    /// `Error: Object of type X is not callable`.
    pub fn invoke(&self, args: CallArgs, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        if let Some(closure) = self.downcast_ref::<Closure>() {
            return closure.invoke(args, diag);
        }
        if self.class().method("__invoke").is_some() {
            return self.invoke_method("__invoke", args, diag);
        }
        Err(PhpError::error(format!(
            "Object of type {} is not callable",
            self.class().display_name()
        )))
    }

    /// Whether `$obj(...)` is possible (`is_callable($obj)`).
    pub fn is_callable(&self) -> bool {
        self.downcast_ref::<Closure>().is_some() || self.class().method("__invoke").is_some()
    }
}

impl fmt::Debug for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.enum_case_name() {
            Some(case) => write!(f, "{}::{}", self.class().display_name(), case),
            None => write!(f, "{}#{}", self.class().display_name(), self.id()),
        }
    }
}

impl PartialEq for ObjectRef {
    /// Identity.
    fn eq(&self, other: &ObjectRef) -> bool {
        ObjectRef::ptr_eq(self, other)
    }
}

impl Eq for ObjectRef {}

/// Port of `zend_std_read_dimension()` (PHPSRC/Zend/zend_object_handlers.c).
pub fn std_read_dimension(
    this: &ObjectRef,
    offset: &Value,
    mode: FetchMode,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let class = this.class();
    if !class.is_subclass_of("ArrayAccess") {
        return Err(bad_array_access(class));
    }
    if mode == FetchMode::Isset {
        let exists =
            this.invoke_method("offsetexists", CallArgs::from(vec![offset.clone()]), diag)?;
        if !crate::convert::to_bool(&exists, diag)? {
            return Ok(Value::Null);
        }
    }
    this.invoke_method("offsetget", CallArgs::from(vec![offset.clone()]), diag)
}

/// Port of `zend_std_has_dimension()` (PHPSRC/Zend/zend_object_handlers.c).
pub fn std_has_dimension(
    this: &ObjectRef,
    offset: &Value,
    check_empty: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    let class = this.class();
    if !class.is_subclass_of("ArrayAccess") {
        return Err(bad_array_access(class));
    }
    let exists = this.invoke_method("offsetexists", CallArgs::from(vec![offset.clone()]), diag)?;
    let mut result = crate::convert::to_bool(&exists, diag)?;
    if check_empty && result {
        let v = this.invoke_method("offsetget", CallArgs::from(vec![offset.clone()]), diag)?;
        result = crate::convert::to_bool(&v, diag)?;
    }
    Ok(result)
}

/// Port of `zend_std_cast_object_tostring()` (PHPSRC/Zend/zend_object_handlers.c), including the
/// coercive-mode `string` return type of `__toString()` (`return 5;` gives `"5"`; non-coercible
/// values throw `X::__toString(): Return value must be of type string, Y returned`).
pub fn std_cast(
    this: &ObjectRef,
    target: CastTarget,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<Value>> {
    match target {
        CastTarget::String => {
            let Some(method) = this.class().method("__tostring") else {
                return Ok(None);
            };
            // zend_verify_return_error() names the declaring class (`ZEND_FN_SCOPE_NAME`).
            let scope = method.declaring_class.clone();
            let ret = this.invoke_method("__tostring", CallArgs::new(), diag)?;
            match ret {
                Value::String(_) => Ok(Some(ret)),
                Value::Bool(_) | Value::Int(_) | Value::Float(_) => {
                    Ok(Some(Value::String(crate::convert::to_string(&ret, diag)?)))
                }
                Value::Object(ref o) if o.class().method("__tostring").is_some() => {
                    Ok(Some(Value::String(o.to_php_string(diag)?)))
                }
                other => Err(PhpError::type_error(PhpString::concat(&[
                    scope.split('\0').next().unwrap_or_default().as_bytes(),
                    b"::__toString(): Return value must be of type string, ",
                    crate::types::zend_zval_value_name(&other).as_bytes(),
                    b" returned",
                ]))),
            }
        }
        CastTarget::Bool => Ok(Some(Value::Bool(true))),
        _ => Ok(None),
    }
}

/// Throws `Exception` with `message` (helper for host objects).
pub fn exception(class: ThrowableClass, message: impl Into<PhpString>) -> PhpError {
    PhpError::new(class, message)
}

/// Builds an [`Array`] from property entries the way an `(array)` cast does
/// (`zend_proptable_to_symtable`): mangled names kept, numeric names become int keys,
/// uninitialised typed properties skipped.
pub fn property_entries_to_array(entries: Vec<PropertyEntry>) -> Array {
    let mut out = Array::new();
    for e in entries {
        if let Some(v) = e.value {
            let key = match e.key {
                crate::key::ArrayKey::Str(s) => crate::key::ArrayKey::from_string(s),
                k => k,
            };
            out.insert(key, v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A class with `run($x)` (public), `hidden()` (private) and, when `call` is set, `__call()`.
    struct Probe {
        class: ClassRef,
    }

    impl Object for Probe {
        fn class(&self) -> &ClassRef {
            &self.class
        }

        fn method_params(&self, method: &str) -> Option<MethodParams> {
            match method {
                "run" => Some(MethodParams::user(vec![Param::required("x")])),
                "hidden" => Some(MethodParams::user(Vec::new())),
                _ => None,
            }
        }
    }

    fn probe(call: bool) -> ObjectRef {
        let mut builder = ClassBuilder::class("Probe").method("run").method_with(
            "hidden",
            Visibility::Private,
            false,
        );
        if call {
            builder = builder.method("__call");
        }
        ObjectRef::new(Probe {
            class: builder.build(),
        })
    }

    #[test]
    fn method_params_follow_the_method_lookup() {
        let names = |params: Option<MethodParams>| {
            params.map(|p| p.params.iter().map(|p| p.name).collect::<Vec<_>>())
        };
        let plain = probe(false);
        assert_eq!(names(plain.method_params("RUN")), Some(vec!["x"]));
        assert_eq!(names(plain.method_params("hidden")), None, "not callable");
        assert_eq!(names(plain.method_params("missing")), None);
        let magic = probe(true);
        assert_eq!(names(magic.method_params("run")), Some(vec!["x"]));
        assert_eq!(
            names(magic.method_params("hidden")),
            Some(vec!["arguments"]),
            "a private method is called through the __call() trampoline"
        );
        let trampoline = magic.method_params("missing").unwrap();
        assert!(trampoline.params[0].variadic);
        assert_eq!(trampoline.kind, FunctionKind::User);
        let closure = Closure::new(|_, _| Ok(Value::Null)).into_ref();
        assert!(closure.method_params("__invoke").is_none());
    }
}
