use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;

pub enum RuntimeError {
    ObjectHasNoProperty(String),
    ObjectHasNoMethod(String),
    ObjectMethodDoesNotExist(String),
    ObjectPropertyIsNotMethod(String),
    ObjectMethodIsNotProperty(String),
    ObjectMethodArgumentMismatch { expected: u16, given: u16 },
}

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
    Func(Rc<RefCell< for<'r> Fn(&'r [Value]) -> Option<Value> >>)
}

pub trait Object {
    fn get(&self, name: &str) -> Result<Value, RuntimeError>;
    fn set(&self, name: &str, value: Value) -> Result<(), RuntimeError>;
    fn call(&mut self, name: &str, values: &[Value]) -> Result<Value, RuntimeError>;
}
