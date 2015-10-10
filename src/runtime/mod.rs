use std::result;
use std::fmt;

/// Stack trace record.
pub enum TraceEntry {
    /// Trace source file change that caused the error.
    SourceFile { target: String },
    /// Trace operator call that caused the error.
    Operator { target: String, extension: String },
    /// Trace position in last known source that cause the error.
    Position { from: Location },
}

/// Location record in source file.
pub struct Location {
    pub line: usize,
}

impl Location {
    pub fn new(line: usize) -> Location {
        Location { line: line }
    }
}

#[derive(Debug)]
pub enum ErrorMessage {
    InvalidArgumentCount { expected: usize, found: usize },
}

pub struct Error {
    pub line: Option<usize>,
    pub message: ErrorMessage,
    pub stack_trace: Vec<TraceEntry>,
}

impl Error {
    pub fn new(message: ErrorMessage) -> Error {
        Error {
            line: None,
            message: message,
            stack_trace: Vec::new(),
        }
    }

    pub fn new_at(message: ErrorMessage, line: usize) -> Error {
        Error {
            line: Some(line),
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

pub type Result<T> = result::Result<T, Error>;
