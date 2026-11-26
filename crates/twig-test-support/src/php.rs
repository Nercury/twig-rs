//! Small pieces of the PHP engine the class ports need and `twig-value` does not expose.

use twig_value::numeric;
use twig_value::object::{CallArgs, FunctionKind, Param};
use twig_value::param::{self, DeclaredType, TypeMask};
use twig_value::{Array, Diagnostics, Number, PhpError, PhpResult, PhpString, Value, types};

/// Binds the arguments of a userland method (`FunctionKind::User`: extra positional arguments are
/// ignored, unknown named arguments throw).
pub(crate) fn bind_user(
    args: &CallArgs,
    function: &str,
    params: &[Param],
) -> PhpResult<Vec<Value>> {
    args.bind(function, params, FunctionKind::User)
}

/// The declared return type of a ported userland method: `zend_verify_return_type()` in coercive
/// mode (PHPSRC/Zend/zend_execute.c), see [`param::verify_user_return`]. A coercion that throws
/// (`ToStringStub::__toString()` returning an object whose `__toString()` throws) gives
/// `TypeError: F(): Return value must be of type T, V returned` with that exception as previous.
pub(crate) fn verify_return(
    value: Value,
    ty: &DeclaredType<'_>,
    function: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    param::verify_user_return(value, ty, function, diag)
}

/// `string` as a declared type.
pub(crate) const STRING: DeclaredType<'static> = DeclaredType::builtin(TypeMask::STRING);
/// `int` as a declared type.
pub(crate) const INT: DeclaredType<'static> = DeclaredType::builtin(TypeMask::INT);
/// `array` as a declared type.
pub(crate) const ARRAY: DeclaredType<'static> = DeclaredType::builtin(TypeMask::ARRAY);

/// Port of `increment_function()` (PHPSRC/Zend/zend_operators.c): `++$v` on a plain (untyped)
/// variable or property.
pub(crate) fn increment(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    match v {
        // fast_long_increment_function(): overflow switches to double.
        Value::Int(i) => Ok(match i.checked_add(1) {
            Some(n) => Value::Int(n),
            None => Value::Float(*i as f64 + 1.0),
        }),
        Value::Float(d) => Ok(Value::Float(d + 1.0)),
        Value::Null => Ok(Value::Int(1)),
        Value::String(s) => match numeric::is_numeric_str_function(s) {
            Some(Number::Int(l)) => Ok(if l == i64::MAX {
                Value::Float(l as f64 + 1.0)
            } else {
                Value::Int(l + 1)
            }),
            Some(Number::Float(d)) => Ok(Value::Float(d + 1.0)),
            None => increment_string(s, diag).map(Value::String),
        },
        Value::Bool(_) => {
            diag.warn(
                "Increment on type bool has no effect, this will change in the next major version of PHP",
            )?;
            Ok(v.clone())
        }
        Value::Object(o) => {
            // No do_operation handler on the ported classes; cast_object(_IS_NUMBER) next.
            if let Some(n) = o.cast(twig_value::object::CastTarget::Number, diag)?
                && matches!(n, Value::Int(_) | Value::Float(_))
            {
                return increment(&n, diag);
            }
            Err(cannot_increment(v))
        }
        Value::Array(_) => Err(cannot_increment(v)),
    }
}

fn cannot_increment(v: &Value) -> PhpError {
    PhpError::type_error(format!(
        "Cannot increment {}",
        types::zend_zval_value_name(v)
    ))
}

/// Port of `increment_string()` (PHPSRC/Zend/zend_operators.c): Perl-style string increment, with
/// the PHP 8.3+ deprecation.
fn increment_string(s: &PhpString, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    diag.deprecated("Increment on non-numeric string is deprecated, use str_increment() instead")?;
    if s.is_empty() {
        return Ok(PhpString::from("1"));
    }
    #[derive(Clone, Copy)]
    enum Last {
        Numeric,
        Upper,
        Lower,
    }
    let mut bytes = s.as_bytes().to_vec();
    let mut pos = bytes.len() - 1;
    let mut carry;
    let mut last = Last::Numeric;
    loop {
        let ch = bytes[pos];
        match ch {
            b'a'..=b'z' => {
                carry = ch == b'z';
                bytes[pos] = if carry { b'a' } else { ch + 1 };
                last = Last::Lower;
            }
            b'A'..=b'Z' => {
                carry = ch == b'Z';
                bytes[pos] = if carry { b'A' } else { ch + 1 };
                last = Last::Upper;
            }
            b'0'..=b'9' => {
                carry = ch == b'9';
                bytes[pos] = if carry { b'0' } else { ch + 1 };
                last = Last::Numeric;
            }
            _ => {
                carry = false;
                break;
            }
        }
        if !carry || pos == 0 {
            break;
        }
        pos -= 1;
    }
    if carry {
        let first = match last {
            Last::Numeric => b'1',
            Last::Upper => b'A',
            Last::Lower => b'a',
        };
        bytes.insert(0, first);
    }
    Ok(PhpString::from(bytes))
}

