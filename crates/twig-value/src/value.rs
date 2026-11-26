//! The PHP value (`zval`), and the conversions of Rust data into values and array keys.
//!
//! # Conversions from Rust types
//!
//! `From` turns Rust data into PHP values, so that template contexts can be built from ordinary Rust values
//! (`Value::from(vec![1, 2])`, `Array::from([("name", "World")])`). Integers outside PHP's `int` range follow what
//! `json_decode($json, true)` gives for the same JSON data: such a value becomes a float, such a key the string of
//! its digits.
//!
//! | Rust | PHP |
//! |---|---|
//! | `bool` | `bool` |
//! | `i8`, `i16`, `i32`, `i64`, `u16`, `u32` | `int` |
//! | `isize`, `u64`, `usize`, `i128`, `u128` | `int`, or `float` when the value is outside the `int` range (`json_decode()` turns such numbers into floats) |
//! | `f32`, `f64` | `float` |
//! | `char`, `&str`, `&String`, `String`, `Cow<str>` | `string` (UTF-8) |
//! | `&[u8]`, `Vec<u8>`, [`PhpString`] | `string` (the bytes) |
//! | `Option<T>` | `null` for `None`, else `T`'s value |
//! | `Vec<T>`, `&[T]`, `[T; N]` | a list (keys `0..n`) |
//! | `HashMap<K, V>`, `BTreeMap<K, V>`, `IndexMap<K, V>`, `[(K, V); N]` | an array in the map's iteration order |
//! | [`Array`], [`ObjectRef`] | themselves |
//!
//! `u8` alone has no conversion: byte slices and byte vectors are PHP strings, so a `Vec<u8>` cannot also be a list
//! of ints (convert the bytes to a wider integer type for that).
//!
//! Map keys become [`ArrayKey`]s with PHP's key normalisation: a string that is a canonical decimal integer becomes an
//! int key (`"1"` → `1`, while `"01"` and `"1.0"` stay strings, `zend_symtable_update()`), integers are int keys
//! (those outside the `int` range become their decimal string, the key `json_decode()` gives for such an object key),
//! `true` and `false` are the keys `1` and `0` (PHP's cast of bool offsets), a `char` is a one-character string. When
//! two keys normalise to the same key, the later value replaces the earlier one in its position, as in a PHP array
//! literal. A `HashMap` iterates in an unspecified order that differs between runs; use a `BTreeMap` or an `IndexMap`
//! when the order of the PHP array matters.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

use indexmap::IndexMap;

use crate::array::Array;
use crate::key::ArrayKey;
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

// ---- conversions from Rust types (see the module documentation) ----------------------------------------------------

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

/// Integer types that always fit PHP's `int`.
macro_rules! value_from_int {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Value {
            fn from(i: $ty) -> Value {
                Value::Int(i64::from(i))
            }
        }
    )*};
}

value_from_int!(i8, i16, i32, i64, u16, u32);

/// Integer types with values outside PHP's `int` range: those become floats, rounded to the nearest float, as
/// `json_decode()` does with an integer it cannot represent (`zend_strtod()` of its digits,
/// PHPSRC/ext/json/json_scanner.re).
macro_rules! value_from_wide_int {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Value {
            /// An `int`, or the nearest `float` when the value is outside the `int` range.
            fn from(i: $ty) -> Value {
                match i64::try_from(i) {
                    Ok(i) => Value::Int(i),
                    Err(_) => Value::Float(i as f64),
                }
            }
        }
    )*};
}

value_from_wide_int!(isize, u64, usize, i128, u128);

impl From<f64> for Value {
    fn from(f: f64) -> Value {
        Value::Float(f)
    }
}

impl From<f32> for Value {
    /// The `float` of the same value (`f32` widens to `f64` exactly).
    fn from(f: f32) -> Value {
        Value::Float(f64::from(f))
    }
}

impl From<char> for Value {
    /// A string of the character's UTF-8 encoding.
    fn from(c: char) -> Value {
        Value::String(PhpString::from(c.encode_utf8(&mut [0; 4]) as &str))
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::String(PhpString::from(s))
    }
}

impl From<&String> for Value {
    fn from(s: &String) -> Value {
        Value::String(PhpString::from(s))
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::String(PhpString::from(s))
    }
}

impl From<Cow<'_, str>> for Value {
    fn from(s: Cow<'_, str>) -> Value {
        Value::String(PhpString::from(s))
    }
}

impl From<&[u8]> for Value {
    /// A string of these bytes.
    fn from(s: &[u8]) -> Value {
        Value::String(PhpString::from(s))
    }
}

