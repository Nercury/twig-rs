//! String builtins (reference/php/stdlib.md §5 and §7): the byte-string functions Twig 3.31.1
//! calls at runtime and compile time, ported from PHP 8.5.13 with coercive-mode argument parsing,
//! verbatim diagnostics and exceptions.
//!
//! | module | functions | upstream |
//! |---|---|---|
//! | [`basic`] | `strlen`, `substr`, `strpos`, `strrpos`, `substr_count`, `str_contains`, `str_starts_with`, `str_ends_with`, `strtolower`, `strtoupper`, `ucfirst`, `lcfirst`, `ucwords`, `strcmp`, `strcasecmp`, `strncasecmp`, `ord`, `chr`, `str_repeat` | `ext/standard/string.c`, `Zend/zend_builtin_functions.c` |
//! | [`mod@trim`] | `trim`, `ltrim`, `rtrim` with `php_charmask()` ranges | `ext/standard/string.c` |
//! | [`split`] | `implode` (regular and frameless), `explode` | `ext/standard/string.c` |
//! | [`replace`] | `strtr` (character and array forms), `str_replace` | `ext/standard/string.c` |
//! | [`markup`] | `nl2br`, `strip_tags` | `ext/standard/string.c` |
//! | [`html`] | `htmlspecialchars` (all 14 charsets of `html.c`) | `ext/standard/html.c` |
//! | [`url`] | `rawurlencode`, `urlencode`, `http_build_query` | `ext/standard/url.c`, `ext/standard/http.c` |
//! | [`mod@levenshtein`] | `levenshtein` | `ext/standard/levenshtein.c` |
//! | [`ctype`] | the eleven `ctype_*` functions (C.UTF-8 `LC_CTYPE`) | `ext/ctype/ctype.c` |
//! | [`mod@iconv`] | `iconv` through an [`IconvBackend`]; the default, [`BasicIconv`], is [`charset_basic`] | `ext/iconv/iconv.c` |
//! | [`charset_basic`] | the clean-room `iconv()`: UTF-8, UTF-16, UTF-32, UCS-2, UCS-4, ASCII, ISO-8859-1 | `ext/iconv/iconv.c` (black-box glibc behaviour) |
//!
//! # Two levels per builtin
//!
//! * **PHP level**: functions named after the PHP builtin (`trim`, `strtr`, `iconv`, ...) take
//!   their arguments as [`Value`]s exactly as PHP receives them (`None` for an
//!   omitted optional argument) and coerce them like `zend_parse_parameters()` does in coercive
//!   mode (`strict_types=0`): the `Passing null to parameter #N ($x) of type T is deprecated`
//!   deprecation, the verbatim `TypeError`/`ValueError` texts, and objects with `__toString()`.
//!   Twig passes template data straight to these, so the messages are observable.
//! * **Byte level**: the algorithm on already-parsed arguments (`php_trim`, `strtr_array`,
//!   `escape_html`, `iconv_bytes`, ...), for callers that hold PHP strings already (the lexer,
//!   the escaper, Twig's own helpers).
//!
//! Where a frameless variant of a builtin (PHP 8.4+, used for direct calls with a fixed argument
//! count, also from namespaced code such as Twig's `CoreExtension`) parses its arguments
//! differently, a separate `*_frameless` function exists ([`implode_frameless`]).
//!
//! # Exceptions in the middle of a builtin
//!
//! Some C loops keep running after a conversion threw (for example `strtr()` converting a
//! replacement object without `__toString()`). Once an exception is pending PHP calls no more
//! user code, and diagnostics raised meanwhile are dropped when a user error handler is installed
//! (the handler is not called while an exception is pending). Every diagnostics policy of twig-rs
//! behaves like such a handler, so these ports return the first exception immediately; the
//! observable result is the same.

#![warn(missing_docs)]

pub mod basic;
pub mod charset_basic;
pub mod ctype;
pub mod html;
#[rustfmt::skip]
mod html_entities;
pub mod iconv;
pub mod levenshtein;
pub mod markup;
pub mod replace;
pub mod split;
pub mod trim;
pub mod url;

