//! PHP `Generator` objects for ported generator functions (PHPSRC/Zend/zend_generators.c).
//!
//! A ported generator body is a step function: each call runs the body up to its next `yield`
//! and returns the yielded `(key, value)`, or `None` when the body returns. The object reproduces
//! the engine's state machine around it: the body starts on first use
//! (`zend_generator_ensure_initialized()`), `rewind()` is only allowed at the first yield, a
//! finished generator is invalid and cannot be traversed again, and an exception thrown by the
//! body closes the generator.

use std::sync::{LazyLock, Mutex, MutexGuard};

use twig_value::object::{
    CallArgs, ClassBuilder, ClassRef, FunctionKind, Object, ObjectIterator, ObjectRef,
    PropertyEntry, PropertyPurpose, builtin,
};
use twig_value::{ArrayKey, Diagnostics, PhpError, PhpResult, PhpString, Value};

/// One step of a generator body: runs to the next `yield` (`Some((key, value))`) or to the end
/// (`None`).
pub type GeneratorStep =
    Box<dyn FnMut(&mut dyn Diagnostics) -> PhpResult<Option<(Value, Value)>> + Send>;

static GENERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    let mut b = ClassBuilder::class("Generator")
        .final_class()
        .implements(&builtin::iterator());
    for m in [
        "rewind",
        "valid",
        "current",
        "key",
        "next",
        "send",
        "throw",
        "getReturn",
        "__debugInfo",
    ] {
        b = b.method(m);
    }
    b.build()
});

/// `final class Generator implements Iterator`.
pub fn generator_class() -> ClassRef {
    GENERATOR.clone()
}

struct State {
    /// The body; `None` once the generator finished (`execute_data == NULL`) or while it runs.
    body: Option<GeneratorStep>,
    /// The body is executing (`ZEND_GENERATOR_CURRENTLY_RUNNING`).
    running: bool,
    /// The current `(key, value)`; `None` is `IS_UNDEF` (before the first yield, after the end).
    current: Option<(Value, Value)>,
    /// `ZEND_GENERATOR_AT_FIRST_YIELD`.
    at_first_yield: bool,
    /// The return value once the body returned.
    retval: Option<Value>,
}

/// A `Generator` object.
pub struct Generator {
    class: ClassRef,
    function: String,
    state: Mutex<State>,
}

impl Generator {
    /// A generator for the function `function` (`Class::method`, for `__debugInfo()`).
    pub fn new(function: String, body: GeneratorStep) -> Generator {
        Generator {
            class: generator_class(),
            function,
            state: Mutex::new(State {
                body: Some(body),
                running: false,
                current: None,
                at_first_yield: false,
                retval: None,
            }),
        }
    }

    /// Wraps into a handle.
    pub fn into_ref(self) -> ObjectRef {
        ObjectRef::new(self)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn finished(&self) -> bool {
        let st = self.state();
        st.body.is_none() && !st.running
    }

    /// `zend_generator_ensure_initialized()`.
    fn ensure_initialized(&self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        let start = {
            let st = self.state();
            st.current.is_none() && st.body.is_some()
        };
        if start {
            self.resume(diag)?;
            self.state().at_first_yield = true;
        }
        Ok(())
    }

    /// `zend_generator_resume()`.
    fn resume(&self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        let mut body = {
            let mut st = self.state();
            if st.running {
                return Err(PhpError::error(
                    "Cannot resume an already running generator",
                ));
            }
            let Some(body) = st.body.take() else {
                return Ok(());
            };
            st.at_first_yield = false;
            st.running = true;
            body
        };
        let result = body(diag);
        let mut st = self.state();
        st.running = false;
        match result {
            Ok(Some(kv)) => {
                st.current = Some(kv);
                st.body = Some(body);
                Ok(())
            }
            Ok(None) => {
                st.current = None;
                st.retval = Some(Value::Null);
                Ok(())
            }
            Err(e) => {
                st.current = None;
                Err(e)
            }
        }
    }

    /// `zend_generator_rewind()`.
    fn rewind(&self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.ensure_initialized(diag)?;
        if !self.state().at_first_yield {
            return Err(PhpError::exception(
                "Cannot rewind a generator that was already run",
            ));
        }
        Ok(())
    }

    fn valid(&self, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        self.ensure_initialized(diag)?;
        Ok(!self.finished())
    }

    fn current(&self, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        self.ensure_initialized(diag)?;
        let st = self.state();
        Ok(st
            .current
            .as_ref()
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Null))
    }

