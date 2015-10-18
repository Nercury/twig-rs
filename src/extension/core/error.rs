use std::convert::From;
use std::fmt;
use error::{ ExtensionError, Location, At, TemplateError };

#[derive(Debug, Clone)]
pub enum CoreTemplateError {
    OnlyVariablesCanBeAssignedTo,
    ExpectedEndmacroName { expected: String, given: String },
    CanNotAssignTo(String),
}

impl CoreTemplateError {
    pub fn at(self, line: usize) -> At<CoreTemplateError> {
        At::new(self, Location::new(line))
    }
}

impl ExtensionError for CoreTemplateError {
    fn boxed_clone(&self) -> Box<ExtensionError> {
        Box::new(self.clone())
    }
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

impl From<At<CoreTemplateError>> for At<TemplateError> {
    fn from(At { loc: Location { line }, err }: At<CoreTemplateError>) -> At<TemplateError> {
        TemplateError::CustomError(Box::new(err))
            .at(line)
    }
}
