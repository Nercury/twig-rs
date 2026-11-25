//! `stdClass`.

use super::builtin;
use super::class::ClassRef;
use super::props::PropertyStore;
use super::{Object, ObjectRef};
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::string::PhpString;
use crate::value::Value;

/// `stdClass`: an object with only dynamic properties (no deprecation on creation).
#[derive(Clone)]
pub struct StdClass {
    class: ClassRef,
    props: PropertyStore,
}

impl StdClass {
    /// An empty `stdClass` (`new stdClass`, `(object) null`).
    pub fn new() -> StdClass {
        StdClass {
            class: builtin::std_class(),
            props: PropertyStore::empty(),
        }
    }

    /// A `stdClass` with the given properties in order (`(object) ['a' => 1]`).
    pub fn with_properties<K: Into<PhpString>>(
        props: impl IntoIterator<Item = (K, Value)>,
    ) -> StdClass {
        let obj = StdClass::new();
        for (k, v) in props {
            obj.props.set(k.into().as_bytes(), v);
        }
        obj
    }

    /// Wraps into a handle.
    pub fn into_ref(self) -> ObjectRef {
        ObjectRef::new(self)
    }

    /// Direct access to the property storage.
    pub fn properties(&self) -> &PropertyStore {
        &self.props
    }
}

impl Default for StdClass {
    fn default() -> Self {
        StdClass::new()
    }
}

impl Object for StdClass {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    /// `zend_objects_clone_obj()`: a shallow copy of the properties.
    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(self.clone()))
    }
}
