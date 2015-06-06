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

    pub fn get_message(&self) -> String {

        let ends_with_dot = {
            let len = self.raw_message.len();
            if len > 0 {
                if &self.raw_message[len - 1 ..] == "." {
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if ends_with_dot {
            let len = self.raw_message.len();
            let without_dot = &self.raw_message[0 .. len - 1];

            format!("{} at line {}.", without_dot, self.line_num)
        } else {
            format!("{} at line {}", self.raw_message, self.line_num)
        }
    }
}

pub type Result<T> = result::Result<T, Error>;
