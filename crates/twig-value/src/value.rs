//! The PHP value (`zval`).

use std::fmt;

use crate::array::Array;
use crate::object::ObjectRef;
use crate::string::PhpString;

/// A PHP value (`zval` without `IS_UNDEF`, `IS_REFERENCE` and resources).
///
/// * `Null`, `Bool`, `Int`, `Float`, `String` and `Array` are values: cloning copies them
///   (strings and arrays share their storage until modified, copy-on-write like PHP's refcounted
///   `zend_string`/`HashTable`).
/// * `Object` is a handle: clones share the object, like PHP objects.
///
/// `PartialEq` is PHP **identity** (`===`, `zend_is_identical`): same type and value, floats compared
/// with `==` (so `NAN !== NAN` and `0.0 === -0.0`), arrays in order with identical keys and values,
/// objects by handle. PHP's loose `==` lives with the comparison operators, not here.
#[derive(Clone, Default)]
pub enum Value {
    /// `null`
    #[default]
    Null,
    /// `true` / `false` (two distinct type tags in PHP, see [`TypeTag`]).
    Bool(bool),
    /// `int` (`zend_long`, 64-bit).
    Int(i64),
    /// `float` (IEEE double).
    Float(f64),
    /// `string` (bytes).
    String(PhpString),
    /// `array` (ordered hash map, copy-on-write).
    Array(Array),
    /// `object` (shared handle).
    Object(ObjectRef),
}

/// The engine's type tags in their numeric order (`Zend/zend_types.h`: `IS_NULL` = 1 ...
/// `IS_OBJECT` = 8). Engine code compares tags numerically (`Z_TYPE_P(op) < IS_TRUE` means "null or
/// false"), so the order is part of the semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TypeTag {
    /// `IS_NULL`
    Null = 1,
    /// `IS_FALSE`
    False = 2,
    /// `IS_TRUE`
    True = 3,
    /// `IS_LONG`
    Long = 4,
    /// `IS_DOUBLE`
    Double = 5,
    /// `IS_STRING`
    String = 6,
    /// `IS_ARRAY`
    Array = 7,
    /// `IS_OBJECT`
    Object = 8,
}

impl TypeTag {
    /// Port of `zend_get_type_by_const()` (PHPSRC/Zend/zend_API.c): `null`, `bool`, `int`,
    /// `float`, `string`, `array`, `object`.
    pub fn name(self) -> &'static str {
        match self {
            TypeTag::Null => "null",
            TypeTag::False | TypeTag::True => "bool",
            TypeTag::Long => "int",
            TypeTag::Double => "float",
            TypeTag::String => "string",
            TypeTag::Array => "array",
            TypeTag::Object => "object",
        }
    }
}

/// A PHP number: the result of numeric conversions (`IS_LONG` or `IS_DOUBLE`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Number {
    /// `int`
    Int(i64),
    /// `float`
    Float(f64),
}

impl Number {
    /// The value as a float (`(double)` of an int).
    pub fn to_f64(self) -> f64 {
        match self {
            Number::Int(i) => i as f64,
            Number::Float(f) => f,
        }
    }
}

impl From<Number> for Value {
    fn from(n: Number) -> Value {
        match n {
            Number::Int(i) => Value::Int(i),
            Number::Float(f) => Value::Float(f),
        }
    }
}

impl Value {
    /// The engine type tag of this value.
    pub fn type_tag(&self) -> TypeTag {
        match self {
            Value::Null => TypeTag::Null,
            Value::Bool(false) => TypeTag::False,
            Value::Bool(true) => TypeTag::True,
            Value::Int(_) => TypeTag::Long,
            Value::Float(_) => TypeTag::Double,
            Value::String(_) => TypeTag::String,
            Value::Array(_) => TypeTag::Array,
            Value::Object(_) => TypeTag::Object,
        }
    }

    /// A string value from anything convertible to bytes.
    pub fn string(s: impl Into<PhpString>) -> Value {
        Value::String(s.into())
    }

