//! Assignments to the typed properties of the Twig classes that twig-rs implements as host objects (`Twig\Markup`,
//! `Twig\Source`). The value model leaves typed properties to each class (reference/deviations.md, "Property
//! model"), so their own code checks what it assigns.

use twig_value::param::{DeclaredType, verify_weak_scalar_type_hint};
use twig_value::{Diagnostics, PhpError, PhpResult, PhpString, Value, types};

/// Port of `zend_verify_property_type()` (PHPSRC/Zend/zend_execute.c) for `$this->name = $value` in the code of
/// `class`, in coercive typing mode (Twig's sources do not declare `strict_types`): returns the value to store.
///
/// The value is kept when its type is declared (`null` only for a nullable type) or it is an instance of a declared
/// class; otherwise the weak scalar coercion of `zend_verify_weak_scalar_type_hint()` applies (an int, float or bool
/// becomes a string for a `string` property, a `Stringable` object its `__toString()`, with the diagnostics of the
/// conversion).
///
/// # Errors
///
/// `TypeError` `Cannot assign array to property Twig\Markup::$charset of type ?string`
/// (`zend_verify_property_type_error()`); an exception the coercion raised (a throwing `__toString()`, a diagnostic
/// the sink turns into an exception) instead of it, as `zend_verify_property_type_error()` does nothing while an
/// exception is pending.
pub(crate) fn verify_property_type(
    class: &str,
    name: &str,
    ty: &DeclaredType<'_>,
    value: Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    if ty.mask.accepts_type_of(&value) {
        return Ok(value);
    }
    if let Value::Object(object) = &value
        && ty.classes.iter().any(|c| object.instance_of(c))
    {
        return Ok(value);
    }
    match verify_weak_scalar_type_hint(ty.mask, &value, diag)? {
        Some(coerced) => Ok(coerced),
        None => Err(PhpError::type_error(PhpString::concat(&[
            b"Cannot assign ",
            types::zend_zval_value_name(&value).as_bytes(),
            b" to property ",
            class.as_bytes(),
            b"::$",
            name.as_bytes(),
            b" of type ",
            ty.to_type_string().as_bytes(),
        ]))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use twig_value::param::TypeMask;
    use twig_value::{Array, Ignore};

    #[test]
    fn coerces_or_rejects_like_a_typed_property() {
        let ty = DeclaredType::builtin(TypeMask::STRING | TypeMask::NULL);
        let assign =
            |value: Value| verify_property_type("Twig\\Markup", "charset", &ty, value, &mut Ignore);
        assert_eq!(assign(Value::Null).unwrap(), Value::Null);
        assert_eq!(assign(Value::Int(5)).unwrap(), Value::from("5"));
        assert_eq!(assign(Value::Float(1.5)).unwrap(), Value::from("1.5"));
        assert_eq!(assign(Value::Bool(false)).unwrap(), Value::from(""));
        let error = assign(Value::Array(Array::new())).unwrap_err();
        assert_eq!(
            error.message(),
            "Cannot assign array to property Twig\\Markup::$charset of type ?string"
        );
        let closure = DeclaredType::new(&["Closure"], TypeMask::STRING);
        let error = verify_property_type(
            "Twig\\Source",
            "path",
            &closure,
            Value::Array(Array::new()),
            &mut Ignore,
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Cannot assign array to property Twig\\Source::$path of type Closure|string"
        );
    }
}
