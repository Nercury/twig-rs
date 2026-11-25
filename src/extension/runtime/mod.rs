//! The runtime of filters, functions and tests: what a call compiled by the code generator runs.
//!
//! A [`TwigFilter`](super::TwigFilter), [`TwigFunction`](super::TwigFunction) or [`TwigTest`](super::TwigTest) carries
//! the reflection data of its PHP callable ([`PhpCallable`]); its implementation is a [`CallableRuntime`] stored in
//! the callable's runtime slot. The VM calls it with a [`Call`]: the PHP arguments after the injected ones (the input
//! of a filter or test first, then the template arguments bound at compile time by the
//! [`CallableArgumentsExtractor`](super::arguments::CallableArgumentsExtractor) port, variadic buckets included), and
//! access to what Twig injects (`needs_charset`, `needs_environment`, `needs_context`, `needs_is_sandboxed`, the parts
//! of a dynamic `*` name) and to the running VM (diagnostics, deprecations, calling closures).
//!
//! The environment installs the runtimes of the callables of the core, escaper, debug and string-loader extensions
//! when it initializes ([`install_runtimes`]); they live in the group modules `strings`, `arrays` and `misc`. A
//! callable without runtime fails with a `LogicException`-like error when called, never a panic.
//!
//! Host callables attach their runtime to the callable before registering it:
//!
//! ```
//! use twig::extension::runtime::CallableRuntime;
//! use twig::extension::{CallableParameter, FilterOptions, PhpCallable, TwigFilter};
//! use twig::loader::ArrayLoader;
//! use twig::runtime::Environment;
//! use twig::value::{Array, Value};
//!
//! let callable = PhpCallable::closure(vec![CallableParameter::required("value")]);
//! CallableRuntime::new(|call| {
//!     let value = call.arg(0).cloned().unwrap_or_default();
//!     let string = twig::value::convert::to_string(&value, call.diagnostics())?;
//!     Ok(Value::from(format!("[{}]", string.to_string_lossy())))
//! })
//! .attach(&callable);
//! let env = Environment::new(ArrayLoader::new([("index", "{{ 'x'|brackets }}")]));
//! env.add_filter(TwigFilter::new("brackets", Some(callable), FilterOptions::default()))?;
//! assert_eq!(env.render("index", Array::new())?, "[x]");
//! # Ok::<(), twig::Error>(())
//! ```

mod arrays;
mod misc;
mod strings;

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, LazyLock};

use twig_value::object::{CallArgs, FunctionKind, Param};
use twig_value::{Array, Diagnostics, PhpError, PhpString, ThrowableClass, Value};

use super::ExtensionSet;
use super::callable::{CallableKind, CallableType, PhpCallable, TwigCallable};
use crate::deprecation::Deprecation;
use crate::lltl::Program;
use crate::runtime::Environment;
use crate::source::Source;
use crate::vm::{Exception, VmState, invoke_value};

/// What a filter, function or test returns: a value, or the exception it throws (a Twig error or another PHP
/// throwable, see [`Exception`]).
pub type CallResult<T = Value> = Result<T, Exception>;

type RuntimeFn = dyn Fn(&mut Call<'_>) -> CallResult + Send + Sync;

/// The implementation of a filter, function or test, stored in the runtime slot of its [`PhpCallable`].
#[derive(Clone)]
pub struct CallableRuntime {
    function: Arc<RuntimeFn>,
}

impl fmt::Debug for CallableRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CallableRuntime(..)")
    }
}

impl CallableRuntime {
    /// A runtime calling `function`.
    pub fn new(function: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static) -> Self {
        CallableRuntime {
            function: Arc::new(function),
        }
    }

    /// Stores the runtime in the callable's runtime slot; returns `false` when the slot was already filled (the
    /// callable keeps its first runtime).
    pub fn attach(&self, callable: &PhpCallable) -> bool {
        callable.set_runtime(Arc::new(self.clone())).is_ok()
    }

    /// The runtime of a callable, if one is attached.
    pub fn of(callable: &PhpCallable) -> Option<&CallableRuntime> {
        callable
            .runtime()
            .and_then(|runtime| runtime.downcast_ref::<CallableRuntime>())
    }

    /// Runs the implementation.
    ///
    /// # Errors
    ///
    /// What the implementation throws.
    pub fn call(&self, call: &mut Call<'_>) -> CallResult {
        (self.function)(call)
    }
}

