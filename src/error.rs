//! Twig's error model: `Twig\Error\Error` and its `SyntaxError`, `LoaderError` and `RuntimeError` subclasses
//! (TWIG/src/Error/), the sandbox's `Twig\Sandbox\SecurityError` family (TWIG/src/Sandbox/), and the PHP throwables
//! that are not Twig errors (`LogicException`, and `ErrorException`, `ValueError`, `TypeError`, ... carried as
//! [`ErrorKind::Php`]).
//!
//! Twig treats the two groups differently: a Twig error keeps its class when it crosses a template and gets the
//! template's name and line (`Error::updateRepr()`), while any other throwable raised inside a template is wrapped by
//! the template that handles it (`Template::handleException()`: `An exception has been thrown during the rendering
//! of a template ("%s").`) and by `Environment::compileSource()` while compiling. [`ErrorKind::is_twig_error`] tells
//! them apart.
//!
//! Messages are Rust strings. Where Twig would interpolate template bytes that are not valid UTF-8 into a message,
//! twig-rs uses a lossy conversion (invalid sequences become U+FFFD); this is the only intended difference from
//! the PHP messages.

use std::any::Any;
use std::fmt::{self, Write as _};
use std::sync::Arc;

use twig_value::{PhpError, ThrowableClass};

use crate::source::Source;

/// Result alias for operations that fail with a Twig [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The PHP exception class an [`Error`] stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// `Twig\Error\SyntaxError`: lexer, parser and compile-time errors.
    Syntax,
    /// `Twig\Error\LoaderError`: template loading errors.
    Loader,
    /// `Twig\Error\RuntimeError`: errors raised while rendering.
    Runtime,
    /// `Twig\Sandbox\SecurityError`: a sandbox violation of its own class (raised by a custom security policy).
    Security,
    /// `Twig\Sandbox\SecurityNotAllowedTagError`: a tag the security policy does not allow.
    SecurityNotAllowedTag,
    /// `Twig\Sandbox\SecurityNotAllowedFilterError`: a filter the security policy does not allow.
    SecurityNotAllowedFilter,
    /// `Twig\Sandbox\SecurityNotAllowedFunctionError`: a function the security policy does not allow.
    SecurityNotAllowedFunction,
    /// `Twig\Sandbox\SecurityNotAllowedTestError`: a test the security policy does not allow.
    SecurityNotAllowedTest,
    /// `Twig\Sandbox\SecurityNotAllowedMethodError`: a method call the security policy does not allow.
    SecurityNotAllowedMethod,
    /// `Twig\Sandbox\SecurityNotAllowedPropertyError`: a property read the security policy does not allow.
    SecurityNotAllowedProperty,
    /// PHP's `LogicException`: a programming error Twig reports outside its own error classes (registering an
    /// extension too late, a node built without a required child, ...). Such errors carry no template location.
    Logic,
    /// Any other PHP throwable that is not a Twig error: an `ErrorException` made from a PHP warning or notice, a
    /// `ValueError`, a `TypeError`, an exception of a host object, ... The throwable itself (its class and verbatim
    /// message) is [`Error::php_error`], its class [`Error::class_name`]; such errors carry no template location.
    Php,
}

