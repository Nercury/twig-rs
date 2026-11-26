//! Runtime closures of node classes defined by extensions: the Rust code a compiled node calls while the template
//! renders, where a Twig node class compiles a call of a PHP method (`$this->env->getRuntime(...)->method(...)`).

use std::fmt;
use std::io;
use std::sync::Arc;

use twig_value::{Array, Diagnostics, PhpString, Value};

use super::program::Program;
use crate::deprecation::Deprecation;
use crate::error::Error;
use crate::extension::runtime::{CallResult, callables};
use crate::runtime::Environment;
use crate::source::Source;
use crate::vm::{Output, VmState};

type RuntimeFn = dyn Fn(&mut NodeCall<'_>) -> CallResult + Send + Sync;

/// A runtime closure of a node class, called by the code [`Compiler::call_runtime`](super::Compiler::call_runtime)
/// compiles. It gets the call ([`NodeCall`]: the arguments, the context, the output, the environment) and returns a
/// value, or the exception the template sees (a Twig error or a PHP throwable; see [`Exception`](crate::vm::Exception)).
///
/// A runtime is cheap to clone (the closure is shared). The program keeps the runtimes its code calls, so a runtime
/// holding state (a cache, a counter) keeps it across renders and threads: such state needs synchronization.
///
/// ```
/// use twig::lltl::NodeRuntime;
/// use twig::value::{Value, convert};
///
/// // `strtoupper((string) $value)`
/// let upper = NodeRuntime::new(|call| {
///     let value = call.arg(0).cloned().unwrap_or_default();
///     let string = convert::to_string(&value, call.diagnostics())?;
///     Ok(Value::from(string.as_bytes().to_ascii_uppercase()))
/// });
/// # let _ = upper;
/// ```
#[derive(Clone)]
pub struct NodeRuntime {
    function: Arc<RuntimeFn>,
}

impl NodeRuntime {
    /// A runtime calling `function`.
    pub fn new(function: impl Fn(&mut NodeCall<'_>) -> CallResult + Send + Sync + 'static) -> Self {
        NodeRuntime {
            function: Arc::new(function),
        }
    }

    /// Whether both are the same closure (clones of one runtime).
    pub fn ptr_eq(&self, other: &NodeRuntime) -> bool {
        Arc::ptr_eq(&self.function, &other.function)
    }

    pub(crate) fn call(&self, call: &mut NodeCall<'_>) -> CallResult {
        (self.function)(call)
    }
}

impl fmt::Debug for NodeRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NodeRuntime(..)")
    }
}

impl PartialEq for NodeRuntime {
    /// The same closure.
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

/// The output a runtime writes to: the template's output (the innermost capture, or the sink).
pub(crate) trait OutputWriter {
    fn write_output(&mut self, bytes: &[u8]) -> io::Result<()>;
}

impl OutputWriter for Output<'_> {
    fn write_output(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.write(bytes)
    }
}

/// A call of a [`NodeRuntime`]: its arguments and the running template (the context of the compiled method, its
/// output, the environment, PHP diagnostics).
pub struct NodeCall<'a> {
    pub(crate) state: &'a mut VmState,
    pub(crate) out: &'a mut dyn OutputWriter,
    pub(crate) context: &'a mut Array,
    pub(crate) program: &'a Arc<Program>,
    pub(crate) args: Vec<Value>,
}

impl NodeCall<'_> {
    /// The arguments, in the order the code put them on the stack.
    pub fn args(&self) -> &[Value] {
        &self.args
    }

    /// The argument `i`.
    pub fn arg(&self, i: usize) -> Option<&Value> {
        self.args.get(i)
    }

    /// Takes the arguments out of the call.
    pub fn take_args(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.args)
    }

    /// The context of the running method (`$context`).
    pub fn context(&self) -> &Array {
        self.context
    }

    /// The context of the running method, to change variables (`$context['name'] = ...`): what the template's code
    /// reads afterwards.
    pub fn context_mut(&mut self) -> &mut Array {
        self.context
    }

    /// Writes bytes to the template's output (`yield`): the innermost capture, or the sink.
    ///
    /// # Errors
    ///
    /// The sink's I/O error, as the `RuntimeError` `Unable to write the template output: %s.`.
    pub fn write(&mut self, bytes: &[u8]) -> CallResult<()> {
        self.out.write_output(bytes).map_err(|error| {
            Error::runtime(format!("Unable to write the template output: {error}.")).into()
        })
    }

    /// The environment.
    pub fn env(&self) -> &Environment {
        &self.state.env
    }

    /// The charset (`$this->env->getCharset()`).
    pub fn charset(&self) -> PhpString {
        self.state.env.charset_value()
    }

    /// The diagnostics sink (the running VM): pass it to `twig-value` functions, which report PHP's warnings, notices
    /// and deprecations to the environment's policy.
    pub fn diagnostics(&mut self) -> &mut dyn Diagnostics {
        &mut *self.state
    }

    /// Reports a deprecation (`trigger_deprecation()`).
    pub fn deprecate(&mut self, deprecation: Deprecation) {
        self.state.env.report_deprecation(deprecation);
    }

    /// Calls a callable value (a template arrow function, a closure, an invokable object) with positional arguments,
    /// as compiled PHP code does: `$callable(...$args)`.
    ///
    /// # Errors
    ///
    /// PHP's errors for values that are not callable and what the callable throws.
    pub fn invoke(&mut self, callable: &Value, args: Vec<Value>) -> CallResult {
        callables::invoke(self.state, callable, args)
    }

    /// Whether the template runs sandboxed (Twig's `$this->sandbox->isSandboxed($this->getSourceContext())`).
    pub fn is_sandboxed(&self) -> bool {
        self.state.env.is_sandboxed(self.program.source())
    }

    /// The name of the running template.
    pub fn template_name(&self) -> &str {
        self.program.name()
    }

    /// The source of the running template.
    pub fn source(&self) -> &Source {
        self.program.source()
    }
}

impl fmt::Debug for NodeCall<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeCall")
            .field("template", &self.program.name())
            .field("args", &self.args)
            .finish_non_exhaustive()
    }
}
