//! Frames: the variables of one running unit.

use std::sync::Arc;

use twig_value::object::{CallArgs, ObjectIterator};
use twig_value::{Array, ArrayKey, Diagnostics, PhpError, PhpString, ThrowableClass, Value};

use super::exception::Exception;
use crate::lltl::Label;
use crate::runtime::{Blocks, Template};

/// The variables of one running unit, the equivalent of the local variables of a compiled method: `$context` (a PHP
/// array, copied by value at calls), `$macros`, `$blocks`, the template (`$this`), the operand stack, the compiler
/// variables `$_vN`, the active `foreach` iterators, the argument lists being built, the `$macros` saved by captures
/// and the open `try` regions.
pub(crate) struct Frame {
    pub(crate) context: Array,
    pub(crate) macros: Array,
    /// `$blocks`; `None` in an arrow function, whose closure does not capture it.
    pub(crate) blocks: Option<Blocks>,
    pub(crate) template: Arc<Template>,
    pub(crate) stack: Vec<Value>,
    pub(crate) locals: Vec<Value>,
    pub(crate) iterators: Vec<LoopIter>,
    pub(crate) args: Vec<ArgList>,
    pub(crate) saved_macros: Vec<Array>,
    pub(crate) handlers: Vec<Handler>,
}

/// An open `try` region of a unit (innermost last).
pub(crate) enum Handler {
    /// `try { ... } catch (LoaderError $e) { $v = null; }`: the frame's state when the region opened, and where to
    /// continue after a caught `LoaderError`.
    Loader {
        catch: Label,
        stack: usize,
        args: usize,
        iterators: usize,
        saved_macros: usize,
        captures: usize,
    },
    /// `try { ... } finally { if (!$alreadySandboxed) { $this->sandbox->setSandboxed(false); } }` (SandboxNode).
    Sandbox { already: bool },
}

impl Frame {
    /// The frame of a compiled method (`doDisplay()`, `block_NAME()`, a macro, `doGetParent()`).
    pub(crate) fn new(
        context: Array,
        template: Arc<Template>,
        macros: Array,
        blocks: Blocks,
    ) -> Self {
        Frame {
            context,
            macros,
            blocks: Some(blocks),
            template,
            stack: Vec::with_capacity(8),
            locals: Vec::new(),
            iterators: Vec::new(),
            args: Vec::new(),
            saved_macros: Vec::new(),
            handlers: Vec::new(),
        }
    }

    /// The frame of an arrow function: `function (...) use ($context, $macros)`, without `$blocks`.
    pub(crate) fn arrow(context: Array, template: Arc<Template>, macros: Array) -> Self {
        let mut frame = Frame::new(context, template, macros, Blocks::default());
        frame.blocks = None;
        frame
    }

    #[inline]
    pub(crate) fn push(&mut self, value: Value) {
        self.stack.push(value);
    }

    /// Pops the top of the stack; the code generator guarantees a value is there.
    #[inline]
    pub(crate) fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or_default()
    }

    /// The array on top of the stack (array literals being built).
    pub(crate) fn top_array(&mut self) -> &mut Array {
        if !matches!(self.stack.last(), Some(Value::Array(_))) {
            self.stack.push(Value::Array(Array::new()));
        }
        match self.stack.last_mut() {
            Some(Value::Array(array)) => array,
            _ => unreachable!("an array was just pushed"),
        }
    }
}

/// An argument list being built for a call (`ZEND_SEND_*`): positional arguments, named arguments from unpacked
/// string keys, the context snapshot of a `needs_context` callable, and whether the call unpacks arguments.
pub(crate) struct ArgList {
    pub(crate) args: CallArgs,
    pub(crate) context: Option<Array>,
    /// The call has a `...` argument (PHP compiles it with `ZEND_SEND_UNPACK`, never as a frameless call).
    pub(crate) unpacked: bool,
}

impl ArgList {
    pub(crate) fn new(context: Option<Array>) -> Self {
        ArgList {
            args: CallArgs::new(),
            context,
            unpacked: false,
        }
    }

    /// Appends a positional argument.
    pub(crate) fn push(&mut self, value: Value) {
        self.args.positional.push(value);
    }

    fn push_named(&mut self, name: PhpString, value: Value) {
        self.args.named.push((name, value));
    }

    /// `...value` in a call: `ZEND_SEND_UNPACK` (PHPSRC/Zend/zend_vm_def.h): array elements with int keys are
    /// positional, string keys named; Traversables are iterated (canonical numeric string keys are positional).
    pub(crate) fn unpack(
        &mut self,
        value: &Value,
        diag: &mut dyn Diagnostics,
    ) -> Result<(), Exception> {
        let positional_after_named = || -> Exception {
            PhpError::error("Cannot use positional argument after named argument during unpacking")
                .into()
        };
        match value {
            Value::Array(array) => {
                for (key, element) in array {
                    match key {
                        ArrayKey::Str(name) => self.push_named(name.clone(), element.clone()),
                        ArrayKey::Int(_) => {
                            if !self.args.named.is_empty() {
                                return Err(positional_after_named());
                            }
                            self.push(element.clone());
                        }
                    }
                }
                Ok(())
            }
            Value::Object(object) => {
                let Some(mut iter) = object.get_iterator(diag)? else {
                    return Err(unpack_type_error(value, true));
                };
                iter.rewind(diag)?;
                while iter.valid(diag)? {
                    let element = iter.current(diag)?;
                    let key = iter.key(diag)?;
                    let name = match key {
                        Value::Int(_) => None,
                        Value::String(name) => match ArrayKey::from_string(name.clone()) {
                            ArrayKey::Int(_) => None,
                            ArrayKey::Str(name) => Some(name),
                        },
                        _ => {
                            return Err(PhpError::error(
                                "Keys must be of type int|string during argument unpacking",
                            )
                            .into());
                        }
                    };
                    match name {
                        Some(name) => self.push_named(name, element),
                        None => {
                            if !self.args.named.is_empty() {
                                return Err(positional_after_named());
                            }
                            self.push(element);
                        }
                    }
                    iter.next(diag)?;
                }
                Ok(())
            }
            _ => Err(unpack_type_error(value, true)),
        }
    }
}