impl ErrorKind {
    /// The fully qualified PHP class name, e.g. `Twig\Error\SyntaxError` (what Twig's test harness prints). For
    /// [`ErrorKind::Php`], which stands for many classes, this is the interface they share, `Throwable`;
    /// [`Error::class_name`] gives the class of a particular error.
    pub fn class_name(self) -> &'static str {
        match self {
            ErrorKind::Syntax => "Twig\\Error\\SyntaxError",
            ErrorKind::Loader => "Twig\\Error\\LoaderError",
            ErrorKind::Runtime => "Twig\\Error\\RuntimeError",
            ErrorKind::Security => "Twig\\Sandbox\\SecurityError",
            ErrorKind::SecurityNotAllowedTag => "Twig\\Sandbox\\SecurityNotAllowedTagError",
            ErrorKind::SecurityNotAllowedFilter => "Twig\\Sandbox\\SecurityNotAllowedFilterError",
            ErrorKind::SecurityNotAllowedFunction => {
                "Twig\\Sandbox\\SecurityNotAllowedFunctionError"
            }
            ErrorKind::SecurityNotAllowedTest => "Twig\\Sandbox\\SecurityNotAllowedTestError",
            ErrorKind::SecurityNotAllowedMethod => "Twig\\Sandbox\\SecurityNotAllowedMethodError",
            ErrorKind::SecurityNotAllowedProperty => {
                "Twig\\Sandbox\\SecurityNotAllowedPropertyError"
            }
            ErrorKind::Logic => "LogicException",
            ErrorKind::Php => "Throwable",
        }
    }

    /// Whether the class extends `Twig\Error\Error` (syntax, loader, runtime and sandbox errors): such errors get the
    /// template's name and line in their message and are not wrapped by templates or by the compilation.
    pub fn is_twig_error(self) -> bool {
        !matches!(self, ErrorKind::Logic | ErrorKind::Php)
    }

    /// Whether the class extends `Twig\Sandbox\SecurityError`.
    pub fn is_security_error(self) -> bool {
        matches!(
            self,
            ErrorKind::Security
                | ErrorKind::SecurityNotAllowedTag
                | ErrorKind::SecurityNotAllowedFilter
                | ErrorKind::SecurityNotAllowedFunction
                | ErrorKind::SecurityNotAllowedTest
                | ErrorKind::SecurityNotAllowedMethod
                | ErrorKind::SecurityNotAllowedProperty
        )
    }
}

/// A Twig error: a kind, the raw message, the template line and column, the template source and an optional
/// previous error. [`Display`](fmt::Display) prints the final message exactly like Twig's `Error::getMessage()`.
///
/// Twig: `Twig\Error\Error` (TWIG/src/Error/Error.php). The template line keeps Twig's conventions: `-1` (the
/// default) means "unknown, may be guessed later" and `0` means "no line"; only lines `> 0` are printed.
///
/// An error of a kind that is not a Twig error ([`ErrorKind::Logic`], [`ErrorKind::Php`]) stands for that PHP
/// throwable: its message is the raw message, whatever line or source is set.
#[derive(Clone)]
pub struct Error {
    inner: Box<Inner>,
}

#[derive(Clone)]
struct Inner {
    kind: ErrorKind,
    raw_message: String,
    line: i64,
    column: Option<usize>,
    source: Option<Source>,
    previous: Option<Arc<dyn std::error::Error + Send + Sync + 'static>>,
    /// The PHP throwable an [`ErrorKind::Php`] (or a converted `LogicException`) error stands for.
    php: Option<PhpError>,
    /// Structured data of the exception object that its message does not show (the names a sandbox violation
    /// carries: `getTagName()`, `getClassName()`, ...).
    detail: Option<Arc<dyn Any + Send + Sync>>,
}

