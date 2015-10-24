use std::fmt;

/// Lexer output token, lexer's output and parser's input.
#[derive(Debug, Clone)]
pub struct TokenRef<'a> {
    pub value: TokenValueRef<'a>,
    pub line: usize,
}

/// Token value.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TokenValueRef<'a> {
    Text(&'a str),
    BlockStart,
    VarStart,
    BlockEnd,
    VarEnd,
    Name(&'a str),
    Value(ConstRef<'a>),
    Operator(&'a str),
    Punctuation(char),
    InterpolationStart,
    InterpolationEnd,
    CommentStart, // Not in vanilla Twig.
}

impl<'a> Into<TokenValue> for TokenValueRef<'a> {
    fn into(self) -> TokenValue {
        match self {
            TokenValueRef::Text(t) => TokenValue::Text(t.into()),
            TokenValueRef::BlockStart => TokenValue::BlockStart,
            TokenValueRef::VarStart => TokenValue::VarStart,
            TokenValueRef::BlockEnd => TokenValue::BlockEnd,
            TokenValueRef::VarEnd => TokenValue::VarEnd,
            TokenValueRef::Name(n) => TokenValue::Name(n.into()),
            TokenValueRef::Value(v) => TokenValue::Value(v.into()),
            TokenValueRef::Operator(s) => TokenValue::Operator(s.into()),
            TokenValueRef::Punctuation(s) => TokenValue::Punctuation(s.into()),
            TokenValueRef::InterpolationStart => TokenValue::InterpolationStart,
            TokenValueRef::InterpolationEnd => TokenValue::InterpolationEnd,
            TokenValueRef::CommentStart => TokenValue::CommentStart,
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
    Value(Const),
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
            TokenValue::Value(Const::Num(ref n)) => ("number", Some(n.to_string())),
            TokenValue::Value(Const::Str(ref s)) => ("string", Some(s.to_string())),
            TokenValue::Operator(ref s) => ("operator", Some(s.to_string())),
            TokenValue::Punctuation(s) => ("punctuation", Some(s.to_string())),
            TokenValue::InterpolationStart => ("begin of string interpolation", None),
            TokenValue::InterpolationEnd => ("end of string interpolation", None),
            TokenValue::CommentStart => ("comment start", None),
        }
    }
}

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum ConstRef<'a> {
    Num(ConstNumberRef<'a>),
    Str(&'a str),
}

impl<'a> ConstRef<'a> {
    pub fn new_big_num<'c>(num: &'c str) -> ConstRef<'c> {
        ConstRef::Num(ConstNumberRef::Big(num))
    }

    pub fn new_float<'c>(num: f64) -> ConstRef<'c> {
        ConstRef::Num(ConstNumberRef::Float(num))
    }

    pub fn new_int<'c>(num: i64) -> ConstRef<'c> {
        ConstRef::Num(ConstNumberRef::Int(num))
    }

    pub fn new_str<'c>(s: &'c str) -> ConstRef<'c> {
        ConstRef::Str(s)
    }
}

impl<'a> Into<Const> for ConstRef<'a> {
    fn into(self) -> Const {
        match self {
            ConstRef::Num(n) => Const::Num(n.into()),
            ConstRef::Str(s) => Const::Str(s.into()),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum Const {
    Num(ConstNumber),
    Str(String),
}

impl fmt::Display for Const {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Const::Num(ref n) => write!(f, "{}", n),
            Const::Str(ref s) => write!(f, "{}", s),
        }
    }
}

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum ConstNumberRef<'a> {
    Big(&'a str),
    Float(f64),
    Int(i64),
}

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Clone)]
pub enum ConstNumber {
    Big(String),
    Float(f64),
    Int(i64),
}

impl fmt::Display for ConstNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ConstNumber::Big(ref n) => write!(f, "{}", n),
            ConstNumber::Float(v) => write!(f, "{}", v),
            ConstNumber::Int(i) => write!(f, "{}", i),
        }
    }
}

impl<'a> Into<ConstNumber> for ConstNumberRef<'a> {
    fn into(self) -> ConstNumber {
        match self {
            ConstNumberRef::Big(n) => ConstNumber::Big(n.to_string()),
            ConstNumberRef::Float(v) => ConstNumber::Float(v),
            ConstNumberRef::Int(v) => ConstNumber::Int(v),
        }
    }
}
