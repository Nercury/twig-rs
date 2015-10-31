use std::fmt;
use std::path::PathBuf;
use error::{ Error, Caused };

#[derive(Clone, Debug)]
pub enum EngineError {
    TemplateNotFound { name: String, search_paths: Vec<PathBuf> },
}

impl EngineError {
    pub fn caused_by<I: Into<Error>>(self, cause: I) -> Caused<EngineError> {
        Caused::new(self, Some(cause.into()))
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            EngineError::TemplateNotFound { ref name, ref search_paths } => {
                if search_paths.len() == 0 {
                    write!(f, "Template \"{}\" was not found", name)
                } else {
                    try!(write!(f, "Template \"{}\" was not found, looked in ", name));
                    write!(f, "{:?}", search_paths)
                }
            }
        }
    }
}

impl From<EngineError> for Error {
    fn from(inner: EngineError) -> Error {
        Error::Engine(Caused::new(inner, None))
    }
}

impl From<Caused<EngineError>> for Error {
    fn from(inner: Caused<EngineError>) -> Error {
        Error::Engine(inner)
    }
}
