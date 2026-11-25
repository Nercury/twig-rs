//! Extensions and the registry of everything they contribute: token parsers, node visitors, filters, functions,
//! tests and expression parsers.
//!
//! Twig: `Twig\Extension\ExtensionInterface`, `Twig\ExtensionSet` (TWIG/src/ExtensionSet.php) and
//! `Twig\Extension\StagingExtension`. [`ExtensionSet`] keeps Twig's semantics: extensions are keyed by class, the
//! registry is built lazily on first use from the extensions in registration order and then the staging area
//! (callables added directly), a later definition of a name replaces an earlier one while the first registration
//! keeps its position (which decides the order of "Did you mean" suggestions), names with `*` are also dynamic
//! patterns, and nothing can be added once the registry is built.
//!
//! The core extensions ([`CoreExtension`], [`EscaperExtension`]) and the optional [`SandboxExtension`],
//! [`DebugExtension`], [`StringLoaderExtension`] and [`SandboxBridgeExtension`] register Twig's tags, node visitors
//! and callable metadata (names, options, PHP signatures, operators); the runtime fills the callables' runtime slots
//! later. The [`TokenParser`] and [`NodeVisitor`] traits live in [`crate::token_parser`] and [`crate::visitor`].

pub mod arguments;
pub mod callable;
mod core;
mod optional;

pub use self::core::{CoreExtension, EscaperExtension};
pub use callable::{
    CallableKind, CallableParameter, CallableType, DeprecatedCallableInfo, FilterNodeClass,
    FilterNodeFactory, FilterOptions, FunctionNodeClass, FunctionNodeFactory, FunctionOptions,
    ParameterDefault, ParserCallable, PhpCallable, SafeCallback, TestNodeClass, TestNodeFactory,
    TestOptions, TwigCallable, TwigFilter, TwigFunction, TwigTest,
};
pub use optional::{
    DebugExtension, SandboxBridgeExtension, SandboxExtension, StringLoaderExtension,
};

use std::any::Any;
use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::deprecation::Deprecation;
use crate::error::{Error, Result};
use crate::lexer::{Lexer, LexerOptions};
use crate::parser::expression::{ExpressionParserEntry, ExpressionParsers};
pub use crate::token_parser::TokenParser;
pub use crate::visitor::NodeVisitor;

/// Validates the constant regular expression on the right of `matches` the way `MatchesBinary`'s constructor
/// does with `preg_match($regexp, '')`. The PCRE port provides the implementation.
pub trait RegexValidator: Send + Sync {
    /// The warning `preg_match($pattern, '')` raises, without the `preg_match(): ` prefix (e.g. `No ending
    /// delimiter '/' found`), or `None` when the pattern is valid.
    fn validate(&self, pattern: &[u8]) -> Option<String>;
}

/// An extension. Twig: `Twig\Extension\ExtensionInterface` / `AbstractExtension`.
///
/// Twig's legacy `getOperators()` (deprecated since 3.21) is not supported: extensions contribute operators as
/// [`expression_parsers`](Self::expression_parsers).
pub trait Extension: Send + Sync + 'static {
    /// The class name extensions are keyed by (Twig's `$extension::class`).
    fn class_name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// Twig: `getTokenParsers()`.
    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        Vec::new()
    }

    /// Twig: `getNodeVisitors()`.
    fn node_visitors(&self) -> Vec<Arc<dyn NodeVisitor>> {
        Vec::new()
    }

    /// Twig: `getFilters()`.
    fn filters(&self) -> Vec<TwigFilter> {
        Vec::new()
    }

    /// Twig: `getFunctions()`.
    fn functions(&self) -> Vec<TwigFunction> {
        Vec::new()
    }

    /// Twig: `getTests()`.
    fn tests(&self) -> Vec<TwigTest> {
        Vec::new()
    }

    /// Twig: `getExpressionParsers()`.
    fn expression_parsers(&self) -> Vec<ExpressionParserEntry> {
        Vec::new()
    }

    /// The extension as `Any`, to reach the concrete type.
    fn as_any(&self) -> &dyn Any;
}

