use std::fmt;
use std::cmp::Ordering;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use error::{ RuntimeError, RuntimeResult, CastError, CastTarget };

pub mod ops;

const MAX_DEBUG_STRING_LENGTH: usize = 128;
const MAX_DEBUG_ARRAY_LENGTH: usize = 4;
const MAX_DEBUG_HASH_LENGTH: usize = 4;

/// Value kind that can be used as Hash key.
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd)]
pub enum HashKey {
    Int(i64),
    Str(String),
}

impl fmt::Debug for HashKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            HashKey::Int(ref v) => write!(f, "{}", v),
            HashKey::Str(ref v) => write!(f, "{:?}", v),
        }
    }
}

/// Represents Twig runtime value.
pub enum Value {
    Null,
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Hash(HashMap<HashKey, Value>),
    Obj(Rc<RefCell<Object>>),
    Func(Rc<for<'r> Fn(&'r [Value]) -> Option<Value> >),
}

impl<'a> From<HashMap<&'a str, &'a str>> for Value {
    fn from(value: HashMap<&'a str, &'a str>) -> Value {
        let hash = value.into_iter()
            .map(|(k, v)| {
                (HashKey::Str(k.into()), Value::Str(v.into()))
            })
            .collect();
        Value::Hash(hash)
    }
}

impl From<HashMap<String, String>> for Value {
    fn from(value: HashMap<String, String>) -> Value {
        let hash = value.into_iter()
            .map(|(k, v)| {
                (HashKey::Str(k), Value::Str(v))
            })
            .collect();
        Value::Hash(hash)
    }
}

impl Clone for Value {
    fn clone(&self) -> Value {
        match *self {
            Value::Null => Value::Null,
            Value::Int(ref v) => Value::Int(v.clone()),
            Value::Float(ref v) => Value::Float(v.clone()),
            Value::Str(ref v) => Value::Str(v.clone()),
            Value::Array(ref v) => Value::Array(v.clone()),
            Value::Hash(ref v) => Value::Hash(v.clone()),
            Value::Obj(ref v) => Value::Obj(v.clone()),
            Value::Func(ref v) => Value::Func(v.clone()),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (&Value::Null, &Value::Null) => true,
            (&Value::Int(ref a), &Value::Int(ref b)) => a.eq(b),
            (&Value::Float(ref a), &Value::Float(ref b)) => a.eq(b),
            (&Value::Str(ref a), &Value::Str(ref b)) => a.eq(b),
            (&Value::Array(ref a), &Value::Array(ref b)) => a.eq(b),
            (&Value::Obj(_), &Value::Obj(_)) => false,
            (&Value::Func(_), &Value::Func(_)) => false,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Value) -> Option<Ordering> {
        match (self, other) {
            (&Value::Null, &Value::Null) => Some(Ordering::Equal),
            (&Value::Int(ref a), &Value::Int(ref b)) => a.partial_cmp(b),
            (&Value::Float(ref a), &Value::Float(ref b)) => a.partial_cmp(b),
            (&Value::Str(ref a), &Value::Str(ref b)) => a.partial_cmp(b),
            (&Value::Array(ref a), &Value::Array(ref b)) => a.partial_cmp(b),
            (&Value::Obj(_), &Value::Obj(_)) => None,
            (&Value::Func(_), &Value::Func(_)) => None,
            _ => None,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Value::Null => write!(f, "null"),
            Value::Int(ref v) => write!(f, "{}", v),
            Value::Float(ref v) => write!(f, "{}", v),
            Value::Str(ref v) => write!(f, "{:?}", ops::to_string_limited(v)),
            Value::Array(ref v) => {
                let mut list = f.debug_list();
                for (i, item) in v.iter().enumerate() {
                    list.entry(item);
                    if i >= MAX_DEBUG_ARRAY_LENGTH {
                        list.entry(&"...");
                        break;
                    }
                }
                list.finish()
            },
            Value::Hash(ref hash) => {
                let mut map = f.debug_map();
                let i = 0;
                for (k, v) in hash {
                    map.entry(k, v);
                    if i >= MAX_DEBUG_HASH_LENGTH {
                        map.entry(&"...", &"...");
                        break;
                    }
                }
                map.finish()
            },
            Value::Obj(_) => write!(f, "Object"),
            Value::Func(_) => write!(f, "Function"),
        }
    }
}

impl Value {
    /// If possible, returns this value represented as integer.
    pub fn int(self) -> RuntimeResult<i64> {
        Ok(match self {
            Value::Null => return Err(RuntimeError::ImpossibleCast {
                target: CastTarget::Int,
                reason: CastError::Null,
            }),
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

/// Twig object abstraction.
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

    fn set(&mut self, name: &str, _value: Value) -> RuntimeResult<()> {
        Err(self.property_error(name))
    }

    fn call(&mut self, name: &str, _values: &[Value]) -> RuntimeResult<Value> {
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
    }

    #[test]
    fn object_getters_and_setters() {
        let mut point = Point { x: 12, y: 13 };
        assert_eq!(point.get("x").ok().unwrap(), Value::Int(12));
        assert_eq!(point.get("y").ok().unwrap(), Value::Int(13));

        point.set("x", Value::Int(42));
        point.set("y", Value::Int(43));
        assert_eq!(point.get("x").ok().unwrap(), Value::Int(42));
        assert_eq!(point.get("y").ok().unwrap(), Value::Int(43));
    }

    #[test]
    fn object_setter_can_convert_values() {
        let mut point = Point { x: 12, y: 13 };

        point.set("x", Value::Str("48".into()));
        assert_eq!(point.get("x").ok().unwrap(), Value::Int(48));
    }
}