/// `++$this->prop` on a property declared `int` (`zend_incdec_typed_prop()`): an `int` at
/// `PHP_INT_MAX` cannot be incremented.
pub(crate) fn increment_typed_int(
    v: &Value,
    class: &str,
    prop: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    if let Value::Int(i64::MAX) = v {
        return Err(PhpError::type_error(format!(
            "Cannot increment property {class}::${prop} of type int past its maximal value"
        )));
    }
    increment(v, diag)
}

/// An array with an internal pointer (`nInternalPointer`), for `current()`, `key()`, `next()`,
/// `reset()` on a property array. The pointer is a position in iteration order; `len` (or more)
/// is past the end.
#[derive(Clone, Debug, Default)]
pub(crate) struct ArrayPointer {
    pub(crate) pos: usize,
}

impl ArrayPointer {
    /// `current($array)`: the element at the pointer, `false` past the end.
    pub(crate) fn current(&self, a: &Array) -> Value {
        a.get_index(self.pos)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Bool(false))
    }

    /// `key($array)`: the key at the pointer, `null` past the end.
    pub(crate) fn key(&self, a: &Array) -> Value {
        a.get_index(self.pos)
            .map(|(k, _)| k.to_value())
            .unwrap_or(Value::Null)
    }

    /// `next($array)` (`zend_hash_move_forward()`; the return value is not needed by the ports).
    pub(crate) fn next(&mut self, a: &Array) {
        if self.pos < a.len() {
            self.pos += 1;
        }
    }

    /// `reset($array)`.
    pub(crate) fn reset(&mut self) {
        self.pos = 0;
    }
}

/// Port of the ASCII-compatible-locale branch of `php_basename()` (PHPSRC/ext/standard/string.c;
/// the oracle runs with `LC_CTYPE=C.UTF-8`, which PHP treats as ASCII compatible), non-Windows.
pub(crate) fn php_basename(s: &[u8], suffix: Option<&[u8]>) -> PhpString {
    let mut end = s.len();
    while end > 0 && s[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        return PhpString::empty();
    }
    let mut start = end - 1;
    while start > 0 && s[start - 1] != b'/' {
        start -= 1;
    }
    if let Some(suffix) = suffix
        && suffix.len() < end - start
        && &s[end - suffix.len()..end] == suffix
    {
        end -= suffix.len();
    }
    PhpString::from(&s[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use twig_value::Collect;

    #[test]
    fn increments() {
        let mut d = Collect::new();
        assert_eq!(increment(&Value::Int(1), &mut d).unwrap(), Value::Int(2));
        assert_eq!(
            increment(&Value::Int(i64::MAX), &mut d).unwrap(),
            Value::Float(i64::MAX as f64 + 1.0)
        );
        assert_eq!(increment(&Value::Null, &mut d).unwrap(), Value::Int(1));
        assert_eq!(
            increment(&Value::from("9"), &mut d).unwrap(),
            Value::Int(10)
        );
        assert_eq!(
            increment(&Value::from("1.5"), &mut d).unwrap(),
            Value::Float(2.5)
        );
        assert!(d.take().is_empty());
        assert_eq!(
            increment(&Value::from("Az"), &mut d).unwrap(),
            Value::from("Ba")
        );
        assert_eq!(
            increment(&Value::from("zz"), &mut d).unwrap(),
            Value::from("aaa")
        );
        assert_eq!(
            increment(&Value::from("a9"), &mut d).unwrap(),
            Value::from("b0")
        );
        assert_eq!(
            increment(&Value::from("Zz"), &mut d).unwrap(),
            Value::from("AAa")
        );
        assert_eq!(
            increment(&Value::from("a-"), &mut d).unwrap(),
            Value::from("a-")
        );
        assert_eq!(
            increment(&Value::from(""), &mut d).unwrap(),
            Value::from("1")
        );
        assert_eq!(d.take().len(), 6);
        assert_eq!(
            increment(&Value::Array(Array::new()), &mut d)
                .unwrap_err()
                .message(),
            "Cannot increment array"
        );
    }

    #[test]
    fn basename() {
        assert_eq!(php_basename(b"/oracle", None), "oracle");
        assert_eq!(php_basename(b"/a/b.txt/", None), "b.txt");
        assert_eq!(php_basename(b"b.txt", Some(b".txt")), "b");
        assert_eq!(php_basename(b".txt", Some(b".txt")), ".txt");
        assert_eq!(php_basename(b"///", None), "");
    }
}
