//! The number filters: `abs` (PHP's `abs()`), `round` (`CoreExtension::round()`) and `number_format`
//! (`CoreExtension::formatNumber()`).

use twig_value::builtins::format;
use twig_value::object::{FunctionKind, Param};
use twig_value::{Diagnostics, Value, convert};

use super::settings::{CoreSettings, core_settings};
use super::{bind, twig_error};
use crate::extension::runtime::{Call, CallResult};

/// PHP's `abs(int|float $num): int|float` (PHPSRC/ext/standard/math.c), registered as the bare PHP function: an
/// internal function, so a second argument is `abs() expects exactly 1 argument, 2 given`.
pub(super) fn abs(call: &mut Call<'_>) -> CallResult {
    if call.named_args().is_empty() {
        let args = call.args().to_vec();
        return Ok(format::abs_call(&args, call.diagnostics())?);
    }
    let args = bind(
        call,
        "abs",
        0,
        &[Param::required("num")],
        FunctionKind::Internal,
    )?;
    Ok(Value::from(format::abs(&args[0], call.diagnostics())?))
}

/// Twig: `CoreExtension::round($value, $precision = 0, $method = 'common')`: `round((float) $value, $precision)`, or
/// `ceil`/`floor` of `$value * 10 ** $precision` divided by `10 ** $precision`; any other method is `The "round"
/// filter only supports the "common", "ceil", and "floor" methods.` (`twig-value`'s
/// [`twig_round`](format::twig_round)).
pub(super) fn round(call: &mut Call<'_>) -> CallResult {
    let args = bind(
        call,
        "Twig\\Extension\\CoreExtension::round",
        0,
        &[
            Param::required("value"),
            Param::optional("precision", 0),
            Param::optional("method", "common"),
        ],
        FunctionKind::User,
    )?;
    format::twig_round(&args[0], &args[1], &args[2], call.diagnostics()).map_err(twig_error)
}

/// Twig: `CoreExtension::formatNumber($number, $decimal = null, $decimalPoint = null, $thousandSep = null): string`,
/// the `number_format` filter: the `null` arguments take the defaults of `setNumberFormat()`, then
/// `number_format((float) $number, $decimal, $decimalPoint, $thousandSep)`.
pub(super) fn format_number(call: &mut Call<'_>) -> CallResult {
    let args = bind(
        call,
        "Twig\\Extension\\CoreExtension::formatNumber",
        0,
        &[
            Param::required("number"),
            Param::optional("decimal", Value::Null),
            Param::optional("decimalPoint", Value::Null),
            Param::optional("thousandSep", Value::Null),
        ],
        FunctionKind::User,
    )?;
    let settings = core_settings(call.env());
    number_format(
        &settings,
        &args[0],
        &args[1],
        &args[2],
        &args[3],
        call.diagnostics(),
    )
}

/// The body of `CoreExtension::formatNumber()` for the given settings.
pub(super) fn number_format(
    settings: &CoreSettings,
    number: &Value,
    decimal: &Value,
    decimal_point: &Value,
    thousand_sep: &Value,
    diag: &mut dyn Diagnostics,
) -> CallResult {
    let [default_decimal, default_point, default_sep] = settings.number_format();
    let pick = |value: &Value, default: Value| {
        if value.is_null() {
            default
        } else {
            value.clone()
        }
    };
    let decimal = pick(decimal, default_decimal);
    let decimal_point = pick(decimal_point, default_point);
    let thousand_sep = pick(thousand_sep, default_sep);
    let number = Value::Float(convert::to_double(number, diag)?);
    Ok(Value::String(format::number_format(
        &number,
        Some(&decimal),
        Some(&decimal_point),
        Some(&thousand_sep),
        diag,
    )?))
}
