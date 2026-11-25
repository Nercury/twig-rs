//! Property storage and the standard property handlers (`zend_std_read_property` & co.).

use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use indexmap::IndexMap;

use super::class::{ClassEntry, PropertyInfo, Visibility};
use super::{CallArgs, ObjectRef, PropertyCheck};
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::key::ArrayKey;
use crate::object::FetchMode;
use crate::string::PhpString;
use crate::value::Value;

/// One entry of a property table (`get_properties_for`).
#[derive(Clone, Debug)]
pub struct PropertyEntry {
    /// Property-table key: the mangled name (`"\0Class\0name"` private, `"\0*\0name"` protected,
    /// plain name otherwise). Custom tables (`ArrayObject` storage) may use int keys.
    pub key: ArrayKey,
    /// The value; `None` for an uninitialised typed property (`var_dump` prints
    /// `uninitialized(type)`, everything else skips it).
    pub value: Option<Value>,
    /// Declared type of the property, if typed.
    pub type_name: Option<Arc<str>>,
}

/// Port of `zend_mangle_property_name()`: `"\0Class\0name"` for private, `"\0*\0name"` for
/// protected, the plain name for public properties.
pub fn mangle_property_name(
    visibility: Visibility,
    declaring_class: &str,
    name: &[u8],
) -> PhpString {
    match visibility {
        Visibility::Public => PhpString::from(name),
        Visibility::Protected => PhpString::concat(&[b"\0*\0", name]),
        Visibility::Private => PhpString::concat(&[b"\0", declaring_class.as_bytes(), b"\0", name]),
    }
}

/// Port of `zend_unmangle_property_name_ex()`: `(class, name)` where `class` is `Some("*")` for
/// protected, `Some(Class)` for private and `None` for public names.
pub fn unmangle_property_name(key: &[u8]) -> (Option<&[u8]>, &[u8]) {
    if key.len() < 3 || key[0] != 0 {
        return (None, key);
    }
    match key[1..].iter().position(|&b| b == 0) {
        Some(pos) => (Some(&key[1..1 + pos]), &key[2 + pos..]),
        None => (None, key),
    }
}

#[derive(Clone, Debug)]
enum Slot {
    Init(Value),
    /// Typed property never initialised (`IS_PROP_UNINIT`): skips `__get`/`__isset`.
    Uninit,
    /// Explicitly unset: magic methods apply.
    Unset,
}

#[derive(Clone, Debug, Default)]
struct Table {
    slots: Vec<(PhpString, Slot)>,
    dynamic: IndexMap<PhpString, Value>,
}

/// Per-object property storage: one slot per declared instance property of the class (in slot
/// order, defaults applied, typed properties without default uninitialised) plus the dynamic
/// property table. Interior mutability makes it usable from `&self` handlers.
#[derive(Debug, Default)]
pub struct PropertyStore {
    table: RwLock<Table>,
}

impl Clone for PropertyStore {
    fn clone(&self) -> Self {
        PropertyStore {
            table: RwLock::new(self.read().clone()),
        }
    }
}

impl PropertyStore {
    /// Storage for an instance of `class` (`object_properties_init()`).
    pub fn new(class: &ClassEntry) -> PropertyStore {
        let slots = class
            .instance_properties()
            .map(|p| {
                let slot = match &p.default {
                    Some(v) => Slot::Init(v.clone()),
                    None => Slot::Uninit,
                };
                (p.name.clone(), slot)
            })
            .collect();
        PropertyStore {
            table: RwLock::new(Table {
                slots,
                dynamic: IndexMap::new(),
            }),
        }
    }

    /// Storage without declared properties (`stdClass`).
    pub fn empty() -> PropertyStore {
        PropertyStore::default()
    }

