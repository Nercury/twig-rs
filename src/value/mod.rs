use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use error::{ RuntimeError, RuntimeResult, CastError, CastTarget };

pub mod ops;

pub enum HashKey {
    Int(i64),
    Str(String),
}

pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Hash(HashMap<HashKey, Value>),
    Obj(Rc<RefCell<Object>>),
    Func(Rc<for<'r> Fn(&'r [Value]) -> Option<Value> >)
}

impl Value {
    /// If possible, returns this value represented as integer.
    pub fn int(self) -> RuntimeResult<i64> {
        Ok(match self {
            Value::Int(v) => v,
            Value::Float(v) => return ops::float_to_int(v),
            Value::Str(v) => {
                match ops::parse_as_numeric(&v) {
                    Ok(ops::ParseAsNumericResult::Int(v)) => v,
                    Ok(ops::ParseAsNumericResult::Float(v)) => try!(ops::float_to_int(v)),
                    Err(e) => return Err(e),
                }
            },
            Value::Array(_) => return Err(RuntimeError::ImpossibleCast {
                target: CastTarget::Int,
                reason: CastError::Array,
            }),
            Value::Hash(_) => return Err(RuntimeError::ImpossibleCast {
                target: CastTarget::Int,
                reason: CastError::Hash,
            }),
            Value::Obj(_) => return Err(RuntimeError::ImpossibleCast {
                target: CastTarget::Int,
                reason: CastError::Object,
            }),
            Value::Func(_) => return Err(RuntimeError::ImpossibleCast {
                target: CastTarget::Int,
                reason: CastError::Function,
            }),
        })
    }
}

pub trait Object {
    fn property_error(&self, name: &str) -> RuntimeError {
        RuntimeError::ObjectHasNoProperty(name.into())
    }

    fn method_error(&self, name: &str) -> RuntimeError {
        RuntimeError::ObjectHasNoMethod(name.into())
    }

    fn get(&self, name: &str) -> RuntimeResult<Value> {
        Err(self.property_error(name))
    }

    fn set(&mut self, name: &str, value: Value) -> RuntimeResult<()> {
        Err(self.property_error(name))
    }

    fn call(&mut self, name: &str, values: &[Value]) -> RuntimeResult<Value> {
        Err(self.method_error(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use error::RuntimeResult;

    struct Point {
        x: i64,
        y: i64,
    }

    impl Object for Point {
        fn get(&self, name: &str) -> RuntimeResult<Value> {
            Ok(match name {
                "x" => Value::Int(self.x),
                "y" => Value::Int(self.y),
                _ => return Err(self.property_error(name)),
            })
        }

        fn set(&mut self, name: &str, value: Value) -> RuntimeResult<()> {
            Ok(match name {
                "x" => self.x = try!(value.int()),
                "y" => self.y = try!(value.int()),
                _ => return Err(self.property_error(name)),
            })
        }

        // fn call(&mut self, name: &str, values: &[Value]) -> RuntimeResult<Value> {
        //
        // }
    }

    #[test]
    fn object_getters_and_setters() {

    }
}
