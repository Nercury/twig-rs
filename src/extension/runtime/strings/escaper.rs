//! `Twig\Runtime\EscaperRuntime` (TWIG/src/Runtime/EscaperRuntime.php): the runtime of the `escape` and `e`
//! filters, with the custom escapers and the safe classes of an environment.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use twig_value::builtins::string::IconvBackend;
use twig_value::object::{CallArgs, FunctionKind, ObjectRef, Param};
use twig_value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::{Diagnostics, PhpString, Value, convert};

use super::{Call, CallResult};
use crate::error::Error;
use crate::escaper::{self, BUILTIN_STRATEGIES, EscapeError, Strategy, implied_strategies};
use crate::extension::TwigCallable;
use crate::extension::runtime::CallableRuntime;
use crate::runtime::{Markup, default_iconv_backend};
use crate::vm::VmState;
use twig_value::Array;

/// A custom escaper registered with [`EscaperRuntime::set_escaper`]: called with the value to escape and the
/// charset (the `$charset` argument of the filter, or the runtime's charset), it returns the escaped value.
///
/// Twig documents escapers as `callable(string $string, string $charset): string`, but `escape()` passes the value
/// as it came when it is neither a string nor `Stringable` (an int, `null`, an array, any object), and returns what
/// the escaper returns. The diagnostics sink receives the PHP warnings and notices the escaper raises.
pub type EscaperCallback =
    Arc<dyn Fn(&Value, &PhpString, &mut dyn Diagnostics) -> CallResult + Send + Sync>;

/// The escaper runtime of an environment: Twig's `EscaperRuntime`, created with the charset of the environment,
/// holding the custom escapers (`setEscaper()`) and the safe classes (`addSafeClass()`), and running the `escape`
/// and `e` filters ([`escape`](Self::escape)). An environment's runtime is
/// [`Environment::escaper_runtime`](crate::runtime::Environment::escaper_runtime). The built-in strategies convert a
/// charset other than UTF-8 with the runtime's [`IconvBackend`] (an environment's runtime has the environment's).
///
/// It can be shared between threads; the escapers and safe classes can change at any time.
///
/// ```
/// use std::sync::Arc;
/// use twig::extension::runtime::EscaperRuntime;
/// use twig::value::{Diagnostic, Value, convert};
///
/// let runtime = EscaperRuntime::new("UTF-8");
/// runtime.set_escaper("csv", Arc::new(|value, _charset, diag| {
///     let string = convert::to_string(value, diag)?;
///     Ok(Value::from(format!("\"{}\"", string.to_string_lossy().replace('"', "\"\""))))
/// }));
/// let mut diag: Vec<Diagnostic> = Vec::new();
/// let escaped = runtime.escape(Value::from("a\"b"), b"csv", None, false, &mut diag)?;
/// assert_eq!(escaped, Value::from("\"a\"\"b\""));
/// # Ok::<(), twig::vm::Exception>(())
/// ```
pub struct EscaperRuntime {
    /// Twig's `$charset` (`new EscaperRuntime($charset)`).
    charset: PhpString,
    /// The charset converter of the built-in strategies (PHP's `iconv()`).
    iconv: Arc<dyn IconvBackend>,
    state: RwLock<State>,
}

/// The mutable part of the runtime.
#[derive(Default)]
struct State {
    /// Twig's `$escapers`, in registration order (a strategy set again keeps its place).
    escapers: Vec<(PhpString, EscaperCallback)>,
    /// Twig's `$safeClasses`: the declared strategies per class name, in insertion order (the classes of the
    /// objects met on the autoescape path are cached here too, with the strategies they inherit).
    safe_classes: Vec<(String, Vec<PhpString>)>,
    /// Twig's `$safeLookup`: per strategy, the classes safe for it.
    safe_lookup: HashMap<PhpString, HashSet<String>>,
}

impl State {
    fn safe_class_index(&self, class: &str) -> Option<usize> {
        self.safe_classes.iter().position(|(name, _)| name == class)
    }

    /// Twig: `EscaperRuntime::addSafeLookup()`: the class is safe for each strategy and the strategies they imply.
    fn add_safe_lookup(&mut self, class: &str, strategies: &[PhpString]) {
        for strategy in strategies {
            self.safe_lookup
                .entry(strategy.clone())
                .or_default()
                .insert(class.to_owned());
            for implied in implied_strategies(strategy) {
                self.safe_lookup
                    .entry(PhpString::from(*implied))
                    .or_default()
                    .insert(class.to_owned());
            }
        }
    }