/// A callback consulted for unknown filters (`registerUndefinedFilterCallback()`): `Ok(None)` stands for Twig's
/// `false` (try the next callback); an error (Twig's exception, typically a `SyntaxError`) stops the lookup.
pub type UndefinedFilterCallback = Arc<dyn Fn(&[u8]) -> Result<Option<TwigFilter>> + Send + Sync>;
/// A callback consulted for unknown functions (`registerUndefinedFunctionCallback()`), see
/// [`UndefinedFilterCallback`].
pub type UndefinedFunctionCallback =
    Arc<dyn Fn(&[u8]) -> Result<Option<TwigFunction>> + Send + Sync>;
/// A callback consulted for unknown tests (`registerUndefinedTestCallback()`), see [`UndefinedFilterCallback`].
pub type UndefinedTestCallback = Arc<dyn Fn(&[u8]) -> Result<Option<TwigTest>> + Send + Sync>;
/// A callback consulted for unknown tags (`registerUndefinedTokenParserCallback()`), see
/// [`UndefinedFilterCallback`].
pub type UndefinedTokenParserCallback =
    Arc<dyn Fn(&[u8]) -> Result<Option<Arc<dyn TokenParser>>> + Send + Sync>;

/// The callables and parsers added directly to the set. Twig: `Twig\Extension\StagingExtension`.
#[derive(Default)]
struct Staging {
    filters: Vec<TwigFilter>,
    functions: Vec<TwigFunction>,
    tests: Vec<TwigTest>,
    token_parsers: Vec<Arc<dyn TokenParser>>,
    visitors: Vec<Arc<dyn NodeVisitor>>,
}

/// An ordered map with PHP array semantics: setting an existing key replaces the value in place.
struct OrderedMap<K, V> {
    entries: Vec<(K, V)>,
    index: std::collections::HashMap<K, usize>,
}

impl<K: Clone + Eq + std::hash::Hash, V> OrderedMap<K, V> {
    fn new() -> Self {
        OrderedMap {
            entries: Vec::new(),
            index: std::collections::HashMap::new(),
        }
    }

    fn insert(&mut self, key: K, value: V) {
        match self.index.get(&key) {
            Some(&i) => self.entries[i].1 = value,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }

    fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }
}

/// The registry built from the extensions (Twig's `initExtensions()`).
struct Registry {
    token_parsers: OrderedMap<Vec<u8>, Arc<dyn TokenParser>>,
    filters: OrderedMap<Vec<u8>, TwigFilter>,
    dynamic_filters: OrderedMap<Vec<u8>, TwigFilter>,
    functions: OrderedMap<Vec<u8>, TwigFunction>,
    dynamic_functions: OrderedMap<Vec<u8>, TwigFunction>,
    tests: OrderedMap<Vec<u8>, TwigTest>,
    dynamic_tests: OrderedMap<Vec<u8>, TwigTest>,
    visitors: Vec<Arc<dyn NodeVisitor>>,
    expression_parsers: ExpressionParsers,
}

/// The extensions of an environment and the registry built from them. Twig: `Twig\ExtensionSet`.
pub struct ExtensionSet {
    extensions: Vec<Arc<dyn Extension>>,
    staging: Staging,
    filter_callbacks: Vec<UndefinedFilterCallback>,
    function_callbacks: Vec<UndefinedFunctionCallback>,
    test_callbacks: Vec<UndefinedTestCallback>,
    token_parser_callbacks: Vec<UndefinedTokenParserCallback>,
    regex_validator: Option<Arc<dyn RegexValidator>>,
    registry: OnceLock<Registry>,
}

impl Default for ExtensionSet {
    fn default() -> Self {
        ExtensionSet::new()
    }
}

impl fmt::Debug for ExtensionSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtensionSet")
            .field(
                "extensions",
                &self
                    .extensions
                    .iter()
                    .map(|e| e.class_name())
                    .collect::<Vec<_>>(),
            )
            .field("initialized", &self.is_initialized())
            .finish_non_exhaustive()
    }
}

