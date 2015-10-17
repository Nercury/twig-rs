use value::{ TwigValueRef, TwigValue };

/// Lexer output token, lexer's output and parser's input.
#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub value: TokenValue<'a>,
    pub line: usize,
}

/// Token value.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TokenValue<'a> {
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

impl<'a> Into<DebugValue> for TokenValue<'a> {
    fn into(self) -> DebugValue {
        match self {
            TokenValue::Text(t) => DebugValue::Text(t.into()),
            TokenValue::BlockStart => DebugValue::BlockStart,
            TokenValue::VarStart => DebugValue::VarStart,
            TokenValue::BlockEnd => DebugValue::BlockEnd,
            TokenValue::VarEnd => DebugValue::VarEnd,
            TokenValue::Name(n) => DebugValue::Name(n.into()),
            TokenValue::Value(v) => DebugValue::Value(v.into()),
            TokenValue::Operator(s) => DebugValue::Operator(s.into()),
            TokenValue::Punctuation(s) => DebugValue::Punctuation(s.into()),
            TokenValue::InterpolationStart => DebugValue::InterpolationStart,
            TokenValue::InterpolationEnd => DebugValue::InterpolationEnd,
            TokenValue::CommentStart => DebugValue::CommentStart,
        }
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
    Value(TwigValue),
    Operator(String),
    Punctuation(char),
    InterpolationStart,
    InterpolationEnd,
    CommentStart, // Not in vanilla Twig.
}

impl DebugValue {
    /// Return english name and value for token.
    pub fn get_english(&self) -> (&'static str, Option<String>) {
        match *self {
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
        }
    }
}
