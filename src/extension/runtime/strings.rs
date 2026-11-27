//! The string group: the string filters of `CoreExtension` and the escaping filters of `EscaperExtension`.
//!
//! | Twig callable | PHP callable | module |
//! |---|---|---|
//! | `escape`, `e` | `Twig\Runtime\EscaperRuntime::escape` | [`escaper`] ([`EscaperRuntime`]) |
//! | `upper`, `lower`, `title`, `capitalize` | `CoreExtension::upper`, `lower`, `titleCase`, `capitalize` | [`case`] |
//! | `trim`, `nl2br`, `spaceless`, `striptags`, `replace`, `format`, `url_encode`, `json_encode`, `convert_encoding` | `CoreExtension::trim`, `nl2br`, `spaceless`, `striptags`, `replace`, `sprintf`, `urlencode`, PHP's `json_encode`, `CoreExtension::convertEncoding` | [`text`] |
//! | `split`, `join` | `CoreExtension::split`, `join` | [`split`] |
//!
//! `raw` has no runtime: `RawFilter` compiles to its input. The `'name'` default strategy
//! (`FileExtensionEscapingStrategy::guess()`) is [`crate::escaper::guess`], resolved by the `EscaperExtension` when a
//! template is compiled.
//!
//! Every runtime is a port of the Twig method (TWIG/src/Extension/CoreExtension.php,
//! TWIG/src/Runtime/EscaperRuntime.php) with its PHP semantics: the arguments are bound like PHP binds them to the
//! method (injected arguments first, named arguments of spreads, PHP's `ArgumentCountError`s), Twig's own checks run
//! in Twig's order, the PHP builtins come from `twig-value` (coercive parameter parsing, verbatim diagnostics), and
//! the declared return type coerces the result. The mbstring functions support UTF-8 and the single-byte encodings
//! that need no character table ([`mb`]).

mod case;
pub(crate) mod escaper;
pub(crate) mod mb;
mod split;
#[cfg(test)]
mod test_vectors;
mod text;

pub use escaper::{EscaperCallback, EscaperRuntime};

use twig_value::object::{FunctionKind, Param};
use twig_value::{PhpString, Value};

use super::{Call, CallResult, RuntimeTable};

/// Registers the runtimes of the string group.
pub(crate) fn register(table: &mut RuntimeTable) {
    table.register_runtime(
        "Twig\\Runtime\\EscaperRuntime::escape",
        escaper::ESCAPE_RUNTIME.clone(),
    );
    table.register("Twig\\Extension\\CoreExtension::upper", case::upper);
    table.register("Twig\\Extension\\CoreExtension::lower", case::lower);
    table.register(
        "Twig\\Extension\\CoreExtension::titleCase",
        case::title_case,
    );
    table.register(
        "Twig\\Extension\\CoreExtension::capitalize",
        case::capitalize,
    );
    table.register("Twig\\Extension\\CoreExtension::trim", text::trim);
    table.register("Twig\\Extension\\CoreExtension::nl2br", text::nl2br);
    table.register("Twig\\Extension\\CoreExtension::spaceless", text::spaceless);
    table.register("Twig\\Extension\\CoreExtension::striptags", text::striptags);
    table.register("Twig\\Extension\\CoreExtension::replace", text::replace);
    table.register("Twig\\Extension\\CoreExtension::sprintf", text::sprintf);
    table.register("Twig\\Extension\\CoreExtension::urlencode", text::urlencode);
    table.register("json_encode", text::json_encode);
    table.register(
        "Twig\\Extension\\CoreExtension::convertEncoding",
        text::convert_encoding,
    );
    table.register("Twig\\Extension\\CoreExtension::split", split::split);
    table.register("Twig\\Extension\\CoreExtension::join", split::join);
}

/// Binds the arguments of a call to the parameters of a userland PHP method the way PHP does when the compiled
/// template calls it: `injected` (Twig's `needs_charset` & co., passed first) and the call's positional arguments
/// fill the parameters in order, the named arguments of spreads (`f(...{name: v})`) go by name, missing optional
/// parameters get their defaults, extra positional arguments are ignored. Returns one value per parameter
/// (injected ones included).
///
/// # Errors
///
/// PHP's binding errors: `Named parameter $x overwrites previous argument`, `Unknown named parameter $x`, `Too few
/// arguments to function X(), N passed and exactly M expected` (without PHP's ` in <file> on line <n>`, see
/// reference/deviations.md).
fn bind_user(
    call: &mut Call<'_>,
    function: &str,
    injected: Vec<Value>,
    params: &[Param],
) -> CallResult<Vec<Value>> {
    // without named arguments and with the required ones, the values are the arguments in order (see
    // `Call::bind_trivially()`)
    let rest = params.get(injected.len()..).unwrap_or_default();
    let required = rest
        .iter()
        .rposition(|param| param.default.is_none())
        .map_or(0, |i| i + 1);
    if call.named_args().is_empty()
        && !params.iter().any(|param| param.variadic)
        && call.args().len() >= required
        && injected.len() <= params.len()
    {
        let mut values = injected;
        values.reserve(rest.len());
        let args = call.args_mut();
        for (i, param) in rest.iter().enumerate() {
            values.push(match args.get_mut(i) {
                Some(value) => std::mem::take(value),
                None => param.default.clone().unwrap_or_default(),
            });
        }
        return Ok(values);
    }
    let mut args = call.take_args();
    if !injected.is_empty() {
        args.positional.splice(0..0, injected);
    }
    Ok(args.bind(function, params, FunctionKind::User)?)
}

/// `$value ?? ''`: `null` becomes the empty string, anything else is kept as is.
fn or_empty(value: Value) -> Value {
    if value.is_null() {
        Value::String(PhpString::empty())
    } else {
        value
    }
}

/// The declared return type `string` of a userland method in coercive mode
/// ([`verify_return_string`](super::verify_return_string)).
use super::verify_return_string as return_string;
