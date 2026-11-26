//! The interpreter loop: executes the instructions of one unit on a frame.

use std::sync::{Arc, LazyLock};

use twig_value::object::FetchMode;
use twig_value::{
    Array, ArrayKey, Diagnostic, Diagnostics, KeyContext, KeyRef, Level, PhpError, PhpString,
    Value, compare, convert, dim, ops,
};

use super::exception::Exception;
use super::frame::{ArgList, Frame, Handler, LoopIter, array_unpack};
use super::output::Output;
use super::state::VmState;
use crate::error::Error;
use crate::extension::runtime::call_callable;
use crate::lltl::{BinaryOp, BlockOp, Instr, NameMode, Program, UnitId, UnitKind, YieldArgs};
use crate::runtime::macros::{self, MacroNamespace};
use crate::runtime::sandbox::{sandbox_end, sandbox_start};
use crate::runtime::{Blocks, Markup, Template, TemplateObject, attribute, composition, core};

/// What an instruction does to the control flow.
enum Flow {
    Next,
    Jump(u32),
    Return(Value),
}

/// Runs unit `unit` of `program` on `frame` and returns what the unit returns (arrow functions: their value;
/// bodies, blocks and macros: `null`, macros leave their result in the returned value as well).
///
/// The unit is pushed on the template stack while it runs; an exception raised by one of its instructions gets the
/// stack as its trace, and the captures it opened are dropped (`captureOutput()` cleans its buffers).
pub(crate) fn run_unit(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    unit: UnitId,
) -> Result<Value, Exception> {
    state.enter(program, unit)?;
    let depth = out.capture_depth();
    let locals = program.unit(unit).locals() as usize;
    if frame.locals.len() < locals {
        frame.locals.resize(locals, None);
    }
    let result = execute(state, out, frame, program, unit);
    if result.is_err() {
        out.truncate_captures(depth);
    }
    state.leave();
    result
}

fn execute(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    unit: UnitId,
) -> Result<Value, Exception> {
    let code = program.unit(unit).code();
    let handlers = frame.handlers.len();
    let mut ip: usize = 0;
    while let Some(instr) = code.get(ip) {
        state.set_ip(ip as u32);
        match step(state, out, frame, program, instr) {
            Ok(Flow::Next) => ip += 1,
            Ok(Flow::Jump(target)) => ip = target as usize,
            Ok(Flow::Return(value)) => {
                frame.handlers.truncate(handlers);
                return Ok(value);
            }
            Err(mut exception) => {
                if !exception.has_trace() {
                    exception.set_trace(state.trace());
                }
                match unwind(state, out, frame, handlers, &exception) {
                    Some(catch) => ip = catch as usize,
                    None => return Err(exception),
                }
            }
        }
    }
    frame.handlers.truncate(handlers);
    Ok(Value::Null)
}

/// Runs the `try` regions the unit opened, innermost first: a `finally` runs and the exception goes on; a
/// `catch (LoaderError $e)` stops a loader error, restores the frame to its state at the start of the region, pushes
/// `null` (`$v = null`) and returns where to continue.
fn unwind(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    base: usize,
    exception: &Exception,
) -> Option<u32> {
    while frame.handlers.len() > base {
        match frame.handlers.pop() {
            Some(Handler::Sandbox { already }) => sandbox_end(&state.env, already),
            Some(Handler::Loader {
                catch,
                stack,
                args,
                iterators,
                saved_macros,
                captures,
            }) => {
                if composition::is_loader_error(exception) {
                    frame.stack.truncate(stack);
                    frame.args.truncate(args);
                    frame.iterators.truncate(iterators);
                    frame.saved_macros.truncate(saved_macros);
                    out.truncate_captures(captures);
                    frame.push(Value::Null);
                    return Some(catch);
                }
            }
            None => break,
        }
    }
    None
}

/// The template of a handle made by `LoadTemplate`/`GetParent`; anything else fails like calling a method on it.
fn handle_template(handle: &Value, method: &str) -> Result<Arc<Template>, Exception> {
    TemplateObject::template_of(handle).ok_or_else(|| {
        PhpError::error(format!(
            "Call to a member function {method}() on {}",
            twig_value::types::zend_zval_type_name(handle)
        ))
        .into()
    })
}

/// PHP's warning for the read of `$blocks` in an arrow function, whose closure does not capture it.
fn undefined_blocks(state: &mut VmState) -> Result<(), Exception> {
    state.report(Diagnostic::new(
        Level::Warning,
        "Undefined variable $blocks",
    ))?;
    Ok(())
}

/// `$blocks` of the running method; in an arrow function the template method rejects the `null` it gets. With
/// `read`, `$blocks` is read here, so PHP warns about the undefined variable first (block methods of `$this` read it
/// in an [`Instr::CheckBlocks`] of their own).
fn frame_blocks(
    state: &mut VmState,
    frame: &Frame,
    method: &str,
    read: bool,
) -> Result<Blocks, Exception> {
    match &frame.blocks {
        Some(blocks) => Ok(blocks.clone()),
        None => {
            if read {
                undefined_blocks(state)?;
            }
            Err(PhpError::type_error(format!(
                "Twig\\Template::{method}(): Argument #3 ($blocks) must be of type array, null given"
            ))
            .into())
        }
    }
}

/// The string of a constant (names, texts).
fn constant_bytes(program: &Program, id: u32) -> &[u8] {
    match program.constant(id) {
        Value::String(s) => s.as_bytes(),
        _ => b"",
    }
}

/// `$array["name"] = $value` with the name constant `id` (normalised like a PHP string offset); the key shares the
/// constant's string.
fn store_constant_key(array: &mut Array, program: &Program, id: u32, value: Value) {
    match program.constant(id) {
        Value::String(name) => array.insert_sym(name, value),
        _ => array.insert_sym(&PhpString::empty(), value),
    };
}

/// The array key of the name constant `id` (normalised like a PHP string offset), sharing the constant's string.
fn constant_key(program: &Program, id: u32) -> ArrayKey {
    match program.constant(id) {
        Value::String(name) => ArrayKey::from_string(name.clone()),
        _ => ArrayKey::Str(PhpString::empty()),
    }
}

/// `echo $value`: PHP's string conversion of the value, written to the output. Strings, ints, `null` and booleans
/// are written without building a string.
fn print(state: &mut VmState, out: &mut Output<'_>, value: &Value) -> Result<(), Exception> {
    match value {
        Value::String(s) => out.write(s),
        Value::Int(i) => out.write(long_digits(*i, &mut [0; 20])),
        Value::Null | Value::Bool(false) => Ok(()),
        Value::Bool(true) => out.write(b"1"),
        _ => {
            let string = convert::to_string(value, state)?;
            out.write(&string)
        }
    }
    .map_err(io_error)
}

