use std::result;

#[derive(Debug)]
pub struct Error {
    line_num: usize,
    raw_message: String,
    previous: Option<Box<Error>>,
}

impl Error {
    pub fn new<M: Into<String>>(message: M, line_num: usize) -> Error {
        Error {
            raw_message: message.into(),
            line_num: line_num,
            previous: None,
        }
    }

    pub fn get_message(&self) -> &str {
        &self.raw_message
    }
}

pub type Result<T> = result::Result<T, Error>;
