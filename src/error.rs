use std::result;

#[derive(Debug)]
pub struct Error {
    line_num: Option<usize>,
    raw_message: String,
    previous: Option<Box<Error>>,
}

impl Error {
    pub fn new_at<M: Into<String>>(message: M, line_num: usize) -> Error {
        Error {
            raw_message: message.into(),
            line_num: Some(line_num),
            previous: None,
        }
    }

    pub fn new<M: Into<String>>(message: M) -> Error {
        Error {
            raw_message: message.into(),
            line_num: None,
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

        match self.line_num {
            Some(line_num) => {
                if ends_with_dot {
                    let len = self.raw_message.len();
                    let without_dot = &self.raw_message[0 .. len - 1];

                    format!("{} at line {}.", without_dot, line_num)
                } else {
                    format!("{} at line {}", self.raw_message, line_num)
                }
            },
            None => {
                self.raw_message.to_string()
            }
        }
    }
}

pub type Result<T> = result::Result<T, Error>;