    /// `is_null()`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// `is_scalar()`: bool, int, float or string.
    pub fn is_scalar(&self) -> bool {
        matches!(
            self,
            Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::String(_)
        )
    }

    /// Port of `is_iterable()` (`zend_is_iterable()`): an array or a `Traversable` object.
    pub fn is_iterable(&self) -> bool {
        match self {
            Value::Array(_) => true,
            Value::Object(o) => o.instance_of("Traversable"),
            _ => false,
        }
    }

    /// Port of `is_countable()` (`zend_is_countable()`): an array or a `Countable` object. (PHP
    /// also accepts objects whose class has a `count_elements` handler without implementing
    /// `Countable`; no built-in class here does that.)
    pub fn is_countable(&self) -> bool {
        match self {
            Value::Array(_) => true,
            Value::Object(o) => o.instance_of("Countable"),
            _ => false,
        }
    }

    /// The boolean, if this is a bool.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The integer, if this is an int.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The float, if this is a float.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// The string, if this is a string.
    pub fn as_str(&self) -> Option<&PhpString> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The array, if this is an array.
    pub fn as_array(&self) -> Option<&Array> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Mutable access to the array (separates a shared array first, copy-on-write).
    pub fn as_array_mut(&mut self) -> Option<&mut Array> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The object handle, if this is an object.
    pub fn as_object(&self) -> Option<&ObjectRef> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    /// Port of `zend_is_identical()` (PHPSRC/Zend/zend_operators.c): PHP `===`.
    pub fn identical(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a.identical(b),
            (Value::Object(a), Value::Object(b)) => ObjectRef::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl PartialEq for Value {
    /// PHP identity (`===`), see [`Value::identical`].
    fn eq(&self, other: &Value) -> bool {
        self.identical(other)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("Null"),
            Value::Bool(b) => write!(f, "Bool({b})"),
            Value::Int(i) => write!(f, "Int({i})"),
            Value::Float(d) => write!(f, "Float({d:?})"),
            Value::String(s) => write!(f, "String({s:?})"),
            Value::Array(a) => write!(f, "Array({a:?})"),
            Value::Object(o) => write!(f, "Object({o:?})"),
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Value {
        Value::Int(i)
    }
}

impl From<i32> for Value {
    fn from(i: i32) -> Value {
        Value::Int(i64::from(i))
    }
}

impl From<u32> for Value {
    fn from(i: u32) -> Value {
        Value::Int(i64::from(i))
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Value {
        Value::Float(f)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::String(PhpString::from(s))
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::String(PhpString::from(s))
    }
}

impl From<&[u8]> for Value {
    fn from(s: &[u8]) -> Value {
        Value::String(PhpString::from(s))
    }
}

impl From<Vec<u8>> for Value {
    fn from(s: Vec<u8>) -> Value {
        Value::String(PhpString::from(s))
    }
}

impl From<PhpString> for Value {
    fn from(s: PhpString) -> Value {
        Value::String(s)
    }
}

impl From<Array> for Value {
    fn from(a: Array) -> Value {
        Value::Array(a)
    }
}

impl From<ObjectRef> for Value {
    fn from(o: ObjectRef) -> Value {
        Value::Object(o)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    /// `None` becomes `null`.
    fn from(v: Option<T>) -> Value {
        match v {
            Some(v) => v.into(),
            None => Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_identity() {
        assert!(TypeTag::Null < TypeTag::True);
        assert!(TypeTag::False < TypeTag::True);
        assert_eq!(Value::Bool(false).type_tag(), TypeTag::False);
        assert_eq!(TypeTag::True.name(), "bool");
        assert!(Value::Float(0.0).identical(&Value::Float(-0.0)));
        assert!(!Value::Float(f64::NAN).identical(&Value::Float(f64::NAN)));
        assert!(!Value::Int(1).identical(&Value::Float(1.0)));
        assert!(!Value::from("1").identical(&Value::from("01")));
        assert_eq!(Value::from(Some(3)), Value::Int(3));
        assert_eq!(Value::from(None::<i64>), Value::Null);
    }
}
