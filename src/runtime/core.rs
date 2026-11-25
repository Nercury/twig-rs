//! The helpers of `Twig\Extension\CoreExtension` that compiled templates call directly (operators, loops, tests)
//! (TWIG/src/Extension/CoreExtension.php).

use twig_value::object::ObjectRef;
use twig_value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::types::get_debug_type;
use twig_value::{
    Array, ArrayKey, Diagnostics, Number, PhpError, PhpString, Value, compare, convert, numeric,
};

use super::markup::Markup;
use crate::deprecation::Deprecation;
use crate::error::Error;
use crate::lltl::Program;
use crate::vm::{Exception, VmState, invoke_value};

/// Twig: `CoreExtension::ensureTraversable()`: the value when it is iterable, else `[]`.
pub fn ensure_traversable(value: Value) -> Value {
    if value.is_iterable() {
        value
    } else {
        Value::Array(Array::new())
    }
}

/// Twig: `CoreExtension::toArray()`: a `Traversable` becomes an array (`iterator_to_array()`), an array is kept (or
/// listed without keys), anything else is returned unchanged.
///
/// # Errors
///
/// The exceptions of the iteration.
pub fn to_array(
    value: Value,
    preserve_keys: bool,
    diag: &mut dyn Diagnostics,
) -> Result<Value, Exception> {
    match value {
        Value::Object(object) if object.instance_of("Traversable") => Ok(Value::Array(
            twig_value::object::iterator_to_array(&object, preserve_keys, diag)?,
        )),
        Value::Array(array) => Ok(Value::Array(if preserve_keys {
            array
        } else {
            Array::from_values(array.values().cloned())
        })),
        other => Ok(other),
    }
}

/// `(string) $markup` when the value is a `Markup`.
fn markup_to_string(value: &Value, diag: &mut dyn Diagnostics) -> Result<Value, Exception> {
    if let Value::Object(object) = value
        && Markup::is_markup(object)
    {
        return Ok(Value::String(object.to_php_string(diag)?));
    }
    Ok(value.clone())
}

/// `trim($s, " \t\n\r\v\f")`.
fn php_space_trim(s: &PhpString) -> PhpString {
    let is_space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C);
    let bytes = s.as_bytes();
    let start = bytes
        .iter()
        .position(|b| !is_space(b))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !is_space(b))
        .map_or(start, |i| i + 1);
    PhpString::from(&bytes[start..end.max(start)])
}

/// `(int) $numericString == $numericString` after `is_numeric()`: whether the numeric string is an integer value.
fn numeric_string_int(s: &PhpString, diag: &mut dyn Diagnostics) -> Result<Option<i64>, Exception> {
    let as_int = convert::to_long(&Value::String(s.clone()), diag)?;
    Ok(
        if compare::equals(&Value::Int(as_int), &Value::String(s.clone()), diag)? {
            Some(as_int)
        } else {
            None
        },
    )
}

