//! Rust data into PHP values through [serde](https://serde.rs) (feature `serde`).
//!
//! [`to_value`] serializes anything that implements [`Serialize`] into a [`Value`], [`to_array`] into an [`Array`]
//! (a template context). The serde data model maps onto PHP values like this:
//!
//! | serde | PHP |
//! |---|---|
//! | `bool` | `bool` |
//! | `i8` to `i64`, `u8` to `u32` | `int` |
//! | `u64`, `i128`, `u128` | `int`, or `float` outside the `int` range (as `json_decode()` decodes such numbers) |
//! | `f32`, `f64` | `float` (infinities and NaN included) |
//! | `char`, string | `string` (UTF-8) |
//! | bytes (`serialize_bytes`, e.g. `serde_bytes`) | `string` (the bytes) |
//! | `None`, unit, unit struct | `null` |
//! | `Some(v)`, newtype struct | the value of `v` |
//! | sequence, tuple, tuple struct | a list (keys `0..n`) |
//! | map, struct | an array in serialization order (struct fields in declaration order) |
//! | unit variant | the variant name |
//! | newtype, tuple and struct variants | `[variant => value]`, serde_json's externally tagged representation |
//!
//! Keys (map keys, field names, variant names) follow PHP's key normalisation, like the `From` conversions of the
//! [`value`](crate::value) module: a string that is a canonical decimal integer becomes an int key (`"1"` → `1`,
//! `"01"` stays a string), integers are int keys (outside the `int` range: their decimal string), `true`/`false` are
//! `1`/`0`, a `char` is a one-character string, a unit variant is its name, `Some(k)` and newtype structs are their
//! content. Any other key (a float, `None`, unit, a sequence, a map, a struct) is an [`Error`]. A repeated key keeps
//! its first position and takes the last value, as in a PHP array literal.
//!
//! A `Vec<u8>` serializes as a sequence, so it becomes a list of ints here, whereas `Value::from` makes a string of
//! it; serialize it with `serialize_bytes` (for instance through `serde_bytes`) to get a PHP string of the bytes.
//!
//! ```
//! use serde::Serialize;
//! use twig_value::{Array, Value};
//!
//! #[derive(Serialize)]
//! struct User {
//!     name: String,
//!     roles: Vec<&'static str>,
//!     manager: Option<u32>,
//! }
//!
//! let user = User { name: "Fabien".into(), roles: vec!["admin"], manager: None };
//! let value = twig_value::serde::to_value(&user)?;
//! let mut expected = Array::new();
//! expected.insert("name", "Fabien");
//! expected.insert("roles", Value::from(vec!["admin"]));
//! expected.insert("manager", Value::Null);
//! assert_eq!(value, Value::Array(expected));
//! # Ok::<(), twig_value::serde::Error>(())
//! ```

use std::fmt;

/// The trait [`to_value`] and [`to_array`] take, re-exported so that code without its own `serde` dependency can name
/// it.
pub use ::serde::Serialize;
use ::serde::ser::{self, Impossible};

use crate::array::Array;
use crate::key::ArrayKey;
use crate::value::Value;

/// Serializes `value` into a PHP value (see the [module documentation](self) for the mapping).
///
/// # Errors
///
/// The error of the value's `Serialize` implementation, or an [`Error`] for a map key that is not a string, an
/// integer, a bool, a char or a unit variant.
pub fn to_value<T: ?Sized + Serialize>(value: &T) -> Result<Value, Error> {
    value.serialize(Serializer)
}

/// Serializes `value` into a PHP array, such as a template context: `value` must serialize to a map, a struct or a
/// sequence (see the [module documentation](self) for the mapping).
///
/// # Errors
///
/// The errors of [`to_value`], and an [`Error`] when the value is not an array (a number, a string, `null`, ...).
pub fn to_array<T: ?Sized + Serialize>(value: &T) -> Result<Array, Error> {
    match to_value(value)? {
        Value::Array(array) => Ok(array),
        other => Err(Error::new(format!(
            "the value must serialize to a map, a struct or a sequence to become an array, not to {}",
            other.type_tag().name()
        ))),
    }
}

/// The error of [`to_value`] and [`to_array`]: the custom error of a `Serialize` implementation, a map key PHP arrays
/// cannot have, or a value that is not an array where one is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl Error {
    fn new(message: String) -> Self {
        Error { message }
    }

    /// The error of a map key of the kind `what`.
    fn key(what: &str) -> Self {
        Error::new(format!(
            "a map key must be a string, an integer, a bool, a char or a unit variant, not {what}"
        ))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Error::new(message.to_string())
    }
}

