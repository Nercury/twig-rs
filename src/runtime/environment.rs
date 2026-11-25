//! `Twig\Environment`: options, extensions, globals, the template cache and the render entry points.

use std::any::Any;
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use twig_date::DateContext;
use twig_value::object::ClassRef;
use twig_value::{Array, Diagnostic, Level, PhpError, PhpResult, PhpString, ThrowableClass, Value};

use super::constants::Constants;
use super::regex::PcreRegexValidator;
use super::sandbox::{SandboxExtension, SecurityChecker, ensure_traits_allowed};
use super::template::{BlockRef, Template, TemplateWrapper, Traits};
use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::{Error, ErrorKind, Result};
use crate::escaper::DefaultStrategy;
use crate::extension::{
    EscaperExtension, Extension, ExtensionSet, NodeVisitor, TokenParser, TwigFilter, TwigFunction,
    TwigTest, UndefinedFilterCallback, UndefinedFunctionCallback, UndefinedTestCallback,
    UndefinedTokenParserCallback,
};
use crate::lexer::LexerOptions;
use crate::lltl::{self, CompileOptions, Program};
use crate::loader::Loader;
use crate::parser::{ParseContext, compile_source_error};
use crate::source::Source;
use crate::visitor::OptimizerExtension;
use crate::vm::Sink;

/// `Twig\Extension\YieldNotReadyExtension` (TWIG/src/Extension/YieldNotReadyExtension.php). With `use_yield`, Twig's
/// visitor rejects nodes that are not flagged `#[YieldReady]`; every node twig-rs compiles is yield-ready, so the
/// extension registers nothing and only keeps `has_extension()` and the extension list as in Twig.
#[derive(Debug)]
struct YieldNotReadyExtension;

impl Extension for YieldNotReadyExtension {
    fn class_name(&self) -> &str {
        "Twig\\Extension\\YieldNotReadyExtension"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// What happens to PHP warnings, notices and deprecations raised while compiling and rendering (and to Twig's
/// deprecations).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum DiagnosticsPolicy {
    /// Discard them (PHP with `error_reporting(0)`).
    #[default]
    Ignore,
    /// Record them; [`Environment::take_notices`] returns them.
    Collect,
    /// Warnings and notices become exceptions (`ErrorException`, which Twig wraps like any other non-Twig exception),
    /// as PHPUnit's `convertWarningsToExceptions`/`convertNoticesToExceptions` do in Twig's test suite; deprecations are
    /// recorded.
    Error,
}

/// A recorded diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A PHP warning, notice or deprecation.
    Php(Diagnostic),
    /// A deprecation raised with `trigger_deprecation()` (Twig's own, a deprecated callable, `{% deprecated %}`).
    Deprecation(Deprecation),
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Notice::Php(diagnostic) => write!(f, "{diagnostic}"),
            Notice::Deprecation(deprecation) => write!(f, "Deprecated: {deprecation}"),
        }
    }
}

/// The options of an environment, with Twig's names and defaults (`Environment::__construct()`,
/// TWIG/src/Environment.php).
///
/// Twig's `cache` option has no equivalent: compiled programs are kept in memory per environment.
#[derive(Debug, Clone)]
pub struct EnvironmentOptions {
    /// `debug` (default `false`).
    pub debug: bool,
    /// `charset` (default `UTF-8`); upper-cased like `setCharset()`, `UTF8` becomes `UTF-8`.
    pub charset: String,
    /// `strict_variables` (default `false`).
    pub strict_variables: bool,
    /// `autoescape` (default `html`): a strategy name (`name` guesses it from the template name), `false`, or a
    /// callback; given to the `EscaperExtension`.
    pub autoescape: DefaultStrategy,
    /// `auto_reload` (default: follows `debug`).
    pub auto_reload: Option<bool>,
    /// `optimizations` (default `-1`, all).
    pub optimizations: i64,
    /// `use_yield` (default `false`; both modes render identically).
    pub use_yield: bool,
    /// The lexer options (Twig's `new Lexer($env, $options)`).
    pub lexer: LexerOptions,
    /// The maximum depth of nested template calls (twig-rs; PHP has no such limit).
    pub max_depth: usize,
    /// The diagnostics policy.
    pub diagnostics: DiagnosticsPolicy,
}

/// The default of [`EnvironmentOptions::max_depth`].
pub const DEFAULT_MAX_DEPTH: usize = 200;

impl Default for EnvironmentOptions {
    fn default() -> Self {
        EnvironmentOptions {
            debug: false,
            charset: "UTF-8".to_owned(),
            strict_variables: false,
            autoescape: DefaultStrategy::html(),
            auto_reload: None,
            optimizations: -1,
            use_yield: false,
            lexer: LexerOptions::default(),
            max_depth: DEFAULT_MAX_DEPTH,
            diagnostics: DiagnosticsPolicy::Ignore,
        }
    }
}

/// Twig: `Environment::setCharset()`: upper case, `UTF8` → `UTF-8`.
fn normalize_charset(charset: &str) -> String {
    let upper = charset.to_ascii_uppercase();
    if upper == "UTF8" {
        "UTF-8".to_owned()
    } else {
        upper
    }
}

