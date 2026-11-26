//! Coercive-mode ("weak") parameter passing: internal functions (`parse_*`) and userland functions
//! with declared types ([`verify_user_arg`], [`verify_user_return`]).
//!
//! reference/php/engine-semantics.md §11 and reference/php/stdlib.md §4.1. Neither Twig nor its
//! compiled templates declare `strict_types`, so every builtin Twig calls coerces its arguments with
//! these rules (stricter than casts: strings must be fully numeric, out-of-range floats are
//! rejected).
//!
//! * Internal functions: ports of `zend_parse_arg_*_weak()` (PHPSRC/Zend/zend_API.c) with the fast
//!   paths of the `Z_PARAM_*` macros, the `null` deprecation (`Passing null to parameter #%u ($%s)
//!   of type %s is deprecated`) and the verbatim `TypeError` texts. Describe the parameter with an
//!   [`Arg`] (function name, position, name, arginfo type).
//! * Userland functions (Twig's own PHP methods such as `CoreExtension::matches(string $regexp,
//!   ?string $str)` receive template values): [`verify_user_arg`] ports `zend_check_type()` and
//!   `zend_verify_weak_scalar_type_hint()` (PHPSRC/Zend/zend_execute.c): exact type matches pass,
//!   objects match their class types, `null` passes only nullable types (no `null` coercion for
//!   userland functions), scalars are coerced in the order int → float → string → bool.
//!   [`verify_user_return`] applies the same rules to a declared return type.

use crate::array::Array;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::numeric;
use crate::object::CastTarget;
use crate::string::PhpString;
use crate::types;
use crate::value::{Number, TypeTag, Value};

/// A parameter of an internal function, for messages.
#[derive(Clone, Copy, Debug)]
pub struct Arg<'a> {
    /// Function name as PHP prints it (`strlen`, `ArrayObject::setFlags`).
    pub function: &'a str,
    /// 1-based position.
    pub position: u32,
    /// Parameter name without `$`.
    pub name: &'a str,
    /// The parameter's declared type from the stub (arginfo), used by the null deprecation
    /// (`implode(): Passing null to parameter #1 ($separator) of type array|string is
    /// deprecated`). `None` falls back to the parser's own type name, like upstream does for
    /// arguments without arginfo type.
    pub declared_type: Option<&'a str>,
}

impl<'a> Arg<'a> {
    /// A parameter description.
    pub fn new(function: &'a str, position: u32, name: &'a str) -> Arg<'a> {
        Arg {
            function,
            position,
            name,
            declared_type: None,
        }
    }

    /// Sets the declared (arginfo) type used in the null deprecation.
    pub fn declared(mut self, declared_type: &'a str) -> Arg<'a> {
        self.declared_type = Some(declared_type);
        self
    }

    /// Port of `zend_argument_type_error()` formatting:
    /// `%s(): Argument #%d ($%s) must be %s, %s given`, with `expected` like `of type int` and the
    /// value named by `zend_zval_value_name()`.
    pub fn type_error(&self, expected: &str, value: &Value) -> PhpError {
        PhpError::type_error(PhpString::concat(&[
            format!(
                "{}(): Argument #{} (${}) must be {}, ",
                self.function, self.position, self.name, expected
            )
            .as_bytes(),
            types::zend_zval_value_name(value).as_bytes(),
            b" given",
        ]))
    }

    /// Port of `zend_argument_value_error()` formatting: `%s(): Argument #%d ($%s) %s`.
    pub fn value_error(&self, message: &str) -> PhpError {
        PhpError::value_error(format!(
            "{}(): Argument #{} (${}) {}",
            self.function, self.position, self.name, message
        ))
    }

    /// Port of `zend_null_arg_deprecated()`:
    /// `%s(): Passing null to parameter #%u ($%s) of type %s is deprecated`.
    pub fn null_deprecated(
        &self,
        fallback_type: &str,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        diag.deprecated(format!(
            "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
            self.function,
            self.position,
            self.name,
            self.declared_type.unwrap_or(fallback_type)
        ))
    }
}

