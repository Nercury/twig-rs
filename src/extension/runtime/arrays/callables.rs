//! PHP callables handed to the internal functions behind the array filters (`array_filter()`, `uasort()`,
//! `CallbackFilterIterator`): the `callable` parameter check and the call itself.

use twig_value::object::{CallArgs, Closure, Visibility};
use twig_value::{Array, Diagnostics, PhpError, PhpResult, PhpString, Value};

/// Why a value is not callable, in the words of `zend_is_callable_at_frame()` (PHPSRC/Zend/zend_API.c); `None`
/// when it is callable.
///
/// Closures and objects with `__invoke()` are callable, `[$object, 'method']` arrays when the method is public (or
/// the class has `__call()`). twig-rs has no table of PHP functions and classes for callable strings: a function
/// name is always unknown (`function "x" not found or invalid function name`) and so is the class of a
/// `Class::method` string or array (`class "X" not found`), see `reference/deviations.md`.
pub(super) fn not_callable_reason(value: &Value) -> Option<PhpString> {
    match value {
        Value::Object(object) => {
            if object.is_callable() {
                None
            } else {
                Some(PhpString::from("no array or string given"))
            }
        }
        Value::String(name) => Some(string_reason(name)),
        Value::Array(array) => array_reason(array),
        _ => Some(PhpString::from("no array or string given")),
    }
}

/// `zend_is_callable_check_func()` for a string that names no function: a compound `Class::method` name fails on
/// its class (`zend_is_callable_check_class()`), anything else as an unknown function.
fn string_reason(name: &PhpString) -> PhpString {
    let bytes = name.as_bytes();
    // zend_memrchr(name, ':') with a ':' before it, not at the start
    if let Some(colon) = memchr::memrchr(b':', bytes)
        && colon > 0
        && bytes[colon - 1] == b':'
    {
        let class_end = colon - 1;
        if class_end == 0 {
            return PhpString::from("invalid function name");
        }
        return PhpString::concat(&[b"class \"", c_str(&bytes[..class_end]), b"\" not found"]);
    }
    PhpString::concat(&[
        b"function \"",
        name.c_str(),
        b"\" not found or invalid function name",
    ])
}

/// The part of a byte string C's `%s` prints (up to the first NUL byte).
fn c_str(bytes: &[u8]) -> &[u8] {
    match memchr::memchr(0, bytes) {
        Some(nul) => &bytes[..nul],
        None => bytes,
    }
}

/// The `IS_ARRAY` branch of `zend_is_callable_at_frame()`.
fn array_reason(array: &Array) -> Option<PhpString> {
    if array.len() != 2 {
        return Some(PhpString::from(
            "array callback must have exactly two members",
        ));
    }
    let (Some(target), Some(method)) = (array.get_int(0), array.get_int(1)) else {
        return Some(PhpString::from(
            "array callback has to contain indices 0 and 1",
        ));
    };
    if !matches!(target, Value::String(_) | Value::Object(_)) {
        return Some(PhpString::from(
            "first array member is not a valid class name or object",
        ));
    }
    let Value::String(method) = method else {
        return Some(PhpString::from("second array member is not a valid method"));
    };
    match target {
        Value::Object(object) => {
            let name = method.to_string_lossy();
            let class = object.class();
            if let Some(info) = class.method(&name) {
                if info.visibility == Visibility::Public || class.method("__call").is_some() {
                    return None;
                }
                return Some(PhpString::from(format!(
                    "cannot access {} method {}::{}()",
                    info.visibility.as_str(),
                    class.display_name(),
                    info.name
                )));
            }
            // zend_closure_get_method() answers `__invoke`; `__call()` catches every other name
            if (object.downcast_ref::<Closure>().is_some() && name.eq_ignore_ascii_case("__invoke"))
                || class.method("__call").is_some()
            {
                return None;
            }
            Some(PhpString::concat(&[
                b"class ",
                class.display_name().as_bytes(),
                b" does not have a method \"",
                method.c_str(),
                b"\"",
            ]))
        }
        Value::String(class) => Some(PhpString::concat(&[
            b"class \"",
            class.c_str(),
            b"\" not found",
        ])),
        _ => None,
    }
}

/// The `Z_PARAM_FUNC` (`callable`) or `Z_PARAM_FUNC_OR_NULL` (`?callable`) check of an internal function's
/// parameter: `TypeError: <function>(): Argument #<n> ($<name>) must be a valid callback[ or null], <reason>`.
pub(super) fn check_callable(
    value: &Value,
    function: &str,
    position: u32,
    name: &str,
    nullable: bool,
) -> PhpResult<()> {
    if nullable && value.is_null() {
        return Ok(());
    }
    let Some(reason) = not_callable_reason(value) else {
        return Ok(());
    };
    Err(PhpError::type_error(PhpString::concat(&[
        format!(
            "{function}(): Argument #{position} (${name}) must be a valid callback{}, ",
            if nullable { " or null" } else { "" }
        )
        .as_bytes(),
        &reason,
    ])))
}

/// Calls a callable that passed [`check_callable`] with positional arguments (`zend_call_function()`).
pub(super) fn call_value(
    callable: &Value,
    args: Vec<Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match callable {
        Value::Object(object) => object.invoke(CallArgs::from(args), diag),
        Value::Array(array) => match (array.get_int(0), array.get_int(1)) {
            (Some(Value::Object(object)), Some(Value::String(method))) => {
                object.call_method(&method.to_string_lossy(), CallArgs::from(args), diag)
            }
            _ => Err(PhpError::error(
                "Array callback must have exactly two elements",
            )),
        },
        Value::String(name) => Err(PhpError::error(PhpString::concat(&[
            b"Call to undefined function ",
            name.as_bytes(),
            b"()",
        ]))),
        other => Err(PhpError::error(format!(
            "Value of type {} is not callable",
            twig_value::types::zend_zval_value_name(other)
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(value: Value) -> Option<String> {
        not_callable_reason(&value).map(|r| r.to_string_lossy().into_owned())
    }

    #[test]
    fn reasons_follow_zend_is_callable() {
        assert_eq!(
            reason(Value::from("nope")).as_deref(),
            Some("function \"nope\" not found or invalid function name")
        );
        assert_eq!(
            reason(Value::from("A::b")).as_deref(),
            Some("class \"A\" not found")
        );
        assert_eq!(
            reason(Value::from("::b")).as_deref(),
            Some("invalid function name")
        );
        assert_eq!(
            reason(Value::Null).as_deref(),
            Some("no array or string given")
        );
        assert_eq!(
            reason(Value::Array(Array::from_values([Value::Int(1)]))).as_deref(),
            Some("array callback must have exactly two members")
        );
        assert_eq!(
            reason(Value::Array(Array::from_values([
                Value::Int(1),
                Value::from("m")
            ])))
            .as_deref(),
            Some("first array member is not a valid class name or object")
        );
        let closure = Value::Object(Closure::new(|_, _| Ok(Value::Null)).into_ref());
        assert_eq!(reason(closure.clone()), None);
        assert_eq!(
            reason(Value::Array(Array::from_values([
                closure,
                Value::from("__invoke")
            ]))),
            None
        );
        let error = check_callable(&Value::Int(1), "uasort", 2, "callback", false).unwrap_err();
        assert_eq!(
            error.message().to_string_lossy(),
            "uasort(): Argument #2 ($callback) must be a valid callback, no array or string given"
        );
        assert!(check_callable(&Value::Null, "array_filter", 2, "callback", true).is_ok());
    }
}