    fn is_listed(&self, strategy: &[u8], class: &str) -> bool {
        self.safe_lookup
            .get(strategy)
            .is_some_and(|classes| classes.contains(class))
    }
}

impl fmt::Debug for EscaperRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("EscaperRuntime")
            .field("charset", &self.charset)
            .field("iconv_backend", &self.iconv)
            .field(
                "escapers",
                &state
                    .escapers
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>(),
            )
            .field("safe_classes", &state.safe_classes)
            .finish()
    }
}

impl Default for EscaperRuntime {
    /// Twig's default charset, `UTF-8`.
    fn default() -> Self {
        EscaperRuntime::new("UTF-8")
    }
}

/// Whether a built-in strategy returns `string` unchanged, so that the string itself can be returned: with the
/// `UTF-8` charset (no conversion), `html` keeps valid UTF-8 without `&<>"'` (`htmlspecialchars()` with
/// `ENT_QUOTES | ENT_SUBSTITUTE` replaces only those and invalid sequences, and PHP's decoder accepts exactly the
/// well-formed UTF-8 that Rust's does), `js`, `css` and `html_attr(_relaxed)` keep their ASCII character sets; `url`
/// (`rawurlencode()`, whatever the charset) keeps `A-Za-z0-9-._~`.
fn escapes_to_itself(strategy: Strategy, charset: &[u8], string: &[u8]) -> bool {
    let utf8 = charset == b"UTF-8";
    match strategy {
        Strategy::Html => {
            utf8 && !string
                .iter()
                .any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\''))
                && std::str::from_utf8(string).is_ok()
        }
        Strategy::Js => {
            utf8 && string
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b',' | b'.' | b'_'))
        }
        Strategy::Css => utf8 && string.iter().all(u8::is_ascii_alphanumeric),
        Strategy::HtmlAttr => {
            utf8 && string
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b',' | b'.' | b'-' | b'_'))
        }
        Strategy::HtmlAttrRelaxed => {
            utf8 && string.iter().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(b, b',' | b'.' | b'-' | b'_' | b':' | b'@' | b'[' | b']')
            })
        }
        Strategy::Url => string
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')),
    }
}

/// `in_array($strategy, ['html', 'js', 'css', 'html_attr', 'html_attr_relaxed', 'url'], true)`.
fn is_builtin(strategy: &[u8]) -> bool {
    BUILTIN_STRATEGIES
        .iter()
        .any(|name| name.as_bytes() == strategy)
}

impl EscaperRuntime {
    /// A runtime escaping with `charset` by default and converting charsets with
    /// [`default_iconv_backend()`]. Twig: `new EscaperRuntime($charset)`.
    ///
    /// A runtime keeps its backend wherever it is registered: for an environment with another
    /// [`iconv_backend`](crate::runtime::EnvironmentOptions::iconv_backend), register
    /// [`with_iconv_backend`](Self::with_iconv_backend)`(charset, env.iconv_backend())` instead.
    pub fn new(charset: impl Into<PhpString>) -> Self {
        Self::with_iconv_backend(charset, default_iconv_backend())
    }

    /// A runtime escaping with `charset` by default and converting charsets with `iconv` (an environment creates its
    /// runtime with its [`iconv_backend`](crate::runtime::Environment::iconv_backend); a runtime registered with
    /// [`add_runtime`](crate::runtime::Environment::add_runtime) keeps the backend it was created with).
    pub fn with_iconv_backend(charset: impl Into<PhpString>, iconv: Arc<dyn IconvBackend>) -> Self {
        EscaperRuntime {
            charset: charset.into(),
            iconv,
            state: RwLock::new(State::default()),
        }
    }
}

impl EscaperRuntime {
    /// The default charset.
    pub fn charset(&self) -> PhpString {
        self.charset.clone()
    }

