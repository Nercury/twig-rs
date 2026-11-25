//! The miscellaneous group: numbers, dates, constants and enums, `default`, `dump` and `template_from_string`.
//!
//! Callables (TWIG/src/Extension/CoreExtension.php unless noted):
//!
//! | callable | PHP callable | module |
//! |---|---|---|
//! | filter `abs` | `abs` | [`numbers`] |
//! | filter `round` | `CoreExtension::round` | [`numbers`] |
//! | filter `number_format` | `CoreExtension::formatNumber` | [`numbers`] |
//! | filter `date` | `CoreExtension::formatDate` | [`dates`] |
//! | filter `date_modify` | `CoreExtension::modifyDate` | [`dates`] |
//! | function `date` | `CoreExtension::convertDate` | [`dates`] |
//! | function `constant` (and `constant(...) is defined`) | `CoreExtension::constant` | [`constants`] |
//! | functions `enum`, `enum_cases` | `CoreExtension::enum`, `CoreExtension::enumCases` | [`constants`] |
//! | filter `default` | `CoreExtension::default` | here |
//! | function `dump` | `DebugExtension::dump` (TWIG/src/Extension/DebugExtension.php) | [`debug`] |
//! | function `template_from_string` | `StringLoaderExtension::templateFromString` | [`string_loader`] |
//!
//! The date, date-modify and number filters read the configuration of the `CoreExtension`
//! ([`CoreSettings`](settings::CoreSettings)). The tests `even`, `odd`, `divisible by`, `same as`, `null`/`none`,
//! `constant`, `true` and `defined` have node classes that compile to inline code (the code generator). The `random`
//! function belongs to the array group (it shares PHP's generator with `shuffle`); the functions that render other
//! templates (`include`, `include_only`, `source`, `render_sandboxed`) live with the template runtime
//! (`crate::runtime::functions`).

pub(crate) mod constants;
mod dates;
mod debug;
mod numbers;
mod php_constants;
pub(crate) mod settings;
mod string_loader;
#[cfg(test)]
mod tests;

use twig_value::object::{CallArgs, FunctionKind, Param};
use twig_value::types::zend_zval_value_name;
use twig_value::{Diagnostics, PhpError, Value, convert};

use super::{Call, CallResult, RuntimeTable};
use crate::error::Error;
use crate::runtime::core::test_empty;
use crate::vm::Exception;

/// The class of the core extension, the prefix of the reflection names of its callables.
const CORE: &str = "Twig\\Extension\\CoreExtension";

/// Registers the runtimes of the misc group.
pub(crate) fn register(table: &mut RuntimeTable) {
    table.register("Twig\\Extension\\CoreExtension::default", default);
    table.register("abs", numbers::abs);
    table.register("Twig\\Extension\\CoreExtension::round", numbers::round);
    table.register(
        "Twig\\Extension\\CoreExtension::formatNumber",
        numbers::format_number,
    );
    table.register(
        "Twig\\Extension\\CoreExtension::formatDate",
        dates::format_date_runtime,
    );
    table.register(
        "Twig\\Extension\\CoreExtension::convertDate",
        dates::convert_date_runtime,
    );
    table.register(
        "Twig\\Extension\\CoreExtension::modifyDate",
        dates::modify_date_runtime,
    );
    table.register(
        "Twig\\Extension\\CoreExtension::constant",
        constants::constant,
    );
    table.register("Twig\\Extension\\CoreExtension::enum", constants::enum_case);
    table.register(
        "Twig\\Extension\\CoreExtension::enumCases",
        constants::enum_cases,
    );
    table.register("Twig\\Extension\\DebugExtension::dump", debug::dump);
    table.register(
        "Twig\\Extension\\StringLoaderExtension::templateFromString",
        string_loader::template_from_string,
    );
}

/// Binds the arguments of a call to the parameters of its PHP callable with PHP's rules (named arguments of spreads,
/// defaults; an internal function rejects extra positional arguments, a userland method ignores them).
///
/// `injected` is the number of arguments Twig passes before the template's (charset, environment, context): they
/// take the first parameters, so a named argument of the same name overwrites them like in PHP, and they are not
/// returned.
///
/// # Errors
///
/// PHP's binding errors (`Unknown named parameter $x`, `Named parameter $x overwrites previous argument`,
/// `abs() expects exactly 1 argument, 2 given`, `Too few arguments to function ...`).
fn bind(
    call: &Call<'_>,
    function: &str,
    injected: usize,
    params: &[Param],
    kind: FunctionKind,
) -> CallResult<Vec<Value>> {
    let mut positional = vec![Value::Null; injected];
    positional.extend(call.args().iter().cloned());
    let args = CallArgs {
        positional,
        named: call.named_args().to_vec(),
    };
    let mut values = args.bind(function, params, kind)?;
    values.drain(..injected.min(values.len()));
    Ok(values)
}

/// An exception of a Twig method that `twig-value` ports (`Twig\Error\RuntimeError` raised by
/// `builtins::format::twig_round`) as the Twig error; any other PHP throwable as it is.
fn twig_error(error: PhpError) -> Exception {
    if error.class().name() == "Twig\\Error\\RuntimeError" {
        return Error::runtime(error.message_lossy().into_owned()).into();
    }
    error.into()
}

/// The `string` return type of a userland method in coercive mode: scalars become strings, a `Stringable` object its
/// string, anything else is `Return value must be of type string, %s returned`.
fn return_string(function: &str, value: Value, diag: &mut dyn Diagnostics) -> CallResult {
    match &value {
        Value::String(_) => Ok(value),
        Value::Int(_) | Value::Float(_) | Value::Bool(_) => {
            Ok(Value::String(convert::to_string(&value, diag)?))
        }
        Value::Object(object) if object.instance_of("Stringable") => {
            Ok(Value::String(object.to_php_string(diag)?))
        }
        other => Err(PhpError::type_error(format!(
            "{function}(): Return value must be of type string, {} returned",
            zend_zval_value_name(other)
        ))
        .into()),
    }
}

/// `$value->method(...$args)` on a value that should be an object: `Call to a member function %s() on %s` otherwise.
fn call_method_on(
    value: &Value,
    method: &str,
    args: Vec<Value>,
    diag: &mut dyn Diagnostics,
) -> CallResult {
    match value {
        Value::Object(object) => Ok(object.call_method(method, CallArgs::from(args), diag)?),
        other => Err(PhpError::error(format!(
            "Call to a member function {method}() on {}",
            zend_zval_value_name(other)
        ))
        .into()),
    }
}

/// Twig: `CoreExtension::default($value, $default = '')`: `$default` when `testEmpty($value)`.
fn default(call: &mut Call<'_>) -> CallResult {
    let args = bind(
        call,
        "Twig\\Extension\\CoreExtension::default",
        0,
        &[Param::required("value"), Param::optional("default", "")],
        FunctionKind::User,
    )?;
    let mut args = args.into_iter();
    let value = args.next().unwrap_or_default();
    let default = args.next().unwrap_or_default();
    if test_empty(&value, call.diagnostics())? {
        Ok(default)
    } else {
        Ok(value)
    }
}
