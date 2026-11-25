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

    /// `zend_closure_compare()`: two closures (not both fake closures) are uncomparable.
    fn compare(
        &self,
        _this: &ObjectRef,
        other: &Value,
        _this_is_left: bool,
        _compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        match other {
            Value::Object(o) if o.downcast_ref::<Closure>().is_some() => Some(Ok(1)),
            _ => None,
        }
    }
}
