use std::result;

#[derive(Debug)]
pub struct Error {
    line_num: usize,
    file_name: Option<String>,
    raw_message: String,
    previous: Option<Box<Error>>,
}

pub type Result<T> = result::Result<T, Error>;