impl From<Vec<u8>> for Value {
    /// A string of these bytes.
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

/// An array with the given entries inserted in order (update semantics: a repeated key keeps its first position and
/// takes the last value).
fn map_array<K: Into<ArrayKey>, V: Into<Value>>(
    entries: impl IntoIterator<Item = (K, V)>,
) -> Array {
    let mut array = Array::new();
    for (key, value) in entries {
        array.insert(key, value);
    }
    array
}

impl<T: Into<Value>> From<Vec<T>> for Array {
    /// A list of the converted elements (keys `0..n`).
    fn from(values: Vec<T>) -> Array {
        Array::from_values(values.into_iter().map(Into::into))
    }
}

impl<T: Clone + Into<Value>> From<&[T]> for Array {
    /// A list of the converted elements (keys `0..n`).
    fn from(values: &[T]) -> Array {
        Array::from_values(values.iter().cloned().map(Into::into))
    }
}

impl<T: Into<Value>, const N: usize> From<[T; N]> for Array {
    /// A list of the converted elements (keys `0..n`).
    fn from(values: [T; N]) -> Array {
        Array::from_values(values.into_iter().map(Into::into))
    }
}

impl<K: Into<ArrayKey>, V: Into<Value>, const N: usize> From<[(K, V); N]> for Array {
    /// An array of these key-value pairs, in order.
    fn from(entries: [(K, V); N]) -> Array {
        map_array(entries)
    }
}

impl<K: Into<ArrayKey>, V: Into<Value>, S> From<HashMap<K, V, S>> for Array {
    /// An array of the map's entries in its iteration order (unspecified for a `HashMap`).
    fn from(map: HashMap<K, V, S>) -> Array {
        map_array(map)
    }
}

impl<K: Into<ArrayKey>, V: Into<Value>> From<BTreeMap<K, V>> for Array {
    /// An array of the map's entries in key order.
    fn from(map: BTreeMap<K, V>) -> Array {
        map_array(map)
    }
}

impl<K: Into<ArrayKey>, V: Into<Value>, S> From<IndexMap<K, V, S>> for Array {
    /// An array of the map's entries in insertion order.
    fn from(map: IndexMap<K, V, S>) -> Array {
        map_array(map)
    }
}

/// The `Value` conversions of the collections that convert into an [`Array`].
macro_rules! value_from_array {
    ($([$($generics:tt)*] $ty:ty),* $(,)?) => {$(
        impl<$($generics)*> From<$ty> for Value {
            /// The array `Array::from()` makes of it.
            fn from(collection: $ty) -> Value {
                Value::Array(Array::from(collection))
            }
        }
    )*};
}

value_from_array!(
    [T: Into<Value>] Vec<T>,
    [T: Clone + Into<Value>] &[T],
    [T: Into<Value>, const N: usize] [T; N],
    [K: Into<ArrayKey>, V: Into<Value>, const N: usize] [(K, V); N],
    [K: Into<ArrayKey>, V: Into<Value>, S] HashMap<K, V, S>,
    [K: Into<ArrayKey>, V: Into<Value>] BTreeMap<K, V>,
    [K: Into<ArrayKey>, V: Into<Value>, S] IndexMap<K, V, S>,
);

/// Integer types whose values are all int keys (`i32`, `i64` and `u32` convert in `key.rs`).
macro_rules! key_from_int {
    ($($ty:ty),*) => {$(
        impl From<$ty> for ArrayKey {
            fn from(i: $ty) -> ArrayKey {
                ArrayKey::Int(i64::from(i))
            }
        }
    )*};
}

key_from_int!(i8, i16, u8, u16);

/// Integer types with values outside PHP's `int` range: such a key becomes the string of its decimal digits, which
/// no int key equals (the key `json_decode($json, true)` gives for an object key of these digits).
macro_rules! key_from_wide_int {
    ($($ty:ty),*) => {$(
        impl From<$ty> for ArrayKey {
            /// An int key, or the decimal string of a value outside the `int` range.
            fn from(i: $ty) -> ArrayKey {
                match i64::try_from(i) {
                    Ok(i) => ArrayKey::Int(i),
                    Err(_) => ArrayKey::from_string(PhpString::from(i.to_string())),
                }
            }
        }
    )*};
}

key_from_wide_int!(isize, u64, usize, i128, u128);

impl From<bool> for ArrayKey {
    /// `1` or `0`: PHP casts a bool offset to int.
    fn from(b: bool) -> ArrayKey {
        ArrayKey::Int(i64::from(b))
    }
}

impl From<char> for ArrayKey {
    /// The one-character string, with symtable normalisation (`'7'` → `7`).
    fn from(c: char) -> ArrayKey {
        ArrayKey::from(c.encode_utf8(&mut [0; 4]) as &str)
    }
}

impl From<&String> for ArrayKey {
    /// Symtable normalisation (`"1"` → `Int(1)`).
    fn from(s: &String) -> ArrayKey {
        ArrayKey::from_string(PhpString::from(s))
    }
}

impl From<Cow<'_, str>> for ArrayKey {
    /// Symtable normalisation (`"1"` → `Int(1)`).
    fn from(s: Cow<'_, str>) -> ArrayKey {
        ArrayKey::from_string(PhpString::from(s))
    }
}