struct Options {
    debug: bool,
    charset: PhpString,
    strict_variables: bool,
    autoescape: DefaultStrategy,
    auto_reload: bool,
    optimizations: i64,
    use_yield: bool,
    max_depth: usize,
    policy: DiagnosticsPolicy,
}

#[derive(Default)]
struct Globals {
    globals: Array,
    resolved: Option<Array>,
}

struct Inner {
    options: RwLock<Options>,
    loader: RwLock<Arc<dyn Loader>>,
    parse: RwLock<ParseContext>,
    runtime_initialized: AtomicBool,
    runtimes_installed: OnceLock<()>,
    globals: Mutex<Globals>,
    /// The compiled files (template class families) by class key.
    programs: RwLock<HashMap<Vec<u8>, Arc<Program>>>,
    /// The template instances by class key (`___N` appended for embedded templates).
    templates: RwLock<HashMap<Vec<u8>, Arc<Template>>>,
    notices: Mutex<Vec<Notice>>,
    /// The runtimes registered with `add_runtime()` (Twig's runtime loaders).
    runtimes: RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>,
    /// The checker of the `SandboxExtension`, fixed once the extensions are initialized.
    sandbox: OnceLock<Option<Arc<SecurityChecker>>>,
    date: DateContext,
    constants: RwLock<Constants>,
    /// The charset of the escaper runtime, fixed when a template first escapes.
    escaper_charset: OnceLock<PhpString>,
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The identity of the next compiled template file.
static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// How many template constructors run on this thread (traits load templates while a template is constructed).
    static CONSTRUCTING: Cell<usize> = const { Cell::new(0) };
}

/// The maximum nesting of template constructors (a template whose traits are being loaded loads its traits, which
/// load theirs): only a cycle of traits gets anywhere near it.
const MAX_CONSTRUCTION_DEPTH: usize = 64;

/// Counts a running template constructor; PHP recurses without limit when templates use each other as traits, a
/// twig-rs error stops it (reference/deviations.md).
struct ConstructionGuard;

impl ConstructionGuard {
    fn enter(max_depth: usize) -> Result<ConstructionGuard> {
        let depth = CONSTRUCTING.with(|depth| {
            let next = depth.get() + 1;
            depth.set(next);
            next
        });
        let guard = ConstructionGuard;
        if depth > max_depth {
            return Err(Error::logic(format!(
                "Maximum template call depth of {max_depth} reached (twig-rs limit)."
            )));
        }
        Ok(guard)
    }
}

impl Drop for ConstructionGuard {
    fn drop(&mut self) {
        CONSTRUCTING.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// The cache key of a template instance: the class key, plus `___N` for an embedded template.
fn instance_key(key: &[u8], index: Option<i64>) -> Vec<u8> {
    let mut instance = key.to_vec();
    if let Some(index) = index {
        instance.extend_from_slice(format!("___{index}").as_bytes());
    }
    instance
}

/// The block table of a trait while the constructor works on it (`$_trait_N_blocks`, a PHP array).
struct TraitBlocks(Vec<(PhpString, BlockRef)>);

impl TraitBlocks {
    fn position(&self, name: &[u8]) -> Option<usize> {
        self.0.iter().position(|(key, _)| key.as_bytes() == name)
    }

    /// `$blocks[$name] = $block`: replaced in place, or appended.
    fn set(&mut self, name: PhpString, block: BlockRef) {
        match self.position(name.as_bytes()) {
            Some(i) => self.0[i].1 = block,
            None => self.0.push((name, block)),
        }
    }

    /// `array_merge($this, $other)`.
    fn merge(&mut self, other: TraitBlocks) {
        for (name, block) in other.0 {
            self.set(name, block);
        }
    }
}

/// The Twig environment: the options, the extensions and everything they register, the globals, the loader and the
/// cache of loaded templates. Twig: `Twig\Environment` (TWIG/src/Environment.php).
///
/// An environment is a cheap handle (clones share it) that can be used from many threads. Extensions, filters,
/// functions, tests, token parsers and node visitors can be added until the extensions are initialized (the first
/// template load), after which Twig's `LogicException`s are returned, like Twig.
///
/// ```
/// use twig::loader::ArrayLoader;
/// use twig::runtime::Environment;
/// use twig::value::{Array, Value};
///
/// let env = Environment::new(ArrayLoader::new([("index", "{% for i in 1..3 %}{{ i }}{% endfor %} {{ greeting }}")]));
/// env.add_global("greeting", "hi")?;
/// assert_eq!(env.render("index", Array::new())?, "123 hi");
/// # Ok::<(), twig::Error>(())
/// ```
#[derive(Clone)]
pub struct Environment {
    inner: Arc<Inner>,
}

impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Environment")
            .field("charset", &self.charset())
            .field("strict_variables", &self.is_strict_variables())
            .finish_non_exhaustive()
    }
}

