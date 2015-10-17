use value::{ TwigValueRef, TwigValue };

/// Lexer output token, lexer's output and parser's input.
#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub value: Value<'a>,
    pub line: usize,
}

/// Token value.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Value<'a> {
    Text(&'a str),
    BlockStart,
    VarStart,
    BlockEnd,
    VarEnd,
    Name(&'a str),
    Value(TwigValueRef<'a>),
    Operator(&'a str),
    Punctuation(char),
    InterpolationStart,
    InterpolationEnd,
    CommentStart, // Not in vanilla Twig.
}

impl<'a> Into<TokenValue> for Value<'a> {
    fn into(self) -> TokenValue {
        match self {
            Value::Text(t) => TokenValue::Text(t.into()),
            Value::BlockStart => TokenValue::BlockStart,
            Value::VarStart => TokenValue::VarStart,
            Value::BlockEnd => TokenValue::BlockEnd,
            Value::VarEnd => TokenValue::VarEnd,
            Value::Name(n) => TokenValue::Name(n.into()),
            Value::Value(v) => TokenValue::Value(v.into()),
            Value::Operator(s) => TokenValue::Operator(s.into()),
            Value::Punctuation(s) => TokenValue::Punctuation(s.into()),
            Value::InterpolationStart => TokenValue::InterpolationStart,
            Value::InterpolationEnd => TokenValue::InterpolationEnd,
            Value::CommentStart => TokenValue::CommentStart,
        }
    }
}

/// Token value.
#[derive(PartialEq, Debug, Clone)]
pub enum TokenValue {
    Text(String),
    BlockStart,
    VarStart,
    BlockEnd,
    VarEnd,
    Name(String),
    Value(TwigValue),
    Operator(String),
    Punctuation(char),
    InterpolationStart,
    InterpolationEnd,
    CommentStart, // Not in vanilla Twig.
}

impl TokenValue {
    /// Return english name and value for token.
    pub fn get_english(&self) -> (&'static str, Option<String>) {
        match *self {
            TokenValue::Text(ref v) => ("text", Some(v.to_string())),
            TokenValue::BlockStart => ("begin of statement block", None),
            TokenValue::VarStart => ("begin of print statement", None),
            TokenValue::BlockEnd => ("end of statement block", None),
            TokenValue::VarEnd => ("end of print statement", None),
            TokenValue::Name(ref n) => ("name", Some(n.to_string())),
            TokenValue::Value(TwigValue::Num(ref n)) => ("number", Some(n.to_string())),
            TokenValue::Value(TwigValue::Str(ref s)) => ("string", Some(s.to_string())),
            TokenValue::Operator(ref s) => ("operator", Some(s.to_string())),
            TokenValue::Punctuation(s) => ("punctuation", Some(s.to_string())),
            TokenValue::InterpolationStart => ("begin of string interpolation", None),
            TokenValue::InterpolationEnd => ("end of string interpolation", None),
            TokenValue::CommentStart => ("comment start", None),
        }
    }
}