    /// The charset converter of the built-in strategies.
    pub fn iconv_backend(&self) -> Arc<dyn IconvBackend> {
        Arc::clone(&self.iconv)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, State> {
        self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, State> {
        self.state.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Defines the escaper of a strategy. The built-in strategies (`html`, `js`, `css`, `html_attr`,
    /// `html_attr_relaxed`, `url`) cannot be replaced: `escape()` handles them first.
    ///
    /// Twig: `EscaperRuntime::setEscaper($strategy, callable $callable)`.
    pub fn set_escaper(&self, strategy: impl Into<PhpString>, escaper: EscaperCallback) {
        let strategy = strategy.into();
        let mut state = self.write();
        match state
            .escapers
            .iter_mut()
            .find(|(name, _)| *name == strategy)
        {
            Some(entry) => entry.1 = escaper,
            None => state.escapers.push((strategy, escaper)),
        }
    }

    /// The custom escapers, in registration order. Twig: `EscaperRuntime::getEscapers()`.
    pub fn escapers(&self) -> Vec<(PhpString, EscaperCallback)> {
        self.read().escapers.clone()
    }

    /// Replaces the safe classes. Twig: `EscaperRuntime::setSafeClasses(array $safeClasses)`.
    pub fn set_safe_classes<C, S, I>(&self, classes: impl IntoIterator<Item = (C, I)>)
    where
        C: AsRef<str>,
        S: Into<PhpString>,
        I: IntoIterator<Item = S>,
    {
        {
            let mut state = self.write();
            state.safe_classes.clear();
            state.safe_lookup.clear();
        }
        for (class, strategies) in classes {
            self.add_safe_class(class.as_ref(), strategies);
        }
    }

    /// Declares that the objects of `class` (a class or an interface; a leading `\` is ignored) are safe for
    /// `strategies` when they are auto-escaped (printed with `{{ }}`); `html_attr` implies `html_attr_relaxed` and
    /// `html`, `html_attr_relaxed` implies `html`, `all` covers every strategy. Subclasses and implementations
    /// inherit the declaration when one of their objects is first auto-escaped. The `escape` filter called
    /// explicitly always escapes.
    ///
    /// Twig: `EscaperRuntime::addSafeClass(string $class, array $strategies)`.
    pub fn add_safe_class<S: Into<PhpString>>(
        &self,
        class: &str,
        strategies: impl IntoIterator<Item = S>,
    ) {
        let class = class.trim_start_matches('\\');
        let strategies: Vec<PhpString> = strategies.into_iter().map(Into::into).collect();
        let mut state = self.write();
        match state.safe_class_index(class) {
            Some(index) => state.safe_classes[index]
                .1
                .extend(strategies.iter().cloned()),
            None => state
                .safe_classes
                .push((class.to_owned(), strategies.clone())),
        }
        state.add_safe_lookup(class, &strategies);
    }

    /// The safe classes with their strategies, in insertion order: the declared ones and the classes of the objects
    /// auto-escaped so far (with what they inherited). Twig: the internal `EscaperRuntime::$safeClasses`.
    pub fn safe_classes(&self) -> Vec<(String, Vec<PhpString>)> {
        self.read().safe_classes.clone()
    }
}

impl EscaperRuntime {
    /// Whether an object's class is safe for `strategy` on the autoescape path: the class inherits the strategies
    /// of its parents and interfaces the first time it is met, then the lookup table decides.
    fn is_safe_object(&self, object: &ObjectRef, strategy: &[u8]) -> bool {
        let class = object.class();
        let name = class.name();
        let mut state = self.write();
        if state.safe_class_index(name).is_none() {
            let own = state.safe_classes.len();
            state.safe_classes.push((name.to_owned(), Vec::new()));
            // `class_parents($string) + class_implements($string)`: parents nearest first, then interfaces
            let ancestors = class
                .parents()
                .into_iter()
                .chain(class.interfaces().iter().cloned());
            for ancestor in ancestors {
                let Some(index) = state.safe_class_index(ancestor.name()) else {
                    continue;
                };
                let inherited = state.safe_classes[index].1.clone();
                // `array_unique(array_merge($this->safeClasses[$c], $this->safeClasses[$class]))`
                let merged = &mut state.safe_classes[own].1;
                for strategy in &inherited {
                    if !merged.contains(strategy) {
                        merged.push(strategy.clone());
                    }
                }
                state.add_safe_lookup(name, &inherited);
            }
        }
        state.is_listed(strategy, name) || state.is_listed(b"all", name)
    }

    /// [`escape`](Self::escape) of a borrowed value: the value itself (borrowed) where `escape()` returns it
    /// unchanged (a string a built-in strategy leaves alone, the empty string, a value that is neither a string nor an
    /// object with a built-in strategy), else what `escape()` returns for a copy of it.
    pub(crate) fn escape_ref<'v>(
        &self,
        value: &'v Value,
        strategy: &[u8],
        charset: Option<&[u8]>,
        autoescape: bool,
        diag: &mut dyn Diagnostics,
    ) -> CallResult<Cow<'v, Value>> {
        match value {
            Value::String(string) => {
                if string.is_empty() {
                    return Ok(Cow::Borrowed(value));
                }
                let Some(builtin) = Strategy::from_name(strategy) else {
                    return self
                        .escape(value.clone(), strategy, charset, autoescape, diag)
                        .map(Cow::Owned);
                };
                // `$charset ?: $this->charset`: null, '' and '0' fall back
                let charset = match charset {
                    Some(charset) if !charset.is_empty() && charset != b"0" => charset,
                    _ => self.charset.as_bytes(),
                };
                if escapes_to_itself(builtin, charset, string) {
                    return Ok(Cow::Borrowed(value));
                }
                match escaper::escape(string, builtin, charset, &*self.iconv, diag) {
                    Ok(Some(escaped)) => Ok(Cow::Owned(Value::String(escaped))),
                    // a failed `iconv()` returned `false` (after its warning or notice)
                    Ok(None) => Ok(Cow::Owned(Value::Bool(false))),
                    Err(EscapeError::InvalidUtf8) => {
                        Err(Error::runtime(escaper::INVALID_UTF8_MESSAGE).into())
                    }
                    Err(EscapeError::Php(error)) => Err(error.into()),
                }
            }
            // we return the input as is (which can be of any type)
            Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Array(_)
                if is_builtin(strategy) =>
            {
                Ok(Cow::Borrowed(value))
            }
            _ => self
                .escape(value.clone(), strategy, charset, autoescape, diag)
                .map(Cow::Owned),
        }
    }

    /// The escaper of a custom strategy.
    fn escaper(&self, strategy: &[u8]) -> Option<EscaperCallback> {
        self.read()
            .escapers
            .iter()
            .find(|(name, _)| name.as_bytes() == strategy)
            .map(|(_, escaper)| Arc::clone(escaper))
    }

    /// Escapes a value. `charset` `None`, `""` or `"0"` stands for the runtime's charset; `autoescape` tells that
    /// the call comes from auto-escaping (`{{ }}`) rather than from an explicit filter.
    ///
    /// Twig: `EscaperRuntime::escape($string, string $strategy = 'html', ?string $charset = null, bool $autoescape =
    /// false)` (reference/twig/core-extension.md §3.2):
    ///
    /// - with `autoescape`, a [`Markup`] safe for the strategy is returned as is, other markup is escaped as a
    ///   string;
    /// - a value that is not a string: a `Stringable` object is converted (with `autoescape`, an object of a safe
    ///   class is returned converted but unescaped); with a built-in strategy anything else is returned as is (ints,
    ///   floats, bools, `null`, arrays, objects), with a custom strategy it goes to the escaper unconverted;
    /// - `''` is returned before the strategy is looked at;
    /// - the built-in strategies are [`escaper::escape`]; a failed `iconv()` gives `false`.
    ///
    /// # Errors
    ///
    /// `RuntimeError`s `Invalid escaping strategy "%s" (valid ones: "%s").` and `The string to escape is not a valid
    /// UTF-8 string.`; what `__toString()`, `Markup::isSafeFor()` or a custom escaper throws; PHP diagnostics the
    /// sink turns into exceptions.
    pub fn escape(
        &self,
        value: Value,
        strategy: &[u8],
        charset: Option<&[u8]>,
        autoescape: bool,
        diag: &mut dyn Diagnostics,
    ) -> CallResult {
        if autoescape
            && let Value::Object(object) = &value
            && Markup::is_markup(object)
        {
            let safe = match object.downcast_ref::<Markup>() {
                // the method of Twig's own class, without the dynamic call
                Some(markup) => markup.is_safe_for(strategy, diag)?,
                None => {
                    let safe = object.call_method(
                        "isSafeFor",
                        CallArgs::from(vec![Value::from(strategy)]),
                        diag,
                    )?;
                    convert::to_bool(&safe, diag)?
                }
            };
            if safe {
                return Ok(value);
            }
            let content = object.to_php_string(diag)?;
            return self.escape(Value::String(content), strategy, charset, false, diag);
        }
        let value = match value {
            Value::String(_) => value,
            Value::Object(object) if object.instance_of("Stringable") => {
                if autoescape && self.is_safe_object(&object, strategy) {
                    return Ok(Value::String(object.to_php_string(diag)?));
                }
                Value::String(object.to_php_string(diag)?)
            }
            // we return the input as is (which can be of any type)
            other if is_builtin(strategy) => return Ok(other),
            other => other,
        };
        if value.as_str().is_some_and(PhpString::is_empty) {
            return Ok(value);
        }
        // `$charset ?: $this->charset`: null, '' and '0' fall back
        let charset = match charset {
            Some(charset) if !charset.is_empty() && charset != b"0" => charset,
            _ => self.charset.as_bytes(),
        };
        if let Some(builtin) = Strategy::from_name(strategy) {
            let Value::String(string) = &value else {
                // non-strings returned above
                return Ok(value);
            };
            if escapes_to_itself(builtin, charset, string) {
                return Ok(value);
            }
            return match escaper::escape(string, builtin, charset, &*self.iconv, diag) {
                Ok(Some(escaped)) => Ok(Value::String(escaped)),
                // a failed `iconv()` returned `false` (after its warning or notice)
                Ok(None) => Ok(Value::Bool(false)),
                Err(EscapeError::InvalidUtf8) => {
                    Err(Error::runtime(escaper::INVALID_UTF8_MESSAGE).into())
                }
                Err(EscapeError::Php(error)) => Err(error.into()),
            };
        }
        if let Some(escaper) = self.escaper(strategy) {
            return escaper(&value, &PhpString::from(charset), diag);
        }
        let custom: Vec<PhpString> = self
            .read()
            .escapers
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        Err(Error::runtime(escaper::invalid_strategy_message(
            strategy,
            custom.iter().map(PhpString::as_bytes),
        ))
        .into())
    }
}

#[cfg(test)]
mod tests;

/// The PHP name of the escape runtime method, used in its argument errors.
const ESCAPE: &str = "Twig\\Runtime\\EscaperRuntime::escape";

/// The runtime of the `escape` and `e` filters (the VM recognizes it, see [`EscapeConstants`]).
pub(crate) static ESCAPE_RUNTIME: LazyLock<CallableRuntime> =
    LazyLock::new(|| CallableRuntime::new(escape));

/// A call of the `escape` filter of the core runtime ([`ESCAPE_RUNTIME`]) with a value and constant arguments that
/// bind to `escape(string $strategy = 'html', ?string $charset = null, bool $autoescape = false)` as they are (see
/// [`binds_unchanged`]), which compiled templates make for every auto-escaped print: the VM calls
/// [`EscaperRuntime::escape`] directly, without an argument list.
pub(crate) struct EscapeConstants<'a> {
    strategy: &'a [u8],
    charset: Option<&'a [u8]>,
    autoescape: bool,
}

impl<'a> EscapeConstants<'a> {
    /// The call of `callable` with the arguments `constants` after the value, when it is such a call.
    pub(crate) fn of(callable: &TwigCallable, constants: &'a Array) -> Option<EscapeConstants<'a>> {
        let runtime = callable.callable().and_then(CallableRuntime::of)?;
        if !runtime.same(&ESCAPE_RUNTIME) || constants.len() > 3 {
            return None;
        }
        let mut values = constants.values();
        let strategy = match values.next() {
            None => b"html".as_slice(),
            Some(Value::String(strategy)) => strategy.as_bytes(),
            Some(_) => return None,
        };
        let charset = match values.next() {
            None | Some(Value::Null) => None,
            Some(Value::String(charset)) => Some(charset.as_bytes()),
            Some(_) => return None,
        };
        let autoescape = match values.next() {
            None => false,
            Some(Value::Bool(autoescape)) => *autoescape,
            Some(_) => return None,
        };
        Some(EscapeConstants {
            strategy,
            charset,
            autoescape,
        })
    }

    /// `escape($value, ...)`: what [`escape`] does for these arguments.
    pub(crate) fn call(&self, state: &mut VmState, value: Value) -> CallResult {
        let runtime = state.take_escaper_runtime();
        let result = runtime.escape(value, self.strategy, self.charset, self.autoescape, state);
        state.restore_escaper_runtime(runtime);
        result
    }

    /// [`call`](Self::call) of a borrowed value: a value the runtime returns unchanged comes back borrowed.
    pub(crate) fn call_ref<'v>(
        &self,
        state: &mut VmState,
        value: &'v Value,
    ) -> CallResult<Cow<'v, Value>> {
        let runtime = state.take_escaper_runtime();
        let result = runtime.escape_ref(value, self.strategy, self.charset, self.autoescape, state);
        state.restore_escaper_runtime(runtime);
        result
    }
}

