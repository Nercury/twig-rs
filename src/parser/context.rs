//! The parsing half of `Twig\Environment`: the extensions and the lexer options, `tokenize()`, `parse()` and the
//! error handling of `compileSource()` (TWIG/src/Environment.php).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{DEFAULT_MAX_NESTING_LEVEL, Parser, TokenStream};
use crate::ast::Node;
use crate::deprecation::DeprecationSink;
use crate::error::{Error, Result};
use crate::extension::{ExtensionSet, OptimizerExtension};
use crate::lexer::{Lexer, LexerOptions};
use crate::source::Source;

/// What an `Environment` needs to turn template sources into visited `ModuleNode`s: the [`ExtensionSet`] and the
/// lexer options. Twig: `Environment::tokenize()` and `Environment::parse()`.
///
/// The lexer is built on first use from the registry's operators. Before anything is lexed or parsed, the context
/// initializes the extensions as Twig does at that point ([`initialize_extensions`](Self::initialize_extensions)):
/// an invalid optimizer mode fails every call, and the deprecations of the initialization (deprecated optimizer
/// modes, precedences outside 0..512) are reported once. A context can be shared between threads.
///
/// Every [`parse`](Self::parse) uses a fresh [`Parser`]. Twig's environment keeps one parser, which does not restore
/// its state after a syntax error, so after a failed parse the indices of embedded templates continue from the failed
/// template's in later templates of the same environment; here they start at 1 for every template.
///
/// ```
/// use twig::deprecation::Deprecation;
/// use twig::extension::ExtensionSet;
/// use twig::parser::ParseContext;
/// use twig::Source;
///
/// let context = ParseContext::new(ExtensionSet::with_core_extensions());
/// let mut deprecations: Vec<Deprecation> = Vec::new();
/// let module = context.parse_source(&Source::new("{% if a %}{{ a }}{% endif %}", "index"), &mut deprecations)?;
/// assert!(module.as_module().is_some());
///
/// let error = context
///     .parse_source(&Source::new("{% extends 'a' %}{% extends 'b' %}", "index"), &mut deprecations)
///     .unwrap_err();
/// assert_eq!(error.to_string(), "Multiple extends tags are forbidden in \"index\" at line 1.");
/// # Ok::<(), twig::Error>(())
/// ```
pub struct ParseContext {
    extensions: ExtensionSet,
    lexer_options: LexerOptions,
    lexer: OnceLock<Lexer>,
    initialization_reported: AtomicBool,
    max_nesting_level: usize,
}

impl std::fmt::Debug for ParseContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParseContext")
            .field("extensions", &self.extensions)
            .field("lexer_options", &self.lexer_options)
            .field("max_nesting_level", &self.max_nesting_level)
            .finish_non_exhaustive()
    }
}

impl ParseContext {
    /// A context with the default lexer options.
    pub fn new(extensions: ExtensionSet) -> Self {
        ParseContext::with_lexer_options(extensions, LexerOptions::default())
    }

    /// A context with the given lexer options (Twig's `Lexer` options, set with `Environment::setLexer()`).
    pub fn with_lexer_options(extensions: ExtensionSet, lexer_options: LexerOptions) -> Self {
        ParseContext {
            extensions,
            lexer_options,
            lexer: OnceLock::new(),
            initialization_reported: AtomicBool::new(false),
            max_nesting_level: DEFAULT_MAX_NESTING_LEVEL,
        }
    }

    /// The extensions.
    pub fn extensions(&self) -> &ExtensionSet {
        &self.extensions
    }

    /// The extensions, to register more before the registry is built (afterwards the registry refuses
    /// registrations, as Twig does).
    pub fn extensions_mut(&mut self) -> &mut ExtensionSet {
        &mut self.extensions
    }

    /// The lexer options.
    pub fn lexer_options(&self) -> &LexerOptions {
        &self.lexer_options
    }

    /// Sets the maximum nesting level of expressions and bodies (see [`Parser::set_max_nesting_level`]).
    pub fn set_max_nesting_level(&mut self, level: usize) {
        self.max_nesting_level = level;
    }

    /// The lexer (built on first use). Twig: `new Lexer($env)` in `Environment::tokenize()`.
    pub fn lexer(&self) -> &Lexer {
        self.lexer
            .get_or_init(|| self.extensions.lexer(self.lexer_options.clone()))
    }