impl ExtensionSet {
    /// An empty set (no extension at all; see [`with_core_extensions`](Self::with_core_extensions)).
    pub fn new() -> Self {
        ExtensionSet {
            extensions: Vec::new(),
            staging: Staging::default(),
            filter_callbacks: Vec::new(),
            function_callbacks: Vec::new(),
            test_callbacks: Vec::new(),
            token_parser_callbacks: Vec::new(),
            regex_validator: None,
            registry: OnceLock::new(),
        }
    }

    /// A set with the extensions `Environment::__construct()` registers for parsing: [`CoreExtension`] and
    /// [`EscaperExtension`] (the yield-ready and optimizer extensions only contribute node visitors, which later
    /// stages add; the escaper's visitor comes with the escaper stage).
    pub fn with_core_extensions() -> Self {
        let mut set = ExtensionSet::new();
        set.extensions.push(Arc::new(CoreExtension::new()));
        set.extensions.push(Arc::new(EscaperExtension::new()));
        set
    }

    /// Whether the registry has been built. Twig: `isInitialized()`.
    pub fn is_initialized(&self) -> bool {
        self.registry.get().is_some()
    }

    fn check_not_initialized(&self, message: impl FnOnce() -> String) -> Result<()> {
        if self.is_initialized() {
            return Err(Error::logic(message()));
        }
        Ok(())
    }

    /// Registers an extension. Twig: `ExtensionSet::addExtension()`.
    ///
    /// # Errors
    ///
    /// `Unable to register extension "%s" as extensions have already been initialized.` and
    /// `Unable to register extension "%s" as it is already registered.` (`LogicException`s).
    pub fn add_extension(&mut self, extension: Arc<dyn Extension>) -> Result<()> {
        let class = extension.class_name().to_owned();
        self.check_not_initialized(|| {
            format!(
                "Unable to register extension \"{class}\" as extensions have already been initialized."
            )
        })?;
        if self.extensions.iter().any(|e| e.class_name() == class) {
            return Err(Error::logic(format!(
                "Unable to register extension \"{class}\" as it is already registered."
            )));
        }
        self.extensions.push(extension);
        Ok(())
    }

    /// Whether an extension with this class name is registered. Twig: `hasExtension()`.
    pub fn has_extension(&self, class: &str) -> bool {
        let class = class.trim_start_matches('\\');
        self.extensions.iter().any(|e| e.class_name() == class)
    }

    /// The extension with this class name. Twig: `getExtension()`.
    ///
    /// # Errors
    ///
    /// `The "%s" extension is not enabled.` (a runtime error).
    pub fn get_extension(&self, class: &str) -> Result<&Arc<dyn Extension>> {
        let class = class.trim_start_matches('\\');
        self.extensions
            .iter()
            .find(|e| e.class_name() == class)
            .ok_or_else(|| Error::runtime(format!("The \"{class}\" extension is not enabled.")))
    }