/// `Only arrays and Traversables can be unpacked, %s given`: a `TypeError` in calls, an `Error` in array literals.
pub(crate) fn unpack_type_error(value: &Value, in_call: bool) -> Exception {
    let message = format!(
        "Only arrays and Traversables can be unpacked, {} given",
        twig_value::types::zend_zval_value_name(value)
    );
    if in_call {
        PhpError::type_error(message).into()
    } else {
        PhpError::error(message).into()
    }
}

/// `...value` in an array literal: `ZEND_ADD_ARRAY_UNPACK` (PHPSRC/Zend/zend_vm_def.h): int keys are appended,
/// string keys overwrite; Traversables are iterated (canonical numeric string keys are appended).
pub(crate) fn array_unpack(
    target: &mut Array,
    value: &Value,
    diag: &mut dyn Diagnostics,
) -> Result<(), Exception> {
    match value {
        Value::Array(array) => {
            for (key, element) in array {
                match key {
                    ArrayKey::Str(_) => {
                        target.insert(key.clone(), element.clone());
                    }
                    ArrayKey::Int(_) => {
                        target.push(element.clone())?;
                    }
                }
            }
            Ok(())
        }
        Value::Object(object) => {
            let Some(mut iter) = object.get_iterator(diag)? else {
                return Err(PhpError::type_error(format!(
                    "Only arrays and Traversables can be unpacked, {} given",
                    twig_value::types::zend_zval_value_name(value)
                ))
                .into());
            };
            iter.rewind(diag)?;
            while iter.valid(diag)? {
                let element = iter.current(diag)?;
                let key = iter.key(diag)?;
                match key {
                    Value::String(name) => match ArrayKey::from_string(name) {
                        ArrayKey::Str(name) => {
                            target.insert(ArrayKey::Str(name), element);
                        }
                        ArrayKey::Int(_) => {
                            target.push(element)?;
                        }
                    },
                    Value::Int(_) => {
                        target.push(element)?;
                    }
                    _ => {
                        return Err(PhpError::error(
                            "Keys must be of type int|string during array unpacking",
                        )
                        .into());
                    }
                }
                iter.next(diag)?;
            }
            Ok(())
        }
        _ => Err(unpack_type_error(value, false)),
    }
}

/// An active `foreach` over `$context['_seq']`.
pub(crate) enum LoopIter {
    /// An array: iterated over a snapshot (`FE_RESET_R` takes the array by value).
    Array(twig_value::array::IntoIter),
    /// A Traversable object: its iterator and PHP's `iter->index` (`-1` after `FE_RESET_R`).
    Object {
        iter: Box<dyn ObjectIterator>,
        index: i64,
        empty: bool,
    },
}

impl LoopIter {
    /// `FE_RESET_R` (PHPSRC/Zend/zend_vm_def.h, `zend_fe_reset_iterator()`): arrays are snapshot; objects get their
    /// iterator, `rewind()` and a first `valid()`.
    pub(crate) fn start(seq: &Value, diag: &mut dyn Diagnostics) -> Result<LoopIter, Exception> {
        match seq {
            Value::Object(object) => {
                let Some(mut iter) = object.get_iterator(diag)? else {
                    return Err(PhpError::new(
                        ThrowableClass::Exception,
                        format!(
                            "Object of type {} did not create an Iterator",
                            object.class().name()
                        ),
                    )
                    .into());
                };
                iter.rewind(diag)?;
                let empty = !iter.valid(diag)?;
                Ok(LoopIter::Object {
                    iter,
                    index: -1,
                    empty,
                })
            }
            Value::Array(array) => Ok(LoopIter::Array(array.clone().into_iter())),
            // ensureTraversable() made the sequence iterable; anything else iterates zero times
            _ => Ok(LoopIter::Array(Array::new().into_iter())),
        }
    }

    /// `FE_FETCH_R`: the next (key, value), or `None` at the end.
    pub(crate) fn next(
        &mut self,
        diag: &mut dyn Diagnostics,
    ) -> Result<Option<(Value, Value)>, Exception> {
        match self {
            LoopIter::Array(iter) => Ok(iter.next().map(|(key, value)| (key.to_value(), value))),
            LoopIter::Object { iter, index, empty } => {
                if *empty {
                    return Ok(None);
                }
                *index += 1;
                if *index > 0 {
                    iter.next(diag)?;
                    if !iter.valid(diag)? {
                        *empty = true;
                        return Ok(None);
                    }
                }
                let value = iter.current(diag)?;
                let key = iter.key(diag)?;
                Ok(Some((key, value)))
            }
        }
    }
}
