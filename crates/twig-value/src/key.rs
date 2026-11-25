//! Array keys and key normalisation.
//!
//! reference/php/engine-semantics.md §8.1-§8.2. PHP arrays have two key kinds, `int` and byte
//! `string`; canonical decimal integer strings are stored as ints (`"1"` → `1`, but `"01"`, `"-0"`,
//! `" 1"`, `"+1"`, `"1.0"` stay strings).

use std::borrow::Borrow;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::convert;
use crate::diag::Diagnostics;
use crate::error::PhpError;
use crate::string::PhpString;
use crate::types;
use crate::value::Value;

/// A normalised array key (`zend_ulong h` or `zend_string *key` of a `Bucket`).
///
/// Construct string keys through the `From` impls, which apply the symtable normalisation (a
/// canonical integer string becomes [`ArrayKey::Int`]); build [`ArrayKey::Str`] directly only for
/// strings known not to be canonical integers.
#[derive(Clone, PartialEq, Eq)]
pub enum ArrayKey {
    /// Integer key.
    Int(i64),
    /// String key (never a canonical decimal integer).
    Str(PhpString),
}

/// A borrowed array key, for lookups that must not allocate ([`Array::get_ref`]).
///
/// Like [`ArrayKey`], a [`KeyRef::Str`] is expected to be already normalised (not a canonical
/// integer string); use [`KeyRef::sym`] to normalise a byte string.
///
/// [`Array::get_ref`]: crate::Array::get_ref
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRef<'a> {
    /// Integer key.
    Int(i64),
    /// String key bytes.
    Str(&'a [u8]),
}

impl<'a> KeyRef<'a> {
    /// Symtable normalisation of a byte string (`"1"` → `Int(1)`, `"01"` stays a string).
    pub fn sym(key: &'a [u8]) -> KeyRef<'a> {
        match handle_numeric_str(key) {
            Some(h) => KeyRef::Int(h),
            None => KeyRef::Str(key),
        }
    }

    /// The owned key.
    pub fn to_owned_key(self) -> ArrayKey {
        match self {
            KeyRef::Int(i) => ArrayKey::Int(i),
            KeyRef::Str(s) => ArrayKey::Str(PhpString::from(s)),
        }
    }
}

impl Hash for KeyRef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match *self {
            KeyRef::Int(i) => {
                state.write_u8(0);
                state.write_i64(i);
            }
            KeyRef::Str(s) => {
                state.write_u8(1);
                s.hash(state);
            }
        }
    }
}

impl Hash for ArrayKey {
    /// Hashes like the equivalent [`KeyRef`], so maps keyed by `ArrayKey` can be queried with a
    /// borrowed key.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_key_ref().hash(state);
    }
}

/// The lookup view shared by [`ArrayKey`] and [`KeyRef`]: lets a `HashMap<ArrayKey, _>` be
/// queried with a [`KeyRef`] (through `Borrow<dyn KeyView>`) without building an `ArrayKey`.
pub(crate) trait KeyView {
    fn view(&self) -> KeyRef<'_>;
}

impl KeyView for ArrayKey {
    fn view(&self) -> KeyRef<'_> {
        self.as_key_ref()
    }
}

impl KeyView for KeyRef<'_> {
    fn view(&self) -> KeyRef<'_> {
        *self
    }
}

impl<'a> Borrow<dyn KeyView + 'a> for ArrayKey {
    fn borrow(&self) -> &(dyn KeyView + 'a) {
        self
    }
}

impl Hash for dyn KeyView + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.view().hash(state);
    }
}

impl PartialEq for dyn KeyView + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.view() == other.view()
    }
}

impl Eq for dyn KeyView + '_ {}

/// Port of `_zend_handle_numeric_str()` / `_zend_handle_numeric_str_ex()` (PHPSRC/Zend/zend_hash.h,
/// PHPSRC/Zend/zend_hash.c): the integer a string key denotes, if it is a canonical decimal integer
/// within `zend_long` range (`-` sign only, no leading zeros, no whitespace, no `+`).
pub fn handle_numeric_str(key: &[u8]) -> Option<i64> {
    let at = |i: usize| -> u8 { if i < key.len() { key[i] } else { 0 } };
    // _zend_handle_numeric_str(): quick rejection (char is signed: bytes >= 0x80 are "< '0'").
    let first = at(0);
    if first > b'9' && first < 0x80 {
        return None;
    }
    if !first.is_ascii_digit() {
        if first != b'-' {
            return None;
        }
        if !at(1).is_ascii_digit() {
            return None;
        }
    }
    // _zend_handle_numeric_str_ex()
    let mut i = usize::from(first == b'-');
    if (at(i) == b'0' && key.len() > 1) || key.len() - i > 19 {
        return None;
    }
    let mut idx: u64 = u64::from(at(i) - b'0');
    loop {
        i += 1;
        if i == key.len() {
            if key[0] == b'-' {
                if idx.wrapping_sub(1) > i64::MAX as u64 {
                    return None;
                }
                return Some(0u64.wrapping_sub(idx) as i64);
            } else if idx > i64::MAX as u64 {
                return None;
            }
            return Some(idx as i64);
        }
        let c = key[i];
        if c.is_ascii_digit() {
            idx = idx * 10 + u64::from(c - b'0');
        } else {
            return None;
        }
    }
}

