//! The `ctype_*()` functions (reference/php/stdlib.md §5.10). Twig uses `ctype_digit` (timestamps
//! in `date`), `ctype_space` (blank text nodes), `ctype_xdigit`/`ctype_alpha` (lexer).
//!
//! PHP runs with `LC_CTYPE=C.UTF-8` (`zend_reset_lc_ctype_locale()`), whose single-byte classes
//! are the C locale's: only ASCII bytes belong to any class (verified for all 256 bytes against
//! the oracle).

use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::types;
use crate::value::Value;

/// A `<ctype.h>` classification function of the C/C.UTF-8 locale.
type CharClass = fn(u8) -> bool;

/// Port of `ctype_impl()` and `ctype_fallback()` (PHPSRC/ext/ctype/ctype.c).
///
/// Strings: `false` when empty, else whether every byte is in the class. Other values deprecate
/// `ctype_x(): Argument of type T will be interpreted as string in the future` (`T` from
/// `zend_zval_type_name()`); ints in `-128..=255` are then classified as one byte (negative ones
/// plus 256), larger ints give `allow_digits`, smaller ones `allow_minus`, everything else `false`.
fn ctype(
    name: &str,
    text: &Value,
    class: CharClass,
    allow_digits: bool,
    allow_minus: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    if let Value::String(s) = text {
        return Ok(!s.is_empty() && s.iter().all(|&c| class(c)));
    }
    diag.deprecated(format!(
        "{name}(): Argument of type {} will be interpreted as string in the future",
        types::zend_zval_type_name(text)
    ))?;
    Ok(match text {
        Value::Int(i) => match *i {
            0..=255 => class(*i as u8),
            -128..=-1 => class((*i + 256) as u8),
            i if i >= 0 => allow_digits,
            _ => allow_minus,
        },
        _ => false,
    })
}

macro_rules! ctype_fn {
    ($(#[$doc:meta])* $name:ident, $class:expr, $digits:expr, $minus:expr) => {
        $(#[$doc])*
        pub fn $name(text: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
            ctype(stringify!($name), text, $class, $digits, $minus, diag)
        }
    };
}

ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_alnum)` (`isalnum`).
    ctype_alnum,
    |c| c.is_ascii_alphanumeric(),
    true,
    false
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_alpha)` (`isalpha`).
    ctype_alpha,
    |c| c.is_ascii_alphabetic(),
    false,
    false
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_cntrl)` (`iscntrl`: `0x00..=0x1F` and `0x7F`).
    ctype_cntrl,
    |c| c.is_ascii_control(),
    false,
    false
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_digit)` (`isdigit`).
    ctype_digit,
    |c| c.is_ascii_digit(),
    true,
    false
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_lower)` (`islower`).
    ctype_lower,
    |c| c.is_ascii_lowercase(),
    false,
    false
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_graph)` (`isgraph`: `0x21..=0x7E`).
    ctype_graph,
    |c| c.is_ascii_graphic(),
    true,
    true
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_print)` (`isprint`: `0x20..=0x7E`).
    ctype_print,
    |c| (0x20..=0x7E).contains(&c),
    true,
    true
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_punct)` (`ispunct`).
    ctype_punct,
    |c| c.is_ascii_punctuation(),
    false,
    false
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_space)` (`isspace`: space, `\t`, `\n`, `\v`, `\f`, `\r`).
    ctype_space,
    |c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'),
    false,
    false
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_upper)` (`isupper`).
    ctype_upper,
    |c| c.is_ascii_uppercase(),
    false,
    false
);
ctype_fn!(
    /// Port of `PHP_FUNCTION(ctype_xdigit)` (`isxdigit`).
    ctype_xdigit,
    |c| c.is_ascii_hexdigit(),
    true,
    false
);