/// Reports compile-time deprecations to the environment.
struct EnvDeprecations<'a>(&'a Environment);

impl DeprecationSink for EnvDeprecations<'_> {
    fn report(&mut self, deprecation: Deprecation) {
        self.0.report_deprecation(deprecation);
    }
}

impl Environment {
    /// An environment with Twig's default options.
    pub fn new(loader: impl Loader + 'static) -> Self {
        Self::with_options(loader, EnvironmentOptions::default())
    }

    /// An environment with the given options. Twig: `new Environment($loader, $options)`, which registers the
    /// `CoreExtension`, the `EscaperExtension` with the `autoescape` strategy, the `YieldNotReadyExtension` and the
    /// `OptimizerExtension` with the `optimizations` mode.
    pub fn with_options(loader: impl Loader + 'static, options: EnvironmentOptions) -> Self {
        Self::with_shared_loader(Arc::new(loader), options)
    }

    /// An environment over a shared loader.
    pub fn with_shared_loader(loader: Arc<dyn Loader>, options: EnvironmentOptions) -> Self {
        let mut extensions = ExtensionSet::with_core_extensions();
        if let Some(escaper) = extensions.extension::<EscaperExtension>() {
            escaper.set_default_strategy(options.autoescape.clone());
        }
        // a fresh set accepts both extensions (Twig's order: core, escaper, yield-not-ready, optimizer)
        let _ = extensions.add_extension(Arc::new(YieldNotReadyExtension));
        let _ = extensions.add_extension(Arc::new(OptimizerExtension::new(options.optimizations)));
        extensions.set_regex_validator(Arc::new(PcreRegexValidator));
        let parse = ParseContext::with_lexer_options(extensions, options.lexer.clone());
        let auto_reload = options.auto_reload.unwrap_or(options.debug);
        Environment {
            inner: Arc::new(Inner {
                options: RwLock::new(Options {
                    debug: options.debug,
                    charset: PhpString::from(normalize_charset(&options.charset)),
                    strict_variables: options.strict_variables,
                    autoescape: options.autoescape,
                    auto_reload,
                    optimizations: options.optimizations,
                    use_yield: options.use_yield,
                    max_depth: options.max_depth.max(1),
                    policy: options.diagnostics,
                }),
                loader: RwLock::new(loader),
                parse: RwLock::new(parse),
                runtime_initialized: AtomicBool::new(false),
                runtimes_installed: OnceLock::new(),
                globals: Mutex::new(Globals::default()),
                programs: RwLock::new(HashMap::new()),
                templates: RwLock::new(HashMap::new()),
                notices: Mutex::new(Vec::new()),
                runtimes: RwLock::new(HashMap::new()),
                sandbox: OnceLock::new(),
                date: DateContext::new(),
                constants: RwLock::new(Constants::new()),
                escaper_charset: OnceLock::new(),
            }),
        }
    }

    /// The identity of the environment (`$this->env === $env`).
    pub(crate) fn id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    /// Whether both handles are the same environment.
    pub(crate) fn same_as(&self, other: &Environment) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    // ---- options -------------------------------------------------------------------------------------------------

    /// The charset. Twig: `getCharset()`.
    pub fn charset(&self) -> String {
        read(&self.inner.options)
            .charset
            .to_string_lossy()
            .into_owned()
    }

    /// The charset as a PHP string (`$this->env->getCharset()` in compiled code).
    pub(crate) fn charset_value(&self) -> PhpString {
        read(&self.inner.options).charset.clone()
    }

    /// The charset of the escaper runtime. Twig's default runtime loader creates `EscaperRuntime` with the charset of
    /// the moment a template first escapes (`new EscaperRuntime($this->charset)`, created lazily since
    /// `EscaperExtension::setEnvironment()` resets it) and keeps the instance, so a later `set_charset()` changes
    /// `_charset` and the `needs_charset` callables but not escaping (VERIFIED with the oracle).
    pub(crate) fn escaper_charset(&self) -> PhpString {
        self.inner
            .escaper_charset
            .get_or_init(|| self.charset_value())
            .clone()
    }

    /// Twig: `setCharset()`: upper case, `UTF8` → `UTF-8`.
    pub fn set_charset(&self, charset: &str) {
        write(&self.inner.options).charset = PhpString::from(normalize_charset(charset));
    }

    /// Twig: `isDebug()`.
    pub fn is_debug(&self) -> bool {
        read(&self.inner.options).debug
    }

    /// Twig: `enableDebug()`.
    pub fn enable_debug(&self) {
        write(&self.inner.options).debug = true;
    }

    /// Twig: `disableDebug()`.
    pub fn disable_debug(&self) {
        write(&self.inner.options).debug = false;
    }

    /// Twig: `isAutoReload()`.
    pub fn is_auto_reload(&self) -> bool {
        read(&self.inner.options).auto_reload
    }

    /// Twig: `enableAutoReload()`.
    pub fn enable_auto_reload(&self) {
        write(&self.inner.options).auto_reload = true;
    }

