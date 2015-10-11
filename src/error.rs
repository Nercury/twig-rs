use std::result;
use std::fmt;
use token::DebugValue;

#[derive(Debug, Clone)]
pub enum Received {
    Token(DebugValue),
    EndOfStream,
}

#[derive(Debug, Clone)]
pub enum ErrorMessage {
    UnexpectedEndOfTemplate,
    ExpectedTokenTypeButReceived((DebugValue, Received)),
    UnexpectedTokenValue(DebugValue),
    ExpectedOtherTokenValue((DebugValue, DebugValue)),
    Unclosed(String),
    UnclosedComment,
    UnclosedBlock(String),
    Unexpected(String),
    UnexpectedCharacter(String),
    ParenthesisNotClosed,
}

impl fmt::Display for ErrorMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ErrorMessage::UnexpectedEndOfTemplate => write!(f, "Unexpected end of template"),
            ErrorMessage::ExpectedTokenTypeButReceived((ref token, ref received)) => {
                let (english_name, _) = token.get_english();
                match *received {
                    Received::EndOfStream => write!(f, "Expected token \"{}\" but received the end of stream", english_name),
                    Received::Token(ref other) => {
                        let (other_english_name, value) = other.get_english();
                        match value {
                            Some(value) => write!(f, "Expected \"{}\" but received \"{}\" with value {:?}", english_name, other_english_name, value),
                            None => write!(f, "Expected \"{}\" but received \"{}\"", english_name, other_english_name),
                        }
                    },
                }
            },
            ErrorMessage::ExpectedOtherTokenValue((ref token, ref other)) => {
                let unexpected_message = format!("{}", ErrorMessage::UnexpectedTokenValue(token.clone()));
                let (other_english_name, other_value) = other.get_english();
                match other_value {
                    Some(value) => write!(f, "{} (\"{}\" expected with value {:?})", unexpected_message, other_english_name, value),
                    None => write!(f, "{} (\"{}\" expected)", unexpected_message, other_english_name),
                }
            },
            ErrorMessage::UnexpectedTokenValue(ref token) => {
                let (english_name, value) = token.get_english();
                match value {
                    Some(value) => write!(f, "Unexpected token \"{}\" of value {:?}", english_name, value),
                    None => write!(f, "Unexpected token \"{}\"", english_name),
                }
            },
            ErrorMessage::Unclosed(ref s) => write!(f, "Unclosed \"{}\"", s),
            ErrorMessage::UnclosedComment => write!(f, "Unclosed comment"),
            ErrorMessage::UnclosedBlock(ref s) => write!(f, "Unexpected end of file: Unclosed \"{}\" block", s),
            ErrorMessage::Unexpected(ref s) => write!(f, "Unexpected \"{}\"", s),
            ErrorMessage::UnexpectedCharacter(ref s) => write!(f, "Unexpected character \"{}\"", s),
            ErrorMessage::ParenthesisNotClosed => write!(f, "An opened parenthesis is not properly closed"),
        }
    }
}

#[derive(Clone)]
pub struct Error {
    line_num: Option<usize>,
    message: Box<ErrorMessage>,
    _previous: Option<Box<Error>>,
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.get_message())
    }
}

impl Error {
    pub fn new_at(message: ErrorMessage, line_num: usize) -> Error {
        Error {
            message: Box::new(message),
            line_num: Some(line_num),
            _previous: None,
        }
    }

    pub fn new(message: ErrorMessage) -> Error {
        Error {
            message: Box::new(message),
            line_num: None,
            _previous: None,
        }
    }

    fn get_message(&self) -> String {
        let raw_message = format!("{}", self.message);
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

        match self.line_num {
            Some(line_num) => {
                if ends_with_dot {
                    let len = raw_message.len();
                    let without_dot = &raw_message[0 .. len - 1];

                    format!("{} at line {}.", without_dot, line_num)
                } else {
                    format!("{} at line {}", raw_message, line_num)
                }
            },
            None => {
                raw_message.to_string()
            }
        }
    }
}

pub type Result<T> = result::Result<T, Error>;