    /// The registered extension of type `T`, if any.
    pub fn extension<T: Extension>(&self) -> Option<&T> {
        self.extensions
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<T>())
    }

    /// The extensions in registration order. Twig: `getExtensions()`.
    pub fn extensions(&self) -> &[Arc<dyn Extension>] {
        &self.extensions
    }

    /// Adds a filter to the staging area. Twig: `ExtensionSet::addFilter()` / `StagingExtension::addFilter()`.
    ///
    /// # Errors
    ///
    /// `Unable to add filter "%s" as extensions have already been initialized.` and `Filter "%s" is already
    /// registered.` (`LogicException`s).
    pub fn add_filter(&mut self, filter: TwigFilter) -> Result<()> {
        self.check_not_initialized(|| {
            format!(
                "Unable to add filter \"{}\" as extensions have already been initialized.",
                String::from_utf8_lossy(filter.name())
            )
        })?;
        if self
            .staging
            .filters
            .iter()
            .any(|f| f.name() == filter.name())
        {
            return Err(Error::logic(format!(
                "Filter \"{}\" is already registered.",
                String::from_utf8_lossy(filter.name())
            )));
        }
        self.staging.filters.push(filter);
        Ok(())
    }

    /// Adds a function to the staging area. Twig: `ExtensionSet::addFunction()`.
    ///
    /// # Errors
    ///
    /// `Unable to add function "%s" as extensions have already been initialized.` and `Function "%s" is already
    /// registered.` (`LogicException`s).
    pub fn add_function(&mut self, function: TwigFunction) -> Result<()> {
        self.check_not_initialized(|| {
            format!(
                "Unable to add function \"{}\" as extensions have already been initialized.",
                String::from_utf8_lossy(function.name())
            )
        })?;
        if self
            .staging
            .functions
            .iter()
            .any(|f| f.name() == function.name())
        {
            return Err(Error::logic(format!(
                "Function \"{}\" is already registered.",
                String::from_utf8_lossy(function.name())
            )));
        }
        self.staging.functions.push(function);
        Ok(())
    }

    /// Adds a test to the staging area. Twig: `ExtensionSet::addTest()`.
    ///
    /// # Errors
    ///
    /// `Unable to add test "%s" as extensions have already been initialized.` and `Test "%s" is already
    /// registered.` (`LogicException`s).
    pub fn add_test(&mut self, test: TwigTest) -> Result<()> {
        self.check_not_initialized(|| {
            format!(
                "Unable to add test \"{}\" as extensions have already been initialized.",
                String::from_utf8_lossy(test.name())
            )
        })?;
        if self.staging.tests.iter().any(|t| t.name() == test.name()) {
            return Err(Error::logic(format!(
                "Test \"{}\" is already registered.",
                String::from_utf8_lossy(test.name())
            )));
        }
        self.staging.tests.push(test);
        Ok(())
    }

    /// Adds a token parser to the staging area. Twig: `ExtensionSet::addTokenParser()`.
    ///
    /// # Errors
    ///
    /// `Unable to add a token parser as extensions have already been initialized.` and `Tag "%s" is already
    /// registered.` (`LogicException`s).
    pub fn add_token_parser(&mut self, parser: Arc<dyn TokenParser>) -> Result<()> {
        self.check_not_initialized(|| {
            "Unable to add a token parser as extensions have already been initialized.".to_owned()
        })?;
        if self
            .staging
            .token_parsers
            .iter()
            .any(|p| p.tag() == parser.tag())
        {
            return Err(Error::logic(format!(
                "Tag \"{}\" is already registered.",
                parser.tag()
            )));
        }
        self.staging.token_parsers.push(parser);
        Ok(())
    }

    /// Adds a node visitor to the staging area. Twig: `ExtensionSet::addNodeVisitor()`.
    ///
    /// # Errors
    ///
    /// `Unable to add a node visitor as extensions have already been initialized.` (a `LogicException`).
    pub fn add_node_visitor(&mut self, visitor: Arc<dyn NodeVisitor>) -> Result<()> {
        self.check_not_initialized(|| {
            "Unable to add a node visitor as extensions have already been initialized.".to_owned()
        })?;
        self.staging.visitors.push(visitor);
        Ok(())
    }

    /// Twig: `registerUndefinedFilterCallback()`.
    pub fn register_undefined_filter_callback(&mut self, callback: UndefinedFilterCallback) {
        self.filter_callbacks.push(callback);
    }

    /// Twig: `registerUndefinedFunctionCallback()`.
    pub fn register_undefined_function_callback(&mut self, callback: UndefinedFunctionCallback) {
        self.function_callbacks.push(callback);
    }

    /// Twig: `registerUndefinedTestCallback()`.
    pub fn register_undefined_test_callback(&mut self, callback: UndefinedTestCallback) {
        self.test_callbacks.push(callback);
    }

    /// Twig: `registerUndefinedTokenParserCallback()`.
    pub fn register_undefined_token_parser_callback(
        &mut self,
        callback: UndefinedTokenParserCallback,
    ) {
        self.token_parser_callbacks.push(callback);
    }

    /// Sets the validator `matches` uses for constant regular expressions at parse time (the PCRE port). Without
    /// one, only the PHP-level check of `null` patterns is done.
    pub fn set_regex_validator(&mut self, validator: Arc<dyn RegexValidator>) {
        self.regex_validator = Some(validator);
    }

    /// The validator for `matches` patterns, if any.
    pub fn regex_validator(&self) -> Option<&dyn RegexValidator> {
        self.regex_validator.as_deref()
    }

    /// Builds the registry if needed. Twig: `initExtensions()`.
    fn registry(&self) -> &Registry {
        self.registry.get_or_init(|| {
            let mut registry = Registry {
                token_parsers: OrderedMap::new(),
                filters: OrderedMap::new(),
                dynamic_filters: OrderedMap::new(),
                functions: OrderedMap::new(),
                dynamic_functions: OrderedMap::new(),
                tests: OrderedMap::new(),
                dynamic_tests: OrderedMap::new(),
                visitors: Vec::new(),
                expression_parsers: ExpressionParsers::new(),
            };
            for extension in &self.extensions {
                init_extension(
                    &mut registry,
                    extension.filters(),
                    extension.functions(),
                    extension.tests(),
                    extension.token_parsers(),
                    extension.node_visitors(),
                    extension.expression_parsers(),
                );
            }
            init_extension(
                &mut registry,
                self.staging.filters.clone(),
                self.staging.functions.clone(),
                self.staging.tests.clone(),
                self.staging.token_parsers.clone(),
                self.staging.visitors.clone(),
                Vec::new(),
            );
            registry
        })
    }

    /// Builds the registry now (it is otherwise built on first use). Twig: the lazy `initExtensions()`.
    pub fn initialize(&self) {
        self.registry();
    }

    /// The deprecations raised while building the registry (precedences outside 0..512).
    pub fn initialization_deprecations(&self) -> &[Deprecation] {
        self.registry().expression_parsers.deprecations()
    }

    /// The filter named `name`: an exact match, else the first dynamic pattern matching it (registration order),
    /// else the first undefined-filter callback that returns one. Twig: `ExtensionSet::getFilter()`.
    ///
    /// # Errors
    ///
    /// The error of an undefined-filter callback.
    pub fn get_filter(&self, name: &[u8]) -> Result<Option<TwigFilter>> {
        let registry = self.registry();
        if let Some(filter) = registry.filters.get(name) {
            return Ok(Some(filter.clone()));
        }
        for (pattern, filter) in &registry.dynamic_filters.entries {
            if let Some(arguments) = match_dynamic_name(pattern, name) {
                return Ok(Some(filter.with_dynamic_arguments(
                    name.to_vec(),
                    filter.name().to_vec(),
                    arguments,
                )));
            }
        }
        first_from_callbacks(&self.filter_callbacks, name)
    }

    /// The function named `name` (see [`get_filter`](Self::get_filter)). Twig: `ExtensionSet::getFunction()`.
    ///
    /// # Errors
    ///
    /// The error of an undefined-function callback.
    pub fn get_function(&self, name: &[u8]) -> Result<Option<TwigFunction>> {
        let registry = self.registry();
        if let Some(function) = registry.functions.get(name) {
            return Ok(Some(function.clone()));
        }
        for (pattern, function) in &registry.dynamic_functions.entries {
            if let Some(arguments) = match_dynamic_name(pattern, name) {
                return Ok(Some(function.with_dynamic_arguments(
                    name.to_vec(),
                    function.name().to_vec(),
                    arguments,
                )));
            }
        }
        first_from_callbacks(&self.function_callbacks, name)
    }

    /// The test named `name` (see [`get_filter`](Self::get_filter)). Twig: `ExtensionSet::getTest()`.
    ///
    /// # Errors
    ///
    /// The error of an undefined-test callback.
    pub fn get_test(&self, name: &[u8]) -> Result<Option<TwigTest>> {
        let registry = self.registry();
        if let Some(test) = registry.tests.get(name) {
            return Ok(Some(test.clone()));
        }
        for (pattern, test) in &registry.dynamic_tests.entries {
            if let Some(arguments) = match_dynamic_name(pattern, name) {
                return Ok(Some(test.with_dynamic_arguments(
                    name.to_vec(),
                    test.name().to_vec(),
                    arguments,
                )));
            }
        }
        first_from_callbacks(&self.test_callbacks, name)
    }

    /// The token parser for a tag: an exact match, else the first undefined-token-parser callback that returns
    /// one. Twig: `ExtensionSet::getTokenParser()`.
    ///
    /// # Errors
    ///
    /// The error of an undefined-token-parser callback.
    pub fn get_token_parser(&self, tag: &[u8]) -> Result<Option<Arc<dyn TokenParser>>> {
        if let Some(parser) = self.registry().token_parsers.get(tag) {
            return Ok(Some(Arc::clone(parser)));
        }
        first_from_callbacks(&self.token_parser_callbacks, tag)
    }

    /// The filters by name, in registration order (later definitions replace earlier ones in place).
    /// Twig: `getFilters()`.
    pub fn filters(&self) -> impl Iterator<Item = (&[u8], &TwigFilter)> {
        self.registry()
            .filters
            .entries
            .iter()
            .map(|(name, filter)| (name.as_slice(), filter))
    }

    /// The functions by name, in registration order. Twig: `getFunctions()`.
    pub fn functions(&self) -> impl Iterator<Item = (&[u8], &TwigFunction)> {
        self.registry()
            .functions
            .entries
            .iter()
            .map(|(name, function)| (name.as_slice(), function))
    }

    /// The tests by name, in registration order. Twig: `getTests()`.
    pub fn tests(&self) -> impl Iterator<Item = (&[u8], &TwigTest)> {
        self.registry()
            .tests
            .entries
            .iter()
            .map(|(name, test)| (name.as_slice(), test))
    }

    /// The token parsers by tag, in registration order. Twig: `getTokenParsers()`.
    pub fn token_parsers(&self) -> impl Iterator<Item = (&[u8], &Arc<dyn TokenParser>)> {
        self.registry()
            .token_parsers
            .entries
            .iter()
            .map(|(tag, parser)| (tag.as_slice(), parser))
    }

    /// The node visitors in registration order. Twig: `getNodeVisitors()`.
    pub fn node_visitors(&self) -> &[Arc<dyn NodeVisitor>] {
        &self.registry().visitors
    }

    /// The expression parsers. Twig: `getExpressionParsers()`.
    pub fn expression_parsers(&self) -> &ExpressionParsers {
        &self.registry().expression_parsers
    }

    /// A lexer whose operators are the operator tokens of the registered expression parsers, in the order
    /// `Lexer::getOperatorRegex()` collects them.
    pub fn lexer(&self, options: LexerOptions) -> Lexer {
        Lexer::new(options, self.expression_parsers().operator_tokens())
    }
}

