//! The interpreter loop: executes the instructions of one unit on a frame.

use std::sync::Arc;

use twig_value::object::FetchMode;
use twig_value::{
    Array, ArrayKey, Diagnostic, Diagnostics, KeyContext, Level, PhpError, PhpString, Value,
    compare, convert, dim, ops,
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
        frame.locals.resize(locals, Value::Null);
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

/// `$blocks` of the running method; in an arrow function, PHP warns about the undefined variable and the template
/// method rejects the `null` it gets.
fn frame_blocks(state: &mut VmState, frame: &Frame, method: &str) -> Result<Blocks, Exception> {
    match &frame.blocks {
        Some(blocks) => Ok(blocks.clone()),
        None => {
            state.report(Diagnostic::new(
                Level::Warning,
                "Undefined variable $blocks",
            ))?;
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

fn io_error(error: std::io::Error) -> Exception {
    Error::runtime(format!("Unable to write the template output: {error}.")).into()
}

/// Executes one instruction. The instructions are grouped in functions of their own so that the frame of each
/// stays small: units call units recursively (blocks, includes, macros, arrow functions), and every level of a
/// template recursion costs the frames on that path.
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
        | Instr::Neg
        | Instr::Pos
        | Instr::StringCast
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
        Instr::ArgsStart { .. } | Instr::Arg | Instr::ArgSpread | Instr::Call(..) => {
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
        Instr::MakeArrow(..) => step_arrows(state, out, frame, program, instr),
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
        | Instr::MacroDefined => step_macros(state, out, frame, program, instr),
        Instr::TryLoader { .. } | Instr::EndTry => step_regions(state, out, frame, program, instr),
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
            let value = frame
                .locals
                .get(*slot as usize)
                .cloned()
                .unwrap_or_default();
            frame.push(value);
        }
        Instr::StoreLocal(slot) => {
            let value = frame.pop();
            let slot = *slot as usize;
            if frame.locals.len() <= slot {
                frame.locals.resize(slot + 1, Value::Null);
            }
            frame.locals[slot] = value;
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
            let string = convert::to_string(&value, state)?;
            out.write(&string).map_err(io_error)?;
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
                frame.push(Markup::new_value(captured, state.env.charset()));
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
        Instr::LoadCharset => frame.push(Value::from(state.env.charset())),
        Instr::StoreName(name) => {
            let value = frame.pop();
            let key = ArrayKey::from_string(PhpString::from(constant_bytes(program, *name)));
            frame.context.insert(key, value);
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
        Instr::PhpConstant { with_object } => {
            let name = frame.pop();
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
                        Some(Value::Array(_)) => true,
                        Some(Value::Object(c)) => {
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
            frame.args.push(ArgList::new(context));
        }
        Instr::Arg => {
            let value = frame.pop();
            if let Some(list) = frame.args.last_mut() {
                list.push(value);
            }
        }
        Instr::ArgSpread => {
            let value = frame.pop();
            if let Some(mut list) = frame.args.pop() {
                list.unpacked = true;
                let result = list.unpack(&value, state);
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
            frame.context.insert("_parent", Value::Array(parent));
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
                    let value_key =
                        ArrayKey::from_string(PhpString::from(constant_bytes(program, *value)));
                    frame.context.insert(value_key, v);
                    let key_key =
                        ArrayKey::from_string(PhpString::from(constant_bytes(program, *key)));
                    frame.context.insert(key_key, k);
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
            let variables = frame
                .locals
                .get(*local as usize)
                .cloned()
                .unwrap_or_default();
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
            frame.push(Markup::new_value(content, state.env.charset()));
        }
        Instr::MarkupUnlessEmpty => {
            let value = frame.pop();
            if matches!(&value, Value::String(s) if s.is_empty()) {
                frame.push(Value::String(PhpString::empty()));
            } else {
                let content = convert::to_string(&value, state)?;
                frame.push(Markup::new_value(content, state.env.charset()));
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
            let slot = *local as usize;
            if frame.locals.len() <= slot {
                frame.locals.resize(slot + 1, Value::Null);
            }
            frame.locals[slot] = Value::Bool(already);
            frame.handlers.push(Handler::Sandbox { already });
        }
        Instr::SandboxEnd { local } => {
            if let Some(Handler::Sandbox { .. }) = frame.handlers.last() {
                frame.handlers.pop();
            }
            let already = matches!(frame.locals.get(*local as usize), Some(Value::Bool(true)));
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
                let blocks = frame_blocks(state, frame, method)?;
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
            let blocks = frame_blocks(state, frame, method)?;
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
            let key = ArrayKey::from_string(PhpString::from(constant_bytes(program, *name)));
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
        _ => unreachable!("step() dispatches by instruction group"),
    }
    Ok(Flow::Next)
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

/// The `loop` initialisation of `ForNode::compile()`.
fn loop_init(state: &mut VmState, frame: &mut Frame) -> Result<(), Exception> {
    let parent = read_context(state, frame, b"_parent")?;
    let mut loop_array = Array::new_hash();
    loop_array.insert("parent", parent);
    loop_array.insert("index0", 0);
    loop_array.insert("index", 1);
    loop_array.insert("first", true);
    frame.context.insert("loop", Value::Array(loop_array));
    let seq = read_context(state, frame, b"_seq")?;
    let countable = match &seq {
        Value::Array(_) => true,
        Value::Object(object) => object.instance_of("Countable"),
        _ => false,
    };
    if countable {
        let length = dim::count(&seq, 0, state)?;
        let revindex0 = ops::sub(&Value::Int(length), &Value::Int(1), state)?;
        assign_loop_field(state, frame, b"revindex0", revindex0)?;
        assign_loop_field(state, frame, b"revindex", Value::Int(length))?;
        assign_loop_field(state, frame, b"length", Value::Int(length))?;
        assign_loop_field(state, frame, b"last", Value::Bool(length == 1))?;
    }
    Ok(())
}

/// `$context['loop'][$field] = $value` (`ZEND_ASSIGN_DIM` on the nested array): `loop` is auto-vivified from
/// `null`, a scalar raises `Cannot use a scalar value as an array`, a string `Cannot access offset of type string on
/// string`.
fn assign_loop_field(
    state: &mut VmState,
    frame: &mut Frame,
    field: &'static [u8],
    value: Value,
) -> Result<(), Exception> {
    let slot = frame.context.get_or_insert_null("loop");
    loop_container(slot, state)?.insert(ArrayKey::from_string(PhpString::from(field)), value);
    Ok(())
}

/// The array of a `$context['loop'][...]` write fetch (`zend_fetch_dimension_address_W`).
fn loop_container<'a>(
    slot: &'a mut Value,
    _diag: &mut dyn Diagnostics,
) -> Result<&'a mut Array, Exception> {
    match slot {
        Value::Array(_) => {}
        Value::Null => *slot = Value::Array(Array::new()),
        Value::Bool(false) => {
            // PHP 8.1+: "Automatic conversion of false to array is deprecated"
            *slot = Value::Array(Array::new());
        }
        Value::String(_) => {
            return Err(
                PhpError::type_error("Cannot access offset of type string on string").into(),
            );
        }
        Value::Object(object) => {
            return Err(PhpError::error(format!(
                "Cannot use object of type {} as array",
                object.class().display_name()
            ))
            .into());
        }
        _ => {
            return Err(PhpError::error("Cannot use a scalar value as an array").into());
        }
    }
    match slot {
        Value::Array(array) => Ok(array),
        _ => unreachable!("the slot was made an array"),
    }
}

/// `++$context['loop'][$field]` / `--...` (`ZEND_PRE_INC`/`ZEND_PRE_DEC` on a `FETCH_DIM_RW` result): a missing field
/// warns `Undefined array key` and counts from `null`.
fn incdec_loop_field(
    state: &mut VmState,
    frame: &mut Frame,
    field: &'static [u8],
    increment: bool,
) -> Result<(), Exception> {
    let key = ArrayKey::from_string(PhpString::from(field));
    let current = {
        let slot = frame.context.get_or_insert_null("loop");
        let array = loop_container(slot, state)?;
        array.get(&key).cloned()
    };
    let current = match current {
        Some(value) => value,
        None => {
            dim::undefined_key(&key, state)?;
            Value::Null
        }
    };
    let updated = core::increment(&current, increment, state)?;
    let slot = frame.context.get_or_insert_null("loop");
    loop_container(slot, state)?.insert(key, updated);
    Ok(())
}

/// `ForLoopNode::compile()` with `with_loop`.
fn loop_update(state: &mut VmState, frame: &mut Frame) -> Result<(), Exception> {
    incdec_loop_field(state, frame, b"index0", true)?;
    incdec_loop_field(state, frame, b"index", true)?;
    assign_loop_field(state, frame, b"first", Value::Bool(false))?;
    // isset($context['loop']['revindex0'], $context['loop']['revindex'])
    let both_set = match frame.context.get_sym(b"loop") {
        Some(loop_value) => {
            let revindex0 = Value::from("revindex0");
            let revindex = Value::from("revindex");
            dim::isset(loop_value, &revindex0, state)? && dim::isset(loop_value, &revindex, state)?
        }
        None => false,
    };
    if both_set {
        incdec_loop_field(state, frame, b"revindex0", false)?;
        incdec_loop_field(state, frame, b"revindex", false)?;
        let revindex0 = match frame.context.get_sym(b"loop") {
            Some(Value::Array(array)) => array.get_sym(b"revindex0").cloned().unwrap_or_default(),
            _ => Value::Null,
        };
        assign_loop_field(
            state,
            frame,
            b"last",
            Value::Bool(revindex0 == Value::Int(0)),
        )?;
    }
    Ok(())
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
    let mut names: Vec<&[u8]> = vec![b"_seq", key, value, b"_parent"];
    if has_else {
        names.push(b"_iterated");
    }
    if with_loop {
        names.push(b"loop");
    }
    for name in names {
        frame
            .context
            .remove(&ArrayKey::from_string(PhpString::from(name)));
    }
    let context = std::mem::take(&mut frame.context);
    let intersected =
        twig_value::builtins::array::array_intersect_key(&[Value::Array(context), parent.clone()])?;
    let Value::Array(parent) = parent else {
        unreachable!("array_intersect_key() rejected a non-array");
    };
    frame.context = ops::array_union(&intersected, &parent);
    Ok(())
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
