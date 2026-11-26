//! `trim`, `nl2br`, `spaceless`, `striptags`, `replace`, `format`, `url_encode`, `json_encode` and
//! `convert_encoding` (TWIG/src/Extension/CoreExtension.php).

use twig_value::builtins::{format, string};
use twig_value::object::{CallArgs, FunctionKind, Param};
use twig_value::param::{self, Arg};
use twig_value::types::get_debug_type;
use twig_value::{Array, ArrayKey, PhpError, PhpString, Value, convert};

use super::{Call, CallResult, bind_user, or_empty, return_string};
use crate::error::Error;
use crate::runtime::Markup;
use crate::runtime::core::to_array;

/// Twig: `CoreExtension::DEFAULT_TRIM_CHARS`.
const DEFAULT_TRIM_CHARS: &[u8] = b" \t\n\r\0\x0B";

/// Twig: `CoreExtension::trim($string, $characterMask = null, $side = 'both'): string|\Stringable`.
///
/// `null` masks become [`DEFAULT_TRIM_CHARS`]; `$side` is matched strictly (`match`), so only the strings `both`,
/// `left` and `right` select PHP's `trim()`, `ltrim()` or `rtrim()` (of `$string ?? ''`), anything else throws
/// `RuntimeError('Trimming side must be "left", "right" or "both".')` before the string is looked at. A
/// [`Markup`] trimmed with the default mask stays markup (`withContent()`), keeping its safety.
pub(super) fn trim(call: &mut Call<'_>) -> CallResult {
    let mut args = bind_user(
        call,
        "Twig\\Extension\\CoreExtension::trim",
        Vec::new(),
        &[
            Param::required("string"),
            Param::optional("characterMask", Value::Null),
            Param::optional("side", "both"),
        ],
    )?
    .into_iter();
    let input = args.next().unwrap_or_default();
    let mask = match args.next().unwrap_or_default() {
        Value::Null => Value::String(PhpString::from(DEFAULT_TRIM_CHARS)),
        mask => mask,
    };
    let side = args.next().unwrap_or_default();
    let trim_fn = match side.as_str().map(PhpString::as_bytes) {
        Some(b"both") => string::trim,
        Some(b"left") => string::ltrim,
        Some(b"right") => string::rtrim,
        _ => {
            return Err(
                Error::runtime("Trimming side must be \"left\", \"right\" or \"both\".").into(),
            );
        }
    };
    let diag = call.diagnostics();
    let trimmed = trim_fn(&or_empty(input.clone()), Some(&mask), diag)?;
    // trimming a safe string with the default character mask keeps it as safe as it was
    if let Value::Object(object) = &input
        && Markup::is_markup(object)
        && mask
            .as_str()
            .is_some_and(|mask| mask.as_bytes() == DEFAULT_TRIM_CHARS)
    {
        return Ok(object.call_method(
            "withContent",
            CallArgs::from(vec![Value::String(trimmed)]),
            diag,
        )?);
    }
    Ok(Value::String(trimmed))
}

/// Twig: `CoreExtension::nl2br($string): string` — PHP's `nl2br($string ?? '')` (`<br />` before every line
/// break). The `pre_escape` option makes the escaper visitor escape the input first.
pub(super) fn nl2br(call: &mut Call<'_>) -> CallResult {
    let mut args = bind_user(
        call,
        "Twig\\Extension\\CoreExtension::nl2br",
        Vec::new(),
        &[Param::required("string")],
    )?
    .into_iter();
    let input = or_empty(args.next().unwrap_or_default());
    Ok(Value::String(string::nl2br(
        &input,
        None,
        call.diagnostics(),
    )?))
}

