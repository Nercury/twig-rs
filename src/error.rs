use std::result;
use std::fmt;
use token::DebugValue;
use value::{ TwigValue, TwigNumber };

#[derive(Debug, Clone)]
pub enum Received {
    Token(DebugValue),
    EndOfStream,
}

#[derive(Debug, Clone)]
pub enum ErrorMessage {
    UnexpectedEndOfTemplate,
    ExpectedTokenButReceived((DebugValue, Received)),
    UnexpectedToken(DebugValue),
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
            ErrorMessage::ExpectedTokenButReceived((ref token, ref received)) => match *received {
                Received::EndOfStream => write!(f, "Expected token {:?} but received the end of stream", token),
                Received::Token(ref other) => write!(f, "Expected token {:?} but received {:?}", token, other),
            },
            ErrorMessage::UnexpectedToken(ref token) => {
                let (english_name, value) = match *token {
                    DebugValue::Text(ref v) => ("text", Some(v.to_string())),
                    DebugValue::BlockStart => ("begin of statement block", None),
                    DebugValue::VarStart => ("begin of print statement", None),
                    DebugValue::BlockEnd => ("end of statement block", None),
                    DebugValue::VarEnd => ("end of print statement", None),
                    DebugValue::Name(ref n) => ("name", Some(n.to_string())),
                    DebugValue::Value(TwigValue::Num(ref n)) => ("number", Some(n.to_string())),
                    DebugValue::Value(TwigValue::Str(ref s)) => ("string", Some(s.to_string())),
                    DebugValue::Operator(ref s) => ("operator", Some(s.to_string())),
                    DebugValue::Punctuation(s) => ("punctuation", Some(s.to_string())),
                    DebugValue::InterpolationStart => ("begin of string interpolation", None),
                    DebugValue::InterpolationEnd => ("end of string interpolation", None),
                    DebugValue::CommentStart => ("comment start", None),
                };
                match value {
                    Some(value) => write!(f, "Unexpected token \"{}\" of value \"{:?}\"", english_name, value),
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
