//! `split` and `join` (TWIG/src/Extension/CoreExtension.php).

use twig_value::builtins::{array, string};
use twig_value::object::Param;
use twig_value::param::{self, Arg, DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::{Array, Diagnostics, PhpString, Value, compare, convert, ops};

use super::mb;
use super::{Call, CallResult, bind_user, or_empty, return_string};
use crate::error::Error;
use crate::runtime::core::to_array;

/// Twig: `CoreExtension::splitIntoCharacters(string $string, string $name): array` (private): the UTF-8 characters
/// of the string, `['']` for an empty one.
///
/// `$string` is a userland `string` parameter (ints, floats, bools and `Stringable` objects are coerced, anything
/// else is a `TypeError`). Twig splits with `preg_match_all('/./us', $string, $matches)`, which fails on invalid
/// UTF-8 (PCRE2's check accepts exactly the strings Rust accepts as `str`): `RuntimeError('Unable to split the
/// string passed to "%s" into characters: %s.')` with `preg_last_error_msg()`.
fn split_into_characters(
    value: Value,
    name: &str,
    diag: &mut dyn Diagnostics,
) -> CallResult<Array> {
    let string = match verify_user_arg(
        value,
        &DeclaredType::builtin(TypeMask::STRING),
        &UserArg::new(
            "Twig\\Extension\\CoreExtension::splitIntoCharacters",
            1,
            "string",
        ),
        diag,
    )? {
        Value::String(string) => string,
        other => convert::to_string(&other, diag)?,
    };
    let Ok(text) = std::str::from_utf8(&string) else {
        return Err(Error::runtime(format!(
            "Unable to split the string passed to \"{name}\" into characters: {}.",
            twig_pcre::PregError::BadUtf8.message()
        ))
        .into());
    };
    if text.is_empty() {
        return Ok(Array::from_values([Value::String(PhpString::empty())]));
    }
    let mut buffer = [0; 4];
    Ok(Array::from_values(text.chars().map(|character| {
        Value::String(PhpString::from(
            character.encode_utf8(&mut buffer).as_bytes(),
        ))
    })))
}

/// Twig: `CoreExtension::split(string $charset, $value, $delimiter, $limit = null): array`.
///
/// ```text
/// $value = $value ?? '';
/// if ('' !== $delimiter) return null === $limit ? explode($delimiter, $value) : explode($delimiter, $value, $limit);
/// if ($limit <= 1) return self::splitIntoCharacters($value, 'split');
/// $length = mb_strlen($value, $charset);
/// if ($length < $limit) return [$value];
/// for ($i = 0; $i < $length; $i += $limit) $r[] = mb_substr($value, $i, $limit, $charset);
/// ```
///
/// The comparisons and `+=` are PHP's (loose comparison, numeric strings, floats with their deprecations when
/// `mb_substr()` receives them); `[$value]` keeps the value as it came (an int stays an int).
pub(super) fn split(call: &mut Call<'_>) -> CallResult {
    let charset = call.charset();
    let mut args = bind_user(
        call,
        "Twig\\Extension\\CoreExtension::split",
        vec![Value::String(charset.clone())],
        &[
            Param::required("charset"),
            Param::required("value"),
            Param::required("delimiter"),
            Param::optional("limit", Value::Null),
        ],
    )?
    .into_iter()
    .skip(1);
    let value = or_empty(args.next().unwrap_or_default());
    let delimiter = args.next().unwrap_or_default();
    let limit = args.next().unwrap_or_default();
    let diag = call.diagnostics();
    if !delimiter.as_str().is_some_and(PhpString::is_empty) {
        let limit = if limit.is_null() { None } else { Some(&limit) };
        return Ok(Value::Array(string::explode(
            &delimiter, &value, limit, diag,
        )?));
    }
    if compare::less_or_equal(&limit, &Value::Int(1), diag)? {
        return Ok(Value::Array(split_into_characters(value, "split", diag)?));
    }
    let length = Value::Int(mb_strlen(&value, &charset, diag)?);
    if compare::less(&length, &limit, diag)? {
        return Ok(Value::Array(Array::from_values([value])));
    }
    let mut parts = Array::new();
    let mut i = Value::Int(0);
    while compare::less(&i, &length, diag)? {
        parts.push(mb_substr(&value, &i, &limit, &charset, diag)?)?;
        i = ops::add(&i, &limit, diag)?;
    }
    Ok(Value::Array(parts))
}

/// `mb_strlen($value, $charset)`.
fn mb_strlen(value: &Value, charset: &[u8], diag: &mut dyn Diagnostics) -> CallResult<i64> {
    let string = param::parse_str(
        value,
        &Arg::new("mb_strlen", 1, "string").declared("string"),
        diag,
    )?;
    let charset = mb::get_encoding("mb_strlen", 2, charset, diag)?;
    Ok(mb::strlen(&string, charset))
}

/// `mb_substr($value, $start, $length, $charset)` with `int $start` and `?int $length` parameters.
fn mb_substr(
    value: &Value,
    start: &Value,
    length: &Value,
    charset: &[u8],
    diag: &mut dyn Diagnostics,
) -> CallResult<Value> {
    let string = param::parse_str(
        value,
        &Arg::new("mb_substr", 1, "string").declared("string"),
        diag,
    )?;
    let start = param::parse_long(
        start,
        &Arg::new("mb_substr", 2, "start").declared("int"),
        diag,
    )?;
    let length = param::parse_long_or_null(
        length,
        &Arg::new("mb_substr", 3, "length").declared("?int"),
        diag,
    )?;
    // the range errors of `$start` and `$length` come before the encoding check
    if let Some(error) = mb::substr_range_error(start, length) {
        return Err(error.into());
    }
    let charset = mb::get_encoding("mb_substr", 4, charset, diag)?;
    Ok(Value::String(mb::substr(&string, start, length, charset)?))
}

/// Twig: `CoreExtension::join($value, $glue = '', $and = null): string`.
///
/// ```text
/// if (!is_iterable($value)) $value = null === $value ? [] : [$value];
/// $value = self::toArray($value, false);
/// if (0 === count($value)) return '';
/// if (null === $and || $and === $glue) return implode($glue, $value);
/// if (1 === count($value)) return $value[0];
/// return implode($glue, array_slice($value, 0, -1)).$and.$value[count($value) - 1];
/// ```
///
/// `implode()` is PHP's two-argument (frameless) variant; the single element is coerced by the return type.
pub(super) fn join(call: &mut Call<'_>) -> CallResult {
    let function = "Twig\\Extension\\CoreExtension::join";
    let mut args = bind_user(
        call,
        function,
        Vec::new(),
        &[
            Param::required("value"),
            Param::optional("glue", ""),
            Param::optional("and", Value::Null),
        ],
    )?
    .into_iter();
    let value = args.next().unwrap_or_default();
    let glue = args.next().unwrap_or_default();
    let and = args.next().unwrap_or_default();
    let value = if value.is_iterable() {
        value
    } else if value.is_null() {
        Value::Array(Array::new())
    } else {
        Value::Array(Array::from_values([value]))
    };
    let diag = call.diagnostics();
    let value = to_array(value, false, diag)?;
    let Value::Array(items) = &value else {
        return Ok(Value::String(PhpString::empty()));
    };
    if items.is_empty() {
        return Ok(Value::String(PhpString::empty()));
    }
    if and.is_null() || and.identical(&glue) {
        return Ok(Value::String(string::implode_frameless(
            &glue,
            Some(&value),
            diag,
        )?));
    }
    if items.len() == 1 {
        let first = items.get_int(0).cloned().unwrap_or_default();
        return return_string(function, first, diag);
    }
    let head = array::array_slice(&value, &Value::Int(0), Some(&Value::Int(-1)), None, diag)?;
    let joined = string::implode_frameless(&glue, Some(&Value::Array(head)), diag)?;
    let last = items
        .get_int(items.len() as i64 - 1)
        .cloned()
        .unwrap_or_default();
    let joined = ops::concat(&Value::String(joined), &and, diag)?;
    let joined = ops::concat(&Value::String(joined), &last, diag)?;
    Ok(Value::String(joined))
}

#[cfg(test)]
mod tests {
    use super::super::test_vectors::run_topic;
    use crate::extension::runtime::{Call, CallResult, CallableRuntime};
    use crate::extension::{CoreExtension, Extension};

    /// The split topic with this group's `split` and `join` (the environment runs whichever group registered a
    /// callable last).
    #[test]
    fn split_vectors() {
        run_topic("split", &|env, _| {
            for filter in CoreExtension::new().filters() {
                let runtime: fn(&mut Call<'_>) -> CallResult = match filter.name() {
                    b"split" => super::split,
                    b"join" => super::join,
                    _ => continue,
                };
                if let Some(callable) = filter.callable() {
                    CallableRuntime::new(runtime).attach(callable);
                }
                env.add_filter(filter).expect("filters can be added");
            }
        });
    }
}