    /// Twig: `disableAutoReload()`.
    pub fn disable_auto_reload(&self) {
        write(&self.inner.options).auto_reload = false;
    }

    /// Twig: `isStrictVariables()`.
    pub fn is_strict_variables(&self) -> bool {
        read(&self.inner.options).strict_variables
    }

    /// Twig: `enableStrictVariables()` (templates loaded afterwards are compiled again).
    pub fn enable_strict_variables(&self) {
        write(&self.inner.options).strict_variables = true;
    }

    /// Twig: `disableStrictVariables()`.
    pub fn disable_strict_variables(&self) {
        write(&self.inner.options).strict_variables = false;
    }

    /// The `optimizations` option.
    pub fn optimizations(&self) -> i64 {
        read(&self.inner.options).optimizations
    }

    /// The `autoescape` option.
    pub fn autoescape(&self) -> DefaultStrategy {
        read(&self.inner.options).autoescape.clone()
    }

    /// Twig: `useYield()`.
    pub fn use_yield(&self) -> bool {
        read(&self.inner.options).use_yield
    }

    /// The maximum depth of nested template calls.
    pub fn max_depth(&self) -> usize {
        read(&self.inner.options).max_depth
    }

    /// Sets the maximum depth of nested template calls.
    pub fn set_max_depth(&self, depth: usize) {
        write(&self.inner.options).max_depth = depth.max(1);
    }

    // ---- extensions ----------------------------------------------------------------------------------------------

    /// Runs `f` with the extension set.
    pub fn with_extensions<R>(&self, f: impl FnOnce(&ExtensionSet) -> R) -> R {
        f(read(&self.inner.parse).extensions())
    }

    fn with_extensions_mut<R>(&self, f: impl FnOnce(&mut ExtensionSet) -> R) -> R {
        f(write(&self.inner.parse).extensions_mut())
    }

    /// Twig: `addExtension()`.
    ///
    /// # Errors
    ///
    /// Twig's `LogicException`s: `Unable to register extension "%s" as extensions have already been initialized.`,
    /// `Unable to register extension "%s" as it is already registered.`
    pub fn add_extension(&self, extension: Arc<dyn Extension>) -> Result<()> {
        self.with_extensions_mut(|set| set.add_extension(extension))
    }

    /// Twig: `hasExtension()`.
    pub fn has_extension(&self, class: &str) -> bool {
        self.with_extensions(|set| set.has_extension(class))
    }

    /// Twig: `addFilter()`.
    ///
    /// # Errors
    ///
    /// `Unable to add filter "%s" as extensions have already been initialized.` and `Filter "%s" is already
    /// registered.`
    pub fn add_filter(&self, filter: TwigFilter) -> Result<()> {
        self.with_extensions_mut(|set| set.add_filter(filter))
    }

    /// Twig: `addFunction()`.
    ///
    /// # Errors
    ///
    /// As [`add_filter`](Self::add_filter), for functions.
    pub fn add_function(&self, function: TwigFunction) -> Result<()> {
        self.with_extensions_mut(|set| set.add_function(function))
    }

    /// Twig: `addTest()`.
    ///
    /// # Errors
    ///
    /// As [`add_filter`](Self::add_filter), for tests.
    pub fn add_test(&self, test: TwigTest) -> Result<()> {
        self.with_extensions_mut(|set| set.add_test(test))
    }

    /// Twig: `addTokenParser()`.
    ///
    /// # Errors
    ///
    /// `Unable to add a token parser as extensions have already been initialized.`
    pub fn add_token_parser(&self, parser: Arc<dyn TokenParser>) -> Result<()> {
        self.with_extensions_mut(|set| set.add_token_parser(parser))
    }

    /// Twig: `addNodeVisitor()`.
    ///
    /// # Errors
    ///
    /// `Unable to add a node visitor as extensions have already been initialized.`
    pub fn add_node_visitor(&self, visitor: Arc<dyn NodeVisitor>) -> Result<()> {
        self.with_extensions_mut(|set| set.add_node_visitor(visitor))
    }

    /// Twig: `registerUndefinedFilterCallback()`.
    pub fn register_undefined_filter_callback(&self, callback: UndefinedFilterCallback) {
        self.with_extensions_mut(|set| set.register_undefined_filter_callback(callback));
    }

    /// Twig: `registerUndefinedFunctionCallback()`.
    pub fn register_undefined_function_callback(&self, callback: UndefinedFunctionCallback) {
        self.with_extensions_mut(|set| set.register_undefined_function_callback(callback));
    }

    /// Twig: `registerUndefinedTestCallback()`.
    pub fn register_undefined_test_callback(&self, callback: UndefinedTestCallback) {
        self.with_extensions_mut(|set| set.register_undefined_test_callback(callback));
    }

    /// Twig: `registerUndefinedTokenParserCallback()`.
    pub fn register_undefined_token_parser_callback(&self, callback: UndefinedTokenParserCallback) {
        self.with_extensions_mut(|set| set.register_undefined_token_parser_callback(callback));
    }

