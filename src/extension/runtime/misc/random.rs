//! The `random` function (`CoreExtension::random()`) over PHP's seeded MT19937 (`mt_rand()`, `array_rand()`).

use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use twig_value::builtins::array::{Mt19937, array_pick_keys, mt_rand, mt_rand_range};
use twig_value::builtins::string::iconv::iconv_bytes;
use twig_value::object::{FunctionKind, Param};
use twig_value::{Array, ArrayKey, Diagnostics, PhpString, Value, compare, convert};

use super::bind;
use crate::error::Error;
use crate::extension::runtime::{Call, CallResult};
use crate::runtime::core::to_array;

/// PHP's per-request MT19937 state, seeded from entropy on first use (templates cannot seed it: `mt_srand()` is not
/// reachable from Twig). One state serves the process.
static RNG: LazyLock<Mutex<Mt19937>> = LazyLock::new(|| Mutex::new(Mt19937::from_entropy()));

/// The generator, locked only around a draw (iterating a `Traversable` can run template code).
fn lock(rng: &Mutex<Mt19937>) -> MutexGuard<'_, Mt19937> {
    rng.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Twig: `CoreExtension::random(string $charset, $values = null, $max = null)`, the `random` function.
pub(super) fn random(call: &mut Call<'_>) -> CallResult {
    let args = bind(
        call,
        "Twig\\Extension\\CoreExtension::random",
        1,
        &[
            Param::required("charset"),
            Param::optional("values", Value::Null),
            Param::optional("max", Value::Null),
        ],
        FunctionKind::User,
    )?;
    let mut args = args.into_iter();
    let values = args.next().unwrap_or_default();
    let max = args.next().unwrap_or_default();
    let charset = call.charset();
    random_value(&RNG, &charset, values, max, call.diagnostics())
}

/// The body of `CoreExtension::random()`: `mt_rand()` for no values (`mt_rand(0, (int) $max)` with a maximum), a
/// number between `0` (or `$max`) and an int or float, a random character of a string (`''` for `''`), a random
/// element of an array or `Traversable`, and any other value as it is.
///
/// Errors: `The "random" function cannot pick from an empty sequence or mapping.`, `Unable to split the string
/// passed to "random" into characters: Malformed UTF-8 characters, possibly incorrectly encoded.` (runtime errors),
/// `mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)` (a `ValueError`).
pub(super) fn random_value(
    rng: &Mutex<Mt19937>,
    charset: &PhpString,
    values: Value,
    max: Value,
    diag: &mut dyn Diagnostics,
) -> CallResult {
    if values.is_null() {
        if max.is_null() {
            return Ok(Value::Int(mt_rand(&mut lock(rng))));
        }
        let max = Value::Int(convert::to_long(&max, diag)?);
        return Ok(Value::Int(mt_rand_range(
            &mut lock(rng),
            &Value::Int(0),
            &max,
            diag,
        )?));
    }

    if let Value::Int(_) | Value::Float(_) = values {
        let (min, max) = if max.is_null() {
            if compare::less(&values, &Value::Int(0), diag)? {
                (values, Value::Int(0))
            } else {
                (Value::Int(0), values)
            }
        } else {
            (values, max)
        };
        let min = Value::Int(convert::to_long(&min, diag)?);
        let max = Value::Int(convert::to_long(&max, diag)?);
        return Ok(Value::Int(mt_rand_range(&mut lock(rng), &min, &max, diag)?));
    }

    let values = match values {
        Value::String(string) => {
            if string.is_empty() {
                return Ok(Value::String(string));
            }
            let utf8 = charset.as_bytes() == b"UTF-8";
            let string = if utf8 {
                string
            } else {
                convert_encoding(&string, b"UTF-8", charset, diag)?
            };
            let mut characters = split_into_characters(&string, "random")?;
            if !utf8 {
                for character in &mut characters {
                    *character = convert_encoding(character, charset, b"UTF-8", diag)?;
                }
            }
            Value::Array(Array::from_values(
                characters.into_iter().map(Value::String),
            ))
        }
        other => other,
    };

    if !values.is_iterable() {
        return Ok(values);
    }
    let Value::Array(values) = to_array(values, true, diag)? else {
        return Ok(Value::Null);
    };
    if values.is_empty() {
        return Err(Error::runtime(
            "The \"random\" function cannot pick from an empty sequence or mapping.",
        )
        .into());
    }
    let key = array_pick_keys(&mut lock(rng), &values, 1)?;
    let key = match key {
        Value::Int(i) => ArrayKey::Int(i),
        Value::String(s) => ArrayKey::from_string(s),
        _ => return Ok(Value::Null),
    };
    Ok(values.get(&key).cloned().unwrap_or_default())
}

/// Twig: `CoreExtension::convertEncoding($string, string $to, string $from)`: `iconv($from, $to, $string)`; a failed
/// conversion (`false`, after iconv's warning or notice) becomes `''` through the `string` return type.
fn convert_encoding(
    string: &PhpString,
    to: &[u8],
    from: &[u8],
    diag: &mut dyn Diagnostics,
) -> CallResult<PhpString> {
    Ok(iconv_bytes(from, to, string, diag)?.unwrap_or_default())
}

/// Twig: `CoreExtension::splitIntoCharacters()`: `preg_match_all('/./us', $string)`, the code points of a UTF-8
/// string (`['']` for `''`); invalid UTF-8 makes `preg_match_all()` fail.
fn split_into_characters(string: &PhpString, name: &str) -> CallResult<Vec<PhpString>> {
    let Ok(text) = std::str::from_utf8(string.as_bytes()) else {
        return Err(Error::runtime(format!(
            "Unable to split the string passed to \"{name}\" into characters: Malformed UTF-8 characters, possibly incorrectly encoded."
        ))
        .into());
    };
    let characters: Vec<PhpString> = text
        .char_indices()
        .map(|(i, c)| PhpString::from(&text.as_bytes()[i..i + c.len_utf8()]))
        .collect();
    // `$m[0] ?: ['']`
    Ok(if characters.is_empty() {
        vec![PhpString::empty()]
    } else {
        characters
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `mt_srand(42)`, then `random(100)`, `random(["a", "b", "c"])` and `random()` (oracle: 38, c, 2041643438).
    #[test]
    fn seeded_draws_match_php() {
        let rng = Mutex::new(Mt19937::new(42));
        let mut diag: Vec<twig_value::Diagnostic> = Vec::new();
        let utf8 = PhpString::from("UTF-8");
        let mut draw =
            |values: Value| random_value(&rng, &utf8, values, Value::Null, &mut diag).unwrap();
        assert_eq!(draw(Value::Int(100)), Value::Int(38));
        assert_eq!(
            draw(Value::Array(Array::from_values(
                ["a", "b", "c"].map(Value::from)
            ))),
            Value::from("c")
        );
        assert_eq!(draw(Value::Null), Value::Int(2041643438));
        drop(draw);
        assert!(diag.is_empty());
    }
}
