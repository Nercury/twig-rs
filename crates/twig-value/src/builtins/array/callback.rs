//! The callable abstraction of the array builtins.

use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::object::{CallArgs, ObjectRef};
use crate::value::Value;

/// A PHP callable as the array builtins invoke it (`usort()`'s comparator, `array_filter()`'s
/// predicate, `array_map()`'s mapper, ...): called with positional arguments (by value) and the
/// diagnostics sink, it returns the callable's return value or the exception it threw.
///
/// Builtins take `&mut Callback<'_>`, so a closure can be passed directly:
///
/// ```
/// use twig_value::builtins::array::usort;
/// use twig_value::{Array, Value};
///
/// let mut a: Array = [3, 1, 2].into_iter().map(Value::from).collect();
/// let mut diag = Vec::new();
/// usort(
///     &mut a,
///     &mut |args, _diag| Ok(Value::Int(args[0].as_int().unwrap() - args[1].as_int().unwrap())),
///     &mut diag,
/// )
/// .unwrap();
/// assert_eq!(a.values().cloned().collect::<Vec<_>>(), [1, 2, 3].map(Value::from));
/// ```
///
/// Resolving PHP callables (`'strtolower'`, `[$obj, 'method']`) is the engine's job; object
/// callables (`Closure`, `__invoke`) are adapted by [`object_callback`].
pub type Callback<'a> = dyn FnMut(&[Value], &mut dyn Diagnostics) -> PhpResult<Value> + 'a;

/// An object callable (`Closure` or a class with `__invoke`) as a [`Callback`]: `$f(...$args)`
/// through [`ObjectRef::invoke`] (non-callable objects throw `Object of type X is not callable`).
pub fn object_callback(
    object: &ObjectRef,
) -> impl FnMut(&[Value], &mut dyn Diagnostics) -> PhpResult<Value> + '_ {
    move |args: &[Value], diag: &mut dyn Diagnostics| {
        object.invoke(CallArgs::from(args.to_vec()), diag)
    }
}