    /// Twig: `ExtensionSet::isInitialized()`: the registry was built or the runtime initialized (a template was
    /// loaded).
    pub fn is_initialized(&self) -> bool {
        self.inner.runtime_initialized.load(Ordering::Acquire)
            || self.with_extensions(ExtensionSet::is_initialized)
    }

    /// Twig: `ExtensionSet::initRuntime()` (on every template load): builds the registry, installs the runtimes of
    /// the core callables (once) and marks the runtime as initialized, which freezes the globals.
    fn init_runtime(&self) {
        self.inner.runtimes_installed.get_or_init(|| {
            self.with_extensions(|set| {
                set.initialize();
                crate::extension::runtime::install_runtimes(set);
            });
        });
        self.inner
            .runtime_initialized
            .store(true, Ordering::Release);
    }

    // ---- globals -------------------------------------------------------------------------------------------------

    /// Twig: `addGlobal()`: after initialization only an existing global may be updated.
    ///
    /// # Errors
    ///
    /// `Unable to add global "%s" as the runtime or the extensions have already been initialized.` (a
    /// `LogicException`).
    pub fn add_global(&self, name: &str, value: impl Into<Value>) -> Result<()> {
        let value = value.into();
        if self.is_initialized() && self.globals().get_sym(name.as_bytes()).is_none() {
            return Err(Error::logic(format!(
                "Unable to add global \"{name}\" as the runtime or the extensions have already been initialized."
            )));
        }
        let mut globals = lock(&self.inner.globals);
        let key = twig_value::ArrayKey::from_string(PhpString::from(name));
        match &mut globals.resolved {
            Some(resolved) => {
                resolved.insert(key, value);
            }
            None => {
                globals.globals.insert(key, value);
            }
        }
        Ok(())
    }

    /// Twig: `getGlobals()`: `array_merge($extensionGlobals, $this->globals)`, computed once after initialization.
    pub fn globals(&self) -> Array {
        let initialized = self.is_initialized();
        let mut globals = lock(&self.inner.globals);
        if initialized && let Some(resolved) = &globals.resolved {
            return resolved.clone();
        }
        let mut parts: Vec<Value> = self.with_extensions(|set| {
            set.extensions()
                .iter()
                .filter_map(|extension| extension.globals())
                .map(Value::Array)
                .collect()
        });
        let extension_globals = if parts.is_empty() {
            Array::new()
        } else {
            twig_value::builtins::array::array_merge(&std::mem::take(&mut parts))
                .unwrap_or_default()
        };
        let merged = twig_value::builtins::array::array_merge(&[
            Value::Array(extension_globals),
            Value::Array(globals.globals.clone()),
        ])
        .unwrap_or_default();
        if initialized {
            globals.resolved = Some(merged.clone());
        }
        merged
    }

    // ---- loader --------------------------------------------------------------------------------------------------

    /// The loader. Twig: `getLoader()`.
    pub fn loader(&self) -> Arc<dyn Loader> {
        Arc::clone(&read(&self.inner.loader))
    }

    /// Replaces the loader. Twig: `setLoader()`.
    pub fn set_loader(&self, loader: impl Loader + 'static) {
        *write(&self.inner.loader) = Arc::new(loader);
    }

    // ---- templates -----------------------------------------------------------------------------------------------

    /// The options that change compiled code, appended to the loader's cache key (Twig's `optionsHash` in
    /// `getTemplateClass()`).
    fn class_key(&self, cache_key: &[u8]) -> Vec<u8> {
        let options = read(&self.inner.options);
        let mut key = cache_key.to_vec();
        key.extend_from_slice(
            format!(
                "\0{}:{}:{}:{}",
                u8::from(options.debug),
                u8::from(options.strict_variables),
                u8::from(options.use_yield),
                options.optimizations
            )
            .as_bytes(),
        );
        key
    }

    /// Loads a template. Twig: `Environment::load()`.
    ///
    /// # Errors
    ///
    /// The loader's errors (`Template "%s" is not defined.`), the errors of compiling the template and of resolving
    /// its traits.
    pub fn load(&self, name: &str) -> Result<TemplateWrapper> {
        Ok(TemplateWrapper::new(
            self.clone(),
            self.load_template(name)?,
        ))
    }

    /// The first template of `names` the loader knows (a single name is loaded without asking). Twig:
    /// `Environment::resolveTemplate()`.
    ///
    /// # Errors
    ///
    /// `Unable to find one of the following templates: "%s".` (a loader error), and the errors of loading.
    pub fn resolve_template(&self, names: &[&str]) -> Result<TemplateWrapper> {
        let loader = self.loader();
        for name in names {
            if names.len() != 1 && !loader.exists(name) {
                continue;
            }
            return self.load(name);
        }
        Err(Error::loader(format!(
            "Unable to find one of the following templates: \"{}\".",
            names.join("\", \"")
        )))
    }

    /// Twig: `loadTemplate()` for a name: the cached instance of the template's class, or a freshly compiled and
    /// constructed one.
    pub(crate) fn load_template(&self, name: &str) -> Result<Arc<Template>> {
        self.load_template_index(name, None)
    }