/// A call of a filter, function or test: its arguments and access to the running template.
pub struct Call<'a> {
    state: &'a mut VmState,
    program: &'a Arc<Program>,
    callable: &'a TwigCallable,
    args: CallArgs,
    context: Option<Array>,
}

impl<'a> Call<'a> {
    /// The callable's name as used in the template (for a dynamic callable matched through a `*` pattern: the full
    /// name).
    pub fn name(&self) -> &[u8] {
        self.callable.name()
    }

    /// The callable.
    pub fn callable(&self) -> &TwigCallable {
        self.callable
    }

    /// `filter`, `function` or `test`.
    pub fn callable_type(&self) -> CallableType {
        self.callable.callable_type()
    }

    /// The environment (`needs_environment`).
    pub fn env(&self) -> &Environment {
        &self.state.env
    }

    /// The charset (`needs_charset`: `$this->env->getCharset()`).
    pub fn charset(&self) -> PhpString {
        self.state.env.charset_value()
    }

    /// The context (`needs_context`: the current `$context`, copied when the call started); an empty array for a
    /// callable that does not need it.
    pub fn context(&self) -> Array {
        self.context.clone().unwrap_or_default()
    }

    /// Whether the calling template runs sandboxed (`needs_is_sandboxed`).
    pub fn is_sandboxed(&self) -> bool {
        self.state.env.is_sandboxed(self.program.source())
    }

    /// The parts of the name matched by the `*` of a dynamic callable (passed before the input in Twig).
    pub fn dynamic_arguments(&self) -> &[Vec<u8>] {
        self.callable.dynamic_arguments()
    }

    /// The positional PHP arguments after the injected ones: the input of a filter or test, then the template
    /// arguments.
    pub fn args(&self) -> &[Value] {
        &self.args.positional
    }

    /// The positional argument `i`.
    pub fn arg(&self, i: usize) -> Option<&Value> {
        self.args.positional.get(i)
    }

    /// The named arguments (only from unpacked string keys: `f(...{name: v})`).
    pub fn named_args(&self) -> &[(PhpString, Value)] {
        &self.args.named
    }

    /// Takes the arguments out of the call.
    pub fn take_args(&mut self) -> CallArgs {
        std::mem::take(&mut self.args)
    }

    /// Binds the arguments to PHP parameters with PHP's rules (named arguments, defaults, a trailing variadic;
    /// internal functions reject extra positional arguments, userland functions ignore them), with PHP's messages
    /// for `function`.
    ///
    /// # Errors
    ///
    /// `ArgumentCountError`/`Error` as PHP reports them.
    pub fn bind(
        &self,
        function: &str,
        params: &[Param],
        kind: FunctionKind,
    ) -> CallResult<Vec<Value>> {
        Ok(self.args.bind(function, params, kind)?)
    }

    /// The diagnostics sink (the running VM): pass it to `twig-value` functions.
    pub fn diagnostics(&mut self) -> &mut dyn Diagnostics {
        &mut *self.state
    }

    /// Reports a deprecation (`trigger_deprecation()`).
    pub fn deprecate(&mut self, deprecation: Deprecation) {
        self.state.env.report_deprecation(deprecation);
    }

    /// Calls a callable value (a template arrow function or another closure or invokable object) with positional
    /// arguments: `$arrow($v, $k)`.
    ///
    /// # Errors
    ///
    /// What the callable throws.
    pub fn invoke(&mut self, callable: &Value, args: Vec<Value>) -> CallResult {
        invoke_value(self.state, callable, args)
    }

    /// The name of the calling template.
    pub fn template_name(&self) -> &str {
        self.program.name()
    }

    /// The source of the calling template.
    pub fn source(&self) -> &Source {
        self.program.source()
    }

    pub(crate) fn state(&mut self) -> &mut VmState {
        self.state
    }
}

/// Calls a compiled filter, function or test with its argument list (the `Call` instruction).
pub(crate) fn call_callable(
    state: &mut VmState,
    program: &Arc<Program>,
    callable: &TwigCallable,
    list: crate::vm::ArgList,
) -> CallResult {
    let runtime = callable.callable().and_then(CallableRuntime::of).cloned();
    let Some(runtime) = runtime else {
        return Err(PhpError::new(
            ThrowableClass::LogicException,
            format!(
                "The {} \"{}\" has no runtime implementation.",
                callable.callable_type().as_str(),
                String::from_utf8_lossy(callable.name())
            ),
        )
        .into());
    };
    let mut call = Call {
        state,
        program,
        callable,
        args: list.args,
        context: list.context,
    };
    runtime.call(&mut call)
}

