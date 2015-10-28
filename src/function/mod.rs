use std::fmt;
use value::Value;
use error::{ RuntimeResult, TemplateResult };
use mold::Staging;
use instructions::CompiledExpression;

/// Callable implementation.
pub enum Callable {
    /// Executable at runtime.
    Dynamic(Box<
        for<'e> Fn(&'e [Value]) -> RuntimeResult<Value>
    >),
    /// Inlined into instructions at compile time.
    Static(Box<
        for<'c> Fn(&mut Staging<'c, Value>) -> TemplateResult<CompiledExpression>
    >)
}

/// Represents environment function.
pub struct Function {
    pub name: &'static str,
    pub callable: Callable,
}

impl Function {
    pub fn dynamic<F: 'static>(
        name: &'static str,
        callable: F
    )
        -> Function
    where
        F: for<'e> Fn(&'e [Value]) -> RuntimeResult<Value>
    {
        Function {
            name: name,
            callable: Callable::Dynamic(Box::new(callable)),
        }
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}()", self.name)
    }
}