    /// Twig: `loadTemplate(getTemplateClass($name), $name, $index)`.
    pub(crate) fn load_template_index(
        &self,
        name: &str,
        index: Option<i64>,
    ) -> Result<Arc<Template>> {
        let loader = self.loader();
        let cache_key = loader.get_cache_key(name)?;
        let key = self.class_key(&cache_key);
        self.load_class(key, name, index, |env| {
            let source = loader.get_source_context(name)?;
            env.compile_source(&source)
        })
    }

    /// Twig: `Template::load()` of a template naming itself: `loadTemplate(static::class without ___N, $name,
    /// $index)`, the class family of `this` (the loader is not asked).
    pub(crate) fn load_family(&self, this: &Template, index: Option<i64>) -> Result<Arc<Template>> {
        let family = Arc::clone(this.family());
        self.load_class(this.key().to_vec(), this.name(), index, move |_| Ok(family))
    }

    /// Twig: `Environment::loadTemplate($cls, $name, $index)`: the cached instance, else an instance of the class
    /// (`___N`: the embedded template `index` of the file), compiling the file when it is not compiled yet.
    fn load_class(
        &self,
        key: Vec<u8>,
        name: &str,
        index: Option<i64>,
        compile: impl FnOnce(&Environment) -> Result<Arc<Program>>,
    ) -> Result<Arc<Template>> {
        let instance = instance_key(&key, index);
        if let Some(template) = read(&self.inner.templates).get(&instance) {
            let template = Arc::clone(template);
            self.init_runtime();
            return Ok(template);
        }
        let cached = read(&self.inner.programs).get(&key).cloned();
        let family = match cached {
            Some(family) => family,
            None => {
                let family = compile(self)?;
                Arc::clone(
                    write(&self.inner.programs)
                        .entry(key.clone())
                        .or_insert(family),
                )
            }
        };
        let program = match index {
            None => Arc::clone(&family),
            Some(i) => match family.embedded_template(i) {
                Some(program) => Arc::clone(program),
                None => {
                    return Err(Error::runtime(format!(
                        "Failed to load Twig template \"{name}\", index \"{i}\": cache might be corrupted."
                    ))
                    .with_source(family.source().clone()));
                }
            },
        };
        // `$this->extensionSet->initRuntime(); return $this->loadedTemplates[$cls] = new $cls($this);`
        self.init_runtime();
        let template = self.construct(program, family, key)?;
        Ok(Arc::clone(
            write(&self.inner.templates)
                .entry(instance)
                .or_insert(template),
        ))
    }

    /// Twig: the compiled constructor (`ModuleNode::compileConstructor()`): the sandbox checker is fetched
    /// (`CheckSecurityCallNode`), the escaper runtime created, and the traits resolved (`ensureTraitsAllowed()`, then
    /// per trait `$this->load()`, `isTraitable()`, `getBlocks()` and the renamed blocks).
    fn construct(
        &self,
        program: Arc<Program>,
        family: Arc<Program>,
        key: Vec<u8>,
    ) -> Result<Arc<Template>> {
        let _guard = ConstructionGuard::enter(self.max_depth().min(MAX_CONSTRUCTION_DEPTH))?;
        if program.security().is_some() {
            // `$this->sandbox = $env->getExtension(SandboxExtension::class)->getChecker();`
            self.with_extensions(|set| {
                set.get_extension("Twig\\Extension\\SandboxExtension")
                    .map(|_| ())
            })?;
        }
        if program.uses_template_escaper() {
            // `$this->escaper = $env->getRuntime(EscaperRuntime::class)`
            self.escaper_charset();
        }
        let traits = self.resolve_traits(&program, &family, &key)?;
        Ok(Arc::new(Template::new(
            program,
            family,
            key,
            self.id(),
            traits,
        )))
    }

    /// The trait part of the compiled constructor.
    fn resolve_traits(
        &self,
        program: &Arc<Program>,
        family: &Arc<Program>,
        key: &[u8],
    ) -> Result<Traits> {
        let mut traits = Traits {
            blocks: Vec::new(),
            aliases: Vec::new(),
        };
        if program.traits().is_empty() {
            return Ok(traits);
        }
        ensure_traits_allowed(self, program)?;
        let mut all = TraitBlocks(Vec::new());
        for definition in program.traits() {
            let template_name = match &definition.template {
                Value::String(name) => name.clone(),
                other => PhpString::from(format!("{other:?}")),
            };
            let template =
                self.load_trait(program, family, key, &template_name, definition.line)?;
            if !template.program().is_traitable() {
                return Err(Error::runtime(format!(
                    "Template \"{}\" cannot be used as a trait.",
                    template_name.to_string_lossy()
                ))
                .with_line(definition.line)
                .with_source(program.source().clone()));
            }
            let table = template.block_table();
            let mut blocks = TraitBlocks(
                table
                    .names()
                    .filter_map(|name| {
                        table
                            .get(name.as_bytes())
                            .map(|block| (name.clone(), block.clone()))
                    })
                    .collect(),
            );
            for (name, alias) in &definition.targets {
                let Some(position) = blocks.position(name) else {
                    return Err(Error::runtime(format!(
                        "Block \"{}\" is not defined in trait \"{}\".",
                        String::from_utf8_lossy(name),
                        template_name.to_string_lossy()
                    ))
                    .with_line(definition.line)
                    .with_source(program.source().clone()));
                };
                // `$b["alias"] = $b["name"]; unset($b["name"]); $this->traitAliases["alias"] = "name";`
                let alias = match alias {
                    Value::String(alias) => alias.clone(),
                    other => PhpString::from(format!("{other:?}")),
                };
                let block = blocks.0[position].1.clone();
                blocks.set(alias.clone(), block);
                if let Some(position) = blocks.position(name) {
                    blocks.0.remove(position);
                }
                let original = PhpString::from(name.as_slice());
                match traits.aliases.iter_mut().find(|(key, _)| *key == alias) {
                    Some(entry) => entry.1 = original,
                    None => traits.aliases.push((alias, original)),
                }
            }
            all.merge(blocks);
        }
        traits.blocks = all.0;
        Ok(traits)
    }

