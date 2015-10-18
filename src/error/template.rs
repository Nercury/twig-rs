use std::result;
use std::fmt;
use tokens::TokenValue;

#[derive(Debug, Clone)]
pub enum Received {
    Token(TokenValue),
    EndOfStream,
}

pub trait CustomErrorAt: fmt::Display {
    fn boxed_clone(&self) -> Box<CustomErrorAt>;
}

impl Clone for Box<CustomErrorAt> {
    fn clone(&self) -> Box<CustomErrorAt> {
        self.boxed_clone()
    }
}

#[derive(Clone)]
pub enum TemplateError {
    UnexpectedEndOfTemplate,
    ExpectedTokenTypeButReceived((TokenValue, Received)),
    UnexpectedTokenValue(TokenValue),
    ExpectedOtherTokenValue((TokenValue, TokenValue)),
    ExpectedArrayElement,
    ArrayValueMustBeFollowedByComma,
    ArrayNotClosed,
    ExpectedHashElement,
    HashValueMustBeFollowedByComma,
    InvalidHashKey { unexpected: TokenValue },
    HashKeyMustBeFollowedByColon,
    HashNotClosed,
    ExpectedNameOrNumber,
    ListOfArgumentsMustBeginWithParenthesis,
    ArgumentsMustBeSeparatedByComma,
    ListOfArgumentsMustCloseWithParenthesis,
    Unclosed(String),
    UnclosedComment,
    UnclosedBlock(String),
    Unexpected(String),
    UnexpectedCharacter(String),
    ParenthesisNotClosed,
    MustStartWithTagName,
    DefaultValueForArgumentMustBeConstant,
    ParameterNameMustBeAString { given: String },
    TemplateNotFound(String),
    CustomError(Box<CustomErrorAt>),
}

impl TemplateError {
    pub fn at(self, line: usize) -> ErrorAt {
        ErrorAt::new(self, line)
    }
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TemplateError::UnexpectedEndOfTemplate => write!(f, "Unexpected end of template"),
            TemplateError::ExpectedTokenTypeButReceived((ref token, ref received)) => {
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
            TemplateError::ExpectedOtherTokenValue((ref token, ref other)) => {
                let unexpected_message = format!("{}", TemplateError::UnexpectedTokenValue(token.clone()));
                let (other_english_name, other_value) = other.get_english();
                match other_value {
                    Some(value) => write!(f, "{} (\"{}\" expected with value {:?})", unexpected_message, other_english_name, value),
                    None => write!(f, "{} (\"{}\" expected)", unexpected_message, other_english_name),
                }
            },
            TemplateError::ExpectedArrayElement => write!(f, "An array element was expected"),
            TemplateError::ArrayValueMustBeFollowedByComma => write!(f, "An array element must be followed by a comma"),
            TemplateError::ArrayNotClosed => write!(f, "An opened array is not properly closed"),
            TemplateError::ExpectedHashElement => write!(f, "A hash element was expected"),
            TemplateError::HashValueMustBeFollowedByComma => write!(f, "A hash value must be followed by a comma"),
            TemplateError::InvalidHashKey { ref unexpected } => {
                let (english_name, value) = unexpected.get_english();
                match value {
                    Some(value) => write!(f, "A hash key must be a quoted string, a number, a name, or an expression enclosed in parentheses: unexpected token \"{}\" of value {:?}", english_name, value),
                    None => write!(f, "A hash key must be a quoted string, a number, a name, or an expression enclosed in parentheses: unexpected token \"{}\"", english_name),
                }
            },
            TemplateError::HashKeyMustBeFollowedByColon => write!(f, "A hash key must be followed by a colon (:)"),
            TemplateError::HashNotClosed => write!(f, "An opened hash is not properly closed"),
            TemplateError::ExpectedNameOrNumber => write!(f, "Expected name or number"),
            TemplateError::ListOfArgumentsMustBeginWithParenthesis => write!(f, "A list of arguments must begin with an opening parenthesis"),
            TemplateError::ArgumentsMustBeSeparatedByComma => write!(f, "Arguments must be separated by a comma"),
            TemplateError::ListOfArgumentsMustCloseWithParenthesis => write!(f, "A list of arguments must be closed by a parenthesis"),
            TemplateError::UnexpectedTokenValue(ref token) => {
                let (english_name, value) = token.get_english();
                match value {
                    Some(value) => write!(f, "Unexpected token \"{}\" of value {:?}", english_name, value),
                    None => write!(f, "Unexpected token \"{}\"", english_name),
                }
            },
            TemplateError::Unclosed(ref s) => write!(f, "Unclosed \"{}\"", s),
            TemplateError::UnclosedComment => write!(f, "Unclosed comment"),
            TemplateError::UnclosedBlock(ref s) => write!(f, "Unexpected end of file: Unclosed \"{}\" block", s),
            TemplateError::Unexpected(ref s) => write!(f, "Unexpected \"{}\"", s),
            TemplateError::UnexpectedCharacter(ref s) => write!(f, "Unexpected character \"{}\"", s),
            TemplateError::ParenthesisNotClosed => write!(f, "An opened parenthesis is not properly closed"),
            TemplateError::MustStartWithTagName => write!(f, "A block must start with a tag name"),
            TemplateError::DefaultValueForArgumentMustBeConstant => write!(f, "A default value for an argument must be a constant (a boolean, a string, a number, or an array)."),
            TemplateError::ParameterNameMustBeAString { ref given } => write!(f, "A parameter name must be a string, \"{}\" given", given),
            TemplateError::TemplateNotFound(ref name) => write!(f, "Template \"{}\" was not found", name),
            TemplateError::CustomError(ref e) => write!(f, "{}", e),
        }
    }
}

#[derive(Clone)]
pub struct ErrorAt {
    line: Option<usize>,
    message: Box<TemplateError>,
}

impl fmt::Debug for ErrorAt {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.get_message())
    }
}

impl ErrorAt {
    pub fn new(message: TemplateError, line: usize) -> ErrorAt {
        ErrorAt {
            message: Box::new(message),
            line: Some(line),
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

        match self.line {
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

pub type Result<T> = result::Result<T, ErrorAt>;