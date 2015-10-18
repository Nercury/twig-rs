use std::convert::From;
use std::fmt;
use std::result;
use error::{ CustomErrorAt, ErrorAt, ErrorMessage };

pub struct CoreErrorAt {
    line: usize,
    message: CoreErrorMessage,
}

impl CoreErrorAt {
    pub fn new_at(message: CoreErrorMessage, line: usize) -> CoreErrorAt {
        CoreErrorAt {
            line: line,
            message: message,
        }
    }
}

#[derive(Clone)]
pub enum CoreErrorMessage {
    OnlyVariablesCanBeAssignedTo,
    ExpectedEndmacroName { expected: String, given: String },
    CanNotAssignTo(String),
}

impl fmt::Display for CoreErrorMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CoreErrorMessage::OnlyVariablesCanBeAssignedTo => write!(f, "Only variables can be assigned to"),
            CoreErrorMessage::ExpectedEndmacroName { ref expected, ref given } => write!(f, "Expected endmacro for macro \"{}\" (but \"{}\" given)", expected, given),
            CoreErrorMessage::CanNotAssignTo(ref v) => write!(f, "You cannot assign a value to \"{}\"", v),
        }
    }
}

impl CustomErrorAt for CoreErrorMessage {
    fn boxed_clone(&self) -> Box<CustomErrorAt> {
        Box::new(self.clone())
    }
}

impl From<CoreErrorAt> for ErrorAt {
    fn from(e: CoreErrorAt) -> ErrorAt {
        ErrorAt::new_at(
            ErrorMessage::CustomError(Box::new(e.message)),
            e.line
        )
    }
}

pub type CoreResult<T> = result::Result<T, CoreErrorAt>;