    /// Initializes the extensions, as Twig's `ExtensionSet::initExtensions()` (TWIG/src/ExtensionSet.php) does the
    /// first time one of the registry's getters is called:
    ///
    /// - the environment's lexer calls one the first time it tokenizes (`Lexer::initialize()` →
    ///   `getOperatorRegex()` → `Environment::getExpressionParsers()`, TWIG/src/Lexer.php), so before the first
    ///   template is lexed, and so does the parser's constructor (`new Parser($env)` in `Environment::parse()`
    ///   calls `getExpressionParsers()`, TWIG/src/Parser.php); [`tokenize`](Self::tokenize) and
    ///   [`parse`](Self::parse) call this method first;
    /// - `Environment::getTemplateClass()` calls one when the `SandboxExtension` is registered
    ///   (`getSandboxSignature()` → `getTokenParsers()`, TWIG/src/Environment.php), before the template is looked
    ///   up, so `load()` throws the error itself, not wrapped by `compileSource()`; a caller modelling that class key
    ///   calls this method there.
    ///
    /// The extensions build their node visitors there. The [`OptimizerExtension`]'s `OptimizerNodeVisitor` rejects an
    /// invalid mode and reports the deprecated ones (`OptimizerExtension::initialize()`). The deprecations raised while
    /// building the registry follow: precedences outside 0..512 come from extensions that define operators, which the
    /// environment registers after the optimizer extension (Twig initializes the extensions in registration order).
    ///
    /// The deprecations are reported once, by the first call that succeeds.
    ///
    /// # Errors
    ///
    /// `Optimizer mode "%s" is not valid.`, an `InvalidArgumentException`, for an invalid optimizer mode. Nothing is
    /// reported then, and every later call fails again: Twig marks the extensions initialized only at the end of
    /// `initExtensions()`, so every getter retries it.
    pub fn initialize_extensions(&self, deprecations: &mut dyn DeprecationSink) -> Result<()> {
        let mode_deprecations = match self.extensions.extension::<OptimizerExtension>() {
            Some(optimizer) => optimizer.initialize()?,
            None => Vec::new(),
        };
        if !self.initialization_reported.swap(true, Ordering::AcqRel) {
            for deprecation in mode_deprecations {
                deprecations.report(deprecation);
            }
            for deprecation in self.extensions.initialization_deprecations() {
                deprecations.report(deprecation.clone());
            }
        }
        Ok(())
    }

    /// Lexes a template. Twig: `Environment::tokenize()`.
    ///
    /// # Errors
    ///
    /// The error of [`initialize_extensions`](Self::initialize_extensions), then the lexer's syntax errors.
    pub fn tokenize(
        &self,
        source: &Source,
        deprecations: &mut dyn DeprecationSink,
    ) -> Result<TokenStream> {
        self.initialize_extensions(deprecations)?;
        self.lexer().tokenize(source, deprecations)
    }

    /// Parses a token stream into a `ModuleNode` and runs the node visitors over it. Twig: `Environment::parse()`.
    ///
    /// # Errors
    ///
    /// The error of [`initialize_extensions`](Self::initialize_extensions), then the parser's syntax errors and the
    /// errors of the node visitors.
    pub fn parse(
        &self,
        stream: TokenStream,
        deprecations: &mut dyn DeprecationSink,
    ) -> Result<Node> {
        self.initialize_extensions(deprecations)?;
        let mut parser = Parser::new(&self.extensions, stream, deprecations);
        parser.set_max_nesting_level(self.max_nesting_level);
        parser.parse(None, false)
    }

    /// Lexes and parses a template: `$env->parse($env->tokenize($source))`.
    ///
    /// # Errors
    ///
    /// As [`tokenize`](Self::tokenize) and [`parse`](Self::parse).
    pub fn parse_source(
        &self,
        source: &Source,
        deprecations: &mut dyn DeprecationSink,
    ) -> Result<Node> {
        let stream = self.tokenize(source, deprecations)?;
        self.parse(stream, deprecations)
    }

    /// Lexes and parses a template with the error handling of `Environment::compileSource()` (see
    /// [`compile_source_error`]); the compilation itself comes with the code generator.
    ///
    /// # Errors
    ///
    /// The errors of [`parse_source`](Self::parse_source), passed through [`compile_source_error`].
    pub fn parse_template(
        &self,
        source: &Source,
        deprecations: &mut dyn DeprecationSink,
    ) -> Result<Node> {
        self.parse_source(source, deprecations)
            .map_err(|error| compile_source_error(error, source))
    }
}

/// The `catch` blocks of `Environment::compileSource()`: a Twig error (syntax, loader, runtime, sandbox) gets the
/// source of the compiled template (`setSourceContext($source)`); any other error (a PHP `LogicException`, another
/// throwable) becomes `An exception has been thrown during the compilation of a template ("%s").` without a line, with
/// the error as its previous error.
pub fn compile_source_error(error: Error, source: &Source) -> Error {
    match error.kind() {
        kind if !kind.is_twig_error() => Error::syntax(format!(
            "An exception has been thrown during the compilation of a template (\"{}\").",
            error.raw_message()
        ))
        .with_line(-1)
        .with_source(source.clone())
        .with_previous(error),
        _ => {
            let mut error = error;
            error.set_source_context(Some(source.clone()));
            error
        }
    }
}
