use std::convert::From;
use std::fmt;
use std::result;
use error::{ CustomError, Error, ErrorMessage };

pub struct CoreError {
    line: usize,
    message: CoreErrorMessage,
}

impl CoreError {
    pub fn new_at(message: CoreErrorMessage, line: usize) -> CoreError {
        CoreError {
            line: line,
            message: message,
        }
    }
}

#[derive(Clone)]
pub enum CoreErrorMessage {
    OnlyVariablesCanBeAssignedTo,
    CanNotAssignTo(String),
}

impl fmt::Display for CoreErrorMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CoreErrorMessage::OnlyVariablesCanBeAssignedTo => write!(f, "Only variables can be assigned to"),
            CoreErrorMessage::CanNotAssignTo(ref v) => write!(f, "You cannot assign a value to \"{}\"", v),
        }
    }
}

impl CustomError for CoreErrorMessage {
    fn boxed_clone(&self) -> Box<CustomError> {
        Box::new(self.clone())
    }
}

impl From<CoreError> for Error {
    fn from(e: CoreError) -> Error {
        Error::new_at(
            ErrorMessage::CustomError(Box::new(e.message)),
            e.line
        )
    }
}

pub type CoreResult<T> = result::Result<T, CoreError>;
