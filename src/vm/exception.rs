//! Exceptions raised while rendering: Twig errors and other PHP throwables, with the template stack they were raised
//! in.

use std::fmt;
use std::sync::Arc;

use twig_value::{PhpError, ThrowableClass};

use crate::error::Error;
use crate::lltl::FileId;

/// One frame of the template stack at the point an exception was raised: the compiled file of the frame's template
/// and the line of the instruction it was executing (`None` before the first line marker of the file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TraceFrame {
    pub(crate) file: FileId,
    pub(crate) line: Option<i64>,
}

/// The template stack of an exception, innermost frame first (what Twig's `Error::guess()` reads from the PHP
/// backtrace of the exception).
pub(crate) type Trace = Arc<[TraceFrame]>;

/// A PHP exception raised while rendering: a Twig error (`Twig\Error\Error` and subclasses) or any other throwable
/// (`TypeError`, `DivisionByZeroError`, an exception of a host object or callable, a `LogicException`, ...).
///
/// Twig lets non-Twig exceptions propagate to the template that handles them (`Template::handleException()`), which
/// wraps them in `RuntimeError('An exception has been thrown during the rendering of a template ("%s").')`; the
/// distinction is kept until then. An exception also remembers the template stack it was raised in, from which the
/// handling template guesses the line of an error without an explicit line.
///
/// Filters, functions, tests and host objects return exceptions from either source through `From`:
///
/// ```
/// use twig::vm::Exception;
/// use twig::value::PhpError;
///
/// let e: Exception = PhpError::type_error("Unsupported operand types: string + int").into();
/// assert!(e.twig_error().is_none());
/// let e: Exception = twig::Error::runtime("The \"merge\" filter expects a sequence or a mapping.").into();
/// assert!(e.twig_error().is_some());
/// ```
#[derive(Clone)]
pub struct Exception {
    inner: Box<Inner>,
}

#[derive(Clone)]
struct Inner {
    kind: Kind,
    trace: Option<Trace>,
}

#[derive(Clone)]
enum Kind {
    Twig(Error),
    Php(PhpError),
}

impl Exception {
    /// The Twig error, when this is one (any [`ErrorKind`](crate::ErrorKind) but
    /// [`ErrorKind::Php`](crate::ErrorKind::Php); a [`ErrorKind::Logic`](crate::ErrorKind::Logic) error stands for
    /// PHP's `LogicException`, which Twig treats like any other non-Twig exception).
    pub fn twig_error(&self) -> Option<&Error> {
        match &self.inner.kind {
            Kind::Twig(error) => Some(error),
            Kind::Php(_) => None,
        }
    }

    /// The PHP throwable, when this is not a Twig error.
    pub fn php_error(&self) -> Option<&PhpError> {
        match &self.inner.kind {
            Kind::Php(error) => Some(error),
            Kind::Twig(_) => None,
        }
    }

    /// Whether this is an instance of `Twig\Error\Error` (syntax, loader, runtime or sandbox error), the errors
    /// `Template::handleException()` does not wrap.
    pub fn is_twig_error(&self) -> bool {
        matches!(&self.inner.kind, Kind::Twig(error) if error.kind().is_twig_error())
    }

    /// The message: `getMessage()` of the exception.
    pub fn message(&self) -> String {
        match &self.inner.kind {
            Kind::Twig(error) => error.message(),
            Kind::Php(error) => error.message_lossy().into_owned(),
        }
    }

    /// Converts into an [`Error`]: a Twig error as it is; any other throwable as the
    /// [`ErrorKind::Php`](crate::ErrorKind::Php) error standing for it ([`Error::php`]: its class and message are
    /// kept), as such exceptions leave the engine outside every template, unwrapped.
    pub fn into_error(self) -> Error {
        match self.inner.kind {
            Kind::Twig(error) => error,
            Kind::Php(error) => Error::php(error),
        }
    }

    pub(crate) fn kind_twig_mut(&mut self) -> Option<&mut Error> {
        match &mut self.inner.kind {
            Kind::Twig(error) => Some(error),
            Kind::Php(_) => None,
        }
    }

    pub(crate) fn into_parts(self) -> (Result<Error, PhpError>, Option<Trace>) {
        let inner = *self.inner;
        let kind = match inner.kind {
            Kind::Twig(error) => Ok(error),
            Kind::Php(error) => Err(error),
        };
        (kind, inner.trace)
    }

    pub(crate) fn trace(&self) -> Option<&Trace> {
        self.inner.trace.as_ref()
    }

    pub(crate) fn has_trace(&self) -> bool {
        self.inner.trace.is_some()
    }

    pub(crate) fn set_trace(&mut self, trace: Trace) {
        self.inner.trace = Some(trace);
    }

    pub(crate) fn with_trace(mut self, trace: Option<Trace>) -> Self {
        self.inner.trace = trace;
        self
    }

    /// The exception as a [`PhpError`], to pass it through `twig-value` code (an arrow function called by a sort
    /// routine, a host object calling a template closure). The original exception rides along as the error's
    /// payload and comes back unchanged through [`Exception::from`].
    pub fn into_php_error(self) -> PhpError {
        let (class, message) = match &self.inner.kind {
            Kind::Php(error) => (error.class().clone(), error.message().clone()),
            Kind::Twig(error) => (
                ThrowableClass::other(error.kind().class_name(), ThrowableClass::Exception),
                twig_value::PhpString::from(error.message()),
            ),
        };
        PhpError::new(class, message).with_payload(Arc::new(self))
    }
}

impl From<Error> for Exception {
    /// A Twig error; an error standing for a PHP throwable ([`Error::php`]) gives the throwable back, so templates
    /// wrap it like any other non-Twig exception.
    fn from(error: Error) -> Self {
        if error.kind() == crate::ErrorKind::Php
            && let Some(php) = error.php_error()
        {
            return Exception::from(php.clone());
        }
        Exception {
            inner: Box::new(Inner {
                kind: Kind::Twig(error),
                trace: None,
            }),
        }
    }
}

impl From<PhpError> for Exception {
    /// A PHP throwable; an error made by [`Exception::into_php_error`] gives the original exception back.
    fn from(error: PhpError) -> Self {
        if let Some(payload) = error.payload()
            && let Some(exception) = payload.downcast_ref::<Exception>()
        {
            return exception.clone();
        }
        Exception {
            inner: Box::new(Inner {
                kind: Kind::Php(error),
                trace: None,
            }),
        }
    }
}

impl fmt::Debug for Exception {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner.kind {
            Kind::Twig(error) => f.debug_tuple("Exception").field(error).finish(),
            Kind::Php(error) => f.debug_tuple("Exception").field(error).finish(),
        }
    }
}

impl fmt::Display for Exception {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner.kind {
            Kind::Twig(error) => write!(f, "{}: {}", error.kind().class_name(), error),
            Kind::Php(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Exception {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_php_errors() {
        let original: Exception = Error::runtime("Variable \"x\" does not exist.")
            .with_line(3)
            .into();
        let php = original.clone().into_php_error();
        assert_eq!(php.class().name(), "Twig\\Error\\RuntimeError");
        assert!(!php.is_a(&ThrowableClass::BadMethodCallException));
        let back = Exception::from(php);
        assert_eq!(
            back.twig_error().map(Error::template_line),
            Some(3),
            "the original comes back"
        );
        let plain: Exception = PhpError::type_error("boom").into();
        assert!(!plain.is_twig_error());
        assert_eq!(plain.message(), "boom");
        assert!(!Exception::from(Error::logic("logic")).is_twig_error());
    }
}
