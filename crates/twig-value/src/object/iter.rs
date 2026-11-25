//! The `Traversable` protocol: object iterators (`zend_object_iterator`) and the consumers
//! `iterator_to_array()` / `iterator_count()` with PHP's exact call order.

use super::{CallArgs, ObjectRef};
use crate::array::Array;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::key::{ArrayKey, KeyContext};
use crate::value::Value;

/// An object iterator (`zend_object_iterator_funcs`). Consumers call the methods in PHP's order:
///
/// * `foreach`: `rewind`; then per element `valid`, `current`, `key` (only when the loop uses the
///   key), body, and `next` before the following `valid`;
/// * `iterator_to_array()` / `iterator_count()`: see [`traverse`].
pub trait ObjectIterator: Send {
    /// `rewind`.
    fn rewind(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()>;
    /// `valid`.
    fn valid(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<bool>;
    /// `get_current_data`.
    fn current(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<Value>;
    /// `get_current_key`.
    fn key(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<Value>;
    /// `move_forward`.
    fn next(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()>;
}

/// Iterator over a userland `Iterator` object (`zend_user_iterator`): calls the object's
/// `rewind()`, `valid()` (result converted with truthiness), `current()` (cached until the next
/// `next()`/`rewind()`), `key()` and `next()` methods.
pub struct MethodIterator {
    object: ObjectRef,
    current: Option<Value>,
}

impl MethodIterator {
    /// Iterates `object` through its `Iterator` methods.
    pub fn new(object: ObjectRef) -> MethodIterator {
        MethodIterator {
            object,
            current: None,
        }
    }
}

impl ObjectIterator for MethodIterator {
    fn rewind(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.current = None;
        self.object
            .invoke_method("rewind", CallArgs::new(), diag)
            .map(|_| ())
    }

    fn valid(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        let v = self.object.invoke_method("valid", CallArgs::new(), diag)?;
        crate::convert::to_bool(&v, diag)
    }

    fn current(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        if let Some(v) = &self.current {
            return Ok(v.clone());
        }
        let v = self
            .object
            .invoke_method("current", CallArgs::new(), diag)?;
        self.current = Some(v.clone());
        Ok(v)
    }

    fn key(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        self.object.invoke_method("key", CallArgs::new(), diag)
    }

    fn next(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.current = None;
        self.object
            .invoke_method("next", CallArgs::new(), diag)
            .map(|_| ())
    }
}

/// Default `get_iterator`: `zend_user_it_get_iterator()` for `Iterator` classes,
/// `zend_user_it_get_new_iterator()` for `IteratorAggregate` classes (PHPSRC/Zend/zend_interfaces.c).
pub(crate) fn std_get_iterator(
    this: &ObjectRef,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<Box<dyn ObjectIterator>>> {
    let class = this.class();
    if class.is_subclass_of("Iterator") {
        return Ok(Some(Box::new(MethodIterator::new(this.clone()))));
    }
    if class.is_subclass_of("IteratorAggregate") {
        let it = this.invoke_method("getiterator", CallArgs::new(), diag)?;
        let bad = || {
            PhpError::exception(format!(
                "Objects returned by {}::getIterator() must be traversable or implement interface Iterator",
                class.display_name()
            ))
        };
        return match &it {
            Value::Object(o) if !ObjectRef::ptr_eq(o, this) && o.instance_of("Traversable") => {
                match o.get_iterator(diag)? {
                    Some(iter) => Ok(Some(iter)),
                    None => Err(bad()),
                }
            }
            _ => Err(bad()),
        };
    }
    Ok(None)
}

/// Port of `spl_iterator_apply()` (PHPSRC/ext/spl/spl_iterators.c): `rewind`, then while `valid`:
/// `current`, `key` (only when `with_keys`), `f(key, value)`, `next`. `f` returns `false` to stop.
/// Without keys `f` receives `Value::Null` as the key.
///
/// Errors with `Error` when the object is not traversable (callers normally check
/// `instance_of("Traversable")` first, as `iterator_to_array()`'s `Traversable|array` parameter does).
pub fn traverse(
    object: &ObjectRef,
    with_keys: bool,
    diag: &mut dyn Diagnostics,
    mut f: impl FnMut(Value, Value, &mut dyn Diagnostics) -> PhpResult<bool>,
) -> PhpResult<()> {
    let mut it = match object.get_iterator(diag)? {
        Some(it) => it,
        None => {
            return Err(PhpError::error(format!(
                "Object of type {} is not traversable",
                object.class().display_name()
            )));
        }
    };
    it.rewind(diag)?;
    while it.valid(diag)? {
        let value = it.current(diag)?;
        let key = if with_keys {
            it.key(diag)?
        } else {
            Value::Null
        };
        if !f(key, value, diag)? {
            break;
        }
        it.next(diag)?;
    }
    Ok(())
}

/// Port of `iterator_to_array($object, $preserve_keys)` for objects
/// (PHPSRC/ext/spl/spl_iterators.c): keys go through `array_set_zval_key()` (string keys
/// normalised, floats/bools/null converted with their diagnostics, arrays/objects throw
/// `Cannot access offset of type X on array`); later duplicates overwrite in place.
pub fn iterator_to_array(
    object: &ObjectRef,
    preserve_keys: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Array> {
    let mut out = Array::new();
    traverse(object, preserve_keys, diag, |k, v, diag| {
        if preserve_keys {
            let key = ArrayKey::from_value(&k, KeyContext::Write, diag)?;
            out.insert(key, v);
        } else {
            out.push(v)?;
        }
        Ok(true)
    })?;
    Ok(out)
}

/// Port of `iterator_count($object)` for objects: counts `valid()` iterations without fetching
/// values (`spl_iterator_count_apply`).
pub fn iterator_count(object: &ObjectRef, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    let mut it = match object.get_iterator(diag)? {
        Some(it) => it,
        None => {
            return Err(PhpError::error(format!(
                "Object of type {} is not traversable",
                object.class().display_name()
            )));
        }
    };
    let mut count: i64 = 0;
    it.rewind(diag)?;
    while it.valid(diag)? {
        if count == i64::MAX {
            break;
        }
        count += 1;
        it.next(diag)?;
    }
    Ok(count)
}
