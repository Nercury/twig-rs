//! `upper`, `lower`, `title` and `capitalize`: Twig's mbstring wrappers (TWIG/src/Extension/CoreExtension.php).

use twig_unicode::CaseMode;
use twig_value::object::Param;
use twig_value::param::{self, Arg};
use twig_value::{Diagnostics, PhpString, Value};

use super::mb;
use super::{Call, CallResult, bind_user, or_empty};

/// The parameters of a `CoreExtension` method `(string $charset, $string)`.
fn charset_and_string() -> [Param; 2] {
    [Param::required("charset"), Param::required("string")]
}

/// Binds `(string $charset, $string)` and returns the charset and `$string ?? ''`.
fn bind_charset_string(call: &mut Call<'_>, function: &str) -> CallResult<(PhpString, Value)> {
    if call.named_args().is_empty() && !call.args().is_empty() {
        // the binding is trivial (see `Call::bind_trivially()`): the charset, then the first argument
        let string = std::mem::take(&mut call.args_mut()[0]);
        return Ok((call.charset(), or_empty(string)));
    }
    let charset = Value::String(call.charset());
    let mut args = bind_user(call, function, vec![charset], &charset_and_string())?.into_iter();
    let charset = match args.next() {
        Some(Value::String(charset)) => charset,
        _ => call.charset(),
    };
    Ok((charset, or_empty(args.next().unwrap_or_default())))
}

/// The `string $string` parameter of an mbstring function (argument #1).
fn mb_string(function: &str, value: &Value, diag: &mut dyn Diagnostics) -> CallResult<PhpString> {
    Ok(param::parse_str(
        value,
        &Arg::new(function, 1, "string").declared("string"),
        diag,
    )?)
}

/// `mb_strtoupper($string, $encoding)` (`upper` = true) or `mb_strtolower()`: the string parameter, then the
/// encoding (argument #2), then the full case mapping.
///
/// Port of `PHP_FUNCTION(mb_strtoupper)` / `PHP_FUNCTION(mb_strtolower)` (PHPSRC/ext/mbstring/mbstring.c).
fn mb_case(
    value: &Value,
    charset: &[u8],
    upper: bool,
    diag: &mut dyn Diagnostics,
) -> CallResult<PhpString> {
    let function = if upper {
        "mb_strtoupper"
    } else {
        "mb_strtolower"
    };
    let string = mb_string(function, value, diag)?;
    let encoding = mb::get_encoding(function, 2, charset, diag)?;
    let mode = if upper {
        CaseMode::Upper
    } else {
        CaseMode::Lower
    };
    mb::convert_case(function, charset, mode, &string, encoding)
}

/// `mb_substr($string, $start, $length, $encoding)` with constant `$start`/`$length`: the string parameter, the
/// encoding (argument #4), then the characters.
///
/// Port of `PHP_FUNCTION(mb_substr)` (PHPSRC/ext/mbstring/mbstring.c).
fn mb_substr_const(
    value: &Value,
    start: i64,
    length: Option<i64>,
    charset: &[u8],
    diag: &mut dyn Diagnostics,
) -> CallResult<Value> {
    let string = mb_string("mb_substr", value, diag)?;
    let charset = mb::get_encoding("mb_substr", 4, charset, diag)?;
    Ok(Value::String(mb::substr(&string, start, length, charset)?))
}

/// Twig: `CoreExtension::upper(string $charset, $string): string` — `mb_strtoupper($string ?? '', $charset)`.
pub(super) fn upper(call: &mut Call<'_>) -> CallResult {
    let (charset, string) = bind_charset_string(call, "Twig\\Extension\\CoreExtension::upper")?;
    Ok(Value::String(mb_case(
        &string,
        &charset,
        true,
        call.diagnostics(),
    )?))
}

/// Twig: `CoreExtension::lower(string $charset, $string): string` — `mb_strtolower($string ?? '', $charset)`.
pub(super) fn lower(call: &mut Call<'_>) -> CallResult {
    let (charset, string) = bind_charset_string(call, "Twig\\Extension\\CoreExtension::lower")?;
    Ok(Value::String(mb_case(
        &string,
        &charset,
        false,
        call.diagnostics(),
    )?))
}

/// Twig: `CoreExtension::titleCase(string $charset, $string): string` — `mb_convert_case($string ?? '',
/// \MB_CASE_TITLE, $charset)`: the string parameter, the encoding (argument #3), then the title-case mapping.
pub(super) fn title_case(call: &mut Call<'_>) -> CallResult {
    let (charset, string) = bind_charset_string(call, "Twig\\Extension\\CoreExtension::titleCase")?;
    let diag = call.diagnostics();
    let string = mb_string("mb_convert_case", &string, diag)?;
    let encoding = mb::get_encoding("mb_convert_case", 3, &charset, diag)?;
    Ok(Value::String(mb::convert_case(
        "mb_convert_case",
        &charset,
        CaseMode::Title,
        &string,
        encoding,
    )?))
}

/// Twig: `CoreExtension::capitalize(string $charset, $string): string` —
/// `mb_strtoupper(mb_substr($string ?? '', 0, 1, $charset), $charset).mb_strtolower(mb_substr($string ?? '', 1,
/// null, $charset), $charset)`, evaluated left to right: an object is converted to a string by each `mb_substr()`.
pub(super) fn capitalize(call: &mut Call<'_>) -> CallResult {
    let (charset, string) =
        bind_charset_string(call, "Twig\\Extension\\CoreExtension::capitalize")?;
    let diag = call.diagnostics();
    let first = mb_substr_const(&string, 0, Some(1), &charset, diag)?;
    let first = mb_case(&first, &charset, true, diag)?;
    let rest = mb_substr_const(&string, 1, None, &charset, diag)?;
    let rest = mb_case(&rest, &charset, false, diag)?;
    Ok(Value::String(PhpString::concat(&[&first, &rest])))
}