/// The runtime of the `escape` and `e` filters: Twig's `EscaperRuntime::escape()` called on the environment's
/// runtime (`$this->env->getRuntime(EscaperRuntime::class)`, created with the charset of the environment when a
/// template first escapes).
fn escape(call: &mut Call<'_>) -> CallResult {
    // the runtime (and its charset) exists before the arguments are checked
    let runtime = call.state().take_escaper_runtime();
    let result = escape_with(&runtime, call);
    call.state().restore_escaper_runtime(runtime);
    result
}

/// The parameters of `EscaperRuntime::escape()`.
static ESCAPE_PARAMS: LazyLock<[Param; 4]> = LazyLock::new(|| {
    [
        Param::required("string"),
        Param::optional("strategy", "html"),
        Param::optional("charset", Value::Null),
        Param::optional("autoescape", false),
    ]
});

/// Whether positional arguments bind to `escape($string, string $strategy = 'html', ?string $charset = null, bool
/// $autoescape = false)` as they are: the value, then a string strategy, a string or `null` charset and a boolean
/// flag where given (extra arguments are ignored). PHP's binding and type checks then neither fail nor convert
/// anything; this is how compiled templates call the filter.
fn binds_unchanged(args: &[Value]) -> bool {
    !args.is_empty()
        && args.get(1).is_none_or(|v| matches!(v, Value::String(_)))
        && args
            .get(2)
            .is_none_or(|v| matches!(v, Value::String(_) | Value::Null))
        && args.get(3).is_none_or(|v| matches!(v, Value::Bool(_)))
}

