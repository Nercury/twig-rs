use std::convert::From;
use std::fmt;
use std::result;
use error::{ CustomErrorAt, ErrorAt, TemplateError };

pub struct CoreErrorAt {
    line: usize,
    message: CoreTemplateError,
}

impl CoreErrorAt {
    pub fn new_at(message: CoreTemplateError, line: usize) -> CoreErrorAt {
        CoreErrorAt {
            line: line,
            message: message,
        }
    }
}

#[derive(Clone)]
pub enum CoreTemplateError {
    OnlyVariablesCanBeAssignedTo,
    ExpectedEndmacroName { expected: String, given: String },
    CanNotAssignTo(String),
}

impl fmt::Display for CoreTemplateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CoreTemplateError::OnlyVariablesCanBeAssignedTo => write!(f, "Only variables can be assigned to"),
            CoreTemplateError::ExpectedEndmacroName { ref expected, ref given } => write!(f, "Expected endmacro for macro \"{}\" (but \"{}\" given)", expected, given),
            CoreTemplateError::CanNotAssignTo(ref v) => write!(f, "You cannot assign a value to \"{}\"", v),
        }
    }
}

impl CustomErrorAt for CoreTemplateError {
    fn boxed_clone(&self) -> Box<CustomErrorAt> {
        Box::new(self.clone())
    }
}

impl From<CoreErrorAt> for ErrorAt {
    fn from(e: CoreErrorAt) -> ErrorAt {
        ErrorAt::new_at(
            TemplateError::CustomError(Box::new(e.message)),
            e.line
        )
    }
}

pub type CoreResult<T> = result::Result<T, CoreErrorAt>;
