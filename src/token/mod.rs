use std::fmt;
use std::convert::Into;
use Error;

/// Lexer output token, lexer's output and parser's input.
#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub value: Value<'a>,
    pub line_num: usize,
}

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TwigNumber<'a> {
    Big(&'a str),
    Float(f64),
    Int(u64),
}

/// Parsed Twig string representation.
#[derive(Eq, PartialEq, Copy, Clone)]
pub struct TwigString<'a>(&'a str);

impl<'a> TwigString<'a> {
    pub fn new<'r>(source: &'r str) -> TwigString<'r> {
        TwigString(source)
    }
}

impl<'a> fmt::Debug for TwigString<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let &TwigString(ref v) = self;
        write!(f, "{}", v)
    }
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
    Number(TwigNumber<'a>),
    String(TwigString<'a>),
    Operator(&'a str),
    Punctuation(char),
    InterpolationStart,
    InterpolationEnd,
    CommentStart, // Not in vanilla Twig.
}

impl<'a> Into<DebugValue> for Value<'a> {
    fn into(self) -> DebugValue {
        match self {
            Value::Text(t) => DebugValue::Text(t.to_string()),
            Value::BlockStart => DebugValue::BlockStart,
            Value::VarStart => DebugValue::VarStart,
            Value::BlockEnd => DebugValue::BlockEnd,
            Value::VarEnd => DebugValue::VarEnd,
            Value::Name(n) => DebugValue::Name(n.to_string()),
            Value::Number(n) => DebugValue::Number(n.into()),
            Value::String(s) => DebugValue::String(s.into()),
            Value::Operator(s) => DebugValue::Operator(s.into()),
            Value::Punctuation(s) => DebugValue::Punctuation(s.into()),
            Value::InterpolationStart => DebugValue::InterpolationStart,
            Value::InterpolationEnd => DebugValue::InterpolationEnd,
            Value::CommentStart => DebugValue::CommentStart,
        }
    }
}

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Clone)]
pub enum DebugTwigNumber {
    Big(String),
    Float(f64),
    Int(u64),
}

impl<'a> Into<DebugTwigNumber> for TwigNumber<'a> {
    fn into(self) -> DebugTwigNumber {
        match self {
            TwigNumber::Big(n) => DebugTwigNumber::Big(n.to_string()),
            TwigNumber::Float(v) => DebugTwigNumber::Float(v),
            TwigNumber::Int(v) => DebugTwigNumber::Int(v),
        }
    }
}

/// Parsed Twig string representation.
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct DebugTwigString(String);

impl<'a> Into<DebugTwigString> for TwigString<'a> {
    fn into(self) -> DebugTwigString {
        DebugTwigString(self.0.to_string())
    }
}

/// Token value.
#[derive(PartialEq, Debug, Clone)]
pub enum DebugValue {
    Text(String),
    BlockStart,
    VarStart,
    BlockEnd,
    VarEnd,
    Name(String),
    Number(DebugTwigNumber),
    String(DebugTwigString),
    Operator(String),
    Punctuation(char),
    InterpolationStart,
    InterpolationEnd,
    CommentStart, // Not in vanilla Twig.
}