/// The runtimes of the core callables by the reflection name of their PHP callable (`Class::method` or the
/// function name).
pub(crate) struct RuntimeTable {
    runtimes: HashMap<&'static str, CallableRuntime>,
}

impl RuntimeTable {
    fn new() -> Self {
        RuntimeTable {
            runtimes: HashMap::new(),
        }
    }

    /// Registers the implementation of a callable.
    pub(crate) fn register(
        &mut self,
        name: &'static str,
        function: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static,
    ) {
        self.runtimes.insert(name, CallableRuntime::new(function));
    }

    /// Registers a callable whose implementation is not written yet: calling it fails with a clear error.
    pub(crate) fn unimplemented(&mut self, name: &'static str, group: &'static str) {
        self.register(name, move |call| {
            Err(PhpError::new(
                ThrowableClass::LogicException,
                format!(
                    "The {} \"{}\" is not implemented yet ({} runtimes, {name}).",
                    call.callable_type().as_str(),
                    String::from_utf8_lossy(call.name()),
                    group
                ),
            )
            .into())
        });
    }

    fn get(&self, name: &str) -> Option<&CallableRuntime> {
        self.runtimes.get(name)
    }
}

/// The table of every core runtime, built once.
static CORE_RUNTIMES: LazyLock<RuntimeTable> = LazyLock::new(|| {
    let mut table = RuntimeTable::new();
    strings::register(&mut table);
    arrays::register(&mut table);
    misc::register(&mut table);
    // the template functions (include, include_only, source, template_from_string, render_sandboxed) live with the
    // template runtime; registered last, they replace the group's placeholders
    crate::runtime::functions::register(&mut table);
    table
});

/// The key of a PHP callable in the runtime table.
fn runtime_key(callable: &PhpCallable) -> Option<String> {
    match callable.kind() {
        CallableKind::Function(name) => Some(name.clone()),
        CallableKind::Method { class, method } => Some(format!("{class}::{method}")),
        CallableKind::Closure => None,
    }
}

/// Attaches the core runtimes to the callables of the registry that have none yet: the callables of the core,
/// escaper, debug and string-loader extensions (and any host callable reusing one of their PHP callables).
pub fn install_runtimes(set: &ExtensionSet) {
    let install = |callable: Option<&PhpCallable>| {
        let Some(callable) = callable else { return };
        if callable.runtime().is_some() {
            return;
        }
        if let Some(key) = runtime_key(callable)
            && let Some(runtime) = CORE_RUNTIMES.get(&key)
        {
            runtime.attach(callable);
        }
    };
    for (_, filter) in set.filters() {
        install(filter.callable());
    }
    for (_, function) in set.functions() {
        install(function.callable());
    }
    for (_, test) in set.tests() {
        install(test.callable());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::{
        CoreExtension, DebugExtension, EscaperExtension, Extension, SandboxBridgeExtension,
        StringLoaderExtension,
    };

    /// Every PHP callable of the bundled extensions has an entry in the runtime table, implemented or not, so the
    /// group modules list all the work that remains.
    #[test]
    fn every_bundled_callable_has_a_runtime_entry() {
        let extensions: Vec<Box<dyn Extension>> = vec![
            Box::new(CoreExtension::new()),
            Box::new(EscaperExtension::new()),
            Box::new(DebugExtension::new()),
            Box::new(StringLoaderExtension::new()),
            Box::new(SandboxBridgeExtension::new()),
        ];
        let mut missing = Vec::new();
        let mut check = |kind: &str, name: &[u8], callable: Option<&PhpCallable>| {
            if let Some(key) = callable.and_then(runtime_key)
                && CORE_RUNTIMES.get(&key).is_none()
            {
                missing.push(format!("{kind} {}: {key}", String::from_utf8_lossy(name)));
            }
        };
        for extension in &extensions {
            for filter in extension.filters() {
                check("filter", filter.name(), filter.callable());
            }
            for function in extension.functions() {
                check("function", function.name(), function.callable());
            }
            for test in extension.tests() {
                check("test", test.name(), test.callable());
            }
        }
        assert!(missing.is_empty(), "no runtime entry: {missing:#?}");
    }
}
