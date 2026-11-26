//! `Closure` objects wrapping Rust callables.

use std::fmt;
use std::sync::Arc;

use super::builtin;
use super::class::ClassRef;
use super::{CallArgs, Object, ObjectRef};
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::value::Value;

/// The callable a [`Closure`] wraps: receives the call arguments and the diagnostics sink.
pub type ClosureFn = dyn Fn(CallArgs, &mut dyn Diagnostics) -> PhpResult<Value> + Send + Sync;

/// A PHP `Closure` (final class) backed by a Rust function.
///
/// Invocation (`$f(...)`, `$f->__invoke(...)`, [`ObjectRef::invoke`]) calls the function. Closures
/// use the standard property handlers without a property table (PHP 8.5: reads warn `Undefined
/// property: Closure::$x`, writes throw `Cannot create dynamic property Closure::$x`), can be cloned
/// (the clone shares the function), and two distinct closures are uncomparable (`==` false, `<=>` 1
/// both ways), like closures that are not first-class-callable "fake closures".
///
/// `bind()`, `bindTo()`, `call()`, `fromCallable()` and `getCurrent()` are listed (so
/// `get_class_methods()` matches PHP) but throw `Error` when called: rebinding `$this`/scope has no
/// meaning for Rust callables.
#[derive(Clone)]
pub struct Closure {
    class: ClassRef,
    func: Arc<ClosureFn>,
    name: Arc<str>,
}

impl Closure {
    /// A closure named `{closure}`.
    pub fn new(
        func: impl Fn(CallArgs, &mut dyn Diagnostics) -> PhpResult<Value> + Send + Sync + 'static,
    ) -> Closure {
        Closure::named("{closure}", func)
    }

    /// A closure with a descriptive name (for debugging output).
    pub fn named(
        name: &str,
        func: impl Fn(CallArgs, &mut dyn Diagnostics) -> PhpResult<Value> + Send + Sync + 'static,
    ) -> Closure {
        Closure {
            class: builtin::closure(),
            func: Arc::new(func),
            name: Arc::from(name),
        }
    }

    /// The descriptive name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Calls the function.
    pub fn invoke(&self, args: CallArgs, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        (self.func)(args, diag)
    }

    /// Whether both closures wrap the same function allocation.
    pub fn same_function(&self, other: &Closure) -> bool {
        Arc::ptr_eq(&self.func, &other.func)
    }

    /// Wraps into a handle.
    pub fn into_ref(self) -> ObjectRef {
        ObjectRef::new(self)
    }
}

impl fmt::Debug for Closure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Closure({})", self.name)
    }
}

impl Object for Closure {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__invoke" => self.invoke(args, diag),
            "bind" | "bindto" | "call" | "fromcallable" | "getcurrent" => {
                let name = self
                    .class
                    .method(method)
                    .map(|m| m.name.to_string())
                    .unwrap_or_else(|| method.to_string());
                Err(PhpError::error(format!(
                    "Closure::{name}() is not supported for host closures"
                )))
            }
            _ => Err(super::undefined_method(&self.class, method)),
        }
    }

    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(self.clone()))
    }

    /// Port of `zend_closure_compare()` (PHPSRC/Zend/zend_closures.c): two closures are equal only when both are
    /// first-class callables (`ZEND_ACC_FAKE_CLOSURE`) of the same function, scope and `$this`. A Rust closure is
    /// never one, so it is uncomparable (`ZEND_UNCOMPARABLE`) with every object of class `Closure`, also with the
    /// closures other Rust types implement (the lazy source path of a template, for example). Any other operand
    /// falls back to `zend_std_compare_objects()` (`ZEND_COMPARE_OBJECTS_FALLBACK`).
    fn compare(
        &self,
        _this: &ObjectRef,
        other: &Value,
        _this_is_left: bool,
        _compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        match other {
            Value::Object(o) if o.instance_of("Closure") => Some(Ok(1)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare;
    use crate::object::StdClass;

    /// A `Closure` that another Rust type implements, without a comparison of its own.
    struct OtherClosure(ClassRef);

    impl Object for OtherClosure {
        fn class(&self) -> &ClassRef {
            &self.0
        }
    }

    /// `$f == $g` and `$f <=> $g`.
    fn compared(f: &Value, g: &Value) -> (bool, i64) {
        let mut diag = Vec::new();
        let equal = compare::equals(f, g, &mut diag).unwrap();
        let order = compare::spaceship(f, g, &mut diag).unwrap();
        assert!(diag.is_empty());
        (equal, order)
    }

    /// PHP (the oracle): with `$f = fn () => 1`, `$f == strlen(...)` is false, and `<=>` gives 1 both ways, like any
    /// pair that is not two first-class callables of one function.
    #[test]
    fn closures_are_uncomparable_with_every_other_closure() {
        let f = Value::Object(Closure::new(|_, _| Ok(Value::Int(1))).into_ref());
        let g = Value::Object(Closure::new(|_, _| Ok(Value::Int(1))).into_ref());
        let other = Value::Object(ObjectRef::new(OtherClosure(builtin::closure())));
        assert_eq!(compared(&f, &g), (false, 1));
        assert_eq!(compared(&g, &f), (false, 1));
        assert_eq!(compared(&f, &other), (false, 1));
        assert_eq!(compared(&f, &f), (true, 0));
    }

    /// Objects of other classes compare as `zend_std_compare_objects()` does: uncomparable.
    #[test]
    fn closures_and_other_objects_compare_as_objects() {
        let f = Value::Object(Closure::new(|_, _| Ok(Value::Int(1))).into_ref());
        let object = Value::Object(StdClass::new().into_ref());
        assert_eq!(compared(&f, &object), (false, 1));
        assert_eq!(compared(&object, &f), (false, 1));
    }
}