impl From<Vec<u8>> for ArrayKey {
    /// Symtable normalisation (`"1"` → `Int(1)`).
    fn from(s: Vec<u8>) -> ArrayKey {
        ArrayKey::from_string(PhpString::from(s))
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

    fn keys(array: &Array) -> Vec<ArrayKey> {
        array.keys().cloned().collect()
    }

    #[test]
    fn integers_outside_the_int_range_become_floats() {
        assert_eq!(Value::from(-5i8), Value::Int(-5));
        assert_eq!(Value::from(65535u16), Value::Int(65535));
        assert_eq!(Value::from(7usize), Value::Int(7));
        assert_eq!(Value::from(i64::MAX as u64), Value::Int(i64::MAX));
        // json_decode('9223372036854775808') === 9.2233720368547758E+18
        assert_eq!(
            Value::from(i64::MAX as u64 + 1),
            Value::Float(9_223_372_036_854_775_808.0)
        );
        assert_eq!(
            Value::from(u64::MAX),
            Value::Float(18_446_744_073_709_551_615.0)
        );
        assert_eq!(Value::from(i128::from(i64::MIN)), Value::Int(i64::MIN));
        assert_eq!(
            Value::from(i128::from(i64::MIN) - 1),
            Value::Float(-9_223_372_036_854_775_809.0)
        );
        assert_eq!(Value::from(u128::MAX), Value::Float(u128::MAX as f64));
        assert_eq!(Value::from(0.5f32), Value::Float(0.5));
        assert_eq!(Value::from(0.1f32), Value::Float(0.100_000_001_490_116_12));
    }

    #[test]
    fn strings_and_characters() {
        assert_eq!(Value::from('é'), Value::from("é"));
        assert_eq!(Value::from(&String::from("a")), Value::from("a"));
        assert_eq!(Value::from(Cow::Borrowed("b")), Value::from("b"));
        // byte vectors and slices are strings, not lists
        assert_eq!(Value::from(vec![0x61u8, 0x62]), Value::from("ab"));
        assert_eq!(Value::from(&b"ab"[..]), Value::from("ab"));
    }

    #[test]
    fn sequences_become_lists() {
        let list = Value::from(vec![1, 2]);
        let expected = Value::Array(Array::from_values([Value::Int(1), Value::Int(2)]));
        assert_eq!(list, expected);
        assert_eq!(Value::from([1, 2]), expected);
        assert_eq!(Value::from(&[1, 2][..]), expected);
        assert_eq!(
            Value::from(vec![Some("a"), None]),
            Value::Array(Array::from_values([Value::from("a"), Value::Null]))
        );
        let nested = Array::from(vec![vec![1], vec![]]);
        assert_eq!(nested.len(), 2);
        assert_eq!(
            nested.get_int(0),
            Some(&Value::Array(Array::from_values([Value::Int(1)])))
        );
        assert!(Array::from(Vec::<Value>::new()).is_empty());
    }

    #[test]
    fn map_keys_are_normalised_like_php() {
        let array = Array::from([("1", "a"), ("01", "b"), ("-0", "c"), ("x", "d"), ("1", "e")]);
        assert_eq!(
            keys(&array),
            vec![
                ArrayKey::Int(1),
                ArrayKey::from("01"),
                ArrayKey::from("-0"),
                ArrayKey::from("x")
            ]
        );
        // the repeated key keeps its first position and takes the last value
        assert_eq!(array.get_int(1), Some(&Value::from("e")));

        let mut map = BTreeMap::new();
        map.insert(u64::MAX, 1);
        map.insert(3, 2);
        let array = Array::from(map);
        assert_eq!(
            keys(&array),
            vec![ArrayKey::Int(3), ArrayKey::from("18446744073709551615")]
        );
        assert_eq!(
            ArrayKey::from("18446744073709551615"),
            ArrayKey::Str(PhpString::from("18446744073709551615"))
        );

        let array = Array::from([(true, "t"), (false, "f")]);
        assert_eq!(keys(&array), vec![ArrayKey::Int(1), ArrayKey::Int(0)]);
        let array = Array::from([('7', 1), ('x', 2)]);
        assert_eq!(keys(&array), vec![ArrayKey::Int(7), ArrayKey::from("x")]);
        assert_eq!(ArrayKey::from(-3i8), ArrayKey::Int(-3));
        assert_eq!(
            ArrayKey::from(i128::MIN),
            ArrayKey::from(i128::MIN.to_string())
        );
        assert_eq!(ArrayKey::from(b"12".to_vec()), ArrayKey::Int(12));
    }

    #[test]
    fn maps_keep_their_iteration_order() {
        let mut map = IndexMap::new();
        map.insert("b".to_owned(), vec![1]);
        map.insert("a".to_owned(), vec![]);
        let value = Value::from(map);
        let array = value.as_array().unwrap();
        assert_eq!(keys(array), vec![ArrayKey::from("b"), ArrayKey::from("a")]);

        let mut map = HashMap::new();
        map.insert(10, "x");
        map.insert(20, "y");
        let array = Array::from(map);
        assert_eq!(array.len(), 2);
        assert_eq!(array.get_int(20), Some(&Value::from("y")));
    }
}