/// The decimal digits of `i` (`zend_long_to_str()`), written at the end of `buf`.
fn long_digits(i: i64, buf: &mut [u8; 20]) -> &[u8] {
    let mut n = i.unsigned_abs();
    let mut at = buf.len();
    loop {
        at -= 1;
        buf[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if i < 0 {
        at -= 1;
        buf[at] = b'-';
    }
    &buf[at..]
}

fn io_error(error: std::io::Error) -> Exception {
    Error::runtime(format!("Unable to write the template output: {error}.")).into()
}

/// Executes one instruction. The instructions are grouped in functions of their own so that the frame of each
/// stays small: units call units recursively (blocks, includes, macros, arrow functions), and every level of a
/// template recursion costs the frames on that path. The dispatch itself is part of the loop of [`execute`], which
/// spares every instruction a call.
#[inline(always)]
fn step(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::Const(..)
        | Instr::Pop
        | Instr::Dup
        | Instr::LoadLocal(..)
        | Instr::LoadVar { .. }
        | Instr::StoreLocal(..) => step_stack(state, out, frame, program, instr),
        Instr::Text(..)
        | Instr::Print
        | Instr::Flush
        | Instr::CaptureStart
        | Instr::CaptureEnd { .. } => step_output(state, out, frame, program, instr),
        Instr::LoadName { .. }
        | Instr::LoadTemplateName
        | Instr::LoadContext
        | Instr::LoadCharset
        | Instr::StoreName(..)
        | Instr::SetContext => step_context(state, out, frame, program, instr),
        Instr::Jump(..) | Instr::JumpIfFalse(..) | Instr::JumpIfTrue(..) | Instr::Return => {
            step_control(state, out, frame, program, instr)
        }
        Instr::ToBool
        | Instr::TrueTest
        | Instr::Not
        | Instr::IsNull
        | Instr::Binary(..)
        | Instr::CommutedBinary(..)
        | Instr::Neg
        | Instr::Pos
        | Instr::StringCast
        | Instr::StringCastBelow
        | Instr::IntCast
        | Instr::In { .. }
        | Instr::Matches
        | Instr::IsString
        | Instr::StartsWith
        | Instr::EndsWith
        | Instr::Range
        | Instr::HasSome
        | Instr::HasEvery
        | Instr::EnsureTraversable
        | Instr::PhpConstant { .. } => step_operators(state, out, frame, program, instr),
        Instr::NewArray | Instr::ArrayAppend | Instr::ArrayInsert | Instr::ArraySpread => {
            step_arrays(state, out, frame, program, instr)
        }
        Instr::GetAttr(..)
        | Instr::ArrayLike { .. }
        | Instr::StringableKey { .. }
        | Instr::DimIsset => step_attributes(state, out, frame, program, instr),
        Instr::ArgsStart { .. } | Instr::Arg | Instr::ArgSpread { .. } | Instr::Call(..) => {
            step_calls(state, out, frame, program, instr)
        }
        Instr::SeqDestructure(..) | Instr::ListAssign(..) => {
            step_assignment(state, out, frame, program, instr)
        }
        Instr::ForParent
        | Instr::LoopInit
        | Instr::IterStart
        | Instr::IterNext { .. }
        | Instr::IterEnd
        | Instr::LoopUpdate
        | Instr::ForRestore { .. } => step_loops(state, out, frame, program, instr),
        Instr::WithVars { .. } => step_with(state, out, frame, program, instr),
        Instr::MakeMarkup | Instr::MarkupUnlessEmpty => {
            step_markup(state, out, frame, program, instr)
        }
        Instr::MakeArrow(..) | Instr::Recv => step_arrows(state, out, frame, program, instr),
        Instr::Deprecated { .. } => step_deprecations(state, out, frame, program, instr),
        Instr::CheckToString { .. } | Instr::SandboxStart { .. } | Instr::SandboxEnd { .. } => {
            step_sandbox(state, out, frame, program, instr)
        }
        Instr::LoadTemplate { .. }
        | Instr::SetParent
        | Instr::GetParent { .. }
        | Instr::IncludeContext { .. }
        | Instr::CheckContext
        | Instr::YieldTemplate(..)
        | Instr::Block { .. }
        | Instr::ParentBlock { .. } => step_templates(state, out, frame, program, instr),
        Instr::MacroNamespace { .. }
        | Instr::SelfMacroNamespace
        | Instr::TemplateMacroNamespace
        | Instr::SetMacroNamespace { .. }
        | Instr::MacroCall { .. }
        | Instr::MacroDefined
        | Instr::RecvInit { .. }
        | Instr::RecvStore { .. } => step_macros(state, out, frame, program, instr),
        Instr::TryLoader { .. } | Instr::EndTry | Instr::CheckBlocks => {
            step_regions(state, out, frame, program, instr)
        }
    }
}

/// The `stack, constants, locals` instructions.
#[allow(unused_variables)]
fn step_stack(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::Const(id) => frame.push(program.constant(*id).clone()),
        Instr::Pop => {
            frame.pop();
        }
        Instr::Dup => {
            let value = frame.stack.last().cloned().unwrap_or_default();
            frame.push(value);
        }
        Instr::LoadLocal(slot) => {
            let value = frame.local(*slot);
            frame.push(value);
        }
        Instr::LoadVar { local, name } => {
            let value = match frame.locals.get(*local as usize) {
                Some(Some(value)) => value.clone(),
                // ZEND_FETCH_* of an undefined CV: `zend_undefined_cv()` (PHPSRC/Zend/zend_execute.c)
                _ => {
                    state.report(Diagnostic::new(
                        Level::Warning,
                        format!("Undefined variable $_v{name}"),
                    ))?;
                    Value::Null
                }
            };
            frame.push(value);
        }
        Instr::StoreLocal(slot) => {
            let value = frame.pop();
            frame.set_local(*slot, value);
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `output` instructions.
#[allow(unused_variables)]
fn step_output(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::Text(id) => out.write(constant_bytes(program, *id)).map_err(io_error)?,
        Instr::Print => {
            let value = frame.pop();
            print(state, out, &value)?;
        }
        Instr::Flush => out.flush().map_err(io_error)?,
        Instr::CaptureStart => {
            out.start_capture();
            frame.saved_macros.push(frame.macros.clone());
        }
        Instr::CaptureEnd { markup } => {
            let captured = PhpString::from(out.end_capture());
            if let Some(macros) = frame.saved_macros.pop() {
                frame.macros = macros;
            }
            if *markup && !captured.is_empty() {
                frame.push(Markup::new_value(captured, state.env.charset_value()));
            } else {
                frame.push(Value::String(captured));
            }
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `context` instructions.
#[allow(unused_variables)]
fn step_context(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::LoadName { name, mode } => {
            let name = constant_bytes(program, *name);
            let found = frame.context.get_sym(name);
            match mode {
                NameMode::Lenient => {
                    let value = found.cloned().unwrap_or_default();
                    frame.push(value);
                }
                NameMode::Defined => {
                    let exists = found.is_some();
                    frame.push(Value::Bool(exists));
                }
                NameMode::Strict { line } => match found {
                    Some(value) => {
                        let value = value.clone();
                        frame.push(value);
                    }
                    None => {
                        return Err(Error::runtime(format!(
                            "Variable \"{}\" does not exist.",
                            String::from_utf8_lossy(name)
                        ))
                        .with_line(*line)
                        .with_source(program.source().clone())
                        .into());
                    }
                },
                NameMode::AlwaysDefined => match found {
                    Some(value) => {
                        let value = value.clone();
                        frame.push(value);
                    }
                    None => {
                        let key = ArrayKey::from_string(PhpString::from(name));
                        dim::undefined_key(&key, state)?;
                        frame.push(Value::Null);
                    }
                },
            }
        }
        Instr::LoadTemplateName => frame.push(Value::from(program.name())),
        Instr::LoadContext => {
            let context = frame.context.clone();
            frame.push(Value::Array(context));
        }
        Instr::LoadCharset => frame.push(Value::String(state.env.charset_value())),
        Instr::StoreName(name) => {
            let value = frame.pop();
            store_constant_key(&mut frame.context, program, *name, value);
        }
        Instr::SetContext => match frame.pop() {
            Value::Array(context) => frame.context = context,
            other => frame.context = convert::to_array(&other, state)?,
        },

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `control flow` instructions.
#[allow(unused_variables)]
fn step_control(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::Jump(target) => return Ok(Flow::Jump(*target)),
        Instr::JumpIfFalse(target) => {
            let value = frame.pop();
            if !convert::to_bool(&value, state)? {
                return Ok(Flow::Jump(*target));
            }
        }
        Instr::JumpIfTrue(target) => {
            let value = frame.pop();
            if convert::to_bool(&value, state)? {
                return Ok(Flow::Jump(*target));
            }
        }
        Instr::Return => {
            let value = match frame.stack.pop() {
                Some(value) => value,
                None => Value::Null,
            };
            return Ok(Flow::Return(value));
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `conversions, tests, operators` instructions.
#[allow(unused_variables)]
fn step_operators(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::ToBool => {
            let value = frame.pop();
            let b = convert::to_bool(&value, state)?;
            frame.push(Value::Bool(b));
        }
        Instr::TrueTest => {
            // (($tmp = <x>) && $tmp instanceof Markup ? (string) $tmp : $tmp)
            let value = frame.pop();
            let truthy = convert::to_bool(&value, state)?;
            match &value {
                Value::Object(object) if truthy && Markup::is_markup(object) => {
                    let string = object.to_php_string(state)?;
                    frame.push(Value::String(string));
                }
                _ => frame.push(value),
            }
        }
        Instr::Not => {
            let value = frame.pop();
            let b = ops::boolean_not(&value, state)?;
            frame.push(Value::Bool(b));
        }
        Instr::IsNull => {
            let value = frame.pop();
            frame.push(Value::Bool(value.is_null()));
        }
        Instr::Binary(op) => {
            let right = frame.pop();
            let left = frame.pop();
            let result = binary(*op, &left, &right, state)?;
            frame.push(result);
        }
        Instr::CommutedBinary(op) => {
            let right = frame.pop();
            let left = frame.pop();
            let result = binary(*op, &right, &left, state)?;
            frame.push(result);
        }
        Instr::Neg => {
            let value = frame.pop();
            let result = ops::negate(&value, state)?;
            frame.push(result);
        }
        Instr::Pos => {
            let value = frame.pop();
            let result = ops::unary_plus(&value, state)?;
            frame.push(result);
        }
        Instr::StringCast => {
            let value = frame.pop();
            let string = convert::to_string(&value, state)?;
            frame.push(Value::String(string));
        }
        Instr::StringCastBelow => {
            let top = frame.pop();
            let value = frame.pop();
            let string = convert::to_string(&value, state)?;
            frame.push(Value::String(string));
            frame.push(top);
        }
        Instr::IntCast => {
            // ZEND_CAST (IS_LONG): zval_get_long()
            let value = frame.pop();
            let int = convert::to_long(&value, state)?;
            frame.push(Value::Int(int));
        }
        Instr::In { negate } => {
            let compare = frame.pop();
            let value = frame.pop();
            let found = core::in_filter(&value, &compare, state)?;
            frame.push(Value::Bool(found != *negate));
        }
        Instr::Matches => {
            let subject = frame.pop();
            let regexp = frame.pop();
            let result = core::matches(&regexp, &subject, state)?;
            frame.push(result);
        }
        Instr::IsString => {
            let value = frame.pop();
            frame.push(Value::Bool(matches!(value, Value::String(_))));
        }
        Instr::StartsWith | Instr::EndsWith => {
            let needle = frame.pop();
            let haystack = frame.pop();
            let (Value::String(haystack), Value::String(needle)) = (&haystack, &needle) else {
                frame.push(Value::Bool(false));
                return Ok(Flow::Next);
            };
            let result = if matches!(instr, Instr::StartsWith) {
                haystack.starts_with(needle.as_bytes())
            } else {
                haystack.ends_with(needle.as_bytes())
            };
            frame.push(Value::Bool(result));
        }
        Instr::Range => {
            let end = frame.pop();
            let start = frame.pop();
            let range = twig_value::builtins::array::range(&start, &end, None, state)?;
            frame.push(Value::Array(range));
        }
        Instr::HasSome | Instr::HasEvery => {
            let arrow = frame.pop();
            let array = frame.pop();
            let sandboxed = state.env.is_sandboxed(program.source());
            let result = core::array_some_every(
                state,
                &array,
                &arrow,
                sandboxed,
                matches!(instr, Instr::HasEvery),
            )?;
            frame.push(Value::Bool(result));
        }
        Instr::EnsureTraversable => {
            let value = frame.pop();
            frame.push(core::ensure_traversable(value));
        }
        Instr::PhpConstant {
            with_object,
            spread,
        } => {
            let mut name = frame.pop();
            if *spread {
                // `constant(...value)`: ZEND_SEND_UNPACK, then the parameters of `constant(string $name)`
                use twig_value::object::{CallArgs, CalleeParams, FunctionKind, Param};
                let mut args = CallArgs::new();
                let callee = CalleeParams {
                    params: &[b"name"],
                    variadic: false,
                    leading: 0,
                    kind: FunctionKind::Internal,
                };
                args.unpack(&name, Some(&callee), state)?;
                let mut bound = args.bind(
                    "constant",
                    &[Param::required("name")],
                    FunctionKind::Internal,
                )?;
                name = bound.pop().unwrap_or_default();
            }
            let object = if *with_object {
                Some(frame.pop())
            } else {
                None
            };
            let value = core::php_constant(state, &name, object.as_ref())?;
            frame.push(value);
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `arrays` instructions.
#[allow(unused_variables)]
fn step_arrays(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::NewArray => frame.push(Value::Array(Array::new())),
        Instr::ArrayAppend => {
            let value = frame.pop();
            frame.top_array().push(value)?;
        }
        Instr::ArrayInsert => {
            let value = frame.pop();
            let key = frame.pop();
            let key = ArrayKey::from_value(&key, KeyContext::Write, state)?;
            frame.top_array().insert(key, value);
        }
        Instr::ArraySpread => {
            let value = frame.pop();
            let mut array = std::mem::take(frame.top_array());
            let result = array_unpack(&mut array, &value, state);
            *frame.top_array() = array;
            result?;
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `attributes` instructions.
#[allow(unused_variables)]
fn step_attributes(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::GetAttr(op) => {
            let arguments = frame.pop();
            let item = frame.pop();
            let object = frame.pop();
            let value = attribute::get_attribute(state, program, object, item, arguments, op)?;
            frame.push(value);
        }
        Instr::ArrayLike { sandboxed } => {
            // ($v && is_array($v)) || $v instanceof ArrayAccess [&& in_array($v::class, ARRAY_LIKE_CLASSES)]
            let value = frame.pop();
            let result = match &value {
                Value::Array(_) => convert::to_bool(&value, state)?,
                Value::Object(object) => {
                    // `$v && is_array($v)` evaluates the truthiness of the object first
                    convert::to_bool(&value, state)?;
                    object.instance_of("ArrayAccess")
                        && (!*sandboxed || attribute::is_array_like_class(object))
                }
                other => {
                    convert::to_bool(other, state)?;
                    false
                }
            };
            frame.push(Value::Bool(result));
        }
        Instr::StringableKey { container } => {
            let key = frame.pop();
            let cast = match &key {
                Value::Object(object) if object.instance_of("Stringable") => {
                    match frame.locals.get(*container as usize) {
                        Some(Some(Value::Array(_))) => true,
                        Some(Some(Value::Object(c))) => {
                            c.instance_of("ArrayObject") || c.instance_of("ArrayIterator")
                        }
                        _ => false,
                    }
                }
                _ => false,
            };
            if cast {
                let string = convert::to_string(&key, state)?;
                frame.push(Value::String(string));
            } else {
                frame.push(key);
            }
        }
        Instr::DimIsset => {
            let key = frame.pop();
            let container = frame.pop();
            let value = dim::read(&container, &key, FetchMode::Isset, state)?;
            frame.push(value);
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `calls` instructions.
#[allow(unused_variables)]
fn step_calls(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::ArgsStart { with_context } => {
            let context = with_context.then(|| frame.context.clone());
            frame
                .args
                .push(ArgList::with_positional(state.arg_vec(), context));
        }
        Instr::Arg => {
            let value = frame.pop();
            if let Some(list) = frame.args.last_mut() {
                list.push(value);
            }
        }
        Instr::ArgSpread { callable } => {
            let value = frame.pop();
            if let Some(mut list) = frame.args.pop() {
                list.unpacked = true;
                let callable = &program.callables()[*callable as usize];
                // a callable declaring no parameter may be a trampoline (`__call`), which accepts any name: its
                // runtime checks the names when it binds the arguments
                let result = match callable
                    .callable()
                    .filter(|php| !php.parameters().is_empty())
                {
                    Some(php) => {
                        let names: Vec<&[u8]> = php
                            .parameters()
                            .iter()
                            .filter(|param| !param.variadic)
                            .map(|param| param.name.as_bytes())
                            .collect();
                        let callee = callee_params(callable, php, &names);
                        list.unpack(&value, Some(&callee), state)
                    }
                    None => list.unpack(&value, None, state),
                };
                frame.args.push(list);
                result?;
            }
        }
        Instr::Call(index) => {
            let list = frame.args.pop().unwrap_or_else(|| ArgList::new(None));
            let callable = &program.callables()[*index as usize];
            let value = call_callable(state, program, callable, list)?;
            frame.push(value);
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `assignment` instructions.
#[allow(unused_variables)]
fn step_assignment(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::SeqDestructure(names) => {
            let value = frame.pop();
            let result = seq_destructure(state, frame, program.constant(*names), value)?;
            frame.push(result);
        }
        Instr::ListAssign(names) => {
            let array = frame.stack.last().cloned().unwrap_or_default();
            list_assign(state, frame, program.constant(*names), &array)?;
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `for loops` instructions.
#[allow(unused_variables)]
fn step_loops(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::ForParent => {
            let parent = frame.context.clone();
            frame.context.insert_sym(&names::PARENT, Value::Array(parent));
        }
        Instr::LoopInit => loop_init(state, frame)?,
        Instr::IterStart => {
            let seq = read_context(state, frame, b"_seq")?;
            let iter = LoopIter::start(&seq, state)?;
            frame.iterators.push(iter);
        }
        Instr::IterNext { exit, key, value } => {
            let Some(iter) = frame.iterators.last_mut() else {
                return Ok(Flow::Jump(*exit));
            };
            match iter.next(state)? {
                None => return Ok(Flow::Jump(*exit)),
                Some((k, v)) => {
                    store_constant_key(&mut frame.context, program, *value, v);
                    store_constant_key(&mut frame.context, program, *key, k);
                }
            }
        }
        Instr::IterEnd => {
            frame.iterators.pop();
        }
        Instr::LoopUpdate => loop_update(state, frame)?,
        Instr::ForRestore {
            key,
            value,
            has_else,
            with_loop,
        } => {
            for_restore(
                state,
                frame,
                constant_bytes(program, *key),
                constant_bytes(program, *value),
                *has_else,
                *with_loop,
            )?;
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `with` instructions.
#[allow(unused_variables)]
fn step_with(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::WithVars { local, only, line } => {
            let variables = frame.local(*local);
            if !variables.is_iterable() {
                return Err(Error::runtime(
                    "Variables passed to the \"with\" tag must be a mapping.",
                )
                .with_line(*line)
                .with_source(program.source().clone())
                .into());
            }
            let variables = match core::to_array(variables, true, state)? {
                Value::Array(array) => array,
                _ => Array::new(),
            };
            if *only {
                frame.context = Array::new();
            }
            let context = std::mem::take(&mut frame.context);
            frame.context = ops::array_union(&variables, &context);
            let globals = state.env.globals();
            frame.context = ops::array_union(&frame.context, &globals);
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `markup` instructions.
#[allow(unused_variables)]
fn step_markup(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::MakeMarkup => {
            let value = frame.pop();
            let content = convert::to_string(&value, state)?;
            frame.push(Markup::new_value(content, state.env.charset_value()));
        }
        Instr::MarkupUnlessEmpty => {
            let value = frame.pop();
            if matches!(&value, Value::String(s) if s.is_empty()) {
                frame.push(Value::String(PhpString::empty()));
            } else {
                let content = convert::to_string(&value, state)?;
                frame.push(Markup::new_value(content, state.env.charset_value()));
            }
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `arrow functions` instructions.
#[allow(unused_variables)]
fn step_arrows(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::MakeArrow(unit) => {
            let closure = super::arrow::make_arrow(
                program,
                *unit,
                frame.context.clone(),
                frame.macros.clone(),
                Arc::clone(&frame.template),
            );
            frame.push(closure);
        }
        Instr::Recv => {}

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `deprecations` instructions.
#[allow(unused_variables)]
fn step_deprecations(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::Deprecated {
            has_package,
            has_version,
            line,
        } => {
            let message = frame.pop();
            let version = if *has_version {
                frame.pop()
            } else {
                Value::from("")
            };
            let package = if *has_package {
                frame.pop()
            } else {
                Value::from("")
            };
            core::trigger_template_deprecation(
                state, program, &package, &version, &message, *line,
            )?;
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `sandbox` instructions.
#[allow(unused_variables)]
fn step_sandbox(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::CheckToString { spread, line } => {
            let value = frame.pop();
            let value = match state.env.sandbox_checker() {
                Some(checker) if *spread => {
                    checker.ensure_spread_allowed(state, value, *line, program.source())?
                }
                Some(checker) => {
                    checker.ensure_to_string_allowed(state, value, *line, program.source())?
                }
                None => value,
            };
            frame.push(value);
        }
        Instr::SandboxStart { local } => {
            let already = sandbox_start(&state.env);
            frame.set_local(*local, Value::Bool(already));
            frame.handlers.push(Handler::Sandbox { already });
        }
        Instr::SandboxEnd { local } => {
            if let Some(Handler::Sandbox { .. }) = frame.handlers.last() {
                frame.handlers.pop();
            }
            let already = matches!(
                frame.locals.get(*local as usize),
                Some(Some(Value::Bool(true)))
            );
            sandbox_end(&state.env, already);
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `templates` instructions.
#[allow(unused_variables)]
fn step_templates(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::LoadTemplate { line, index } => {
            let template = frame.pop();
            let this = Arc::clone(&frame.template);
            let loaded = composition::load(state, &this, template, *line, *index)?;
            frame.push(TemplateObject::value(loaded));
        }
        Instr::SetParent => {
            let parent = handle_template(frame.stack.last().unwrap_or(&Value::Null), "unwrap")?;
            frame.template.set_parent(parent);
        }
        Instr::GetParent { this } => {
            let target = if *this {
                Arc::clone(&frame.template)
            } else {
                let handle = frame.pop();
                handle_template(&handle, "getParent")?
            };
            let context = frame.context.clone();
            match composition::get_parent(state, &target, &context)? {
                Some(parent) => frame.push(TemplateObject::value(parent)),
                None => frame.push(Value::Bool(false)),
            }
        }
        Instr::IncludeContext { merge } => {
            let variables = frame.pop();
            let context = if *merge {
                let context = frame.context.clone();
                Value::Array(composition::merge_include_variables(
                    context, variables, state,
                )?)
            } else {
                core::to_array(variables, true, state)?
            };
            frame.push(context);
        }
        Instr::CheckContext => {
            if let Some(other) = frame.stack.last()
                && !matches!(other, Value::Array(_))
            {
                return Err(composition::yield_context_type_error(other));
            }
        }
        Instr::YieldTemplate(args) => {
            let (context, blocks) = match args {
                YieldArgs::Context => (frame.context.clone(), Blocks::default()),
                YieldArgs::Empty => (Array::new(), Blocks::default()),
                YieldArgs::Given => match frame.pop() {
                    Value::Array(context) => (context, Blocks::default()),
                    other => return Err(composition::yield_context_type_error(&other)),
                },
                YieldArgs::Parent => {
                    let blocks = frame.blocks.clone().unwrap_or_default();
                    (
                        frame.context.clone(),
                        frame.template.block_table().merge(&blocks),
                    )
                }
            };
            let handle = frame.pop();
            let template = handle_template(&handle, "unwrap")?;
            composition::yield_template(state, out, &template, context, &blocks)?;
        }
        Instr::Block { op, template } => {
            let name = frame.pop();
            let method = match op {
                BlockOp::Yield => "yieldBlock",
                BlockOp::Render => "renderBlock",
                BlockOp::Has => "hasBlock",
            };
            let (target, blocks) = if *template {
                let handle = frame.pop();
                (handle_template(&handle, "unwrap")?, Blocks::default())
            } else {
                let blocks = frame_blocks(state, frame, method, false)?;
                (Arc::clone(&frame.template), blocks)
            };
            let context = frame.context.clone();
            match op {
                BlockOp::Yield => {
                    composition::yield_block(
                        state, out, &target, &name, context, blocks, true, None,
                    )?;
                }
                BlockOp::Render => {
                    let rendered = composition::render_block(
                        state, out, &target, &name, context, blocks, true,
                    )?;
                    frame.push(Value::String(rendered));
                }
                BlockOp::Has => {
                    let has = composition::has_block(state, &target, &name, &context, &blocks)?;
                    frame.push(Value::Bool(has));
                }
            }
        }
        Instr::ParentBlock { name, render } => {
            let method = if *render {
                "renderParentBlock"
            } else {
                "yieldParentBlock"
            };
            let blocks = frame_blocks(state, frame, method, true)?;
            let name = constant_bytes(program, *name);
            let context = frame.context.clone();
            let this = Arc::clone(&frame.template);
            if *render {
                let rendered =
                    composition::render_parent_block(state, out, &this, name, context, blocks)?;
                frame.push(Value::String(rendered));
            } else {
                composition::yield_parent_block(state, out, &this, name, context, blocks)?;
            }
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `macros` instructions.
#[allow(unused_variables)]
fn step_macros(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::MacroNamespace { name, line } => {
            match frame.macros.get_sym(constant_bytes(program, *name)) {
                Some(namespace) if !namespace.is_null() => {
                    let namespace = namespace.clone();
                    frame.push(namespace);
                }
                _ => return Err(macros::uninitialized_namespace(&frame.template, *line)),
            }
        }
        Instr::SelfMacroNamespace => {
            let namespace = MacroNamespace::value(&frame.template);
            frame.push(namespace);
        }
        Instr::TemplateMacroNamespace => {
            let handle = frame.pop();
            let template = handle_template(&handle, "unwrap")?;
            frame.push(MacroNamespace::value(&template));
        }
        Instr::SetMacroNamespace { name, global } => {
            let namespace = frame.pop();
            let key = constant_key(program, *name);
            if *global {
                frame.template.set_macro(key.clone(), namespace.clone());
            }
            frame.macros.insert(key, namespace);
        }
        Instr::MacroCall { line } => {
            let arguments = frame.pop();
            let name = frame.pop();
            let namespace = frame.pop();
            let context = frame.context.clone();
            let result = macros::call(
                state,
                &namespace,
                &name,
                arguments,
                &context,
                *line,
                program.source(),
            )?;
            frame.push(result);
        }
        Instr::MacroDefined => {
            let name = frame.pop();
            let namespace = frame.pop();
            let context = frame.context.clone();
            let defined = macros::has(state, &namespace, &name, &context)?;
            frame.push(Value::Bool(defined));
        }
        Instr::RecvInit {
            name: name_id,
            constant_ast,
            cache,
            skip,
        } => {
            // ZEND_RECV_INIT (PHPSRC/Zend/zend_vm_def.h): an argument that was passed is kept
            let name = constant_bytes(program, *name_id);
            let missing = match frame.context.get_sym(name) {
                Some(value) => compare::identical(value, program.constant(*constant_ast)),
                None => false,
            };
            if !missing {
                return Ok(Flow::Jump(*skip));
            }
            if let Some(cached) = program.recv_cache[*cache as usize].get() {
                store_constant_key(&mut frame.context, program, *name_id, cached.clone());
                return Ok(Flow::Jump(*skip));
            }
        }
        Instr::RecvStore { name, cache } => {
            let value = frame.pop();
            // `if (!Z_REFCOUNTED_P(param) && !ctx.had_side_effects) ZVAL_COPY_VALUE(cache_val, param);`
            if matches!(
                value,
                Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_)
            ) {
                let _ = program.recv_cache[*cache as usize].set(value.clone());
            }
            store_constant_key(&mut frame.context, program, *name, value);
        }

        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// The `exception regions` instructions.
#[allow(unused_variables)]
fn step_regions(
    state: &mut VmState,
    out: &mut Output<'_>,
    frame: &mut Frame,
    program: &Arc<Program>,
    instr: &Instr,
) -> Result<Flow, Exception> {
    match instr {
        Instr::TryLoader { catch } => {
            frame.handlers.push(Handler::Loader {
                catch: *catch,
                stack: frame.stack.len(),
                args: frame.args.len(),
                iterators: frame.iterators.len(),
                saved_macros: frame.saved_macros.len(),
                captures: out.capture_depth(),
            });
        }
        Instr::EndTry => {
            frame.handlers.pop();
        }
        Instr::CheckBlocks => {
            if frame.blocks.is_none() {
                undefined_blocks(state)?;
            }
        }
        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
}

/// What `zend_handle_named_arg()` needs to know about the PHP callable of `callable` while arguments are unpacked
/// into it: its parameters (`names`, those before a trailing variadic one), whether a variadic parameter collects
/// unknown names, the arguments Twig passes before the template's (`CallExpression::compileArguments()`: the
/// charset, the environment, the context, the sandbox flag and the dynamic-name parts, which come before the input
/// of a filter or test), and whether it is an internal function (a string callable) or userland code.
fn callee_params<'a>(
    callable: &crate::extension::TwigCallable,
    php: &crate::extension::PhpCallable,
    names: &'a [&'a [u8]],
) -> twig_value::object::CalleeParams<'a> {
    use crate::extension::{CallableKind, TwigCallable};
    use twig_value::object::FunctionKind;
    let input = usize::from(!matches!(callable, TwigCallable::Function(_)));
    twig_value::object::CalleeParams {
        params: names,
        variadic: php.parameters().last().is_some_and(|param| param.variadic),
        leading: callable
            .minimal_number_of_required_arguments()
            .saturating_sub(input),
        kind: match php.kind() {
            CallableKind::Function(_) => FunctionKind::Internal,
            _ => FunctionKind::User,
        },
    }
}

/// Evaluates a PHP binary operator.
pub(crate) fn binary(
    op: BinaryOp,
    left: &Value,
    right: &Value,
    diag: &mut dyn Diagnostics,
) -> Result<Value, Exception> {
    use twig_value::ops::BinaryOp as V;
    Ok(match op {
        BinaryOp::Add => ops::binary_op(V::Add, left, right, diag)?,
        BinaryOp::Sub => ops::binary_op(V::Sub, left, right, diag)?,
        BinaryOp::Mul => ops::binary_op(V::Mul, left, right, diag)?,
        BinaryOp::Div => ops::binary_op(V::Div, left, right, diag)?,
        BinaryOp::FloorDiv => ops::floor_div(left, right, diag)?,
        // floor(a / b): ints become floats, floats are rounded down (PHPSRC/ext/standard/math.c, PHP_FUNCTION(floor))
        BinaryOp::FloorOfDiv => match ops::div(left, right, diag)? {
            Value::Int(i) => Value::Float(i as f64),
            Value::Float(d) => Value::Float(twig_value::math::floor(d)),
            other => other,
        },
        BinaryOp::Mod => ops::binary_op(V::Mod, left, right, diag)?,
        BinaryOp::Pow => ops::binary_op(V::Pow, left, right, diag)?,
        BinaryOp::Concat => ops::binary_op(V::Concat, left, right, diag)?,
        BinaryOp::BitAnd => ops::binary_op(V::BitAnd, left, right, diag)?,
        BinaryOp::BitOr => ops::binary_op(V::BitOr, left, right, diag)?,
        BinaryOp::BitXor => ops::binary_op(V::BitXor, left, right, diag)?,
        BinaryOp::Equal => Value::Bool(compare::equals(left, right, diag)?),
        BinaryOp::NotEqual => Value::Bool(compare::not_equals(left, right, diag)?),
        BinaryOp::Less => Value::Bool(compare::less(left, right, diag)?),
        BinaryOp::Greater => Value::Bool(compare::greater(left, right, diag)?),
        BinaryOp::LessEqual => Value::Bool(compare::less_or_equal(left, right, diag)?),
        BinaryOp::GreaterEqual => Value::Bool(compare::greater_or_equal(left, right, diag)?),
        BinaryOp::Spaceship => Value::Int(compare::spaceship(left, right, diag)?),
        BinaryOp::Identical => Value::Bool(compare::identical(left, right)),
        BinaryOp::NotIdentical => Value::Bool(!compare::identical(left, right)),
        BinaryOp::Xor => Value::Bool(ops::boolean_xor(left, right, diag)?),
    })
}

/// `$context['name']` read with `BP_VAR_R`: a missing key warns `Undefined array key "name"` and reads as `null`.
fn read_context(state: &mut VmState, frame: &Frame, name: &[u8]) -> Result<Value, Exception> {
    match frame.context.get_sym(name) {
        Some(value) => Ok(value.clone()),
        None => {
            dim::undefined_key(&ArrayKey::from_string(PhpString::from(name)), state)?;
            Ok(Value::Null)
        }
    }
}

/// The names the code of a `for` loop uses as array keys (none is a canonical integer string), allocated once.
mod names {
    use std::sync::LazyLock;

    use twig_value::PhpString;

    macro_rules! name {
        ($($id:ident = $text:literal),* $(,)?) => {
            $(pub(super) static $id: LazyLock<PhpString> = LazyLock::new(|| PhpString::from($text));)*
        };
    }

    name!(
        PARENT = "_parent",
        LOOP = "loop",
        LOOP_PARENT = "parent",
        INDEX0 = "index0",
        INDEX = "index",
        FIRST = "first",
        REVINDEX0 = "revindex0",
        REVINDEX = "revindex",
        LENGTH = "length",
        LAST = "last",
    );
}

/// The `loop` initialisation of `ForNode::compile()`.
fn loop_init(state: &mut VmState, frame: &mut Frame) -> Result<(), Exception> {
    let parent = read_context(state, frame, b"_parent")?;
    let mut loop_array = Array::new_hash();
    let key = |name: &LazyLock<PhpString>| ArrayKey::Str(PhpString::clone(name));
    loop_array.insert(key(&names::LOOP_PARENT), parent);
    loop_array.insert(key(&names::INDEX0), 0);
    loop_array.insert(key(&names::INDEX), 1);
    loop_array.insert(key(&names::FIRST), true);
    frame
        .context
        .insert_sym(&names::LOOP, Value::Array(loop_array));
    let seq = read_context(state, frame, b"_seq")?;
    let countable = match &seq {
        Value::Array(_) => true,
        Value::Object(object) => object.instance_of("Countable"),
        _ => false,
    };
    if countable {
        let length = dim::count(&seq, 0, state)?;
        let revindex0 = ops::sub(&Value::Int(length), &Value::Int(1), state)?;
        assign_loop_field(state, frame, &names::REVINDEX0, revindex0)?;
        assign_loop_field(state, frame, &names::REVINDEX, Value::Int(length))?;
        assign_loop_field(state, frame, &names::LENGTH, Value::Int(length))?;
        assign_loop_field(state, frame, &names::LAST, Value::Bool(length == 1))?;
    }
    Ok(())
}

/// `$context['loop']` fetched to write into it (`ZEND_FETCH_DIM_W`, or `ZEND_FETCH_DIM_RW` for `++`/`--`, of
/// `$context`): a missing key is added as `null`, after PHP's warning `Undefined array key "loop"` when the fetch also
/// reads (`zend_undefined_index_write()`).
fn loop_slot<'a>(
    state: &mut VmState,
    frame: &'a mut Frame,
    read: bool,
) -> Result<&'a mut Value, Exception> {
    if !frame.context.contains_ref(KeyRef::Str(b"loop")) {
        let key = ArrayKey::Str(PhpString::clone(&names::LOOP));
        if read {
            dim::undefined_key(&key, state)?;
        }
        frame.context.insert(key, Value::Null);
    }
    Ok(frame
        .context
        .get_mut_ref(KeyRef::Str(b"loop"))
        .expect("the key was just added"))
}

/// The container `$context['loop'][...]` writes into: `zend_fetch_dimension_address()` with `BP_VAR_W`/`BP_VAR_RW`
/// (PHPSRC/Zend/zend_execute.c) turns `null` into an array, `false` too (with PHP 8.1's deprecation, from
/// `zend_false_to_array_deprecated()`); objects handle the dimension themselves; a string raises `Cannot access
/// offset of type string on string`, another scalar `Cannot use a scalar value as an array`.
fn loop_container<'a>(
    slot: &'a mut Value,
    diag: &mut dyn Diagnostics,
) -> Result<LoopContainer<'a>, Exception> {
    match slot {
        Value::Array(_) => {}
        Value::Null => *slot = Value::Array(Array::new()),
        Value::Bool(false) => {
            diag.report(Diagnostic::new(
                Level::Deprecated,
                "Automatic conversion of false to array is deprecated",
            ))?;
            *slot = Value::Array(Array::new());
        }
        Value::Object(object) => return Ok(LoopContainer::Object(object.clone())),
        Value::String(_) => {
            return Err(
                PhpError::type_error("Cannot access offset of type string on string").into(),
            );
        }
        _ => {
            return Err(PhpError::error("Cannot use a scalar value as an array").into());
        }
    }
    match slot {
        Value::Array(array) => Ok(LoopContainer::Array(array)),
        _ => unreachable!("the slot was made an array"),
    }
}

/// What a `$context['loop'][...]` write fetch found.
enum LoopContainer<'a> {
    Array(&'a mut Array),
    Object(twig_value::ObjectRef),
}

/// Whether an object's dimensions are the elements of its storage (`ArrayObject`, `ArrayIterator`: their
/// `read_dimension` handler returns the element itself for writing, `spl_array_get_dimension_ptr()`), rather than a
/// copy (`offsetGet()` of a userland `ArrayAccess`).
fn writes_through(object: &twig_value::ObjectRef) -> bool {
    object.instance_of("ArrayObject") || object.instance_of("ArrayIterator")
}

/// `$context['loop'][$field] = $value` (`ZEND_ASSIGN_DIM` on the `FETCH_DIM_W` of `$context['loop']`): the container
/// of [`loop_container()`]; an object gets `write_dimension()` (`offsetSet()`).
fn assign_loop_field(
    state: &mut VmState,
    frame: &mut Frame,
    field: &PhpString,
    value: Value,
) -> Result<(), Exception> {
    let slot = loop_slot(state, frame, false)?;
    match loop_container(slot, state)? {
        LoopContainer::Array(array) => {
            array.insert_sym(field, value);
        }
        LoopContainer::Object(object) => {
            let offset = Value::String(field.clone());
            object.write_dimension(Some(&offset), value, state)?;
        }
    }
    Ok(())
}

/// `++$context['loop'][$field]` / `--...` (`ZEND_PRE_INC`/`ZEND_PRE_DEC` on the `FETCH_DIM_RW` of the field): in an
/// array, a missing field warns `Undefined array key` and counts from `null`; an `ArrayObject`/`ArrayIterator`
/// updates its storage the same way; any other object returns a copy from `offsetGet()`, which PHP increments in vain
/// after the notice `Indirect modification of overloaded element of %s has no effect` (unless it is an object).
fn incdec_loop_field(
    state: &mut VmState,
    frame: &mut Frame,
    field: &PhpString,
    increment: bool,
) -> Result<(), Exception> {
    let slot = loop_slot(state, frame, true)?;
    match loop_container(slot, state)? {
        LoopContainer::Array(array) => {
            if let Some(current) = array.get_mut_ref(KeyRef::Str(field)) {
                let updated = core::increment(current, increment, state)?;
                *current = updated;
            } else {
                let key = ArrayKey::Str(field.clone());
                dim::undefined_key(&key, state)?;
                array.insert(key.clone(), Value::Null);
                let updated = core::increment(&Value::Null, increment, state)?;
                array.insert(key, updated);
            }
        }
        LoopContainer::Object(object) => {
            let offset = Value::String(field.clone());
            let current = object.read_dimension(&offset, FetchMode::Read, state)?;
            if writes_through(&object) {
                let updated = core::increment(&current, increment, state)?;
                object.write_dimension(Some(&offset), updated, state)?;
            } else {
                if !matches!(current, Value::Object(_)) {
                    state.report(Diagnostic::new(
                        Level::Notice,
                        format!(
                            "Indirect modification of overloaded element of {} has no effect",
                            object.class().display_name()
                        ),
                    ))?;
                }
                core::increment(&current, increment, state)?;
            }
        }
    }
    Ok(())
}

/// `ForLoopNode::compile()` with `with_loop`, statement by statement.
fn loop_update(state: &mut VmState, frame: &mut Frame) -> Result<(), Exception> {
    incdec_loop_field(state, frame, &names::INDEX0, true)?;
    incdec_loop_field(state, frame, &names::INDEX, true)?;
    assign_loop_field(state, frame, &names::FIRST, Value::Bool(false))?;
    // isset($context['loop']['revindex0'], $context['loop']['revindex'])
    let both_set = match frame.context.get_sym(b"loop") {
        Some(loop_value) => {
            loop_field_isset(loop_value, &names::REVINDEX0, state)?
                && loop_field_isset(loop_value, &names::REVINDEX, state)?
        }
        None => false,
    };
    if both_set {
        incdec_loop_field(state, frame, &names::REVINDEX0, false)?;
        incdec_loop_field(state, frame, &names::REVINDEX, false)?;
        // `$context['loop']['last'] = 0 === $context['loop']['revindex0'];`: the right side is read first (and no
        // copy of `$context['loop']` outlives the read, so the assignment does not separate it)
        let last = match frame.context.get_sym(b"loop") {
            Some(Value::Array(array)) => match array.get_ref(KeyRef::Str(b"revindex0")) {
                Some(revindex0) => compare::identical(&Value::Int(0), revindex0),
                None => {
                    dim::undefined_key(&ArrayKey::Str(PhpString::clone(&names::REVINDEX0)), state)?;
                    false
                }
            },
            _ => {
                let loop_value = read_context(state, frame, b"loop")?;
                let offset = Value::String(PhpString::clone(&names::REVINDEX0));
                let revindex0 = dim::read(&loop_value, &offset, FetchMode::Read, state)?;
                compare::identical(&Value::Int(0), &revindex0)
            }
        };
        assign_loop_field(state, frame, &names::LAST, Value::Bool(last))?;
    }
    Ok(())
}

/// `isset($context['loop'][$field])`.
fn loop_field_isset(
    loop_value: &Value,
    field: &PhpString,
    state: &mut VmState,
) -> Result<bool, Exception> {
    Ok(match loop_value {
        Value::Array(array) => array
            .get_ref(KeyRef::Str(field))
            .is_some_and(|value| !value.is_null()),
        other => dim::isset(other, &Value::String(field.clone()), state)?,
    })
}

/// The restore sequence of `ForNode::compile()`.
fn for_restore(
    state: &mut VmState,
    frame: &mut Frame,
    key: &[u8],
    value: &[u8],
    has_else: bool,
    with_loop: bool,
) -> Result<(), Exception> {
    let parent = read_context(state, frame, b"_parent")?;
    for name in [&b"_seq"[..], key, value, b"_parent"] {
        frame.context.remove_ref(KeyRef::sym(name));
    }
    if has_else {
        frame.context.remove_ref(KeyRef::Str(b"_iterated"));
    }
    if with_loop {
        frame.context.remove_ref(KeyRef::Str(b"loop"));
    }
    let context = std::mem::take(&mut frame.context);
    frame.context = restore_parent_context(context, parent)?;
    Ok(())
}

/// `array_intersect_key($context, $_parent) + $_parent`, the context after a `for` loop.
///
/// That builds a new array. When neither array has ever had an int key (no `nNextFreeElement`) and the parent is not
/// empty, the new array is a hash table without a next free element holding the context's entries whose keys the
/// parent has, in order, then the parent's entries whose keys the context lacks: an unshared context becomes exactly
/// that in place.
fn restore_parent_context(mut context: Array, parent: Value) -> Result<Array, Exception> {
    if let Value::Array(parent) = &parent
        && !parent.is_empty()
        && parent.next_free().is_none()
        && context.next_free().is_none()
        && parent.keys().all(|key| matches!(key, ArrayKey::Str(_)))
        && context.keys().all(|key| matches!(key, ArrayKey::Str(_)))
    {
        if context.keys().any(|key| !parent.contains_key(key)) {
            context.retain(|key, _| parent.contains_key(key));
        }
        for (key, value) in parent {
            if !context.contains_key(key) {
                context.insert(key.clone(), value.clone());
            }
        }
        return Ok(context);
    }
    rebuild_parent_context(context, parent)
}

/// `array_intersect_key($context, $_parent) + $_parent`, computed as written.
fn rebuild_parent_context(context: Array, parent: Value) -> Result<Array, Exception> {
    let intersected =
        twig_value::builtins::array::array_intersect_key(&[Value::Array(context), parent.clone()])?;
    let Value::Array(parent) = parent else {
        unreachable!("array_intersect_key() rejected a non-array");
    };
    Ok(ops::array_union(&intersected, &parent))
}

/// The names constant of a destructuring instruction.
fn names_of(names: &Value) -> Vec<Option<PhpString>> {
    match names {
        Value::Array(array) => array
            .values()
            .map(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `SequenceDestructuringSetBinary::compile()`: `[(($v = value) instanceof \Traversable ?
/// CoreExtension::destructureSequence($context, $names, $v) : ([$context["a"], , ...] = array_pad($v, n, null))), $v =
/// null][0]`.
fn seq_destructure(
    state: &mut VmState,
    frame: &mut Frame,
    names: &Value,
    value: Value,
) -> Result<Value, Exception> {
    let names = names_of(names);
    if let Value::Object(object) = &value
        && object.instance_of("Traversable")
    {
        core::destructure_sequence(state, &mut frame.context, &names, object)?;
        return Ok(value);
    }
    let padded = twig_value::builtins::array::array_pad(
        &value,
        &Value::Int(names.len() as i64),
        &Value::Null,
        state,
    )?;
    for (i, name) in names.iter().enumerate() {
        let Some(name) = name else { continue };
        let element = match padded.get_int(i as i64) {
            Some(element) => element.clone(),
            None => {
                dim::undefined_key(&ArrayKey::Int(i as i64), state)?;
                Value::Null
            }
        };
        frame
            .context
            .insert(ArrayKey::from_string(name.clone()), element);
    }
    Ok(Value::Array(padded))
}

/// `[$context["a"], $context["b"]] = $array` (`ZEND_FETCH_LIST_R` with int keys `0..n`, then the assignments).
fn list_assign(
    state: &mut VmState,
    frame: &mut Frame,
    names: &Value,
    array: &Value,
) -> Result<(), Exception> {
    for (i, name) in names_of(names).iter().enumerate() {
        let Some(name) = name else { continue };
        let element = match array {
            Value::Array(a) => match a.get_int(i as i64) {
                Some(element) => element.clone(),
                None => {
                    dim::undefined_key(&ArrayKey::Int(i as i64), state)?;
                    Value::Null
                }
            },
            other => dim::read(other, &Value::Int(i as i64), FetchMode::Read, state)?,
        };
        frame
            .context
            .insert(ArrayKey::from_string(name.clone()), element);
    }
    Ok(())
}

/// The kind of a unit, for arrow-function parameter binding.
pub(crate) fn arrow_params(program: &Program, unit: UnitId) -> &[PhpString] {
    match program.unit(unit).kind() {
        UnitKind::Arrow(params) => params,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys and values of an array, then the keys that appends, an int key and a removal give it (which depend on
    /// its next free element and its packed or hash layout).
    fn behaviour(array: &Array) -> (Vec<(ArrayKey, Value)>, Vec<ArrayKey>) {
        let entries = array.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let mut copy = array.clone();
        let _ = copy.push("a");
        copy.insert(1, "b");
        copy.remove(&ArrayKey::Int(1));
        copy.insert(1, "c");
        let _ = copy.push("d");
        (entries, copy.keys().cloned().collect())
    }

    /// The in-place restore of a loop's context gives what `array_intersect_key() + $_parent` gives.
    #[test]
    fn restoring_the_parent_context_in_place_matches_the_rebuilt_array() {
        let names = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for round in 0..2000 {
            let mut parent = Array::new();
            for name in names {
                if next(3) > 0 {
                    parent.insert(name, round);
                }
            }
            // the loop's context: a copy of the parent, changed, extended and emptied in places
            let mut context = parent.clone();
            for name in names {
                match next(5) {
                    0 => {
                        context.remove(&ArrayKey::from(name));
                    }
                    1 => {
                        context.insert(name, -round);
                    }
                    2 => {
                        context.insert(format!("{name}{round}"), 0);
                    }
                    _ => {}
                }
            }
            if next(8) == 0 {
                context.insert(next(3) as i64, "int key");
            }
            if next(8) == 0 {
                parent.insert(next(3) as i64, "int key");
            }
            if next(16) == 0 {
                context = Array::new();
            }
            let in_place = restore_parent_context(context.clone(), Value::Array(parent.clone()));
            let rebuilt = rebuild_parent_context(context, Value::Array(parent));
            let (Ok(in_place), Ok(rebuilt)) = (in_place, rebuilt) else {
                panic!("both restores succeed");
            };
            assert_eq!(behaviour(&in_place), behaviour(&rebuilt), "round {round}");
        }
    }
}