    /// Twig: `Template::load()` from the constructor of a template using a trait: the template's own class family
    /// when the trait names it; a Twig error gets the template's source when it has none and the line of the `use`
    /// tag unless it has a line.
    fn load_trait(
        &self,
        program: &Arc<Program>,
        family: &Arc<Program>,
        key: &[u8],
        name: &PhpString,
        line: i64,
    ) -> Result<Arc<Template>> {
        let result = if name.as_bytes() == program.name().as_bytes() {
            let family = Arc::clone(family);
            self.load_class(key.to_vec(), program.name(), None, move |_| Ok(family))
        } else {
            self.load_template_index(&name.to_string_lossy(), None)
        };
        result.map_err(|mut error| {
            if error.kind() != ErrorKind::Logic {
                if error.source_context().is_none() {
                    error.set_source_context(Some(program.source().clone()));
                }
                if error.template_line() <= 0 {
                    error.set_template_line(line);
                }
            }
            error
        })
    }

    /// Lexes, parses and compiles a template. Twig: `compileSource()`: Twig errors get the source; other errors
    /// become `An exception has been thrown during the compilation of a template ("%s").`
    ///
    /// # Errors
    ///
    /// The syntax errors of the template.
    pub fn compile_source(&self, source: &Source) -> Result<Arc<Program>> {
        let mut deprecations = EnvDeprecations(self);
        let parse = read(&self.inner.parse);
        let module = parse.parse_template(source, &mut deprecations)?;
        let options = CompileOptions {
            strict_variables: self.is_strict_variables(),
            sandboxed: parse
                .extensions()
                .has_extension("Twig\\Extension\\SandboxExtension"),
        };
        let file = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        lltl::compile(&module, &options, file, &mut deprecations)
            .map_err(|error| compile_source_error(error, source))
    }

    /// Renders a template to a string. Twig: `Environment::render()`.
    ///
    /// # Errors
    ///
    /// Loading, compiling and rendering errors (see [`TemplateWrapper::render`]).
    pub fn render(&self, name: &str, context: Array) -> Result<String> {
        self.load(name)?.render(context)
    }

    /// Renders a template to bytes.
    ///
    /// # Errors
    ///
    /// Loading, compiling and rendering errors.
    pub fn render_bytes(&self, name: &str, context: Array) -> Result<Vec<u8>> {
        self.load(name)?.render_bytes(context)
    }

    /// Displays a template into `sink`. Twig: `Environment::display()`.
    ///
    /// # Errors
    ///
    /// Loading, compiling and rendering errors.
    pub fn display(&self, name: &str, context: Array, sink: &mut dyn Sink) -> Result<()> {
        self.load(name)?.display(context, sink)
    }

    /// Creates a template from source code. Twig: `createTemplate()`: the template is named
    /// `__string_template__<xxh128 of the source>`, or `<name> (string template <hash>)`, and loaded through an
    /// `ArrayLoader` holding it (the environment's loader resolves everything else).
    ///
    /// # Errors
    ///
    /// The errors of compiling the template.
    pub fn create_template(&self, template: &[u8], name: Option<&str>) -> Result<TemplateWrapper> {
        let hash = twig_value::builtins::format::hash_xxh128(template);
        let hash = hash.to_string_lossy();
        let name = match name {
            Some(name) => format!("{name} (string template {hash})"),
            None => format!("__string_template__{hash}"),
        };
        // the ArrayLoader of the ChainLoader answers getCacheKey() with "name:source"
        let mut cache_key = name.as_bytes().to_vec();
        cache_key.push(b':');
        cache_key.extend_from_slice(template);
        let key = self.class_key(&cache_key);
        let source = Source::new(template.to_vec(), name.clone());
        let template = self.load_class(key, &name, None, |env| env.compile_source(&source))?;
        Ok(TemplateWrapper::new(self.clone(), template))
    }

