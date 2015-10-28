use std::fmt;
use value::Value;
use error::{ RuntimeResult, TemplateResult };
use mold::Staging;
use instructions::CompiledExpression;

pub enum Arg {
    Anon,
    Named(&'static str),
}

/// Callable implementation.
pub enum Callable {
    /// Executable at runtime.
    Dynamic(Box<
        for<'e> Fn(&'e [Value]) -> RuntimeResult<Value>
    >),
    /// Inlined into instructions at compile time.
    Static {
        arguments: Vec<Arg>,
        compile: Box<
            for<'c> Fn(&mut Staging<'c, Value>) -> TemplateResult<CompiledExpression>
        >
    }
}

/// Represents environment function.
pub struct Function {
    pub name: &'static str,
    pub callable: Callable,
}

impl Function {
    pub fn new_dynamic<F: 'static>(
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

    pub fn new_static<F: 'static, I: IntoIterator<Item=Arg>>(
        name: &'static str,
        arguments: I,
        compile: F
    )
        -> Function
    where
        F: for<'c> Fn(&mut Staging<'c, Value>) -> TemplateResult<CompiledExpression>
    {
        Function {
            name: name,
            callable: Callable::Static {
                arguments: arguments.into_iter().collect(),
                compile: Box::new(compile)
            },
        }
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}()", self.name)
    }
}