/// Twig: `CoreExtension::spaceless($content): string` — `trim(preg_replace('/>\s+</', '><', $content ?? ''))`.
///
/// The pattern has no `u` modifier, so `\s` is a byte of PCRE's default space class (`\t`, `\n`, `\v`, `\f`, `\r`
/// and space) and the replacement runs left to right without overlaps; PCRE2 auto-possessifies `\s+` before `<`,
/// so neither the backtracking nor the JIT stack limit can be reached and the match never fails.
///
/// `preg_replace()` takes `string|array $subject`: scalars and `Stringable` objects become strings, an array is
/// replaced element by element (each converted with PHP's string conversion) and gives an array, which `trim()`
/// then rejects; other objects throw `preg_replace(): Argument #3 ($subject) must be of type array|string, X given`.
pub(super) fn spaceless(call: &mut Call<'_>) -> CallResult {
    let mut args = bind_user(
        call,
        "Twig\\Extension\\CoreExtension::spaceless",
        Vec::new(),
        &[Param::required("content")],
    )?
    .into_iter();
    let content = or_empty(args.next().unwrap_or_default());
    let diag = call.diagnostics();
    let replaced = match &content {
        Value::Array(subjects) => {
            let mut result = Array::new();
            for (key, subject) in subjects {
                let subject = convert::to_string(subject, diag)?;
                result.insert(key.clone(), Value::String(collapse_tag_spaces(&subject)));
            }
            Value::Array(result)
        }
        other => {
            let arg = Arg::new("preg_replace", 3, "subject").declared("array|string");
            let subject = match other {
                Value::Object(object) => {
                    match object.cast(twig_value::object::CastTarget::String, diag)? {
                        Some(Value::String(subject)) => subject,
                        Some(value) => convert::to_string(&value, diag)?,
                        None => return Err(arg.type_error("of type array|string", other).into()),
                    }
                }
                scalar => param::parse_str(scalar, &arg, diag)?,
            };
            Value::String(collapse_tag_spaces(&subject))
        }
    };
    Ok(Value::String(string::trim(&replaced, None, diag)?))
}

