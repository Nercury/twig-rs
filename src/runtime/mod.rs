use std::result;
use std::fmt;
use error::Location;

/// Stack trace record.
#[derive(Clone)]
pub enum TraceEntry {
    /// Trace source file change that caused the error.
    SourceFile { target: String },
    /// Trace operator call that caused the error.
    Operator { target: String, extension: String },
    /// Trace position in last known source that cause the error.
    Position { from: Location },
}

#[derive(Debug, Copy, Clone)]
/// Runtime error message.
pub enum ErrorMessage {
    /// Callable invoked with argument count that does not match defined count.
    InvalidArgumentCount { expected: usize, found: usize },
}

/// Runtime error with stack trace.
#[derive(Clone)]
pub struct Error {
    pub message: ErrorMessage,
    pub stack_trace: Vec<TraceEntry>,
}

impl Error {
    pub fn new(message: ErrorMessage) -> Error {
        Error {
            message: message,
            stack_trace: Vec::new(),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.message)
    }
}

/// Runtime operation result.
pub type Result<T> = result::Result<T, Error>;
