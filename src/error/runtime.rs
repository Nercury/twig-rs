use std::fmt;
use error::{ Location, Error };

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
pub enum RuntimeError {
    /// Callable invoked with argument count that does not match defined count.
    InvalidArgumentCount { expected: usize, found: usize },
}

impl RuntimeError {
    pub fn at(self, stack_trace: Vec<TraceEntry>) -> TracedRuntimeError {
        TracedRuntimeError {
            message: self,
            stack_trace: stack_trace
        }
    }
}

/// Runtime error with stack trace.
#[derive(Clone)]
pub struct TracedRuntimeError {
    pub message: RuntimeError,
    pub stack_trace: Vec<TraceEntry>,
}

impl TracedRuntimeError {
    pub fn new(message: RuntimeError) -> TracedRuntimeError {
        TracedRuntimeError {
            message: message,
            stack_trace: Vec::new(),
        }
    }
}

impl fmt::Debug for TracedRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.message)
    }
}

impl From<TracedRuntimeError> for Error {
    fn from(inner: TracedRuntimeError) -> Error {
        Error::Runtime(inner)
    }
}
