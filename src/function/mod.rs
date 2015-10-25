use std::fmt;
use value::Value;
use error::{ RuntimeResult };

pub struct Function {
    pub name: &'static str,
    pub callable: Box<
        for<'e> Fn(&'e [Value]) -> RuntimeResult<Value>
    >,
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}()", self.name)
    }
}
