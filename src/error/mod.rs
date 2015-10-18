mod template;
mod runtime;

use std::fmt;
use std::result;

pub use self::template::{ TemplateError, Received };
pub use self::runtime::{ RuntimeError, TracedRuntimeError };

#[derive(Clone)]
pub enum Error {
    /// Error lexing or parsing the template source file.
    Template(At<TemplateError>),
    /// Error reading files, compiling templates or writing cache.
    Engine,
    /// Error executing template.
    Runtime(TracedRuntimeError),
}

pub trait ExtensionError: fmt::Display {
    fn boxed_clone(&self) -> Box<ExtensionError>;
}

impl Clone for Box<ExtensionError> {
    fn clone(&self) -> Box<ExtensionError> {
        self.boxed_clone()
    }
}

impl fmt::Debug for Box<ExtensionError> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (&**self).fmt(f)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Template(ref e) => write!(f, "{:?}", e),
            Error::Engine => write!(f, "Engine Error"),
            Error::Runtime(ref e) => write!(f, "{:?}", e),
        }
    }
}

/// Pins any error type to source file location.
#[derive(Copy, Clone)]
pub struct At<E: fmt::Display + fmt::Debug> {
    pub loc: Location,
    pub err: E,
}

impl<E: fmt::Display + fmt::Debug> At<E> {
    pub fn new(err: E, loc: Location) -> At<E> {
        At {
            loc: loc,
            err: err,
        }
    }
}

impl<E: fmt::Display + fmt::Debug> fmt::Debug for At<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let message = {
            let raw_message = format!("{}", self.err);
            let ends_with_dot = {
                let len = raw_message.len();
                if len > 0 {
                    if &raw_message[len - 1 ..] == "." {
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if ends_with_dot {
                let len = raw_message.len();
                let without_dot = &raw_message[0 .. len - 1];

                format!("{} at line {}.", without_dot, self.loc.line)
            } else {
                format!("{} at line {}", raw_message, self.loc.line)
            }
        };

        write!(f, "{}", message)
    }
}

/// Location record in source file.
#[derive(Debug, Copy, Clone)]
pub struct Location {
    pub line: usize,
}

impl Location {
    pub fn new(line: usize) -> Location {
        Location { line: line }
    }
}

pub type Result<T> = result::Result<T, Error>;
pub type TemplateResult<T> = result::Result<T, At<TemplateError>>;
pub type RuntimeResult<T> = result::Result<T, RuntimeError>;
pub type TracedRuntimeResult<T> = result::Result<T, RuntimeError>;
