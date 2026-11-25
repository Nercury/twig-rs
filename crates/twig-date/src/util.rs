//! Helpers shared by the date objects: access to the standard (`zend_std_*`) property handlers, property-table
//! updates and the `update_property()` / `add_common_properties()` routines of php_date.c.

use std::sync::LazyLock;

use twig_value::object::builtin::std_class;
use twig_value::object::{
    ClassRef, FetchMode, Object, ObjectRef, PropertyCheck, PropertyEntry, PropertyPurpose,
    unmangle_property_name,
};
use twig_value::{Array, ArrayKey, Diagnostics, PhpResult, PhpString, Value};

/// An object type that keeps every default handler of [`Object`]. The defaults implement the standard handlers
/// on the `this` handle they receive, so calling them with another object's handle runs `zend_std_*` for that
/// object (the date objects override a handler and fall back to the standard one, like php_date.c does).
struct StdHandlers(ClassRef);

impl Object for StdHandlers {
    fn class(&self) -> &ClassRef {
        &self.0
    }
}

static STD: LazyLock<StdHandlers> = LazyLock::new(|| StdHandlers(std_class()));

/// `zend_std_read_property()`.
pub(crate) fn std_read_property(
    this: &ObjectRef,
    name: &[u8],
    mode: FetchMode,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    STD.read_property(this, name, mode, diag)
}

/// `zend_std_has_property()`.
pub(crate) fn std_has_property(
    this: &ObjectRef,
    name: &[u8],
    check: PropertyCheck,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    STD.has_property(this, name, check, diag)
}

/// `zend_std_write_property()`.
pub(crate) fn std_write_property(
    this: &ObjectRef,
    name: &[u8],
    value: Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    STD.write_property(this, name, value, diag)
}

/// `zend_std_get_properties()`: the object's own property table (declared slots, then dynamic properties).
pub(crate) fn std_properties(
    this: &ObjectRef,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Vec<PropertyEntry>> {
    STD.properties_for(this, PropertyPurpose::GetObjectVars, diag)
}

/// `zend_hash_str_update(props, key, value)`: replaces the value in place or appends.
pub(crate) fn update_prop(props: &mut Vec<PropertyEntry>, key: &str, value: Value) {
    let k = ArrayKey::Str(PhpString::from(key));
    match props.iter_mut().find(|e| e.key == k) {
        Some(e) => e.value = Some(value),
        None => props.push(PropertyEntry {
            key: k,
            value: Some(value),
            type_name: None,
        }),
    }
}

/// Port of `add_common_properties()` (PHPSRC/ext/date/php_date.c): appends the object's own properties to a
/// `__serialize()` array, skipping keys already present (`zend_hash_add()`).
pub(crate) fn add_common_properties(
    out: &mut Array,
    this: &ObjectRef,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    for e in std_properties(this, diag)? {
        let (ArrayKey::Str(name), Some(v)) = (e.key, e.value) else {
            continue;
        };
        let key = ArrayKey::Str(name);
        if out.get(&key).is_none() {
            out.insert(key, v);
        }
    }
    Ok(())
}

/// Port of `update_property()` (PHPSRC/ext/date/php_date.c) as used by the `restore_custom_*_properties()`
/// helpers: writes a property from an unserialization array. Mangled names (`"\0*\0p"`, `"\0Class\0p"`) write
/// the unmangled property (the object's own code may access it); malformed mangled names are ignored.
pub(crate) fn update_property(
    this: &ObjectRef,
    key: &[u8],
    value: Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    if key.first() == Some(&0) {
        // not public
        let (class, name) = unmangle_property_name(key);
        if class.is_none() {
            return Ok(());
        }
        if let Some(store) = this.object().property_store() {
            store.set(name, value);
        }
        return Ok(());
    }
    // public
    this.write_property(key, value, diag)
}

/// `ZEND_HASH_FOREACH_STR_KEY_VAL(myht, ...)` over the string keys of an array, skipping the internal ones, then
/// [`update_property`] (`restore_custom_datetime_properties()` & co.).
pub(crate) fn restore_custom_properties(
    this: &ObjectRef,
    data: &Array,
    is_internal: impl Fn(&[u8]) -> bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    for (k, v) in data {
        let ArrayKey::Str(name) = k else { continue };
        if is_internal(name.as_bytes()) {
            continue;
        }
        update_property(this, name.as_bytes(), v.clone(), diag)?;
    }
    Ok(())
}

/// An object whose only job is to report a warning when cast to `int` (see [`warn_from_compare`]).
struct WarningRelay {
    class: ClassRef,
    message: PhpString,
}

impl Object for WarningRelay {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn cast(
        &self,
        _this: &ObjectRef,
        _target: twig_value::object::CastTarget,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Value>> {
        diag.warn(self.message.clone())?;
        Ok(Some(Value::Int(0)))
    }
}

/// Reports `message` as an `E_WARNING` from inside an [`Object::compare`] handler, which receives no
/// diagnostics sink: the engine's comparison (`compare_values`) casts a relay object to `int` with the
/// comparison's own sink, and the relay's cast handler reports the warning there. An `Err` returned by the sink
/// (a throwing error handler) is passed on.
pub(crate) fn warn_from_compare(
    compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    message: &str,
) -> PhpResult<()> {
    static RELAY_CLASS: LazyLock<ClassRef> =
        LazyLock::new(|| twig_value::object::ClassBuilder::class("DateWarningRelay").build());
    let relay = ObjectRef::new(WarningRelay {
        class: RELAY_CLASS.clone(),
        message: PhpString::from(message),
    });
    compare_values(&Value::Object(relay), &Value::Int(0)).map(|_| ())
}

/// The deprecation PHP 8.5 raises when a `#[\Deprecated]` `__wakeup()` of a date class is called
/// (`zend_deprecated_function()`).
pub(crate) fn wakeup_deprecated(class: &str, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    diag.deprecated(format!(
        "Method {class}::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize()"
    ))
}

/// An array from `(key, value)` pairs in order (`add_assoc_*()`: symtable keys).
pub(crate) fn assoc<I, K>(pairs: I) -> Array
where
    I: IntoIterator<Item = (K, Value)>,
    K: Into<ArrayKey>,
{
    let mut a = Array::new();
    for (k, v) in pairs {
        a.insert(k, v);
    }
    a
}