    fn key(&self, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        self.ensure_initialized(diag)?;
        let st = self.state();
        Ok(st
            .current
            .as_ref()
            .map(|(k, _)| k.clone())
            .unwrap_or(Value::Null))
    }

    fn next(&self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.ensure_initialized(diag)?;
        self.resume(diag)
    }
}

impl Object for Generator {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let name = self
            .class
            .method(method)
            .map(|m| m.name.to_string())
            .unwrap_or_else(|| method.to_string());
        let none = |args: &CallArgs| {
            args.bind(&format!("Generator::{name}"), &[], FunctionKind::Internal)
                .map(|_| ())
        };
        match method {
            "rewind" => none(&args)
                .and_then(|_| self.rewind(diag))
                .map(|_| Value::Null),
            "valid" => none(&args).and_then(|_| self.valid(diag)).map(Value::Bool),
            "current" => none(&args).and_then(|_| self.current(diag)),
            "key" => none(&args).and_then(|_| self.key(diag)),
            "next" => none(&args)
                .and_then(|_| self.next(diag))
                .map(|_| Value::Null),
            "getreturn" => {
                none(&args)?;
                self.ensure_initialized(diag)?;
                self.state().retval.clone().ok_or_else(|| {
                    PhpError::exception(
                        "Cannot get return value of a generator that hasn't returned",
                    )
                })
            }
            "__debuginfo" => {
                none(&args)?;
                let mut a = twig_value::Array::new();
                a.insert("function", Value::from(self.function.as_str()));
                Ok(Value::Array(a))
            }
            "send" | "throw" => Err(PhpError::error(format!(
                "Generator::{name}() is not supported by twig-test-support"
            ))),
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }

    /// `Generator::__debugInfo()` (PHP 8.4+): `["function" => "Class::method"]`.
    fn properties_for(
        &self,
        _this: &ObjectRef,
        purpose: PropertyPurpose,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        Ok(match purpose {
            PropertyPurpose::Debug => vec![PropertyEntry {
                key: ArrayKey::Str(PhpString::from("function")),
                value: Some(Value::from(self.function.as_str())),
                type_name: None,
            }],
            _ => Vec::new(),
        })
    }

    /// `zend_generator_get_iterator()`.
    fn get_iterator(
        &self,
        this: &ObjectRef,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Box<dyn ObjectIterator>>> {
        if self.finished() {
            return Err(PhpError::exception(
                "Cannot traverse an already closed generator",
            ));
        }
        Ok(Some(Box::new(GeneratorIterator {
            generator: this.clone(),
        })))
    }
}

/// `zend_generator_iterator_functions`.
struct GeneratorIterator {
    generator: ObjectRef,
}

impl GeneratorIterator {
    fn generator(&self) -> PhpResult<&Generator> {
        self.generator
            .downcast_ref::<Generator>()
            .ok_or_else(|| PhpError::error("not a generator"))
    }
}

impl ObjectIterator for GeneratorIterator {
    fn rewind(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.generator()?.rewind(diag)
    }

    fn valid(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        self.generator()?.valid(diag)
    }

    fn current(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        self.generator()?.current(diag)
    }

    fn key(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        self.generator()?.key(diag)
    }

    fn next(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.generator()?.next(diag)
    }
}