impl Error {
    /// An error with Twig's constructor defaults: line `-1`, no source, no column, no previous error.
    ///
    /// Twig: `new Error($message)`.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error {
            inner: Box::new(Inner {
                kind,
                raw_message: message.into(),
                line: -1,
                column: None,
                source: None,
                previous: None,
                php: None,
                detail: None,
            }),
        }
    }

    /// The error standing for a PHP throwable that is not a Twig error: [`ErrorKind::Logic`] for a
    /// `LogicException`, [`ErrorKind::Php`] for anything else (`ErrorException`, `ValueError`, `TypeError`, ...).
    /// The message is the throwable's (lossy UTF-8); the throwable stays available as [`php_error`](Self::php_error)
    /// and comes back unchanged when the error crosses the engine again.
    pub fn php(error: PhpError) -> Self {
        let kind = if *error.class() == ThrowableClass::LogicException {
            ErrorKind::Logic
        } else {
            ErrorKind::Php
        };
        let mut converted = Error::new(kind, error.message_lossy().into_owned());
        converted.inner.php = Some(error);
        converted
    }

    /// Attaches structured data of the exception object (see [`detail`](Self::detail)).
    #[must_use]
    pub(crate) fn with_detail(mut self, detail: Arc<dyn Any + Send + Sync>) -> Self {
        self.inner.detail = Some(detail);
        self
    }

    /// Structured data of the exception object that is not part of its message, e.g. the
    /// [`SecurityError`](crate::runtime::SecurityError) of a sandbox violation.
    pub(crate) fn detail<T: Any>(&self) -> Option<&T> {
        self.inner.detail.as_deref()?.downcast_ref::<T>()
    }

    /// The PHP throwable this error stands for: the one given to [`Error::php`], else (for a `LogicException` made
    /// with [`Error::logic`]) `None`.
    pub fn php_error(&self) -> Option<&PhpError> {
        self.inner.php.as_ref()
    }

    /// The fully qualified PHP class of the error: [`ErrorKind::class_name`], or the class of the PHP throwable an
    /// [`ErrorKind::Php`] error stands for (e.g. `ErrorException`, `ValueError`).
    pub fn class_name(&self) -> &str {
        match &self.inner.php {
            Some(php) => php.class().name(),
            None => self.inner.kind.class_name(),
        }
    }

    /// Twig: `new SyntaxError($message)`.
    pub fn syntax(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Syntax, message)
    }

    /// Twig: `new LoaderError($message)`.
    pub fn loader(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Loader, message)
    }

    /// Twig: `new RuntimeError($message)`.
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Runtime, message)
    }

    /// PHP: `new LogicException($message)`.
    pub fn logic(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Logic, message)
    }

    /// Sets the template line (constructor argument `$lineno`).
    #[must_use]
    pub fn with_line(mut self, line: i64) -> Self {
        self.inner.line = line;
        self
    }

    /// Sets the template column (constructor argument `$columnno`).
    #[must_use]
    pub fn with_column(mut self, column: Option<usize>) -> Self {
        self.inner.column = column;
        self
    }

    /// Sets the template source (constructor argument `$source`). A Twig error asks the source for its path now,
    /// which resolves a lazily resolved path ([`Source::with_lazy_path`]), as `Error::updateRepr()` does.
    #[must_use]
    pub fn with_source(mut self, source: Source) -> Self {
        self.inner.source = Some(source);
        self.update_repr();
        self
    }

    /// Sets the previous error (constructor argument `$previous`).
    #[must_use]
    pub fn with_previous<E>(mut self, previous: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        self.inner.previous = Some(Arc::new(previous));
        self
    }

    /// The Twig exception class this error stands for.
    pub fn kind(&self) -> ErrorKind {
        self.inner.kind
    }

    /// The message as passed to the constructor plus appended parts. Twig: `Error::getRawMessage()`.
    pub fn raw_message(&self) -> &str {
        &self.inner.raw_message
    }

    /// Twig: `Error::getTemplateLine()`.
    pub fn template_line(&self) -> i64 {
        self.inner.line
    }

    /// Twig: `Error::setTemplateLine()`.
    pub fn set_template_line(&mut self, line: i64) {
        self.inner.line = line;
    }

    /// The 1-based template column, if known. Twig: `Error::getTemplateColumn()`.
    pub fn template_column(&self) -> Option<usize> {
        self.inner.column
    }

    /// Twig: `Error::setTemplateColumn()`.
    pub fn set_template_column(&mut self, column: Option<usize>) {
        self.inner.column = column;
    }

    /// The source of the template the error belongs to. Twig: `Error::getSourceContext()`.
    pub fn source_context(&self) -> Option<&Source> {
        self.inner.source.as_ref()
    }

    /// Twig: `Error::setSourceContext()`. A Twig error asks the source for its path now (see
    /// [`with_source`](Self::with_source)).
    pub fn set_source_context(&mut self, source: Option<Source>) {
        self.inner.source = source;
        self.update_repr();
    }

    /// The source part of `Error::updateRepr()` (TWIG/src/Error/Error.php): `if ($this->source &&
    /// $this->source->getPath())` resolves a lazily resolved path the first time the error gets the source (the
    /// source keeps the path, so the later calls of `updateRepr()` find it resolved). The message itself is built by
    /// [`message`](Self::message). Errors that are not Twig errors have no `updateRepr()`.
    fn update_repr(&self) {
        if self.inner.kind.is_twig_error()
            && let Some(source) = &self.inner.source
        {
            source.path();
        }
    }

    /// The previous error, if any. Twig: `Exception::getPrevious()`. Also available as
    /// [`std::error::Error::source`].
    pub fn previous(&self) -> Option<&(dyn std::error::Error + Send + Sync + 'static)> {
        self.inner.previous.as_deref()
    }

    /// Appends to the raw message. Twig: `Error::appendMessage()`.
    pub fn append_message(&mut self, message: &str) {
        self.inner.raw_message.push_str(message);
    }

    /// Fills in an unknown template line: when the line is `-1` (or lower), it becomes the line returned by
    /// `find_line`, or `0` (no line in the message) when that returns `None`. A known line (`> -1`) is kept and
    /// `find_line` is not called.
    ///
    /// Twig: `Error::guess()` (TWIG/src/Error/Error.php), whose `guessTemplateInfo()` sets the line to `0` and then
    /// looks for the line of the template being executed in the PHP backtrace; `find_line` stands for that search.
    pub fn guess(&mut self, find_line: impl FnOnce(&Self) -> Option<i64>) {
        if self.inner.line > -1 {
            return;
        }
        self.inner.line = 0;
        if let Some(line) = find_line(self) {
            self.inner.line = line;
        }
    }

    /// Appends `Did you mean "a", "b"?` listing the items close to `name`: items within a Levenshtein distance of
    /// `strlen(name) / 3`, or containing `name`, sorted by distance (ties keep the order of `items`). Does nothing
    /// when there is no such item.
    ///
    /// Twig: `SyntaxError::addSuggestions()` (TWIG/src/Error/SyntaxError.php).
    pub fn add_suggestions<I>(&mut self, name: &[u8], items: I)
    where
        I: IntoIterator,
        I::Item: AsRef<[u8]>,
    {
        // `$alternatives[$item] = $lev`: an ordered map, a repeated item keeps its first position.
        let mut alternatives: Vec<(Vec<u8>, usize)> = Vec::new();
        for item in items {
            let item = item.as_ref();
            let lev = levenshtein(name, item);
            if lev as f64 <= name.len() as f64 / 3.0 || contains(item, name) {
                match alternatives
                    .iter_mut()
                    .find(|(existing, _)| existing.as_slice() == item)
                {
                    Some(entry) => entry.1 = lev,
                    None => alternatives.push((item.to_vec(), lev)),
                }
            }
        }
        if alternatives.is_empty() {
            return;
        }
        // `asort()` is stable since PHP 8.0.
        alternatives.sort_by_key(|&(_, lev)| lev);
        let mut message = String::from(" Did you mean \"");
        for (i, (item, _)) in alternatives.iter().enumerate() {
            if i > 0 {
                message.push_str("\", \"");
            }
            message.push_str(&String::from_utf8_lossy(item));
        }
        message.push_str("\"?");
        self.append_message(&message);
    }

    /// The final message: the raw message with ` in "<name>"`, ` at line N` and ` column C` inserted before a
    /// trailing `.` or `?`. The message of an error that is not a Twig error (a `LogicException`, an
    /// [`ErrorKind::Php`] throwable) is its raw message.
    ///
    /// Twig: `Error::updateRepr()` (TWIG/src/Error/Error.php), i.e. what `getMessage()` returns.
    pub fn message(&self) -> String {
        let inner = &*self.inner;
        let mut message = inner.raw_message.clone();
        if !inner.kind.is_twig_error() {
            return message;
        }
        let punctuation = match message.as_bytes().last() {
            Some(b'.') => Some('.'),
            Some(b'?') => Some('?'),
            _ => None,
        };
        if punctuation.is_some() {
            message.pop();
        }
        if let Some(source) = &inner.source {
            let name = source.name();
            // `if ($this->source && $this->source->getName())`: PHP truthiness, so "0" counts as no name.
            if !name.is_empty() && name != "0" {
                let _ = write!(message, " in \"{name}\"");
            }
        }
        if inner.line > 0 {
            let _ = write!(message, " at line {}", inner.line);
            if let Some(column) = inner.column {
                let _ = write!(message, " column {column}");
            }
        }
        if let Some(punctuation) = punctuation {
            message.push(punctuation);
        }
        message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = &*self.inner;
        f.debug_struct("Error")
            .field("kind", &inner.kind)
            .field("raw_message", &inner.raw_message)
            .field("line", &inner.line)
            .field("column", &inner.column)
            .field("source", &inner.source.as_ref().map(Source::name))
            .field("previous", &inner.previous)
            .field("php", &inner.php)
            .finish()
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner
            .previous
            .as_deref()
            .map(|previous| previous as &(dyn std::error::Error + 'static))
    }
}

/// PHP `str_contains($haystack, $needle)`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// The Levenshtein distance of two byte strings with unit insertion, replacement and deletion costs.
///
/// Port of `reference_levdist()` behind PHP's `levenshtein()` with its default costs
/// (PHPSRC/ext/standard/levenshtein.c).
pub fn levenshtein(string1: &[u8], string2: &[u8]) -> usize {
    let (cost_ins, cost_rep, cost_del) = (1, 1, 1);
    if string1.is_empty() {
        return string2.len() * cost_ins;
    }
    if string2.is_empty() {
        return string1.len() * cost_del;
    }
    let mut p1: Vec<usize> = (0..=string2.len()).map(|i2| i2 * cost_ins).collect();
    let mut p2: Vec<usize> = vec![0; string2.len() + 1];
    for &c1 in string1 {
        p2[0] = p1[0] + cost_del;
        for (i2, &c2) in string2.iter().enumerate() {
            let mut c0 = p1[i2] + if c1 == c2 { 0 } else { cost_rep };
            let deletion = p1[i2 + 1] + cost_del;
            if deletion < c0 {
                c0 = deletion;
            }
            let insertion = p2[i2] + cost_ins;
            if insertion < c0 {
                c0 = insertion;
            }
            p2[i2 + 1] = c0;
        }
        std::mem::swap(&mut p1, &mut p2);
    }
    p1[string2.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_update_repr() {
        let source = Source::new("", "index");
        let error = Error::syntax("Unclosed comment.")
            .with_line(2)
            .with_source(source.clone());
        assert_eq!(
            error.to_string(),
            "Unclosed comment in \"index\" at line 2."
        );
        let error = Error::syntax("Unexpected \"}\".")
            .with_line(1)
            .with_column(Some(6))
            .with_source(source.clone());
        assert_eq!(
            error.to_string(),
            "Unexpected \"}\" in \"index\" at line 1 column 6."
        );
        let error = Error::runtime("twig error with line")
            .with_line(42)
            .with_source(Source::new("", "t"));
        assert_eq!(
            error.to_string(),
            "twig error with line in \"t\" at line 42"
        );
        let error = Error::syntax("No line.")
            .with_line(0)
            .with_column(Some(3))
            .with_source(source);
        assert_eq!(error.to_string(), "No line in \"index\".");
        let error = Error::syntax("Nameless?")
            .with_line(3)
            .with_source(Source::new("", "0"));
        assert_eq!(error.to_string(), "Nameless at line 3?");
        assert_eq!(Error::syntax("").to_string(), "");
    }

    #[test]
    fn suggests_alternatives() {
        let mut error = Error::syntax("Unknown \"fo\" tag.")
            .with_line(1)
            .with_source(Source::new("", "index"));
        error.add_suggestions(b"fo", ["apply", "for", "if"]);
        assert_eq!(
            error.to_string(),
            "Unknown \"fo\" tag. Did you mean \"for\" in \"index\" at line 1?"
        );
        let mut error = Error::syntax("Unknown \"zzz\" tag.");
        error.add_suggestions(b"zzz", ["apply", "for"]);
        assert_eq!(error.raw_message(), "Unknown \"zzz\" tag.");
    }

    #[test]
    fn computes_php_levenshtein() {
        assert_eq!(levenshtein(b"", b""), 0);
        assert_eq!(levenshtein(b"", b"abc"), 3);
        assert_eq!(levenshtein(b"kitten", b"sitting"), 3);
        assert_eq!(levenshtein(b"sitting", b"kitten"), 3);
        assert_eq!(levenshtein("é".as_bytes(), b"e"), 2);
    }

    #[test]
    fn guesses_only_unknown_lines() {
        let source = Source::new("", "index");
        let mut error = Error::runtime("Boom.").with_source(source.clone());
        error.guess(|_| Some(4));
        assert_eq!(error.to_string(), "Boom in \"index\" at line 4.");
        error.guess(|_| panic!("a known line is kept"));

        let mut error = Error::runtime("Boom.").with_source(source.clone());
        error.guess(|_| None);
        assert_eq!(error.template_line(), 0);
        assert_eq!(error.to_string(), "Boom in \"index\".");

        let mut error = Error::runtime("Boom.").with_line(-7).with_source(source);
        error.guess(|error| {
            assert_eq!(error.template_line(), 0);
            None
        });
        assert_eq!(error.template_line(), 0);
    }

    #[test]
    fn carries_php_throwables() {
        let error = Error::php(PhpError::new(
            ThrowableClass::ErrorException,
            "file_get_contents(x): Failed to open stream: No such file or directory",
        ))
        .with_line(3)
        .with_source(Source::new("", "index"));
        assert_eq!(error.kind(), ErrorKind::Php);
        assert!(!error.kind().is_twig_error());
        assert_eq!(error.class_name(), "ErrorException");
        assert_eq!(
            error.to_string(),
            "file_get_contents(x): Failed to open stream: No such file or directory",
            "a PHP throwable's message is not decorated"
        );
        let logic = Error::php(PhpError::new(ThrowableClass::LogicException, "late"));
        assert_eq!(logic.kind(), ErrorKind::Logic);
        assert_eq!(logic.class_name(), "LogicException");
        assert_eq!(
            Error::php(PhpError::value_error("x")).class_name(),
            "ValueError"
        );
        assert_eq!(
            Error::new(
                ErrorKind::SecurityNotAllowedFilter,
                "Filter \"x\" is not allowed."
            )
            .with_line(2)
            .with_source(Source::new("", "index"))
            .to_string(),
            "Filter \"x\" is not allowed in \"index\" at line 2."
        );
        assert!(ErrorKind::SecurityNotAllowedFilter.is_security_error());
        assert!(ErrorKind::SecurityNotAllowedFilter.is_twig_error());
        assert!(!ErrorKind::Runtime.is_security_error());
    }

    #[test]
    fn twig_errors_resolve_a_lazy_path_when_they_get_the_source() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let lazy = || {
            let counter = Arc::clone(&calls);
            Source::with_lazy_path("", "index", move || {
                counter.fetch_add(1, Ordering::SeqCst);
                "/path/to/index".to_owned()
            })
        };
        let _ = Error::php(PhpError::value_error("x")).with_source(lazy());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "not a Twig error: no updateRepr()"
        );
        let error = Error::runtime("Boom.").with_source(lazy());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            error.source_context().map(Source::path),
            Some("/path/to/index")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mut error = Error::runtime("Boom.");
        error.set_source_context(Some(lazy()));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn exposes_the_previous_error() {
        let previous = Error::runtime("inner");
        let error = Error::runtime("outer").with_previous(previous);
        let source = std::error::Error::source(&error).map(ToString::to_string);
        assert_eq!(source.as_deref(), Some("inner"));
    }
}