/// `$runtime->escape(...)` with the arguments of a call: the typed parameters (`string $strategy = 'html'`,
/// `?string $charset = null`, `bool $autoescape = false`) are checked in order like PHP does for a userland method
/// (extra arguments are ignored).
fn escape_with(runtime: &EscaperRuntime, call: &mut Call<'_>) -> CallResult {
    if call.named_args().is_empty() && binds_unchanged(call.args()) {
        let mut args = call.args_mut().iter_mut().map(std::mem::take);
        let value = args.next().unwrap_or_default();
        let strategy = args.next();
        let charset = args.next();
        let autoescape = matches!(args.next(), Some(Value::Bool(true)));
        let strategy = match &strategy {
            Some(Value::String(strategy)) => strategy.as_bytes(),
            _ => b"html",
        };
        let charset = match &charset {
            Some(Value::String(charset)) => Some(charset.as_bytes()),
            _ => None,
        };
        return runtime.escape(value, strategy, charset, autoescape, call.diagnostics());
    }
    let args = call.bind(ESCAPE, &*ESCAPE_PARAMS, FunctionKind::User)?;
    let mut args = args.into_iter();
    let value = args.next().unwrap_or_default();
    let diag = call.diagnostics();
    let strategy = match verify_user_arg(
        args.next().unwrap_or_default(),
        &DeclaredType::builtin(TypeMask::STRING),
        &UserArg::new(ESCAPE, 2, "strategy"),
        diag,
    )? {
        Value::String(strategy) => strategy,
        other => convert::to_string(&other, diag)?,
    };
    let charset = match verify_user_arg(
        args.next().unwrap_or_default(),
        &DeclaredType::builtin(TypeMask::STRING | TypeMask::NULL),
        &UserArg::new(ESCAPE, 3, "charset"),
        diag,
    )? {
        Value::String(charset) => Some(charset),
        _ => None,
    };
    let autoescape = verify_user_arg(
        args.next().unwrap_or_default(),
        &DeclaredType::builtin(TypeMask::BOOL),
        &UserArg::new(ESCAPE, 4, "autoescape"),
        diag,
    )?;
    let autoescape = matches!(autoescape, Value::Bool(true));
    runtime.escape(value, &strategy, charset.as_deref(), autoescape, diag)
}
