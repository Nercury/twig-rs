//! Arrow functions: PHP closures that snapshot the context and the macros where they are created.

use std::sync::Arc;

use twig_value::object::{CallArgs, Closure};
use twig_value::{Array, ArrayKey, Diagnostics, PhpError, PhpResult, PhpString, Value};

use super::exception::Exception;
use super::frame::Frame;
use super::interp::{arrow_params, run_unit};
use super::output::Output;
use super::state::VmState;
use crate::lltl::{Program, UnitId};
use crate::runtime::Template;

/// What an arrow-function closure captured at creation: `function (...) use ($context, $macros) { ... }` defined in
/// the method of `template` (its `$this`).
struct ArrowData {
    program: Arc<Program>,
    unit: UnitId,
    context: Array,
    macros: Array,
    template: Arc<Template>,
}

/// Twig: `ArrowFunctionExpression::compile()`: a `Closure` object whose call runs the arrow unit with a copy of the
/// creation context in which the parameters are assigned.
pub(crate) fn make_arrow(
    program: &Arc<Program>,
    unit: UnitId,
    context: Array,
    macros: Array,
    template: Arc<Template>,
) -> Value {
    let data = Arc::new(ArrowData {
        program: Arc::clone(program),
        unit,
        context,
        macros,
        template,
    });
    let closure = Closure::named(
        "{closure}",
        move |args: CallArgs, diag: &mut dyn Diagnostics| -> PhpResult<Value> {
            let Some(host) = diag.host() else {
                return Err(not_rendering());
            };
            let Some(state) = host.downcast_mut::<VmState>() else {
                return Err(not_rendering());
            };
            call_arrow(state, &data, args).map_err(Exception::into_php_error)
        },
    );
    Value::Object(closure.into_ref())
}

fn not_rendering() -> PhpError {
    PhpError::error("A Twig arrow function can only be called while a template is rendering.")
}

/// Calls an arrow function: PHP binds the arguments to the parameters `$__name__` (a userland function: missing
/// arguments throw, extra ones are ignored), then the body runs on a copy of the captured context with
/// `$context["name"] = $__name__` for each parameter.
fn call_arrow(state: &mut VmState, data: &ArrowData, args: CallArgs) -> Result<Value, Exception> {
    let params = arrow_params(&data.program, data.unit);
    let mut values: Vec<Option<Value>> = vec![None; params.len()];
    for (i, value) in args.positional.into_iter().enumerate() {
        if let Some(slot) = values.get_mut(i) {
            *slot = Some(value);
        }
    }
    for (name, value) in args.named {
        let position = params.iter().position(|param| {
            let mut php_name = b"__".to_vec();
            php_name.extend_from_slice(param.as_bytes());
            php_name.extend_from_slice(b"__");
            php_name == name.as_bytes()
        });
        match position {
            Some(i) if values[i].is_none() => values[i] = Some(value),
            Some(_) => {
                return Err(PhpError::error(PhpString::concat(&[
                    b"Named parameter $",
                    name.as_bytes(),
                    b" overwrites previous argument",
                ]))
                .into());
            }
            None => {
                return Err(PhpError::error(PhpString::concat(&[
                    b"Unknown named parameter $",
                    name.as_bytes(),
                ]))
                .into());
            }
        }
    }
    let passed = values.iter().filter(|v| v.is_some()).count();
    if passed < params.len() {
        // PHP names the closure after the compiled template class and adds the calling file and line, which have no
        // equivalent here (reference/deviations.md).
        return Err(PhpError::argument_count_error(format!(
            "Too few arguments to function {{closure}}(), {passed} passed and exactly {} expected",
            params.len()
        ))
        .into());
    }
    let mut context = data.context.clone();
    for (param, value) in params.iter().zip(values) {
        context.insert(
            ArrayKey::from_string(param.clone()),
            value.unwrap_or_default(),
        );
    }
    let mut frame = Frame::arrow(context, Arc::clone(&data.template), data.macros.clone());
    let mut out = Output::detached();
    run_unit(state, &mut out, &mut frame, &data.program, data.unit)
}

/// Calls a callable value from a filter or operator runtime (`$arrow($v, $k)`): closures and invokable objects.
///
/// # Errors
///
/// The exception the callable throws; `Error: Object of type X is not callable` for other objects, `Error: Value
/// not callable` for other values.
pub(crate) fn invoke_value(
    state: &mut VmState,
    callable: &Value,
    args: Vec<Value>,
) -> Result<Value, Exception> {
    match callable {
        Value::Object(object) => Ok(object.invoke(CallArgs::from(args), state)?),
        // ZEND_INIT_DYNAMIC_CALL; twig-rs has no table of PHP functions
        Value::String(name) => Err(PhpError::error(PhpString::concat(&[
            b"Call to undefined function ",
            name.as_bytes(),
            b"()",
        ]))
        .into()),
        Value::Array(array) => {
            if array.len() != 2 {
                return Err(
                    PhpError::error("Array callback must have exactly two elements").into(),
                );
            }
            match (array.get_int(0), array.get_int(1)) {
                (Some(Value::Object(object)), Some(Value::String(method))) => Ok(
                    object.call_method(&method.to_string_lossy(), CallArgs::from(args), state)?
                ),
                _ => Err(PhpError::error("Array callback has to contain indices 0 and 1").into()),
            }
        }
        other => Err(PhpError::error(format!(
            "Value of type {} is not callable",
            twig_value::types::zend_zval_value_name(other)
        ))
        .into()),
    }
}