/// A one-entry array `[variant => value]`: the externally tagged representation of an enum variant with data.
fn tagged(variant: &'static str, value: Value) -> Value {
    let mut array = Array::new();
    array.insert(variant, value);
    Value::Array(array)
}

/// Serializes into a [`Value`].
struct Serializer;

impl ser::Serializer for Serializer {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = SerializeList;
    type SerializeTuple = SerializeList;
    type SerializeTupleStruct = SerializeList;
    type SerializeTupleVariant = SerializeTupleVariant;
    type SerializeMap = SerializeMap;
    type SerializeStruct = SerializeMap;
    type SerializeStructVariant = SerializeStructVariant;

    fn serialize_bool(self, v: bool) -> Result<Value, Error> {
        Ok(Value::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_i16(self, v: i16) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_i32(self, v: i32) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_i64(self, v: i64) -> Result<Value, Error> {
        Ok(Value::Int(v))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_u8(self, v: u8) -> Result<Value, Error> {
        Ok(Value::Int(i64::from(v)))
    }

    fn serialize_u16(self, v: u16) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_u32(self, v: u32) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_u64(self, v: u64) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_u128(self, v: u128) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_f32(self, v: f32) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_f64(self, v: f64) -> Result<Value, Error> {
        Ok(Value::Float(v))
    }

    fn serialize_char(self, v: char) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_str(self, v: &str) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_none(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Value, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Value, Error> {
        Ok(Value::from(variant))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, Error> {
        Ok(tagged(variant, value.serialize(Serializer)?))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SerializeList, Error> {
        Ok(SerializeList {
            values: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SerializeList, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SerializeList, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SerializeTupleVariant, Error> {
        Ok(SerializeTupleVariant {
            variant,
            values: Vec::with_capacity(len),
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<SerializeMap, Error> {
        Ok(SerializeMap {
            array: Array::new(),
            key: None,
        })
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<SerializeMap, Error> {
        self.serialize_map(None)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<SerializeStructVariant, Error> {
        Ok(SerializeStructVariant {
            variant,
            array: Array::new(),
        })
    }
}

/// A sequence, tuple or tuple struct being serialized into a list.
struct SerializeList {
    values: Vec<Value>,
}

impl ser::SerializeSeq for SerializeList {
    type Ok = Value;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.values.push(value.serialize(Serializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        Ok(Value::Array(Array::from_values(self.values)))
    }
}

impl ser::SerializeTuple for SerializeList {
    type Ok = Value;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, Error> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SerializeList {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, Error> {
        ser::SerializeSeq::end(self)
    }
}

/// A tuple variant being serialized into `[variant => [values]]`.
struct SerializeTupleVariant {
    variant: &'static str,
    values: Vec<Value>,
}

impl ser::SerializeTupleVariant for SerializeTupleVariant {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.values.push(value.serialize(Serializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        Ok(tagged(
            self.variant,
            Value::Array(Array::from_values(self.values)),
        ))
    }
}

/// A map or struct being serialized into an array.
struct SerializeMap {
    array: Array,
    /// The key of the entry whose value comes next.
    key: Option<ArrayKey>,
}

impl ser::SerializeMap for SerializeMap {
    type Ok = Value;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        self.key = Some(key.serialize(KeySerializer)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| Error::new("a map value was serialized before its key".to_owned()))?;
        let value = value.serialize(Serializer)?;
        self.array.insert(key, value);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        Ok(Value::Array(self.array))
    }
}

impl ser::SerializeStruct for SerializeMap {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let value = value.serialize(Serializer)?;
        self.array.insert(key, value);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        Ok(Value::Array(self.array))
    }
}

/// A struct variant being serialized into `[variant => [field => value, ...]]`.
struct SerializeStructVariant {
    variant: &'static str,
    array: Array,
}

impl ser::SerializeStructVariant for SerializeStructVariant {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let value = value.serialize(Serializer)?;
        self.array.insert(key, value);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        Ok(tagged(self.variant, Value::Array(self.array)))
    }
}

/// Serializes a map key into an [`ArrayKey`] (see the module documentation for the rules).
struct KeySerializer;

impl ser::Serializer for KeySerializer {
    type Ok = ArrayKey;
    type Error = Error;
    type SerializeSeq = Impossible<ArrayKey, Error>;
    type SerializeTuple = Impossible<ArrayKey, Error>;
    type SerializeTupleStruct = Impossible<ArrayKey, Error>;
    type SerializeTupleVariant = Impossible<ArrayKey, Error>;
    type SerializeMap = Impossible<ArrayKey, Error>;
    type SerializeStruct = Impossible<ArrayKey, Error>;
    type SerializeStructVariant = Impossible<ArrayKey, Error>;

    fn serialize_bool(self, v: bool) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_i8(self, v: i8) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_i16(self, v: i16) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_i32(self, v: i32) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_i64(self, v: i64) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::Int(v))
    }

    fn serialize_i128(self, v: i128) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_u8(self, v: u8) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_u16(self, v: u16) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_u32(self, v: u32) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_u64(self, v: u64) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_u128(self, v: u128) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_f32(self, _v: f32) -> Result<ArrayKey, Error> {
        Err(Error::key("a float"))
    }

    fn serialize_f64(self, _v: f64) -> Result<ArrayKey, Error> {
        Err(Error::key("a float"))
    }

    fn serialize_char(self, v: char) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_str(self, v: &str) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(v))
    }

    fn serialize_none(self) -> Result<ArrayKey, Error> {
        Err(Error::key("None"))
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<ArrayKey, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<ArrayKey, Error> {
        Err(Error::key("a unit"))
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<ArrayKey, Error> {
        Err(Error::key(&format!("the unit struct {name}")))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<ArrayKey, Error> {
        Ok(ArrayKey::from(variant))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<ArrayKey, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _value: &T,
    ) -> Result<ArrayKey, Error> {
        Err(Error::key(&format!(
            "the newtype variant {name}::{variant}"
        )))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Err(Error::key("a sequence"))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Error> {
        Err(Error::key("a tuple"))
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Err(Error::key(&format!("the tuple struct {name}")))
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(Error::key(&format!("the tuple variant {name}::{variant}")))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(Error::key("a map"))
    }

    fn serialize_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        Err(Error::key(&format!("the struct {name}")))
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Err(Error::key(&format!("the struct variant {name}::{variant}")))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use ::serde::Serialize;

    use super::*;

    fn list(values: impl IntoIterator<Item = Value>) -> Value {
        Value::Array(Array::from_values(values))
    }

    fn map<const N: usize>(entries: [(&str, Value); N]) -> Value {
        Value::Array(Array::from(entries))
    }

    #[derive(Serialize)]
    struct Unit;

    #[derive(Serialize)]
    struct Meters(f64);

    #[derive(Serialize)]
    struct Pair(i32, &'static str);

    #[derive(Serialize)]
    enum Shape {
        Empty,
        Circle(f64),
        Line(i32, i32),
        Rect { w: u8, h: u8 },
    }

    #[test]
    fn scalars() {
        assert_eq!(to_value(&true).unwrap(), Value::Bool(true));
        assert_eq!(to_value(&-3i8).unwrap(), Value::Int(-3));
        assert_eq!(to_value(&200u8).unwrap(), Value::Int(200));
        assert_eq!(to_value(&u64::MAX).unwrap(), Value::Float(u64::MAX as f64));
        assert_eq!(to_value(&(i64::MAX as u64)).unwrap(), Value::Int(i64::MAX));
        assert_eq!(to_value(&-1i128).unwrap(), Value::Int(-1));
        assert_eq!(
            to_value(&u128::MAX).unwrap(),
            Value::Float(u128::MAX as f64)
        );
        assert_eq!(to_value(&1.5f32).unwrap(), Value::Float(1.5));
        assert!(matches!(to_value(&f64::NAN).unwrap(), Value::Float(f) if f.is_nan()));
        assert_eq!(to_value(&'x').unwrap(), Value::from("x"));
        assert_eq!(to_value("héllo").unwrap(), Value::from("héllo"));
        assert_eq!(to_value(&None::<i32>).unwrap(), Value::Null);
        assert_eq!(to_value(&Some(4)).unwrap(), Value::Int(4));
        assert_eq!(to_value(&()).unwrap(), Value::Null);
        assert_eq!(to_value(&Unit).unwrap(), Value::Null);
        assert_eq!(to_value(&Meters(2.5)).unwrap(), Value::Float(2.5));
    }

    /// `serialize_bytes`, which `serde_bytes` and byte-string types use, gives a PHP string of the bytes.
    #[test]
    fn bytes_are_strings() {
        struct Bytes(&'static [u8]);
        impl Serialize for Bytes {
            fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_bytes(self.0)
            }
        }
        assert_eq!(
            to_value(&Bytes(b"\xff\x00")).unwrap(),
            Value::from(&b"\xff\x00"[..])
        );
        // a plain Vec<u8> serializes as a sequence
        assert_eq!(
            to_value(&vec![1u8, 2]).unwrap(),
            list([Value::Int(1), Value::Int(2)])
        );
    }

    #[test]
    fn sequences_and_tuples() {
        assert_eq!(
            to_value(&vec!["a", "b"]).unwrap(),
            list([Value::from("a"), Value::from("b")])
        );
        assert_eq!(
            to_value(&(1, "x", None::<u8>)).unwrap(),
            list([Value::Int(1), Value::from("x"), Value::Null])
        );
        assert_eq!(
            to_value(&Pair(7, "y")).unwrap(),
            list([Value::Int(7), Value::from("y")])
        );
        assert_eq!(to_value(&Vec::<i32>::new()).unwrap(), list([]));
    }

    #[test]
    fn enums_are_externally_tagged() {
        assert_eq!(to_value(&Shape::Empty).unwrap(), Value::from("Empty"));
        assert_eq!(
            to_value(&Shape::Circle(1.0)).unwrap(),
            map([("Circle", Value::Float(1.0))])
        );
        assert_eq!(
            to_value(&Shape::Line(1, 2)).unwrap(),
            map([("Line", list([Value::Int(1), Value::Int(2)]))])
        );
        assert_eq!(
            to_value(&Shape::Rect { w: 3, h: 4 }).unwrap(),
            map([("Rect", map([("w", Value::Int(3)), ("h", Value::Int(4))]))])
        );
    }

    #[test]
    fn map_keys_follow_php_normalisation() {
        let mut m = BTreeMap::new();
        m.insert("10", 1);
        m.insert("010", 2);
        m.insert("a", 3);
        let value = to_value(&m).unwrap();
        let keys: Vec<ArrayKey> = value.as_array().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
                ArrayKey::from("010"),
                ArrayKey::Int(10),
                ArrayKey::from("a")
            ]
        );

        let mut m = BTreeMap::new();
        m.insert(u64::MAX, "big");
        m.insert(1, "one");
        let keys: Vec<ArrayKey> = to_value(&m)
            .unwrap()
            .as_array()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            keys,
            vec![ArrayKey::Int(1), ArrayKey::from("18446744073709551615")]
        );

        let mut m = BTreeMap::new();
        m.insert(false, "f");
        m.insert(true, "t");
        assert_eq!(
            to_value(&m).unwrap(),
            Value::Array(Array::from([(0, "f"), (1, "t")]))
        );

        #[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
        enum Key {
            Alpha,
        }
        let mut m = BTreeMap::new();
        m.insert(Key::Alpha, 1);
        assert_eq!(to_value(&m).unwrap(), map([("Alpha", Value::Int(1))]));

        // a struct field named like an integer is an int key, as in a PHP array literal
        #[derive(Serialize)]
        struct Renamed {
            #[serde(rename = "0")]
            zero: bool,
        }
        assert_eq!(
            to_value(&Renamed { zero: true }).unwrap(),
            Value::Array(Array::from([(0, true)]))
        );
    }

    #[test]
    fn unsupported_keys_are_errors() {
        let mut m = HashMap::new();
        m.insert(vec![1], 1);
        assert_eq!(
            to_value(&m).unwrap_err().to_string(),
            "a map key must be a string, an integer, a bool, a char or a unit variant, not a sequence"
        );
        let mut m = BTreeMap::new();
        m.insert(None::<i32>, 1);
        assert_eq!(
            to_value(&m).unwrap_err().to_string(),
            "a map key must be a string, an integer, a bool, a char or a unit variant, not None"
        );
        struct FloatKey;
        impl Serialize for FloatKey {
            fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                use ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(&1.5, &1)?;
                map.end()
            }
        }
        assert_eq!(
            to_value(&FloatKey).unwrap_err().to_string(),
            "a map key must be a string, an integer, a bool, a char or a unit variant, not a float"
        );
    }

    #[test]
    fn custom_errors_pass_through() {
        struct Failing;
        impl Serialize for Failing {
            fn serialize<S: ser::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(ser::Error::custom("lock poisoned"))
            }
        }
        assert_eq!(
            to_value(&vec![Failing]).unwrap_err().to_string(),
            "lock poisoned"
        );
    }

    #[test]
    fn arrays_for_contexts() {
        #[derive(Serialize)]
        struct Page {
            title: &'static str,
        }
        let array = to_array(&Page { title: "Home" }).unwrap();
        assert_eq!(array.get_str(b"title"), Some(&Value::from("Home")));
        assert_eq!(to_array(&[1, 2]).unwrap().len(), 2);
        assert_eq!(
            to_array(&"text").unwrap_err().to_string(),
            "the value must serialize to a map, a struct or a sequence to become an array, not to string"
        );
        assert_eq!(
            to_array(&None::<Page>).unwrap_err().to_string(),
            "the value must serialize to a map, a struct or a sequence to become an array, not to null"
        );
    }
}