/// Port of `Z_PARAM_BOOL` / `zend_parse_arg_bool_weak()`: scalars by truthiness (NaN warns), `null`
/// with the deprecation; arrays and objects are rejected (`must be of type bool`).
pub fn parse_bool(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Null => {
            arg.null_deprecated("bool", diag)?;
            Ok(false)
        }
        Value::Int(_) | Value::Float(_) | Value::String(_) => convert::to_bool(v, diag),
        _ => Err(arg.type_error("of type bool", v)),
    }
}

/// `zend_parse_arg_long_weak()`; `None` = rejected.
fn long_weak(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<Option<i64>> {
    Ok(match v {
        Value::Int(i) => Some(*i),
        Value::Float(d) => {
            if d.is_nan() || !convert::double_fits_long(*d) {
                None
            } else {
                let l = *d as i64;
                if !convert::is_long_compatible(*d, l) {
                    convert::incompatible_double_to_long(*d, diag)?;
                }
                Some(l)
            }
        }
        Value::String(s) => match numeric::is_numeric_str_function(s) {
            Some(Number::Int(i)) => Some(i),
            Some(Number::Float(d)) => {
                if d.is_nan() || !convert::double_fits_long(d) {
                    None
                } else {
                    let l = d as i64;
                    if !convert::is_long_compatible(d, l) {
                        convert::incompatible_string_to_long(s, diag)?;
                    }
                    Some(l)
                }
            }
            None => None,
        },
        Value::Null => {
            arg.null_deprecated("int", diag)?;
            Some(0)
        }
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    })
}

/// Port of `Z_PARAM_LONG` / `zend_parse_arg_long_weak()`: ints; bools; `null` (deprecated) → 0;
/// floats that are not NaN and fit (fractional ones truncated with `Implicit conversion from float
/// %s to int loses precision`); fully numeric strings (float strings under the same rules with the
/// `float-string` deprecation). Everything else: `must be of type int, X given`.
pub fn parse_long(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    long_weak(v, arg, diag)?.ok_or_else(|| arg.type_error("of type int", v))
}

/// `Z_PARAM_LONG_OR_NULL`: `null` is accepted silently.
pub fn parse_long_or_null(
    v: &Value,
    arg: &Arg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<i64>> {
    if v.is_null() {
        return Ok(None);
    }
    long_weak(v, arg, diag)?
        .map(Some)
        .ok_or_else(|| arg.type_error("of type ?int", v))
}

/// `zend_parse_arg_double_weak()`; `None` = rejected.
fn double_weak(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<Option<f64>> {
    Ok(match v {
        Value::Float(d) => Some(*d),
        Value::Int(i) => Some(*i as f64),
        Value::String(s) => numeric::is_numeric_str_function(s).map(Number::to_f64),
        Value::Null => {
            arg.null_deprecated("float", diag)?;
            Some(0.0)
        }
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    })
}

/// Port of `Z_PARAM_DOUBLE` / `zend_parse_arg_double_weak()`.
pub fn parse_double(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<f64> {
    double_weak(v, arg, diag)?.ok_or_else(|| arg.type_error("of type float", v))
}

/// `Z_PARAM_DOUBLE_OR_NULL`.
pub fn parse_double_or_null(
    v: &Value,
    arg: &Arg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<f64>> {
    if v.is_null() {
        return Ok(None);
    }
    double_weak(v, arg, diag)?
        .map(Some)
        .ok_or_else(|| arg.type_error("of type ?float", v))
}

/// `zend_parse_arg_number_slow()`; `None` = rejected.
fn number_weak(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<Option<Number>> {
    Ok(match v {
        Value::Int(i) => Some(Number::Int(*i)),
        Value::Float(d) => Some(Number::Float(*d)),
        Value::String(s) => numeric::is_numeric_str_function(s),
        Value::Null => {
            arg.null_deprecated("int|float", diag)?;
            Some(Number::Int(0))
        }
        Value::Bool(b) => Some(Number::Int(i64::from(*b))),
        _ => None,
    })
}

/// Port of `Z_PARAM_NUMBER` (`int|float`) / `zend_parse_arg_number_slow()`.
pub fn parse_number(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<Number> {
    number_weak(v, arg, diag)?.ok_or_else(|| arg.type_error("of type int|float", v))
}

/// `Z_PARAM_NUMBER_OR_NULL`.
pub fn parse_number_or_null(
    v: &Value,
    arg: &Arg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<Number>> {
    if v.is_null() {
        return Ok(None);
    }
    number_weak(v, arg, diag)?
        .map(Some)
        .ok_or_else(|| arg.type_error("of type int|float|null", v))
}

/// `zend_parse_arg_str_weak()`; `None` = rejected.
fn str_weak(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<Option<PhpString>> {
    Ok(match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => {
            arg.null_deprecated("string", diag)?;
            Some(PhpString::empty())
        }
        Value::Bool(_) | Value::Int(_) | Value::Float(_) => Some(convert::to_string(v, diag)?),
        Value::Object(o) => match o.cast(CastTarget::String, diag)? {
            Some(Value::String(s)) => Some(s),
            Some(other) => Some(convert::to_string(&other, diag)?),
            None => None,
        },
        Value::Array(_) => None,
    })
}

/// Port of `Z_PARAM_STR` / `zend_parse_arg_str_weak()`: scalars converted with PHP string
/// conversion (floats with precision 14), `null` deprecated → `""`, objects via `__toString()`;
/// arrays and other objects: `must be of type string, X given`.
pub fn parse_str(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    str_weak(v, arg, diag)?.ok_or_else(|| arg.type_error("of type string", v))
}

/// `Z_PARAM_STR_OR_NULL`.
pub fn parse_str_or_null(
    v: &Value,
    arg: &Arg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<PhpString>> {
    if v.is_null() {
        return Ok(None);
    }
    str_weak(v, arg, diag)?
        .map(Some)
        .ok_or_else(|| arg.type_error("of type ?string", v))
}

/// Result of [`parse_str_or_long`].
#[derive(Clone, Debug, PartialEq)]
pub enum StrOrLong {
    /// The argument was (or coerced to) an int.
    Long(i64),
    /// The argument was (or coerced to) a string.
    Str(PhpString),
}

/// Port of `Z_PARAM_STR_OR_LONG` / `zend_parse_arg_str_or_long_slow()`: strings and ints as is;
/// otherwise the weak int rules first, then the weak string rules.
pub fn parse_str_or_long(
    v: &Value,
    arg: &Arg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<StrOrLong> {
    match v {
        Value::String(s) => Ok(StrOrLong::Str(s.clone())),
        Value::Int(i) => Ok(StrOrLong::Long(*i)),
        _ => {
            if let Some(l) = long_weak(v, arg, diag)? {
                return Ok(StrOrLong::Long(l));
            }
            match str_weak(v, arg, diag)? {
                Some(s) => Ok(StrOrLong::Str(s)),
                None => Err(arg.type_error("of type string|int", v)),
            }
        }
    }
}

/// Port of `Z_PARAM_NUMBER_OR_STR` (`string|int|float`) / `zend_parse_arg_number_or_str_slow()`:
/// ints, floats and strings as is; `null` (deprecated) and bools → int; objects with
/// `__toString()` → string.
pub fn parse_number_or_str(
    v: &Value,
    arg: &Arg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match v {
        Value::Int(_) | Value::Float(_) | Value::String(_) => Ok(v.clone()),
        Value::Null => {
            arg.null_deprecated("string|int|float", diag)?;
            Ok(Value::Int(0))
        }
        Value::Bool(b) => Ok(Value::Int(i64::from(*b))),
        Value::Object(o) => match o.cast(CastTarget::String, diag)? {
            Some(s @ Value::String(_)) => Ok(s),
            Some(other) => Ok(Value::String(convert::to_string(&other, diag)?)),
            None => Err(arg.type_error("of type string|int|float", v)),
        },
        Value::Array(_) => Err(arg.type_error("of type string|int|float", v)),
    }
}

/// `Z_PARAM_ARRAY`: arrays only (`must be of type array, X given`).
pub fn parse_array(v: &Value, arg: &Arg<'_>) -> PhpResult<Array> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        _ => Err(arg.type_error("of type array", v)),
    }
}

/// `Z_PARAM_ARRAY_OR_NULL`.
pub fn parse_array_or_null(v: &Value, arg: &Arg<'_>) -> PhpResult<Option<Array>> {
    match v {
        Value::Null => Ok(None),
        Value::Array(a) => Ok(Some(a.clone())),
        _ => Err(arg.type_error("of type ?array", v)),
    }
}

/// The built-in part of a declared type: the `MAY_BE_*` bits of a `zend_type` (PHPSRC/Zend/zend_types.h).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TypeMask(u16);

impl TypeMask {
    /// No built-in type (a pure class type).
    pub const NONE: TypeMask = TypeMask(0);
    /// `null` (`MAY_BE_NULL`; `?T` or `T|null`).
    pub const NULL: TypeMask = TypeMask(1 << 1);
    /// `false`.
    pub const FALSE: TypeMask = TypeMask(1 << 2);
    /// `true`.
    pub const TRUE: TypeMask = TypeMask(1 << 3);
    /// `bool` (`false|true`).
    pub const BOOL: TypeMask = TypeMask((1 << 2) | (1 << 3));
    /// `int`.
    pub const INT: TypeMask = TypeMask(1 << 4);
    /// `float`.
    pub const FLOAT: TypeMask = TypeMask(1 << 5);
    /// `string`.
    pub const STRING: TypeMask = TypeMask(1 << 6);
    /// `array`.
    pub const ARRAY: TypeMask = TypeMask(1 << 7);
    /// `object`.
    pub const OBJECT: TypeMask = TypeMask(1 << 8);
    /// `mixed` (`MAY_BE_ANY`: every value passes).
    pub const MIXED: TypeMask = TypeMask(0x1FE);

    /// Whether every bit of `other` is set.
    pub fn contains(self, other: TypeMask) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any bit of `other` is set.
    pub fn intersects(self, other: TypeMask) -> bool {
        self.0 & other.0 != 0
    }

    /// `ZEND_TYPE_CONTAINS_CODE(type, Z_TYPE_P(value))`: the value's own type is declared.
    pub fn accepts_type_of(self, v: &Value) -> bool {
        let bit = match v {
            Value::Null => TypeMask::NULL,
            Value::Bool(false) => TypeMask::FALSE,
            Value::Bool(true) => TypeMask::TRUE,
            Value::Int(_) => TypeMask::INT,
            Value::Float(_) => TypeMask::FLOAT,
            Value::String(_) => TypeMask::STRING,
            Value::Array(_) => TypeMask::ARRAY,
            Value::Object(_) => TypeMask::OBJECT,
        };
        self.intersects(bit)
    }
}

impl std::ops::BitOr for TypeMask {
    type Output = TypeMask;

    fn bitor(self, rhs: TypeMask) -> TypeMask {
        TypeMask(self.0 | rhs.0)
    }
}

/// A declared parameter type of a userland function (`zend_type`): class/interface names plus
/// built-in types. `iterable` is `Traversable|array` since PHP 8.2 ([`DeclaredType::iterable`]).
/// `callable`, `static`, intersection types and `void`/`never` are not modelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeclaredType<'a> {
    /// Class and interface names in declaration order, matched with `instanceof`.
    pub classes: &'a [&'a str],
    /// Built-in types.
    pub mask: TypeMask,
}

impl<'a> DeclaredType<'a> {
    /// A type made of built-in types only (`int`, `?string`, `int|float`, `mixed`, ...).
    pub const fn builtin(mask: TypeMask) -> DeclaredType<'static> {
        DeclaredType { classes: &[], mask }
    }

    /// Class types plus built-in types (`Countable|array`, `?\DateTimeInterface`).
    pub const fn new(classes: &'a [&'a str], mask: TypeMask) -> DeclaredType<'a> {
        DeclaredType { classes, mask }
    }

    /// `iterable` (`Traversable|array`).
    pub const fn iterable() -> DeclaredType<'static> {
        DeclaredType {
            classes: &["Traversable"],
            mask: TypeMask::ARRAY,
        }
    }

    /// Port of `zend_type_to_string()` (PHPSRC/Zend/zend_compile.c): the type as PHP prints it in
    /// messages: class names first, then `object`, `array`, `string`, `int`, `float`, `bool` (or
    /// `false`/`true`); a single type plus `null` prints as `?T`, a union appends `|null`; `mixed`
    /// for [`TypeMask::MIXED`].
    pub fn to_type_string(&self) -> String {
        let mut parts: Vec<&str> = self
            .classes
            .iter()
            .map(|c| c.strip_prefix('\\').unwrap_or(c))
            .collect();
        let m = self.mask;
        if m == TypeMask::MIXED {
            parts.push("mixed");
            return parts.join("|");
        }
        for (bit, name) in [
            (TypeMask::OBJECT, "object"),
            (TypeMask::ARRAY, "array"),
            (TypeMask::STRING, "string"),
            (TypeMask::INT, "int"),
            (TypeMask::FLOAT, "float"),
        ] {
            if m.intersects(bit) {
                parts.push(name);
            }
        }
        if m.contains(TypeMask::BOOL) {
            parts.push("bool");
        } else if m.intersects(TypeMask::FALSE) {
            parts.push("false");
        } else if m.intersects(TypeMask::TRUE) {
            parts.push("true");
        }
        if m.intersects(TypeMask::NULL) {
            if parts.len() == 1 {
                return format!("?{}", parts[0]);
            }
            parts.push("null");
        }
        parts.join("|")
    }
}

/// A parameter of a userland function, for messages.
#[derive(Clone, Copy, Debug)]
pub struct UserArg<'a> {
    /// Function name as PHP prints it (`twig_fn`, `Twig\Extension\CoreExtension::matches`).
    pub function: &'a str,
    /// 1-based position.
    pub position: u32,
    /// Parameter name without `$`.
    pub name: &'a str,
    /// The calling userland file and line: PHP appends `, called in %s on line %d` to the type error
    /// when the caller is userland code (a compiled template is). `None` omits the suffix.
    pub called_in: Option<(&'a str, u32)>,
}

impl<'a> UserArg<'a> {
    /// A parameter description without the `called in` suffix.
    pub fn new(function: &'a str, position: u32, name: &'a str) -> UserArg<'a> {
        UserArg {
            function,
            position,
            name,
            called_in: None,
        }
    }

    /// Port of `zend_verify_arg_error()` (PHPSRC/Zend/zend_execute.c):
    /// `%s(): Argument #%d ($%s) must be of type %s, %s given[, called in %s on line %d]`.
    pub fn type_error(&self, ty: &DeclaredType<'_>, value: &Value) -> PhpError {
        let mut msg = format!(
            "{}(): Argument #{} (${}) must be of type {}, ",
            self.function,
            self.position,
            self.name,
            ty.to_type_string()
        )
        .into_bytes();
        msg.extend_from_slice(types::zend_zval_value_name(value).as_bytes());
        msg.extend_from_slice(b" given");
        if let Some((file, line)) = self.called_in {
            msg.extend_from_slice(format!(", called in {file} on line {line}").as_bytes());
        }
        PhpError::type_error(msg)
    }
}

/// The internal parsers below only consult their [`Arg`] for the `null` deprecation, which the
/// userland path never reaches (`null` is rejected first).
const NO_ARG: Arg<'static> = Arg {
    function: "",
    position: 0,
    name: "",
    declared_type: None,
};

/// Port of `zend_verify_weak_scalar_type_hint()` (PHPSRC/Zend/zend_execute.c): coerces a non-null
/// value to the first of int, float, string, bool that `mask` declares and accepts it. For an
/// `int|float` type a string becomes whichever `is_numeric_string()` returns. `Ok(None)` when no
/// declared scalar type accepts the value.
pub fn verify_weak_scalar_type_hint(
    mask: TypeMask,
    v: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<Value>> {
    weak_scalar(mask, v, diag).map_err(|failure| failure.error)
}

/// An exception raised while [`weak_scalar`] coerced a value, with the value PHP holds after it:
/// `zend_parse_arg_str_weak()` (PHPSRC/Zend/zend_API.c) converts a scalar to a string in place
/// (`convert_to_string()`) before it checks for an exception, so the value is a string from then on.
struct WeakFailure {
    error: PhpError,
    converted: Option<Value>,
}

/// [`verify_weak_scalar_type_hint`], keeping what a failed coercion did to the value.
fn weak_scalar(
    mask: TypeMask,
    v: &Value,
    diag: &mut dyn Diagnostics,
) -> Result<Option<Value>, WeakFailure> {
    let failed = |error: PhpError| WeakFailure {
        error,
        converted: None,
    };
    if v.is_null() {
        return Ok(None);
    }
    if mask.intersects(TypeMask::INT) {
        match v {
            // int|float and a string: is_numeric_string() picks the type.
            Value::String(s) if mask.intersects(TypeMask::FLOAT) => {
                if let Some(n) = numeric::is_numeric_str_function(s) {
                    return Ok(Some(Value::from(n)));
                }
            }
            _ => {
                if let Some(l) = long_weak(v, &NO_ARG, diag).map_err(failed)? {
                    return Ok(Some(Value::Int(l)));
                }
            }
        }
    }
    if mask.intersects(TypeMask::FLOAT)
        && let Some(d) = double_weak(v, &NO_ARG, diag).map_err(failed)?
    {
        return Ok(Some(Value::Float(d)));
    }
    if mask.intersects(TypeMask::STRING) {
        if let Value::Bool(_) | Value::Int(_) | Value::Float(_) = v {
            // Converting `NAN` warns (`unexpected NAN value was coerced to string`); a sink that
            // throws leaves the string in place.
            return match convert::to_string(v, diag) {
                Ok(s) => Ok(Some(Value::String(s))),
                Err(error) => Err(WeakFailure {
                    error,
                    converted: Some(Value::String(
                        convert::to_string(v, &mut crate::diag::Ignore).unwrap_or_default(),
                    )),
                }),
            };
        }
        if let Some(s) = str_weak(v, &NO_ARG, diag).map_err(failed)? {
            return Ok(Some(Value::String(s)));
        }
    }
    if mask.contains(TypeMask::BOOL)
        && matches!(
            v,
            Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::String(_)
        )
    {
        return Ok(Some(Value::Bool(
            convert::to_bool(v, diag).map_err(failed)?,
        )));
    }
    Ok(None)
}

/// Port of `zend_check_type()` / `zend_check_type_slow()` + `zend_verify_scalar_type_hint()` for a
/// userland function parameter in coercive mode (PHPSRC/Zend/zend_execute.c), with
/// `zend_verify_arg_error()`'s message.
///
/// Returns the (possibly coerced) argument: the value as is when its type is declared or it is an
/// instance of a declared class; `null` only for nullable types; otherwise the weak scalar
/// coercion of [`verify_weak_scalar_type_hint`] (with its deprecations, e.g. `Implicit conversion
/// from float 1.5 to int loses precision`). Failure: `TypeError` `f(): Argument #1 ($x) must be of
/// type int, array given`. An exception raised by the coercion (a throwing `__toString()`, a
/// diagnostic the sink turns into an exception) is returned as it is: `zend_verify_arg_error()`
/// reports through `zend_argument_error_variadic()` (PHPSRC/Zend/zend_API.c), which does nothing
/// while an exception is pending.
pub fn verify_user_arg(
    value: Value,
    ty: &DeclaredType<'_>,
    arg: &UserArg<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    if ty.mask.accepts_type_of(&value) {
        return Ok(value);
    }
    if let Value::Object(o) = &value
        && ty.classes.iter().any(|c| o.instance_of(c))
    {
        return Ok(value);
    }
    match verify_weak_scalar_type_hint(ty.mask, &value, diag)? {
        Some(v) => Ok(v),
        None => Err(arg.type_error(ty, &value)),
    }
}

/// Port of `zend_verify_return_type()` / `zend_check_type_slow()` (PHPSRC/Zend/zend_execute.c) for
/// the declared return type of a userland function in coercive mode, with
/// `zend_verify_return_error()`'s message: the value as is when its type is declared or it is an
/// instance of a declared class; `null` only for nullable types; otherwise the weak scalar
/// coercion of [`verify_weak_scalar_type_hint`]. Failure: `TypeError` `f(): Return value must be of
/// type string, array returned` (`function` is the name PHP prints, `Class::method` for a method).
///
/// Unlike an argument check, the return check throws its `TypeError` even when the coercion failed
/// with an exception (a throwing `__toString()`, a diagnostic the sink turns into an exception):
/// `zend_type_error()` chains the pending exception as the previous one of the `TypeError`
/// (`zend_throw_exception_internal()`, PHPSRC/Zend/zend_exceptions.c). The message names the value
/// as the failed coercion left it (a scalar already converted to a string reads `string`).
pub fn verify_user_return(
    value: Value,
    ty: &DeclaredType<'_>,
    function: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    if ty.mask.accepts_type_of(&value) {
        return Ok(value);
    }
    if let Value::Object(o) = &value
        && ty.classes.iter().any(|c| o.instance_of(c))
    {
        return Ok(value);
    }
    let error = |returned: &Value| {
        PhpError::type_error(PhpString::concat(&[
            function.as_bytes(),
            b"(): Return value must be of type ",
            ty.to_type_string().as_bytes(),
            b", ",
            types::zend_zval_value_name(returned).as_bytes(),
            b" returned",
        ]))
    };
    match weak_scalar(ty.mask, &value, diag) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Err(error(&value)),
        Err(failure) => {
            let returned = failure.converted.as_ref().unwrap_or(&value);
            Err(error(returned).with_previous(failure.error))
        }
    }
}

/// Whether a value would pass the weak scalar check for `tag` without being rejected (no
/// diagnostics emitted); a helper for builtins that pick an overload by argument type.
pub fn accepts_weak(v: &Value, tag: TypeTag) -> bool {
    match tag {
        TypeTag::Long => match v {
            Value::Int(_) | Value::Bool(_) | Value::Null => true,
            Value::Float(d) => !d.is_nan() && convert::double_fits_long(*d),
            Value::String(s) => match numeric::is_numeric_str_function(s) {
                Some(Number::Int(_)) => true,
                Some(Number::Float(d)) => !d.is_nan() && convert::double_fits_long(d),
                None => false,
            },
            _ => false,
        },
        TypeTag::Double => match v {
            Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::Null => true,
            Value::String(s) => numeric::is_numeric_str_function(s).is_some(),
            _ => false,
        },
        TypeTag::String => {
            !matches!(v, Value::Array(_))
                && !matches!(v, Value::Object(o) if !o.class().is_subclass_of("Stringable"))
        }
        TypeTag::False | TypeTag::True => matches!(
            v,
            Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::String(_)
        ),
        TypeTag::Array => matches!(v, Value::Array(_)),
        TypeTag::Object => matches!(v, Value::Object(_)),
        TypeTag::Null => v.is_null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{Diagnostic, Level};

    #[test]
    fn weak_ints() {
        let arg = Arg::new("str_repeat", 2, "times");
        let mut d: Vec<Diagnostic> = Vec::new();
        assert_eq!(parse_long(&Value::Float(2.5), &arg, &mut d).unwrap(), 2);
        assert_eq!(parse_long(&Value::from("2.5"), &arg, &mut d).unwrap(), 2);
        assert_eq!(parse_long(&Value::from(" 2"), &arg, &mut d).unwrap(), 2);
        assert_eq!(parse_long(&Value::from("2 "), &arg, &mut d).unwrap(), 2);
        assert_eq!(parse_long(&Value::from("1e1"), &arg, &mut d).unwrap(), 10);
        assert_eq!(parse_long(&Value::Null, &arg, &mut d).unwrap(), 0);
        assert_eq!(parse_long(&Value::Bool(true), &arg, &mut d).unwrap(), 1);
        assert_eq!(
            d,
            vec![
                Diagnostic::new(
                    Level::Deprecated,
                    "Implicit conversion from float 2.5 to int loses precision"
                ),
                Diagnostic::new(
                    Level::Deprecated,
                    "Implicit conversion from float-string \"2.5\" to int loses precision"
                ),
                Diagnostic::new(
                    Level::Deprecated,
                    "str_repeat(): Passing null to parameter #2 ($times) of type int is deprecated"
                ),
            ]
        );
        for bad in [
            Value::from("2abc"),
            Value::Float(1e20),
            Value::Float(f64::NAN),
            Value::Array(Array::new()),
        ] {
            let e = parse_long(&bad, &arg, &mut d).unwrap_err();
            assert_eq!(e.class().name(), "TypeError");
            assert!(
                e.message()
                    .to_string()
                    .starts_with("str_repeat(): Argument #2 ($times) must be of type int, ")
            );
        }
        let e = parse_long(&Value::from("2abc"), &arg, &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "str_repeat(): Argument #2 ($times) must be of type int, string given"
        );
    }

    #[test]
    fn weak_strings_and_numbers() {
        let mut d: Vec<Diagnostic> = Vec::new();
        let arg = Arg::new("strlen", 1, "string");
        assert_eq!(
            parse_str(&Value::Float(1e25), &arg, &mut d).unwrap(),
            "1.0E+25"
        );
        assert_eq!(parse_str(&Value::Bool(false), &arg, &mut d).unwrap(), "");
        let e = parse_str(&Value::Array(Array::new()), &arg, &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "strlen(): Argument #1 ($string) must be of type string, array given"
        );
        let abs = Arg::new("abs", 1, "num");
        assert_eq!(
            parse_number(&Value::from("5.5"), &abs, &mut d).unwrap(),
            Number::Float(5.5)
        );
        let e = parse_number(&Value::from("x"), &abs, &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "abs(): Argument #1 ($num) must be of type int|float, string given"
        );
        assert!(d.is_empty());
        parse_number(&Value::Null, &abs, &mut d).unwrap();
        assert_eq!(
            d[0].message,
            "abs(): Passing null to parameter #1 ($num) of type int|float is deprecated"
        );
        let e =
            parse_bool(&Value::Array(Array::new()), &Arg::new("f", 1, "b"), &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "f(): Argument #1 ($b) must be of type bool, array given"
        );
        let e = parse_long(&Value::Bool(true), &Arg::new("f", 1, "b"), &mut d);
        assert_eq!(e.unwrap(), 1);
        let e = parse_str(&Value::Bool(true), &Arg::new("f", 1, "b"), &mut d);
        assert_eq!(e.unwrap(), "1");
        let e = parse_array(&Value::Bool(true), &Arg::new("f", 1, "a")).unwrap_err();
        assert_eq!(
            e.message(),
            "f(): Argument #1 ($a) must be of type array, true given"
        );
    }
}
