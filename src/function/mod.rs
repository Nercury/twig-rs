use crate::error::RuntimeResult;
use crate::value::Value;
use std::fmt;

/// Callable implementation.
pub enum Callable {
    /// Executable at runtime.
    #[allow(clippy::type_complexity)]
    Dynamic(Box<dyn for<'e> Fn(&'e [Value]) -> RuntimeResult<Value>>),
}

/// Represents environment function.
pub struct Function {
    pub name: &'static str,
    pub callable: Callable,
}

impl Function {
    pub fn new_dynamic<F>(name: &'static str, callable: F) -> Function
    where
        F: for<'e> Fn(&'e [Value]) -> RuntimeResult<Value> + 'static,
    {
        Function {
            name,
            callable: Callable::Dynamic(Box::new(callable)),
        }
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}()", self.name)
    }
}