    fn read(&self) -> RwLockReadGuard<'_, Table> {
        self.table.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Table> {
        self.table.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Direct read of a property value (declared slot or dynamic), ignoring visibility: for the
    /// object's own code (`$this->name` inside the class). `None` when missing or uninitialised.
    pub fn get(&self, name: &[u8]) -> Option<Value> {
        let t = self.read();
        if let Some((_, slot)) = t.slots.iter().find(|(n, _)| n.as_bytes() == name) {
            return match slot {
                Slot::Init(v) => Some(v.clone()),
                _ => None,
            };
        }
        t.dynamic.get(name).cloned()
    }

    /// Direct write (declared slot or dynamic property), ignoring visibility and deprecations: for
    /// the object's own code (`$this->name = v` inside the class).
    pub fn set(&self, name: &[u8], value: Value) {
        let mut t = self.write();
        if let Some((_, slot)) = t.slots.iter_mut().find(|(n, _)| n.as_bytes() == name) {
            *slot = Slot::Init(value);
            return;
        }
        t.dynamic.insert(PhpString::from(name), value);
    }

    /// Number of dynamic properties.
    pub fn dynamic_len(&self) -> usize {
        self.read().dynamic.len()
    }

    /// The declared slots in slot order, `None` for an uninitialised or unset slot (`IS_UNDEF`):
    /// what `zend_std_compare_objects()` walks.
    pub(crate) fn slot_values(&self) -> Vec<Option<Value>> {
        self.read()
            .slots
            .iter()
            .map(|(_, slot)| match slot {
                Slot::Init(v) => Some(v.clone()),
                Slot::Uninit | Slot::Unset => None,
            })
            .collect()
    }

    /// The dynamic properties in insertion order.
    pub(crate) fn dynamic_entries(&self) -> Vec<(PhpString, Value)> {
        self.read()
            .dynamic
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// A dynamic property by name.
    pub(crate) fn dynamic_value(&self, name: &[u8]) -> Option<Value> {
        self.dynamic_get(name)
    }

    fn slot(&self, name: &[u8]) -> Option<Slot> {
        self.read()
            .slots
            .iter()
            .find(|(n, _)| n.as_bytes() == name)
            .map(|(_, s)| s.clone())
    }

    fn set_slot(&self, name: &[u8], slot: Slot) {
        let mut t = self.write();
        if let Some((_, s)) = t.slots.iter_mut().find(|(n, _)| n.as_bytes() == name) {
            *s = slot;
        }
    }

    fn dynamic_get(&self, name: &[u8]) -> Option<Value> {
        self.read().dynamic.get(name).cloned()
    }

    fn dynamic_contains(&self, name: &[u8]) -> bool {
        self.read().dynamic.contains_key(name)
    }

    fn dynamic_set(&self, name: &[u8], value: Value) {
        self.write().dynamic.insert(PhpString::from(name), value);
    }

    fn dynamic_remove(&self, name: &[u8]) -> bool {
        self.write().dynamic.shift_remove(name).is_some()
    }

    /// The standard property table of this storage for `class` (mangled keys, slot order then
    /// dynamic properties).
    pub fn entries(&self, class: &ClassEntry) -> Vec<PropertyEntry> {
        let t = self.read();
        let mut out = Vec::with_capacity(t.slots.len() + t.dynamic.len());
        for (name, slot) in &t.slots {
            let info = class.property(name);
            let (vis, decl, type_name) = match info {
                Some(p) => (p.visibility, p.declaring_class.clone(), p.type_name.clone()),
                None => (Visibility::Public, Arc::from(class.name()), None),
            };
            let key = ArrayKey::Str(mangle_property_name(vis, &decl, name));
            match slot {
                Slot::Init(v) => out.push(PropertyEntry {
                    key,
                    value: Some(v.clone()),
                    type_name,
                }),
                Slot::Uninit | Slot::Unset => {
                    if type_name.is_some() {
                        out.push(PropertyEntry {
                            key,
                            value: None,
                            type_name,
                        });
                    }
                }
            }
        }
        for (name, v) in &t.dynamic {
            out.push(PropertyEntry {
                key: ArrayKey::Str(name.clone()),
                value: Some(v.clone()),
                type_name: None,
            });
        }
        out
    }
}

/// How a property name resolves for a foreign-scope access (`zend_get_property_offset()`).
enum Offset<'a> {
    /// Declared, accessible instance property.
    Declared(&'a PropertyInfo),
    /// Declared but inaccessible from a foreign scope.
    Wrong(&'a PropertyInfo),
    /// Not declared (or a parent's private property, or a static property).
    Dynamic { is_static: bool },
}

fn property_offset<'a>(class: &'a ClassEntry, name: &[u8]) -> Offset<'a> {
    match class.property(name) {
        Some(p) if p.is_static => Offset::Dynamic { is_static: true },
        Some(p) if p.visibility == Visibility::Private && *p.declaring_class != *class.name() => {
            Offset::Dynamic { is_static: false }
        }
        Some(p) if p.visibility != Visibility::Public => Offset::Wrong(p),
        Some(p) => Offset::Declared(p),
        None => Offset::Dynamic { is_static: false },
    }
}

fn bad_property_access(info: &PropertyInfo, class: &ClassEntry, name: &[u8]) -> PhpError {
    PhpError::error(PhpString::concat(&[
        b"Cannot access ",
        info.visibility.as_str().as_bytes(),
        b" property ",
        class.display_name().as_bytes(),
        b"::$",
        name,
    ]))
}

fn static_as_non_static(
    class: &ClassEntry,
    name: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    diag.notice(PhpString::concat(&[
        b"Accessing static property ",
        class.display_name().as_bytes(),
        b"::$",
        name,
        b" as non static",
    ]))
}

fn name_arg(name: &[u8]) -> CallArgs {
    CallArgs::from(vec![Value::String(PhpString::from(name))])
}

/// Port of `zend_std_read_property()` (PHPSRC/Zend/zend_object_handlers.c) from a foreign scope.
pub(crate) fn std_read_property(
    this: &ObjectRef,
    name: &[u8],
    mode: FetchMode,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let class = this.class().clone();
    let store = this.object().property_store();
    let has_get = class.method("__get").is_some();
    let silent = mode == FetchMode::Isset || has_get;
    let mut typed_info: Option<&PropertyInfo> = None;
    match property_offset(&class, name) {
        Offset::Declared(info) => {
            match store.and_then(|s| s.slot(name)) {
                Some(Slot::Init(v)) => return Ok(v),
                Some(Slot::Uninit) | None => {
                    // Skip __get() for uninitialised typed properties.
                    return uninit_error(
                        &class,
                        info.type_name.is_some().then_some(info),
                        name,
                        mode,
                        diag,
                    );
                }
                Some(Slot::Unset) => {
                    if info.type_name.is_some() {
                        typed_info = Some(info);
                    }
                }
            }
        }
        Offset::Wrong(info) => {
            if !silent {
                return Err(bad_property_access(info, &class, name));
            }
        }
        Offset::Dynamic { is_static } => {
            if is_static && !silent {
                static_as_non_static(&class, name, diag)?;
            }
            if let Some(v) = store.and_then(|s| s.dynamic_get(name)) {
                return Ok(v);
            }
        }
    }
    // magic isset / get
    if mode == FetchMode::Isset && class.method("__isset").is_some() {
        let r = this.invoke_method("__isset", name_arg(name), diag)?;
        if !crate::convert::to_bool(&r, diag)? {
            return Ok(Value::Null);
        }
        if has_get {
            return this.invoke_method("__get", name_arg(name), diag);
        }
    } else if has_get {
        return this.invoke_method("__get", name_arg(name), diag);
    }
    uninit_error(&class, typed_info, name, mode, diag)
}

fn uninit_error(
    class: &ClassEntry,
    typed: Option<&PropertyInfo>,
    name: &[u8],
    mode: FetchMode,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    if mode != FetchMode::Isset {
        match typed {
            Some(info) => {
                return Err(PhpError::error(PhpString::concat(&[
                    b"Typed property ",
                    info.declaring_class.as_bytes(),
                    b"::$",
                    name,
                    b" must not be accessed before initialization",
                ])));
            }
            None => diag.warn(PhpString::concat(&[
                b"Undefined property: ",
                class.display_name().as_bytes(),
                b"::$",
                name,
            ]))?,
        }
    }
    Ok(Value::Null)
}

/// Port of `zend_std_has_property()` (PHPSRC/Zend/zend_object_handlers.c) from a foreign scope.
pub(crate) fn std_has_property(
    this: &ObjectRef,
    name: &[u8],
    check: PropertyCheck,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    let class = this.class().clone();
    let store = this.object().property_store();
    let found = |v: Value, diag: &mut dyn Diagnostics| -> PhpResult<bool> {
        match check {
            PropertyCheck::NotEmpty => crate::convert::to_bool(&v, diag),
            PropertyCheck::Isset => Ok(!v.is_null()),
            PropertyCheck::Exists => Ok(true),
        }
    };
    match property_offset(&class, name) {
        Offset::Declared(_) => match store.and_then(|s| s.slot(name)) {
            Some(Slot::Init(v)) => return found(v, diag),
            Some(Slot::Uninit) | None => return Ok(false),
            Some(Slot::Unset) => {}
        },
        Offset::Wrong(_) => {}
        Offset::Dynamic { .. } => {
            if let Some(v) = store.and_then(|s| s.dynamic_get(name)) {
                return found(v, diag);
            }
        }
    }
    if check != PropertyCheck::Exists && class.method("__isset").is_some() {
        let r = this.invoke_method("__isset", name_arg(name), diag)?;
        let mut result = crate::convert::to_bool(&r, diag)?;
        if check == PropertyCheck::NotEmpty && result {
            if class.method("__get").is_some() {
                let v = this.invoke_method("__get", name_arg(name), diag)?;
                result = crate::convert::to_bool(&v, diag)?;
            } else {
                result = false;
            }
        }
        return Ok(result);
    }
    Ok(false)
}

/// Port of `zend_std_write_property()` (PHPSRC/Zend/zend_object_handlers.c) from a foreign scope.
/// Typed-property coercion is not applied (host objects own their typing).
pub(crate) fn std_write_property(
    this: &ObjectRef,
    name: &[u8],
    value: Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    let class = this.class().clone();
    let store = this.object().property_store();
    let has_set = class.method("__set").is_some();
    match property_offset(&class, name) {
        Offset::Declared(info) => {
            let slot = store.and_then(|s| s.slot(name));
            match slot {
                Some(Slot::Init(_)) if info.is_readonly => {
                    // zend_readonly_property_modification_error()
                    return Err(PhpError::error(PhpString::concat(&[
                        b"Cannot modify readonly property ",
                        info.declaring_class.as_bytes(),
                        b"::$",
                        name,
                    ])));
                }
                Some(Slot::Uninit) | Some(Slot::Unset) if info.is_readonly => {
                    // readonly implies protected(set): zend_asymmetric_visibility_property_modification_error()
                    return Err(PhpError::error(PhpString::concat(&[
                        b"Cannot modify protected(set) readonly property ",
                        info.declaring_class.as_bytes(),
                        b"::$",
                        name,
                        b" from global scope",
                    ])));
                }
                Some(Slot::Unset) if has_set => {
                    this.invoke_method(
                        "__set",
                        CallArgs::from(vec![Value::from(name), value]),
                        diag,
                    )?;
                    return Ok(());
                }
                _ => {}
            }
            if let Some(s) = store {
                s.set_slot(name, Slot::Init(value));
            }
            Ok(())
        }
        Offset::Wrong(info) => {
            if !has_set {
                return Err(bad_property_access(info, &class, name));
            }
            this.invoke_method(
                "__set",
                CallArgs::from(vec![Value::from(name), value]),
                diag,
            )?;
            Ok(())
        }
        Offset::Dynamic { is_static } => {
            if is_static && !has_set {
                static_as_non_static(&class, name, diag)?;
            }
            if let Some(s) = store
                && s.dynamic_contains(name)
            {
                s.dynamic_set(name, value);
                return Ok(());
            }
            if has_set {
                this.invoke_method(
                    "__set",
                    CallArgs::from(vec![Value::from(name), value]),
                    diag,
                )?;
                return Ok(());
            }
            let store = match store {
                Some(s) if !class.is_enum() => s,
                _ => {
                    return Err(PhpError::error(PhpString::concat(&[
                        b"Cannot create dynamic property ",
                        class.display_name().as_bytes(),
                        b"::$",
                        name,
                    ])));
                }
            };
            if !class.allows_dynamic_properties() {
                diag.deprecated(PhpString::concat(&[
                    b"Creation of dynamic property ",
                    class.display_name().as_bytes(),
                    b"::$",
                    name,
                    b" is deprecated",
                ]))?;
            }
            store.dynamic_set(name, value);
            Ok(())
        }
    }
}

/// Port of `zend_std_unset_property()` (PHPSRC/Zend/zend_object_handlers.c) from a foreign scope.
pub(crate) fn std_unset_property(
    this: &ObjectRef,
    name: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    let class = this.class().clone();
    let store = this.object().property_store();
    let has_unset = class.method("__unset").is_some();
    match property_offset(&class, name) {
        Offset::Declared(info) => match store.and_then(|s| s.slot(name)) {
            slot @ (Some(Slot::Init(_)) | Some(Slot::Uninit)) => {
                if info.is_readonly {
                    // zend_readonly_property_unset_error() when initialised, else the protected(set)
                    // visibility error
                    return Err(PhpError::error(if matches!(slot, Some(Slot::Init(_))) {
                        PhpString::concat(&[
                            b"Cannot unset readonly property ",
                            info.declaring_class.as_bytes(),
                            b"::$",
                            name,
                        ])
                    } else {
                        PhpString::concat(&[
                            b"Cannot unset protected(set) readonly property ",
                            info.declaring_class.as_bytes(),
                            b"::$",
                            name,
                            b" from global scope",
                        ])
                    }));
                }
                if let Some(s) = store {
                    s.set_slot(name, Slot::Unset);
                }
                Ok(())
            }
            _ => {
                if has_unset {
                    this.invoke_method("__unset", name_arg(name), diag)?;
                }
                Ok(())
            }
        },
        Offset::Wrong(info) => {
            if !has_unset {
                return Err(bad_property_access(info, &class, name));
            }
            this.invoke_method("__unset", name_arg(name), diag)?;
            Ok(())
        }
        Offset::Dynamic { .. } => {
            if let Some(s) = store
                && s.dynamic_remove(name)
            {
                return Ok(());
            }
            if has_unset {
                this.invoke_method("__unset", name_arg(name), diag)?;
            }
            Ok(())
        }
    }
}

/// The standard property table (`zend_std_get_properties()`), empty without a store.
pub(crate) fn std_properties(this: &ObjectRef) -> Vec<PropertyEntry> {
    match this.object().property_store() {
        Some(store) => store.entries(this.class()),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangling() {
        assert_eq!(
            mangle_property_name(Visibility::Private, "A", b"p"),
            PhpString::from(&b"\0A\0p"[..])
        );
        assert_eq!(
            mangle_property_name(Visibility::Protected, "A", b"p"),
            PhpString::from(&b"\0*\0p"[..])
        );
        assert_eq!(
            unmangle_property_name(b"\0A\0p"),
            (Some(&b"A"[..]), &b"p"[..])
        );
        assert_eq!(
            unmangle_property_name(b"\0*\0p"),
            (Some(&b"*"[..]), &b"p"[..])
        );
        assert_eq!(unmangle_property_name(b"plain"), (None, &b"plain"[..]));
    }
}