impl ArrayKey {
    /// A string key with symtable normalisation (`zend_symtable_*`): canonical integer strings
    /// become int keys.
    pub fn from_string(s: PhpString) -> ArrayKey {
        match handle_numeric_str(&s) {
            Some(i) => ArrayKey::Int(i),
            None => ArrayKey::Str(s),
        }
    }

    /// The borrowed form of this key.
    pub fn as_key_ref(&self) -> KeyRef<'_> {
        match self {
            ArrayKey::Int(i) => KeyRef::Int(*i),
            ArrayKey::Str(s) => KeyRef::Str(s.as_bytes()),
        }
    }

    /// The key as a PHP value (`int` or `string`), as `foreach`/`array_keys()` return it.
    pub fn to_value(&self) -> Value {
        match self {
            ArrayKey::Int(i) => Value::Int(*i),
            ArrayKey::Str(s) => Value::String(s.clone()),
        }
    }

    /// The key as bytes, the way PHP prints it with `%s` after string conversion (ints in decimal).
    pub fn to_php_string(&self) -> PhpString {
        match self {
            ArrayKey::Int(i) => PhpString::from(i.to_string()),
            ArrayKey::Str(s) => s.clone(),
        }
    }

    /// The integer, for int keys.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            ArrayKey::Int(i) => Some(*i),
            ArrayKey::Str(_) => None,
        }
    }

    /// The string, for string keys.
    pub fn as_str(&self) -> Option<&PhpString> {
        match self {
            ArrayKey::Int(_) => None,
            ArrayKey::Str(s) => Some(s),
        }
    }

    /// Normalises a PHP value used as an array offset, with PHP 8.5 diagnostics.
    ///
    /// Port of the offset conversions in `slow_index_convert()` / `slow_index_convert_w()` /
    /// `zend_find_array_dim_slow()` / `ZEND_ADD_ARRAY_ELEMENT` / `ZEND_UNSET_DIM`
    /// (PHPSRC/Zend/zend_execute.c, PHPSRC/Zend/zend_vm_def.h) and `zend_array_key_exists_fast()`:
    ///
    /// * int → itself; string → symtable normalisation; `false`/`true` → `0`/`1`;
    /// * float → `zend_dval_to_lval_safe()`: `Implicit conversion from float %s to int loses
    ///   precision` (deprecation) and/or `The float %s is not representable as an int, cast occurred`
    ///   (warning, wrapped value);
    /// * `null` → `""` with `Using null as an array offset is deprecated, use an empty string
    ///   instead` (no diagnostic for `unset`; a different text for `array_key_exists()`);
    /// * array/object → `TypeError` (`Cannot access offset of type %s on array`, `... in isset or
    ///   empty`, `Cannot unset offset of type %s on array`).
    pub fn from_value(
        value: &Value,
        context: KeyContext,
        diag: &mut dyn Diagnostics,
    ) -> Result<ArrayKey, PhpError> {
        match value {
            Value::Int(i) => Ok(ArrayKey::Int(*i)),
            Value::String(s) => Ok(ArrayKey::from_string(s.clone())),
            Value::Bool(b) => Ok(ArrayKey::Int(i64::from(*b))),
            Value::Float(d) => Ok(ArrayKey::Int(convert::dval_to_lval_safe(*d, diag)?)),
            Value::Null => {
                match context {
                    KeyContext::Unset => {}
                    KeyContext::ArrayKeyExists => diag.deprecated(
                        "Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead",
                    )?,
                    _ => diag.deprecated("Using null as an array offset is deprecated, use an empty string instead")?,
                }
                Ok(ArrayKey::Str(PhpString::empty()))
            }
            Value::Array(_) | Value::Object(_) => Err(illegal_offset(value, context, b"array")),
        }
    }
}

/// The access an offset is converted for; selects PHP's diagnostics and error texts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyContext {
    /// Read (`$a[$k]`, `$a[$k] ?? x`, `BP_VAR_R`/`BP_VAR_IS`).
    Read,
    /// Write (`$a[$k] = v`, array literals, `BP_VAR_W`/`BP_VAR_RW`).
    Write,
    /// `isset($a[$k])` / `empty($a[$k])` (`BP_VAR_IS` with the isset error text).
    Isset,
    /// `unset($a[$k])` (`BP_VAR_UNSET`): `null` is converted silently.
    Unset,
    /// `array_key_exists($k, $a)`: its own `null` deprecation text.
    ArrayKeyExists,
}