/// `preg_replace('/>\s+</', '><', $subject)` on a string (see [`spaceless`]).
fn collapse_tag_spaces(subject: &[u8]) -> PhpString {
    /// PCRE's default `\s` class (`pcre2_chartables.c`, C locale).
    fn is_space(byte: u8) -> bool {
        matches!(byte, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ')
    }
    let mut out = Vec::with_capacity(subject.len());
    let mut i = 0;
    while i < subject.len() {
        if subject[i] == b'>' {
            let mut j = i + 1;
            while j < subject.len() && is_space(subject[j]) {
                j += 1;
            }
            if j > i + 1 && j < subject.len() && subject[j] == b'<' {
                out.extend_from_slice(b"><");
                i = j + 1;
                continue;
            }
        }
        out.push(subject[i]);
        i += 1;
    }
    PhpString::from(out)
}

/// Twig: `CoreExtension::striptags($string, $allowable_tags = null): string` — PHP's `strip_tags($string ?? '',
/// $allowable_tags)`.
pub(super) fn striptags(call: &mut Call<'_>) -> CallResult {
    let mut args = bind_user(
        call,
        "Twig\\Extension\\CoreExtension::striptags",
        Vec::new(),
        &[
            Param::required("string"),
            Param::optional("allowable_tags", Value::Null),
        ],
    )?
    .into_iter();
    let input = or_empty(args.next().unwrap_or_default());
    let allowed = args.next().unwrap_or_default();
    Ok(Value::String(string::strip_tags(
        &input,
        Some(&allowed),
        call.diagnostics(),
    )?))
}

/// Twig: `CoreExtension::replace($str, $from): string` — `$from` must be iterable (`The "replace" filter expects a
/// sequence or a mapping, got "%s".`), then `strtr($str ?? '', self::toArray($from))`: the pairs are collected
/// (keys preserved) before `strtr()` parses the subject.
pub(super) fn replace(call: &mut Call<'_>) -> CallResult {
    let mut args = bind_user(
        call,
        "Twig\\Extension\\CoreExtension::replace",
        Vec::new(),
        &[Param::required("str"), Param::required("from")],
    )?
    .into_iter();
    let input = args.next().unwrap_or_default();
    let from = args.next().unwrap_or_default();
    if !from.is_iterable() {
        return Err(Error::runtime(format!(
            "The \"replace\" filter expects a sequence or a mapping, got \"{}\".",
            get_debug_type(&from)
        ))
        .into());
    }
    let input = or_empty(input);
    let diag = call.diagnostics();
    let pairs = to_array(from, true, diag)?;
    Ok(Value::String(string::strtr(&input, &pairs, None, diag)?))
}

/// Twig: `CoreExtension::sprintf($format, ...$values): string` — `\sprintf($format ?? '', ...$values)`.
///
/// Named arguments of a spread that are not `$format` land in `$values` under their names; unpacking them into
/// PHP's `sprintf()` fails with `ArgumentCountError: sprintf() does not accept unknown named parameters`.
pub(super) fn sprintf(call: &mut Call<'_>) -> CallResult {
    let mut args = bind_user(
        call,
        "Twig\\Extension\\CoreExtension::sprintf",
        Vec::new(),
        &[Param::required("format"), Param::variadic("values")],
    )?
    .into_iter();
    let format = or_empty(args.next().unwrap_or_default());
    let values = match args.next() {
        Some(Value::Array(values)) => values,
        _ => Array::new(),
    };
    if values.keys().any(|key| matches!(key, ArrayKey::Str(_))) {
        return Err(PhpError::argument_count_error(
            "sprintf() does not accept unknown named parameters",
        )
        .into());
    }
    let values: Vec<Value> = values.values().cloned().collect();
    Ok(Value::String(format::sprintf(
        &format,
        &values,
        call.diagnostics(),
    )?))
}

/// Twig: `CoreExtension::urlencode($url): string` — an array becomes a query string (`http_build_query($url, '',
/// '&', PHP_QUERY_RFC3986)`), anything else goes through `rawurlencode($url ?? '')`.
pub(super) fn urlencode(call: &mut Call<'_>) -> CallResult {
    let mut args = bind_user(
        call,
        "Twig\\Extension\\CoreExtension::urlencode",
        Vec::new(),
        &[Param::required("url")],
    )?
    .into_iter();
    let url = args.next().unwrap_or_default();
    let diag = call.diagnostics();
    if let Value::Array(_) = &url {
        return Ok(Value::String(string::url::build_query(
            &url,
            b"",
            Some(b"&"),
            string::url::PHP_QUERY_RFC3986,
            diag,
        )?));
    }
    Ok(Value::String(string::rawurlencode(&or_empty(url), diag)?))
}

/// PHP's `json_encode(mixed $value, int $flags = 0, int $depth = 512): string|false`, called directly by the
/// compiled template: extra arguments are an `ArgumentCountError`, a failure returns `false`.
pub(super) fn json_encode(call: &mut Call<'_>) -> CallResult {
    if call.named_args().is_empty() {
        let args = call.args().to_vec();
        return Ok(format::json_encode_call(&args, call.diagnostics())?);
    }
    let args = call.bind(
        "json_encode",
        &[
            Param::required("value"),
            Param::optional("flags", 0),
            Param::optional("depth", 512),
        ],
        FunctionKind::Internal,
    )?;
    Ok(format::json_encode(
        &args[0],
        Some(&args[1]),
        Some(&args[2]),
        call.diagnostics(),
    )?)
}

/// Twig: `CoreExtension::convertEncoding($string, $to, $from): string` — `iconv($from, $to, $string ?? '')` with the
/// environment's [`iconv_backend`](crate::runtime::Environment::iconv_backend); a failed conversion returns `false`
/// (after `iconv()`'s warning or notice), which the return type turns into `''`.
pub(super) fn convert_encoding(call: &mut Call<'_>) -> CallResult {
    let function = "Twig\\Extension\\CoreExtension::convertEncoding";
    let mut args = bind_user(
        call,
        function,
        Vec::new(),
        &[
            Param::required("string"),
            Param::required("to"),
            Param::required("from"),
        ],
    )?
    .into_iter();
    let input = or_empty(args.next().unwrap_or_default());
    let to = args.next().unwrap_or_default();
    let from = args.next().unwrap_or_default();
    let iconv = call.env().iconv_backend();
    let diag = call.diagnostics();
    let converted = match iconv.iconv(&from, &to, &input, diag)? {
        Some(converted) => Value::String(converted),
        None => Value::Bool(false),
    };
    return_string(function, converted, diag)
}
