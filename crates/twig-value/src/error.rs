//! PHP exceptions (`Throwable`s) as Rust errors.
//!
//! Every operation that throws in PHP returns `Err(PhpError)` carrying the PHP class and the
//! verbatim message (reference/conventions.md, "Errors and diagnostics"). Non-fatal PHP
//! diagnostics (warnings, notices, deprecations) are not errors; they go through
//! [`Diagnostics`](crate::Diagnostics).

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use crate::string::PhpString;

/// The class of a PHP `Throwable`.
///
/// Built-in classes are listed explicitly so that `instanceof` checks against the core hierarchy
/// (for example Twig catching `\BadMethodCallException` in `CoreExtension::getAttribute()`) work
/// without a class table. [`ThrowableClass::Other`] covers every other class name (Twig's own
/// errors, user exceptions, PHPUnit's converted warnings, ...).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ThrowableClass {
    /// `Error`
    Error,
    /// `CompileError extends Error`
    CompileError,
    /// `ParseError extends CompileError`
    ParseError,
    /// `TypeError extends Error`
    TypeError,
    /// `ArgumentCountError extends TypeError`
    ArgumentCountError,
    /// `ValueError extends Error`
    ValueError,
    /// `ArithmeticError extends Error`
    ArithmeticError,
    /// `DivisionByZeroError extends ArithmeticError`
    DivisionByZeroError,
    /// `UnhandledMatchError extends Error`
    UnhandledMatchError,
    /// `Exception`
    Exception,
    /// `ErrorException extends Exception`
    ErrorException,
    /// `JsonException extends Exception`
    JsonException,
    /// `LogicException extends Exception`
    LogicException,
    /// `BadFunctionCallException extends LogicException`
    BadFunctionCallException,
    /// `BadMethodCallException extends BadFunctionCallException`
    BadMethodCallException,
    /// `DomainException extends LogicException`
    DomainException,
    /// `InvalidArgumentException extends LogicException`
    InvalidArgumentException,
    /// `LengthException extends LogicException`
    LengthException,
    /// `OutOfRangeException extends LogicException`
    OutOfRangeException,
    /// `RuntimeException extends Exception`
    RuntimeException,
    /// `OutOfBoundsException extends RuntimeException`
    OutOfBoundsException,
    /// `OverflowException extends RuntimeException`
    OverflowException,
    /// `RangeException extends RuntimeException`
    RangeException,
    /// `UnderflowException extends RuntimeException`
    UnderflowException,
    /// `UnexpectedValueException extends RuntimeException`
    UnexpectedValueException,
    /// Any other class: its fully qualified name (without leading backslash) and its parent class
    /// (`None` means the class directly implements `Throwable`, which userland cannot do; use
    /// `Some(Exception)` for ordinary user exceptions).
    Other {
        /// Fully qualified class name, e.g. `Twig\Error\RuntimeError`.
        name: Arc<str>,
        /// Parent class, used by [`ThrowableClass::is_a`].
        parent: Option<Arc<ThrowableClass>>,
    },
}

impl ThrowableClass {
    /// A class outside the built-in list, with its parent.
    pub fn other(name: impl Into<Arc<str>>, parent: ThrowableClass) -> Self {
        ThrowableClass::Other {
            name: name.into(),
            parent: Some(Arc::new(parent)),
        }
    }

    /// The fully qualified class name (as `get_class()` returns it).
    pub fn name(&self) -> &str {
        use ThrowableClass::*;
        match self {
            Error => "Error",
            CompileError => "CompileError",
            ParseError => "ParseError",
            TypeError => "TypeError",
            ArgumentCountError => "ArgumentCountError",
            ValueError => "ValueError",
            ArithmeticError => "ArithmeticError",
            DivisionByZeroError => "DivisionByZeroError",
            UnhandledMatchError => "UnhandledMatchError",
            Exception => "Exception",
            ErrorException => "ErrorException",
            JsonException => "JsonException",
            LogicException => "LogicException",
            BadFunctionCallException => "BadFunctionCallException",
            BadMethodCallException => "BadMethodCallException",
            DomainException => "DomainException",
            InvalidArgumentException => "InvalidArgumentException",
            LengthException => "LengthException",
            OutOfRangeException => "OutOfRangeException",
            RuntimeException => "RuntimeException",
            OutOfBoundsException => "OutOfBoundsException",
            OverflowException => "OverflowException",
            RangeException => "RangeException",
            UnderflowException => "UnderflowException",
            UnexpectedValueException => "UnexpectedValueException",
            Other { name, .. } => name,
        }
    }

    /// The parent class, or `None` for the roots `Error` and `Exception`.
    pub fn parent(&self) -> Option<ThrowableClass> {
        use ThrowableClass::*;
        Some(match self {
            Error | Exception => return None,
            CompileError | TypeError | ValueError | ArithmeticError | UnhandledMatchError => Error,
            ParseError => CompileError,
            ArgumentCountError => TypeError,
            DivisionByZeroError => ArithmeticError,
            ErrorException | JsonException | LogicException | RuntimeException => Exception,
            BadFunctionCallException
            | DomainException
            | InvalidArgumentException
            | LengthException
            | OutOfRangeException => LogicException,
            BadMethodCallException => BadFunctionCallException,
            OutOfBoundsException
            | OverflowException
            | RangeException
            | UnderflowException
            | UnexpectedValueException => RuntimeException,
            Other { parent, .. } => return parent.as_deref().cloned(),
        })
    }