/// Port of `zend_illegal_container_offset()` (PHPSRC/Zend/zend.c) for an array/object offset
/// used on `container` (`array`, `string`, or a class name such as `ArrayObject`).
pub fn illegal_offset(offset: &Value, context: KeyContext, container: &[u8]) -> PhpError {
    let ty = types::zend_zval_type_name(offset);
    match context {
        KeyContext::Isset => PhpError::type_error(PhpString::concat(&[
            b"Cannot access offset of type ",
            ty.as_bytes(),
            b" in isset or empty",
        ])),
        KeyContext::Unset => {
            if container == b"string" {
                PhpError::error("Cannot unset string offsets")
            } else {
                PhpError::type_error(PhpString::concat(&[
                    b"Cannot unset offset of type ",
                    ty.as_bytes(),
                    b" on ",
                    container,
                ]))
            }
        }
        _ => PhpError::type_error(PhpString::concat(&[
            b"Cannot access offset of type ",
            ty.as_bytes(),
            b" on ",
            container,
        ])),
    }
}

impl From<i64> for ArrayKey {
    fn from(i: i64) -> ArrayKey {
        ArrayKey::Int(i)
    }
}

impl From<i32> for ArrayKey {
    fn from(i: i32) -> ArrayKey {
        ArrayKey::Int(i64::from(i))
    }
}

impl From<u32> for ArrayKey {
    fn from(i: u32) -> ArrayKey {
        ArrayKey::Int(i64::from(i))
    }
}

impl From<PhpString> for ArrayKey {
    /// Symtable normalisation (`"1"` → `Int(1)`).
    fn from(s: PhpString) -> ArrayKey {
        ArrayKey::from_string(s)
    }
}

impl From<&PhpString> for ArrayKey {
    /// Symtable normalisation (`"1"` → `Int(1)`).
    fn from(s: &PhpString) -> ArrayKey {
        ArrayKey::from_string(s.clone())
    }
}

impl From<&str> for ArrayKey {
    /// Symtable normalisation (`"1"` → `Int(1)`).
    fn from(s: &str) -> ArrayKey {
        ArrayKey::from_string(PhpString::from(s))
    }
}

impl From<String> for ArrayKey {
    /// Symtable normalisation (`"1"` → `Int(1)`).
    fn from(s: String) -> ArrayKey {
        ArrayKey::from_string(PhpString::from(s))
    }
}

impl From<&[u8]> for ArrayKey {
    /// Symtable normalisation (`"1"` → `Int(1)`).
    fn from(s: &[u8]) -> ArrayKey {
        ArrayKey::from_string(PhpString::from(s))
    }
}

impl fmt::Debug for ArrayKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArrayKey::Int(i) => write!(f, "{i}"),
            ArrayKey::Str(s) => write!(f, "{s:?}"),
        }
    }
}

impl fmt::Display for ArrayKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArrayKey::Int(i) => write!(f, "{i}"),
            ArrayKey::Str(s) => write!(f, "{s}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{Diagnostic, Level};

    #[test]
    fn numeric_keys() {
        let h = |s: &str| handle_numeric_str(s.as_bytes());
        assert_eq!(h("0"), Some(0));
        assert_eq!(h("1"), Some(1));
        assert_eq!(h("-1"), Some(-1));
        assert_eq!(h("9223372036854775807"), Some(i64::MAX));
        assert_eq!(h("-9223372036854775808"), Some(i64::MIN));
        for s in [
            "-0",
            "00",
            "01",
            "08",
            "-01",
            "1.5",
            "1.0",
            " 1",
            "1 ",
            "+1",
            "9223372036854775808",
            "-9223372036854775809",
            "0x1",
            "1e3",
            "",
            "a",
            "-",
            "0.0",
            "1\0",
            "99999999999999999999",
        ] {
            assert_eq!(h(s), None, "{s:?}");
        }
        assert_eq!(handle_numeric_str(b"\xff1"), None);
    }

    #[test]
    fn offsets() {
        let mut d: Vec<Diagnostic> = Vec::new();
        let k = ArrayKey::from_value(&Value::Float(1.5), KeyContext::Write, &mut d).unwrap();
        assert_eq!(k, ArrayKey::Int(1));
        let k = ArrayKey::from_value(&Value::Null, KeyContext::Read, &mut d).unwrap();
        assert_eq!(k, ArrayKey::Str(PhpString::empty()));
        let k = ArrayKey::from_value(&Value::Null, KeyContext::Unset, &mut d).unwrap();
        assert_eq!(k, ArrayKey::Str(PhpString::empty()));
        assert_eq!(
            d,
            vec![
                Diagnostic::new(
                    Level::Deprecated,
                    "Implicit conversion from float 1.5 to int loses precision"
                ),
                Diagnostic::new(
                    Level::Deprecated,
                    "Using null as an array offset is deprecated, use an empty string instead"
                ),
            ]
        );
        let e = ArrayKey::from_value(
            &Value::Array(crate::Array::new()),
            KeyContext::Isset,
            &mut d,
        )
        .unwrap_err();
        assert_eq!(
            e.message(),
            "Cannot access offset of type array in isset or empty"
        );
        let e = ArrayKey::from_value(
            &Value::Array(crate::Array::new()),
            KeyContext::Unset,
            &mut d,
        )
        .unwrap_err();
        assert_eq!(e.message(), "Cannot unset offset of type array on array");
    }
}