pub use basic::{
    chr, lcfirst, ord, str_contains, str_ends_with, str_repeat, str_starts_with, strcasecmp,
    strcmp, strlen, strncasecmp, strpos, strrpos, strtolower, strtoupper, substr, substr_count,
    ucfirst, ucwords,
};
pub use ctype::{
    ctype_alnum, ctype_alpha, ctype_cntrl, ctype_digit, ctype_graph, ctype_lower, ctype_print,
    ctype_punct, ctype_space, ctype_upper, ctype_xdigit,
};
pub use html::htmlspecialchars;
pub use iconv::{BasicIconv, IconvBackend, iconv};
pub use levenshtein::levenshtein;
pub use markup::{nl2br, strip_tags};
pub use replace::{str_replace, str_replace_count, strtr};
pub use split::{explode, implode, implode_frameless};
pub use trim::{ltrim, rtrim, trim};
pub use url::{http_build_query, rawurlencode, urlencode};

use crate::convert;
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::object::CastTarget;
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::Value;

/// `zend_parse_arg_str_weak()` (PHPSRC/Zend/zend_API.c) for any value: `Ok(None)` when the value
/// is rejected (arrays, objects whose `cast_object` fails), so that callers parsing a union type
/// (`array|string`) can report their own expected type.
fn str_weak(v: &Value, arg: &Arg<'_>, diag: &mut dyn Diagnostics) -> PhpResult<Option<PhpString>> {
    Ok(match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => {
            arg.null_deprecated("string", diag)?;
            Some(PhpString::empty())
        }
        Value::Object(o) => match o.cast(CastTarget::String, diag)? {
            Some(Value::String(s)) => Some(s),
            Some(other) => Some(convert::to_string(&other, diag)?),
            None => None,
        },
        Value::Array(_) => None,
        scalar => Some(convert::to_string(scalar, diag)?),
    })
}

/// `Z_PARAM_STR` / `Z_PARAM_STRING` for a parameter declared `string $name`.
fn str_param(
    v: &Value,
    function: &str,
    position: u32,
    name: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    param::parse_str(
        v,
        &Arg::new(function, position, name).declared("string"),
        diag,
    )
}

/// `Z_PARAM_LONG` for a parameter declared `int $name`.
fn long_param(
    v: &Value,
    function: &str,
    position: u32,
    name: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i64> {
    param::parse_long(v, &Arg::new(function, position, name).declared("int"), diag)
}

/// `Z_PARAM_LONG_OR_NULL` for a parameter declared `?int $name = null`.
fn long_or_null_param(
    v: Option<&Value>,
    function: &str,
    position: u32,
    name: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<i64>> {
    match v {
        None => Ok(None),
        Some(v) => param::parse_long_or_null(
            v,
            &Arg::new(function, position, name).declared("?int"),
            diag,
        ),
    }
}

/// `Z_PARAM_BOOL` for a parameter declared `bool $name`.
fn bool_param(
    v: &Value,
    function: &str,
    position: u32,
    name: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<bool> {
    param::parse_bool(
        v,
        &Arg::new(function, position, name).declared("bool"),
        diag,
    )
}

/// Port of `zend_memnstr()` (PHPSRC/Zend/zend_operators.h): the first occurrence of `needle` in
/// `haystack` (an empty needle matches at 0).
fn memnstr(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    match needle.len() {
        0 => Some(0),
        1 => memchr::memchr(needle[0], haystack),
        _ => memchr::memmem::find(haystack, needle),
    }
}

/// Replaces every non-overlapping occurrence of `needle` (non-empty) from left to right, the
/// common result of `php_char_to_str_ex()` and `php_str_to_str_ex()` (PHPSRC/ext/standard/string.c).
/// Returns the new string (the input itself when nothing matched) and the number of replacements.
fn replace_all(haystack: &PhpString, needle: &[u8], replacement: &[u8]) -> (PhpString, i64) {
    debug_assert!(!needle.is_empty());
    let bytes = haystack.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut count = 0;
    let mut pos = 0;
    while let Some(found) = memnstr(&bytes[pos..], needle) {
        if count == 0 {
            out.reserve(bytes.len());
        }
        out.extend_from_slice(&bytes[pos..pos + found]);
        out.extend_from_slice(replacement);
        pos += found + needle.len();
        count += 1;
    }
    if count == 0 {
        return (haystack.clone(), 0);
    }
    out.extend_from_slice(&bytes[pos..]);
    (PhpString::from(out), count)
}