    /// `instanceof`: whether this class is `other` or one of its subclasses. Class names compare
    /// case-insensitively, as PHP class names do.
    pub fn is_a(&self, other: &ThrowableClass) -> bool {
        let mut cur = Some(self.clone());
        while let Some(c) = cur {
            if c.name().eq_ignore_ascii_case(other.name()) {
                return true;
            }
            cur = c.parent();
        }
        false
    }
}

impl fmt::Display for ThrowableClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A thrown PHP exception: class + verbatim message (+ optional previous exception and an opaque
/// payload that lets embedding code carry its own error through value-level calls).
#[derive(Clone)]
pub struct PhpError {
    class: ThrowableClass,
    message: PhpString,
    previous: Option<Box<PhpError>>,
    payload: Option<Arc<dyn Any + Send + Sync>>,
}

impl PhpError {
    /// An exception of `class` with `message`.
    pub fn new(class: ThrowableClass, message: impl Into<PhpString>) -> Self {
        PhpError {
            class,
            message: message.into(),
            previous: None,
            payload: None,
        }
    }

    /// `Error` (`zend_throw_error(NULL, ...)`).
    pub fn error(message: impl Into<PhpString>) -> Self {
        Self::new(ThrowableClass::Error, message)
    }

    /// `TypeError` (`zend_type_error`).
    pub fn type_error(message: impl Into<PhpString>) -> Self {
        Self::new(ThrowableClass::TypeError, message)
    }

    /// `ValueError` (`zend_value_error`).
    pub fn value_error(message: impl Into<PhpString>) -> Self {
        Self::new(ThrowableClass::ValueError, message)
    }

    /// `ArgumentCountError` (`zend_argument_count_error`).
    pub fn argument_count_error(message: impl Into<PhpString>) -> Self {
        Self::new(ThrowableClass::ArgumentCountError, message)
    }

    /// `ArithmeticError`.
    pub fn arithmetic_error(message: impl Into<PhpString>) -> Self {
        Self::new(ThrowableClass::ArithmeticError, message)
    }

    /// `DivisionByZeroError`.
    pub fn division_by_zero(message: impl Into<PhpString>) -> Self {
        Self::new(ThrowableClass::DivisionByZeroError, message)
    }

    /// `Exception` (`zend_throw_exception(NULL, ...)`).
    pub fn exception(message: impl Into<PhpString>) -> Self {
        Self::new(ThrowableClass::Exception, message)
    }

    /// Attaches the previous exception (`$previous`).
    pub fn with_previous(mut self, previous: PhpError) -> Self {
        self.previous = Some(Box::new(previous));
        self
    }

    /// Attaches an opaque payload (for example the embedding engine's own error type).
    pub fn with_payload(mut self, payload: Arc<dyn Any + Send + Sync>) -> Self {
        self.payload = Some(payload);
        self
    }

    /// The exception class.
    pub fn class(&self) -> &ThrowableClass {
        &self.class
    }

    /// The verbatim message (`getMessage()`), as bytes.
    pub fn message(&self) -> &PhpString {
        &self.message
    }

    /// The message as UTF-8 (lossy).
    pub fn message_lossy(&self) -> std::borrow::Cow<'_, str> {
        self.message.to_string_lossy()
    }

    /// The previous exception, if any.
    pub fn previous(&self) -> Option<&PhpError> {
        self.previous.as_deref()
    }

    /// The opaque payload, if any.
    pub fn payload(&self) -> Option<&Arc<dyn Any + Send + Sync>> {
        self.payload.as_ref()
    }

    /// `$e instanceof $class`.
    pub fn is_a(&self, class: &ThrowableClass) -> bool {
        self.class.is_a(class)
    }
}

impl fmt::Debug for PhpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("PhpError");
        d.field("class", &self.class.name())
            .field("message", &self.message);
        if let Some(prev) = &self.previous {
            d.field("previous", prev);
        }
        if self.payload.is_some() {
            d.field("payload", &"..");
        }
        d.finish()
    }
}

impl fmt::Display for PhpError {
    /// `Class: message`, the way PHP's uncaught-exception output starts.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.class.name(), self.message)
    }
}

impl std::error::Error for PhpError {}

/// Shorthand for results of operations that may throw.
pub type PhpResult<T> = Result<T, PhpError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy() {
        use ThrowableClass::*;
        assert!(BadMethodCallException.is_a(&LogicException));
        assert!(BadMethodCallException.is_a(&Exception));
        assert!(!BadMethodCallException.is_a(&Error));
        assert!(ArgumentCountError.is_a(&TypeError));
        assert!(DivisionByZeroError.is_a(&Error));
        let custom = ThrowableClass::other("App\\MyException", BadMethodCallException);
        assert!(custom.is_a(&BadMethodCallException));
        assert!(custom.is_a(&ThrowableClass::other("app\\myexception", Exception)));
        assert!(!custom.is_a(&RuntimeException));
        let e = PhpError::type_error("Unsupported operand types: string + int");
        assert_eq!(
            e.to_string(),
            "TypeError: Unsupported operand types: string + int"
        );
    }
}
