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
mod escaper;
mod mb;
mod split;
#[cfg(test)]
mod test_vectors;
mod text;

// The public API of the escaper runtime, for `extension::runtime` to re-export.
#[allow(unused_imports)]
pub use escaper::{EscaperCallback, EscaperRuntime};

use twig_value::object::{FunctionKind, Param};
use twig_value::types::zend_zval_value_name;
use twig_value::{Diagnostics, PhpError, PhpString, Value, convert};

use super::{Call, CallResult, RuntimeTable};

/// The class of Twig's core callables.
const CORE: &str = "Twig\\Extension\\CoreExtension";

/// Registers the runtimes of the string group.
pub(crate) fn register(table: &mut RuntimeTable) {
    table.register("Twig\\Runtime\\EscaperRuntime::escape", escaper::escape);
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

/// The declared return type `string` of a userland method in coercive mode (`zend_verify_return_type()`,
/// PHPSRC/Zend/zend_execute.c): strings pass, ints, floats and bools are converted, `Stringable` objects are cast;
/// anything else throws `TypeError: X(): Return value must be of type string, Y returned`.
fn return_string(function: &str, value: Value, diag: &mut dyn Diagnostics) -> CallResult {
    match value {
        Value::String(_) => Ok(value),
        Value::Int(_) | Value::Float(_) | Value::Bool(_) => {
            Ok(Value::String(convert::to_string(&value, diag)?))
        }
        Value::Object(object) if object.instance_of("Stringable") => {
            Ok(Value::String(object.to_php_string(diag)?))
        }
        other => Err(PhpError::type_error(format!(
            "{function}(): Return value must be of type string, {} returned",
            zend_zval_value_name(&other)
        ))
        .into()),
    }
}