/// An undefined-callable callback (see [`UndefinedFilterCallback`]).
type UndefinedCallback<T> = Arc<dyn Fn(&[u8]) -> Result<Option<T>> + Send + Sync>;

/// The first callback result that is not "not found" (Twig: `if (false !== $x = $callback($name)) return $x;`).
fn first_from_callbacks<T>(callbacks: &[UndefinedCallback<T>], name: &[u8]) -> Result<Option<T>> {
    for callback in callbacks {
        if let Some(found) = callback(name)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// Twig: `ExtensionSet::initExtension()`.
fn init_extension(
    registry: &mut Registry,
    filters: Vec<TwigFilter>,
    functions: Vec<TwigFunction>,
    tests: Vec<TwigTest>,
    token_parsers: Vec<Arc<dyn TokenParser>>,
    visitors: Vec<Arc<dyn NodeVisitor>>,
    expression_parsers: Vec<ExpressionParserEntry>,
) {
    for filter in filters {
        let name = filter.name().to_vec();
        if name.contains(&b'*') {
            registry
                .dynamic_filters
                .insert(name.clone(), filter.clone());
        }
        registry.filters.insert(name, filter);
    }
    for function in functions {
        let name = function.name().to_vec();
        if name.contains(&b'*') {
            registry
                .dynamic_functions
                .insert(name.clone(), function.clone());
        }
        registry.functions.insert(name, function);
    }
    for test in tests {
        let name = test.name().to_vec();
        if name.contains(&b'*') {
            registry.dynamic_tests.insert(name.clone(), test.clone());
        }
        registry.tests.insert(name, test);
    }
    for parser in token_parsers {
        registry
            .token_parsers
            .insert(parser.tag().as_bytes().to_vec(), parser);
    }
    registry.visitors.extend(visitors);
    registry.expression_parsers.add(expression_parsers);
}

/// Matches `name` against the dynamic name `pattern` the way `ExtensionSet` does with
/// `'#^'.str_replace('\*', '(.*?)', preg_quote($pattern, '#')).'$#'`: literal parts, each `*` a lazy group of any
/// bytes but `\n`, anchored at the start and at the end (or before a final `\n`). Returns the captured groups.
pub fn match_dynamic_name(pattern: &[u8], name: &[u8]) -> Option<Vec<Vec<u8>>> {
    let parts: Vec<&[u8]> = pattern.split(|&b| b == b'*').collect();
    let mut captures = Vec::new();
    if match_parts(&parts, name, 0, &mut captures) {
        Some(captures)
    } else {
        None
    }
}

/// Backtracking matcher for [`match_dynamic_name`]: `parts[0]` is literal at `position`, then each further part is
/// preceded by a lazy group (shortest first, PCRE's backtracking order).
fn match_parts(parts: &[&[u8]], name: &[u8], position: usize, captures: &mut Vec<Vec<u8>>) -> bool {
    let Some((first, rest)) = parts.split_first() else {
        // `$` without the D modifier: the end, or before a final newline.
        return position == name.len() || (position + 1 == name.len() && name[position] == b'\n');
    };
    if !name[position..].starts_with(first) {
        return false;
    }
    let start = position + first.len();
    if rest.is_empty() {
        return match_parts(rest, name, start, captures);
    }
    // a lazy `(.*?)`: try the shortest group first; `.` does not match "\n"
    let mut end = start;
    loop {
        captures.push(name[start..end].to_vec());
        if match_parts(rest, name, end, captures) {
            return true;
        }
        captures.pop();
        if end >= name.len() || name[end] == b'\n' {
            return false;
        }
        end += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(pattern: &str, name: &str) -> Option<Vec<String>> {
        match_dynamic_name(pattern.as_bytes(), name.as_bytes()).map(|c| {
            c.into_iter()
                .map(|v| String::from_utf8(v).unwrap())
                .collect()
        })
    }

    #[test]
    fn matches_dynamic_names_like_preg_match() {
        assert_eq!(caps("foo_*_bar", "foo_x_bar"), Some(vec!["x".into()]));
        assert_eq!(caps("*_path", "x_y_path"), Some(vec!["x_y".into()]));
        assert_eq!(
            caps("*_foo_*_bar", "foo_foo_bar_bar"),
            Some(vec!["foo".into(), "bar".into()])
        );
        assert_eq!(
            caps("foo_*_bar_*", "foo_a_b_bar_c_bar_d"),
            Some(vec!["a_b".into(), "c_bar_d".into()])
        );
        assert_eq!(caps("*_path", "path"), None);
        assert_eq!(caps("a*", "ab\n"), Some(vec!["b".into()]));
        assert_eq!(caps("a*b", "a\nb"), None);
        assert_eq!(caps("*", ""), Some(vec!["".into()]));
    }
}
