//! Type names used in PHP messages.
//!
//! reference/php/engine-semantics.md §2.3. Four naming functions exist and different messages use
//! different ones:
//!
//! | value | [`zend_zval_type_name`] | [`zend_zval_value_name`] | [`gettype`] | [`get_debug_type`] |
//! |---|---|---|---|---|
//! | `null` | `null` | `null` | `NULL` | `null` |
//! | `false` / `true` | `bool` | `false` / `true` | `boolean` | `bool` |
//! | int | `int` | `int` | `integer` | `int` |
//! | float | `float` | `float` | `double` | `float` |
//! | string | `string` | `string` | `string` | `string` |
//! | array | `array` | `array` | `array` | `array` |
//! | object | class name | class name | `object` | class name (`class@anonymous`) |
//!
//! Class names are printed with C `%s`, i.e. up to the first NUL byte (anonymous classes).

use std::borrow::Cow;

use crate::value::Value;

/// Port of `zend_zval_type_name()` (PHPSRC/Zend/zend_API.c): used by `Unsupported operand types:
/// %s %s %s`, `Cannot access offset of type %s on %s`, `Cannot use %s as array`.
pub fn zend_zval_type_name(v: &Value) -> Cow<'static, str> {
    match v {
        Value::Object(o) => Cow::Owned(o.class().display_name().to_string()),
        other => Cow::Borrowed(other.type_tag().name()),
    }
}

/// Port of `zend_zval_value_name()` (PHPSRC/Zend/zend_API.c): like [`zend_zval_type_name`] but
/// booleans print as `true`/`false`. Used by parameter type errors (`must be of type %s, %s
/// given`), `Cannot perform bitwise not on %s`, `Trying to access array offset on %s`.
pub fn zend_zval_value_name(v: &Value) -> Cow<'static, str> {
    match v {
        Value::Bool(true) => Cow::Borrowed("true"),
        Value::Bool(false) => Cow::Borrowed("false"),
        other => zend_zval_type_name(other),
    }
}

/// Port of `get_debug_type()` (PHPSRC/ext/standard/type.c): `null`, `bool`, `int`, `float`,
/// `string`, `array`, or the class name (anonymous classes up to the NUL: `class@anonymous`). Twig
/// embeds it in many error messages.
pub fn get_debug_type(v: &Value) -> Cow<'static, str> {
    zend_zval_type_name(v)
}

/// Port of `gettype()` (PHPSRC/ext/standard/type.c, `zend_zval_get_legacy_type()`).
pub fn gettype(v: &Value) -> &'static str {
    match v {
        Value::Null => "NULL",
        Value::Bool(_) => "boolean",
        Value::Int(_) => "integer",
        Value::Float(_) => "double",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::StdClass;

    #[test]
    fn names() {
        let obj = Value::Object(StdClass::new().into_ref());
        assert_eq!(zend_zval_type_name(&Value::Bool(true)), "bool");
        assert_eq!(zend_zval_value_name(&Value::Bool(true)), "true");
        assert_eq!(zend_zval_value_name(&Value::Bool(false)), "false");
        assert_eq!(zend_zval_type_name(&obj), "stdClass");
        assert_eq!(get_debug_type(&Value::Float(1.0)), "float");
        assert_eq!(gettype(&Value::Float(1.0)), "double");
        assert_eq!(gettype(&Value::Null), "NULL");
        assert_eq!(gettype(&obj), "object");
    }
}
