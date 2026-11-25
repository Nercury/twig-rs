//! The `dump` function of the `DebugExtension` (TWIG/src/Extension/DebugExtension.php): `var_dump()` captured into
//! a string.

use twig_value::{Array, PhpError, PhpString, Value, dump};

use crate::extension::runtime::{Call, CallResult};

/// Twig: `DebugExtension::dump(Environment $env, $context, ...$vars)`: nothing (`null`) unless the environment is in
/// debug mode; else the output of `var_dump(...$vars)`, or, without arguments, of `var_dump()` of the context minus
/// its `Twig\Template` and `Twig\TemplateWrapper` values.
///
/// The template's named arguments reach `var_dump()` as named arguments, with PHP's rules for an internal variadic
/// function: `value` fills its first parameter (`Named parameter $value overwrites previous argument` after a
/// positional argument), others are `var_dump() does not accept unknown named parameters`, checked after the
/// argument count (`var_dump() expects at least 1 argument, 0 given`).
///
/// The output is HTML-safe (`is_safe: ['html']`), as Twig decides under the CLI SAPI of the target.
pub(super) fn dump(call: &mut Call<'_>) -> CallResult {
    if !call.env().is_debug() {
        return Ok(Value::Null);
    }
    let mut out = Vec::new();
    if call.args().is_empty() && call.named_args().is_empty() {
        let mut vars = Array::new();
        for (key, value) in call.context() {
            let template = matches!(
                &value,
                Value::Object(o) if o.instance_of("Twig\\Template") || o.instance_of("Twig\\TemplateWrapper")
            );
            if !template {
                vars.insert(key, value);
            }
        }
        dump::var_dump_into(&mut out, &Value::Array(vars), call.diagnostics())?;
        return Ok(Value::String(PhpString::from(out)));
    }
    // `var_dump(...$vars)`: zend_handle_named_arg() binds `value`, collects the other names as extra named
    // parameters; ZEND_PARSE_PARAMETERS_START(1, -1) then Z_PARAM_VARIADIC('+') check them
    let mut values: Vec<Value> = call.args().to_vec();
    let mut extra_named = false;
    for (name, value) in call.named_args() {
        if name.as_bytes() == b"value" {
            if !values.is_empty() {
                return Err(
                    PhpError::error("Named parameter $value overwrites previous argument").into(),
                );
            }
            values.push(value.clone());
        } else {
            extra_named = true;
        }
    }
    if values.is_empty() {
        return Err(PhpError::argument_count_error(
            "var_dump() expects at least 1 argument, 0 given",
        )
        .into());
    }
    if extra_named {
        return Err(PhpError::argument_count_error(
            "var_dump() does not accept unknown named parameters",
        )
        .into());
    }
    for value in &values {
        dump::var_dump_into(&mut out, value, call.diagnostics())?;
    }
    Ok(Value::String(PhpString::from(out)))
}