/// Twig: `CoreExtension::compare()`: PHP 8 comparison, with the int/float against numeric-string cases spelled out
/// (whitespace-trimmed numeric strings compare as numbers).
///
/// # Errors
///
/// The exceptions of comparing objects.
pub fn compare_values(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> Result<i64, Exception> {
    let string_cmp = |x: &Value, y: &Value, diag: &mut dyn Diagnostics| -> Result<i64, Exception> {
        Ok(compare::spaceship(x, y, diag)?)
    };
    match (a, b) {
        (Value::Int(x), Value::String(s)) => {
            let trimmed = php_space_trim(s);
            if !numeric::is_numeric(&Value::String(trimmed.clone())) {
                return string_cmp(&Value::String(convert::to_string(a, diag)?), b, diag);
            }
            match numeric_string_int(&trimmed, diag)? {
                Some(n) => string_cmp(&Value::Int(*x), &Value::Int(n), diag),
                None => {
                    let f = convert::to_double(&Value::String(trimmed), diag)?;
                    string_cmp(&Value::Float(*x as f64), &Value::Float(f), diag)
                }
            }
        }
        (Value::String(s), Value::Int(y)) => {
            let trimmed = php_space_trim(s);
            if !numeric::is_numeric(&Value::String(trimmed.clone())) {
                return string_cmp(a, &Value::String(convert::to_string(b, diag)?), diag);
            }
            match numeric_string_int(&trimmed, diag)? {
                Some(n) => string_cmp(&Value::Int(n), &Value::Int(*y), diag),
                None => {
                    let f = convert::to_double(&Value::String(trimmed), diag)?;
                    string_cmp(&Value::Float(f), &Value::Float(*y as f64), diag)
                }
            }
        }
        (Value::Float(x), Value::String(s)) => {
            if x.is_nan() {
                return Ok(1);
            }
            let trimmed = php_space_trim(s);
            if !numeric::is_numeric(&Value::String(trimmed.clone())) {
                return string_cmp(&Value::String(convert::to_string(a, diag)?), b, diag);
            }
            let f = convert::to_double(&Value::String(trimmed), diag)?;
            string_cmp(a, &Value::Float(f), diag)
        }
        (Value::String(s), Value::Float(y)) => {
            if y.is_nan() {
                return Ok(1);
            }
            let trimmed = php_space_trim(s);
            if !numeric::is_numeric(&Value::String(trimmed.clone())) {
                return string_cmp(a, &Value::String(convert::to_string(b, diag)?), diag);
            }
            let f = convert::to_double(&Value::String(trimmed), diag)?;
            string_cmp(&Value::Float(f), b, diag)
        }
        _ => string_cmp(a, b, diag),
    }
}

/// Twig: `CoreExtension::inFilter()` (the `in` and `not in` operators).
///
/// # Errors
///
/// The exceptions of iterating or comparing.
pub fn in_filter(
    value: &Value,
    compare: &Value,
    diag: &mut dyn Diagnostics,
) -> Result<bool, Exception> {
    let value = markup_to_string(value, diag)?;
    let compare = markup_to_string(compare, diag)?;
    if let Value::String(haystack) = &compare {
        return Ok(match &value {
            Value::String(_) | Value::Int(_) | Value::Float(_) => {
                let needle = convert::to_string(&value, diag)?;
                matches!(&value, Value::String(s) if s.is_empty())
                    || needle.is_empty()
                    || memchr::memmem::find(haystack, &needle).is_some()
            }
            _ => false,
        });
    }
    if !compare.is_iterable() {
        return Ok(false);
    }
    if let Value::Object(_) = &value {
        return match &compare {
            Value::Array(array) => Ok(array.values().any(|item| item.identical(&value))),
            Value::Object(object) => {
                let mut found = false;
                twig_value::object::traverse(object, false, diag, |_, item, _| {
                    if item.identical(&value) {
                        found = true;
                        return Ok(false);
                    }
                    Ok(true)
                })?;
                Ok(found)
            }
            _ => Ok(false),
        };
    }
    match &compare {
        Value::Array(array) => {
            for item in array.values() {
                if compare_values(&value, item, diag)? == 0 {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Value::Object(object) => {
            let mut found = false;
            let mut failure: Option<Exception> = None;
            twig_value::object::traverse(
                object,
                false,
                diag,
                |_, item, diag| match compare_values(&value, &item, diag) {
                    Ok(0) => {
                        found = true;
                        Ok(false)
                    }
                    Ok(_) => Ok(true),
                    Err(e) => {
                        failure = Some(e);
                        Ok(false)
                    }
                },
            )?;
            if let Some(e) = failure {
                return Err(e);
            }
            Ok(found)
        }
        _ => Ok(false),
    }
}

/// Twig: `CoreExtension::matches(string $regexp, ?string $str): int`: `preg_match()` with an error handler turning
/// warnings into `RuntimeError('Regexp "%s" passed to "matches" is not valid'.substr($m, 12))`, and
/// `RuntimeError('Regexp "%s" passed to "matches" failed: %s.')` when it returns `false`.
///
/// # Errors
///
/// The errors above, and the `TypeError`s of the typed parameters.
pub fn matches(
    regexp: &Value,
    subject: &Value,
    diag: &mut dyn Diagnostics,
) -> Result<Value, Exception> {
    const FUNCTION: &str = "Twig\\Extension\\CoreExtension::matches";
    let regexp = verify_user_arg(
        regexp.clone(),
        &DeclaredType::builtin(TypeMask::STRING),
        &UserArg::new(FUNCTION, 1, "regexp"),
        diag,
    )?;
    let subject = verify_user_arg(
        subject.clone(),
        &DeclaredType::builtin(TypeMask::STRING | TypeMask::NULL),
        &UserArg::new(FUNCTION, 2, "str"),
        diag,
    )?;
    let regexp = match regexp {
        Value::String(s) => s,
        other => convert::to_string(&other, diag)?,
    };
    let subject = match subject {
        Value::String(s) => s,
        Value::Null => PhpString::empty(),
        other => convert::to_string(&other, diag)?,
    };
    match twig_pcre::matches(&regexp, &subject) {
        Ok(result) => Ok(Value::Int(result)),
        Err(error) => Err(Error::runtime(
            String::from_utf8_lossy(&error.message(&regexp)).into_owned(),
        )
        .into()),
    }
}

/// Twig: `CoreExtension::checkArrow()`: a `Closure` passes; in the sandbox anything else fails, outside it is
/// deprecated.
///
/// # Errors
///
/// `The callable passed to the "%s" %s must be a Closure in sandbox mode.`
pub fn check_arrow(
    state: &mut VmState,
    is_sandboxed: bool,
    arrow: &Value,
    thing: &str,
    kind: &str,
) -> Result<(), Exception> {
    if let Value::Object(object) = arrow
        && object
            .downcast_ref::<twig_value::object::Closure>()
            .is_some()
    {
        return Ok(());
    }
    if is_sandboxed {
        return Err(Error::runtime(format!(
            "The callable passed to the \"{thing}\" {kind} must be a Closure in sandbox mode."
        ))
        .into());
    }
    state.env.report_deprecation(Deprecation::twig(
        "3.15",
        format!(
            "Passing a callable that is not a PHP \\Closure as an argument to the \"{thing}\" {kind} is deprecated."
        ),
    ));
    Ok(())
}

/// Twig: `CoreExtension::arraySome()` / `arrayEvery()` (the `has some` and `has every` operators).
///
/// # Errors
///
/// `The "has some" test expects a sequence or a mapping, got "%s".` (resp. `has every`), and the exceptions of the
/// callable.
pub fn array_some_every(
    state: &mut VmState,
    array: &Value,
    arrow: &Value,
    is_sandboxed: bool,
    every: bool,
) -> Result<bool, Exception> {
    let thing = if every { "has every" } else { "has some" };
    if !array.is_iterable() {
        return Err(Error::runtime(format!(
            "The \"{thing}\" test expects a sequence or a mapping, got \"{}\".",
            get_debug_type(array)
        ))
        .into());
    }
    check_arrow(state, is_sandboxed, arrow, thing, "operator")?;
    let mut result = every;
    for_each(state, array, |state, key, value| {
        let returned = invoke_value(state, arrow, vec![value, key])?;
        let truthy = convert::to_bool(&returned, state)?;
        if every && !truthy {
            result = false;
            return Ok(false);
        }
        if !every && truthy {
            result = true;
            return Ok(false);
        }
        Ok(true)
    })?;
    Ok(result)
}

/// `foreach ($iterable as $k => $v)`: arrays over a snapshot, Traversables through their iterator; `f` returns
/// `false` to stop.
///
/// # Errors
///
/// The exceptions of the iteration and of `f`.
pub fn for_each(
    state: &mut VmState,
    iterable: &Value,
    mut f: impl FnMut(&mut VmState, Value, Value) -> Result<bool, Exception>,
) -> Result<(), Exception> {
    match iterable {
        Value::Array(array) => {
            for (key, value) in array.clone() {
                if !f(state, key.to_value(), value)? {
                    break;
                }
            }
            Ok(())
        }
        Value::Object(object) => {
            let Some(mut iter) = object.get_iterator(state)? else {
                return Ok(());
            };
            iter.rewind(state)?;
            while iter.valid(state)? {
                let value = iter.current(state)?;
                let key = iter.key(state)?;
                if !f(state, key, value)? {
                    break;
                }
                iter.next(state)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Twig: `CoreExtension::destructureSequence()`: assigns the first values of a `Traversable` to the non-null names,
/// then `null` to the names left.
///
/// # Errors
///
/// The exceptions of the iteration.
pub fn destructure_sequence(
    state: &mut VmState,
    context: &mut Array,
    names: &[Option<PhpString>],
    sequence: &ObjectRef,
) -> Result<(), Exception> {
    let count = names.len();
    if count == 0 {
        return Ok(());
    }
    let mut i = 0;
    let mut done = false;
    twig_value::object::traverse(sequence, false, state, |_, value, _| {
        if let Some(Some(name)) = names.get(i) {
            context.insert(ArrayKey::from_string(name.clone()), value);
        }
        i += 1;
        if i == count {
            done = true;
            return Ok(false);
        }
        Ok(true)
    })?;
    if !done {
        while i < count {
            if let Some(name) = &names[i] {
                context.insert(ArrayKey::from_string(name.clone()), Value::Null);
            }
            i += 1;
        }
    }
    Ok(())
}

/// PHP's `constant($name)` (the `constant` test), or `constant(get_class($object)."::".$name)`: the compiled code of
/// Twig's `ConstantTest` (TWIG/src/Node/Expression/Test/ConstantTest.php). The lookup is PHP's `constant()` over the
/// environment's classes and constants and the PHP constants twig-rs ships
/// ([`constant_value`](crate::extension::runtime::misc::constants::constant_value): visibility checks, deprecations
/// naming the declaring class).
///
/// # Errors
///
/// `get_class(): Argument #1 ($object) must be of type object, %s given`, the `TypeError` of `constant()`'s `string
/// $name` parameter, and the errors of the lookup (`Undefined constant "X"`, `Class "X" not found`, `Undefined
/// constant X::Y`, `Cannot access private constant X::Y`).
pub fn php_constant(
    state: &mut VmState,
    name: &Value,
    object: Option<&Value>,
) -> Result<Value, Exception> {
    let full = match object {
        Some(object) => {
            let class = match object {
                Value::Object(o) => o.class_name().to_owned(),
                other => {
                    return Err(PhpError::type_error(format!(
                        "get_class(): Argument #1 ($object) must be of type object, {} given",
                        twig_value::types::zend_zval_value_name(other)
                    ))
                    .into());
                }
            };
            // `get_class($object)."::".$name`: the concatenation converts the name
            let name = convert::to_string(name, state)?;
            PhpString::concat(&[class.as_bytes(), b"::", name.as_bytes()])
        }
        // `constant($name)`: the internal function's `string $name` parameter
        None => twig_value::param::parse_str(
            name,
            &twig_value::param::Arg::new("constant", 1, "name"),
            state,
        )?,
    };
    let env = state.env.clone();
    Ok(crate::extension::runtime::misc::constants::constant_value(
        &env, &full, state,
    )?)
}

/// PHP's `++`/`--` (`increment_function()`/`decrement_function()`, PHPSRC/Zend/zend_operators.c).
///
/// # Errors
///
/// `Cannot increment array` and the like.
pub fn increment(value: &Value, up: bool, diag: &mut dyn Diagnostics) -> Result<Value, Exception> {
    Ok(match value {
        Value::Int(i) => {
            if up {
                i.checked_add(1)
                    .map_or(Value::Float(*i as f64 + 1.0), Value::Int)
            } else {
                i.checked_sub(1)
                    .map_or(Value::Float(*i as f64 - 1.0), Value::Int)
            }
        }
        Value::Float(f) => Value::Float(if up { f + 1.0 } else { f - 1.0 }),
        Value::Null => {
            if up {
                Value::Int(1)
            } else {
                diag.warn("Decrement on type null has no effect, this will change in the next major version of PHP")?;
                Value::Null
            }
        }
        Value::Bool(_) => {
            diag.warn(if up {
                "Increment on type bool has no effect, this will change in the next major version of PHP"
            } else {
                "Decrement on type bool has no effect, this will change in the next major version of PHP"
            })?;
            value.clone()
        }
        Value::String(s) => {
            if !up && s.is_empty() {
                diag.deprecated("Decrement on empty string is deprecated as non-numeric")?;
                return Ok(Value::Int(-1));
            }
            match numeric::is_numeric_str_function(s) {
                Some(Number::Int(i)) => {
                    return increment(&Value::Int(i), up, diag);
                }
                Some(Number::Float(f)) => Value::Float(if up { f + 1.0 } else { f - 1.0 }),
                None => {
                    if up {
                        diag.deprecated(
                            "Increment on non-numeric string is deprecated, use str_increment() instead",
                        )?;
                        Value::String(increment_string(s))
                    } else {
                        diag.deprecated(
                            "Decrement on non-numeric string has no effect and is deprecated",
                        )?;
                        value.clone()
                    }
                }
            }
        }
        Value::Array(_) | Value::Object(_) => {
            return Err(PhpError::type_error(format!(
                "Cannot {} {}",
                if up { "increment" } else { "decrement" },
                twig_value::types::zend_zval_value_name(value)
            ))
            .into());
        }
    })
}

/// Port of `increment_string()` (PHPSRC/Zend/zend_operators.c): Perl-style alphanumeric increment.
fn increment_string(s: &PhpString) -> PhpString {
    if s.is_empty() {
        return PhpString::from("1");
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
                if ch == b'z' {
                    bytes[pos] = b'a';
                    carry = true;
                } else {
                    bytes[pos] += 1;
                    carry = false;
                }
                last = Last::Lower;
            }
            b'A'..=b'Z' => {
                if ch == b'Z' {
                    bytes[pos] = b'A';
                    carry = true;
                } else {
                    bytes[pos] += 1;
                    carry = false;
                }
                last = Last::Upper;
            }
            b'0'..=b'9' => {
                if ch == b'9' {
                    bytes[pos] = b'0';
                    carry = true;
                } else {
                    bytes[pos] += 1;
                    carry = false;
                }
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
    PhpString::from(bytes)
}

/// `DeprecatedNode`'s `trigger_deprecation($package, $version, sprintf('%s in "%s" at line %d.', $message, $name))`.
///
/// # Errors
///
/// The exceptions of converting the values to strings.
pub fn trigger_template_deprecation(
    state: &mut VmState,
    program: &Program,
    package: &Value,
    version: &Value,
    message: &Value,
    line: i64,
) -> Result<(), Exception> {
    let message = convert::to_string(message, state)?;
    let text = format!(
        "{} in \"{}\" at line {line}.",
        message.to_string_lossy(),
        program.name()
    );
    // symfony/deprecation-contracts: ($package || $version ? "Since $package $version: " : '').$message
    let shown = convert::to_bool(package, state)? || convert::to_bool(version, state)?;
    let deprecation = if shown {
        let package = convert::to_string(package, state)?;
        let version = convert::to_string(version, state)?;
        Deprecation::new(
            package.to_string_lossy().into_owned(),
            version.to_string_lossy().into_owned(),
            text,
        )
    } else {
        Deprecation::new("", "", text)
    };
    state.env.report_deprecation(deprecation);
    Ok(())
}

/// Twig: `CoreExtension::testEmpty()`: `Countable` → `count() == 0`, `Traversable` → no element, `Stringable` → `''`,
/// otherwise `''`, `false`, `null` or `[]`.
///
/// # Errors
///
/// The exceptions of counting, iterating or converting.
pub fn test_empty(value: &Value, diag: &mut dyn Diagnostics) -> Result<bool, Exception> {
    if let Value::Object(object) = value {
        if object.instance_of("Countable") {
            return Ok(twig_value::dim::count(value, 0, diag)? == 0);
        }
        if object.instance_of("Traversable") {
            return Ok(twig_value::object::iterator_count(object, diag)? == 0);
        }
        if object.instance_of("Stringable") {
            return Ok(object.to_php_string(diag)?.is_empty());
        }
        return Ok(false);
    }
    Ok(match value {
        Value::String(s) => s.is_empty(),
        Value::Bool(b) => !*b,
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments_strings_like_php() {
        assert_eq!(increment_string(&PhpString::from("a")), "b");
        assert_eq!(increment_string(&PhpString::from("z")), "aa");
        assert_eq!(increment_string(&PhpString::from("Az")), "Ba");
        assert_eq!(increment_string(&PhpString::from("a9")), "b0");
        assert_eq!(increment_string(&PhpString::from("Zz")), "AAa");
        assert_eq!(increment_string(&PhpString::from("a-z")), "a-a");
        let mut diag = Vec::new();
        assert_eq!(
            increment(&Value::Int(i64::MAX), true, &mut diag).unwrap(),
            Value::Float(i64::MAX as f64 + 1.0)
        );
        assert_eq!(
            increment(&Value::from("5"), true, &mut diag).unwrap(),
            Value::Int(6)
        );
    }

    #[test]
    fn compares_like_twig() {
        let mut diag = Vec::new();
        assert_eq!(
            compare_values(&Value::Int(1), &Value::from(" 1"), &mut diag).unwrap(),
            0
        );
        assert_eq!(
            compare_values(&Value::Int(0), &Value::from("a"), &mut diag).unwrap(),
            -1
        );
        assert_eq!(
            compare_values(&Value::from("1e1"), &Value::from("10"), &mut diag).unwrap(),
            0
        );
        assert!(in_filter(&Value::Int(1), &Value::from("123"), &mut diag).unwrap());
        assert!(in_filter(&Value::from(""), &Value::from("abc"), &mut diag).unwrap());
        assert!(!in_filter(&Value::Null, &Value::from("abc"), &mut diag).unwrap());
    }
}