    // ---- diagnostics ---------------------------------------------------------------------------------------------

    /// The diagnostics policy.
    pub fn diagnostics_policy(&self) -> DiagnosticsPolicy {
        read(&self.inner.options).policy
    }

    /// Sets the diagnostics policy.
    pub fn set_diagnostics_policy(&self, policy: DiagnosticsPolicy) {
        write(&self.inner.options).policy = policy;
    }

    /// Removes and returns the recorded diagnostics and deprecations (policies [`DiagnosticsPolicy::Collect`] and
    /// [`DiagnosticsPolicy::Error`]).
    pub fn take_notices(&self) -> Vec<Notice> {
        std::mem::take(&mut *lock(&self.inner.notices))
    }

    /// Reports a PHP diagnostic under the environment's policy; with [`DiagnosticsPolicy::Error`] a warning or notice
    /// comes back as the exception to throw.
    pub(crate) fn report_diagnostic(
        &self,
        diagnostic: Diagnostic,
    ) -> std::result::Result<(), PhpError> {
        match self.diagnostics_policy() {
            DiagnosticsPolicy::Ignore => Ok(()),
            DiagnosticsPolicy::Collect => {
                lock(&self.inner.notices).push(Notice::Php(diagnostic));
                Ok(())
            }
            DiagnosticsPolicy::Error => {
                let level = diagnostic.level;
                let message = diagnostic.message.clone();
                lock(&self.inner.notices).push(Notice::Php(diagnostic));
                match level {
                    Level::Warning | Level::Notice => {
                        Err(PhpError::new(ThrowableClass::ErrorException, message))
                    }
                    Level::Deprecated | Level::UserDeprecated => Ok(()),
                }
            }
        }
    }

    /// Reports a deprecation (`trigger_deprecation()`).
    pub fn report_deprecation(&self, deprecation: Deprecation) {
        if self.diagnostics_policy() != DiagnosticsPolicy::Ignore {
            lock(&self.inner.notices).push(Notice::Deprecation(deprecation));
        }
    }

    // ---- dates, constants, sandbox ------------------------------------------------------------------------------

    /// The date context (default timezone, clock) of the date filters and functions.
    pub fn date_context(&self) -> &DateContext {
        &self.inner.date
    }

    /// Defines a global PHP constant for `constant()` and the `constant` test.
    pub fn add_constant(&self, name: &str, value: impl Into<Value>) {
        write(&self.inner.constants).add_constant(name, value.into());
    }

    /// Registers a class (its constants and enum cases become visible to `constant()`, `enum()`, ...).
    ///
    /// # Errors
    ///
    /// `Cannot declare class X, because the name is already in use`.
    pub fn add_class(&self, class: ClassRef) -> PhpResult<()> {
        write(&self.inner.constants).add_class(class)
    }

    /// Runs `f` with the constant and class registry.
    pub fn with_constants<R>(&self, f: impl FnOnce(&Constants) -> R) -> R {
        f(&read(&self.inner.constants))
    }

    /// The checker of the registered [`SandboxExtension`], if any (Twig:
    /// `getExtension(SandboxExtension::class)->getChecker()`).
    pub fn sandbox_checker(&self) -> Option<Arc<SecurityChecker>> {
        if let Some(checker) = self.inner.sandbox.get() {
            return checker.clone();
        }
        let checker = self.with_extensions(|set| {
            set.extension::<SandboxExtension>()
                .map(|extension| Arc::clone(extension.checker()))
        });
        if self.is_initialized() {
            let _ = self.inner.sandbox.set(checker.clone());
        }
        checker
    }

    /// Whether code of `source` runs sandboxed: `hasExtension(SandboxExtension) &&
    /// getChecker()->isSandboxed($source)`.
    pub fn is_sandboxed(&self, source: &Source) -> bool {
        self.sandbox_checker()
            .is_some_and(|checker| checker.is_sandboxed(Some(source)))
    }

    // ---- runtimes ------------------------------------------------------------------------------------------------

    /// Registers the runtime object of a runtime class (Twig's runtime loaders, e.g. a `FactoryRuntimeLoader`); the
    /// callables implemented by that class find it with [`runtime`](Self::runtime). For `render_sandboxed()`, register
    /// a [`SandboxBridgeRuntime`](super::SandboxBridgeRuntime) under its class name.
    pub fn add_runtime(&self, class: &str, runtime: Arc<dyn Any + Send + Sync>) {
        write(&self.inner.runtimes).insert(class.trim_start_matches('\\').to_owned(), runtime);
    }

    /// The runtime object of a runtime class. Twig: `getRuntime()`.
    ///
    /// # Errors
    ///
    /// `Unable to load the "%s" runtime.` (a runtime error).
    pub fn runtime(&self, class: &str) -> Result<Arc<dyn Any + Send + Sync>> {
        read(&self.inner.runtimes)
            .get(class.trim_start_matches('\\'))
            .cloned()
            .ok_or_else(|| Error::runtime(format!("Unable to load the \"{class}\" runtime.")))
    }
}
