use std::fmt;
use value::Value;
use error::{ RuntimeResult, TemplateResult };
use mold::Staging;
use instructions::CompiledExpression;

pub enum Callable {
    Dynamic(Box<
        for<'e> Fn(&'e [Value]) -> RuntimeResult<Value>
    >),
    Static(Box<
        for<'c> Fn(&mut Staging<'c, Value>) -> TemplateResult<CompiledExpression>
    >)
}

pub struct Function {
    pub name: &'static str,
    pub callable: Callable,
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}()", self.name)
    }
}
