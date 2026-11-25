//! Non-fatal PHP diagnostics: `E_WARNING`, `E_NOTICE`, `E_DEPRECATED`, `E_USER_DEPRECATED`.
//!
//! PHP reports these through `zend_error()` and keeps running. The library never prints or drops
//! them: every function that can raise one takes a `&mut dyn Diagnostics` sink and reports the
//! verbatim message there (reference/conventions.md). The sink may turn a diagnostic into an
//! exception by returning `Err` (PHPUnit converts warnings to exceptions; Twig's `matches` installs
//! an error handler that throws). Library code propagates that `Err` immediately, which mirrors the
//! `if (EG(exception)) return FAILURE;` checks that follow most `zend_error()` calls.

use std::any::Any;
use std::fmt;

use crate::error::PhpError;
use crate::string::PhpString;

/// Severity of a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// `E_WARNING` (2).
    Warning,
    /// `E_NOTICE` (8).
    Notice,
    /// `E_DEPRECATED` (8192).
    Deprecated,
    /// `E_USER_DEPRECATED` (16384), raised by `trigger_error()`/`trigger_deprecation()`.
    UserDeprecated,
}

impl Level {
    /// The `E_*` constant value.
    pub fn code(self) -> i32 {
        match self {
            Level::Warning => 2,
            Level::Notice => 8,
            Level::Deprecated => 8192,
            Level::UserDeprecated => 16384,
        }
    }

    /// The word PHP's default handler prints (`php_error_cb`): `Warning`, `Notice` or
    /// `Deprecated`.
    pub fn label(self) -> &'static str {
        match self {
            Level::Warning => "Warning",
            Level::Notice => "Notice",
            Level::Deprecated | Level::UserDeprecated => "Deprecated",
        }
    }
}

/// One reported diagnostic.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    /// Severity.
    pub level: Level,
    /// Verbatim message (bytes: messages can embed user strings).
    pub message: PhpString,
}

impl Diagnostic {
    /// A diagnostic of `level` with `message`.
    pub fn new(level: Level, message: impl Into<PhpString>) -> Self {
        Diagnostic {
            level,
            message: message.into(),
        }
    }
}

impl fmt::Debug for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.level.label(), self.message)
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.level.label(), self.message)
    }
}

/// A sink for PHP diagnostics (the PHP error handler).
///
/// Implementations decide the policy: collect, ignore, or convert to an exception by returning
/// `Err`. Embedding code that needs its own state inside callbacks invoked through value-level
/// code (for example a template VM re-entered from a `Closure` called by a sort routine) can expose
/// it through [`Diagnostics::host`].
pub trait Diagnostics {
    /// Reports one diagnostic. Returning `Err` aborts the running operation with that exception.
    fn report(&mut self, diagnostic: Diagnostic) -> Result<(), PhpError>;

    /// Escape hatch for embedding code: access to the concrete sink (for example the engine's
    /// render state). Defaults to `None`.
    fn host(&mut self) -> Option<&mut dyn Any> {
        None
    }
}

/// Convenience reporters; library code always holds a `&mut dyn Diagnostics`.
impl dyn Diagnostics + '_ {
    /// Reports an `E_WARNING`.
    pub fn warn(&mut self, message: impl Into<PhpString>) -> Result<(), PhpError> {
        self.report(Diagnostic::new(Level::Warning, message))
    }

    /// Reports an `E_NOTICE`.
    pub fn notice(&mut self, message: impl Into<PhpString>) -> Result<(), PhpError> {
        self.report(Diagnostic::new(Level::Notice, message))
    }

    /// Reports an `E_DEPRECATED`.
    pub fn deprecated(&mut self, message: impl Into<PhpString>) -> Result<(), PhpError> {
        self.report(Diagnostic::new(Level::Deprecated, message))
    }

    /// Reports an `E_USER_DEPRECATED`.
    pub fn user_deprecated(&mut self, message: impl Into<PhpString>) -> Result<(), PhpError> {
        self.report(Diagnostic::new(Level::UserDeprecated, message))
    }
}

/// Collects every diagnostic, in order.
impl Diagnostics for Vec<Diagnostic> {
    fn report(&mut self, diagnostic: Diagnostic) -> Result<(), PhpError> {
        self.push(diagnostic);
        Ok(())
    }
}

/// Collects every diagnostic, in order (a named wrapper around `Vec<Diagnostic>`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Collect {
    /// The diagnostics reported so far.
    pub diagnostics: Vec<Diagnostic>,
}

impl Collect {
    /// An empty collector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Removes and returns everything collected so far.
    pub fn take(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }
}

impl Diagnostics for Collect {
    fn report(&mut self, diagnostic: Diagnostic) -> Result<(), PhpError> {
        self.diagnostics.push(diagnostic);
        Ok(())
    }
}

/// Discards every diagnostic (an explicit caller choice, like `@` or `error_reporting(0)`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Ignore;

impl Diagnostics for Ignore {
    fn report(&mut self, _diagnostic: Diagnostic) -> Result<(), PhpError> {
        Ok(())
    }
}

/// Converts every diagnostic into an exception (like PHPUnit's `convertWarningsToExceptions`, or
/// an error handler that throws `ErrorException`). The exception class is chosen per level by the
/// mapping function; [`Throw::error_exception`] uses `ErrorException` for all levels.
pub struct Throw<F: FnMut(&Diagnostic) -> PhpError> {
    map: F,
}

impl<F: FnMut(&Diagnostic) -> PhpError> Throw<F> {
    /// A sink that throws `map(diagnostic)` for every diagnostic.
    pub fn new(map: F) -> Self {
        Throw { map }
    }
}

impl Throw<fn(&Diagnostic) -> PhpError> {
    /// A sink that throws `ErrorException` with the diagnostic's message.
    pub fn error_exception() -> Self {
        fn map(d: &Diagnostic) -> PhpError {
            PhpError::new(
                crate::error::ThrowableClass::ErrorException,
                d.message.clone(),
            )
        }
        Throw { map }
    }
}

impl<F: FnMut(&Diagnostic) -> PhpError> Diagnostics for Throw<F> {
    fn report(&mut self, diagnostic: Diagnostic) -> Result<(), PhpError> {
        Err((self.map)(&diagnostic))
    }
}

/// Adapts a closure into a sink.
pub struct FnDiagnostics<F: FnMut(Diagnostic) -> Result<(), PhpError>>(pub F);

impl<F: FnMut(Diagnostic) -> Result<(), PhpError>> Diagnostics for FnDiagnostics<F> {
    fn report(&mut self, diagnostic: Diagnostic) -> Result<(), PhpError> {
        (self.0)(diagnostic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sinks() {
        let mut v: Vec<Diagnostic> = Vec::new();
        {
            let d: &mut dyn Diagnostics = &mut v;
            d.warn("a").unwrap();
            d.deprecated("b").unwrap();
        }
        assert_eq!(
            v,
            vec![
                Diagnostic::new(Level::Warning, "a"),
                Diagnostic::new(Level::Deprecated, "b")
            ]
        );

        let mut t = Throw::error_exception();
        let d: &mut dyn Diagnostics = &mut t;
        let err = d.notice("boom").unwrap_err();
        assert_eq!(err.class().name(), "ErrorException");
        assert_eq!(err.message(), "boom");

        let mut ig = Ignore;
        let d: &mut dyn Diagnostics = &mut ig;
        d.warn("x").unwrap();
        assert_eq!(Level::Warning.code(), 2);
        assert_eq!(Level::UserDeprecated.label(), "Deprecated");
    }
}
