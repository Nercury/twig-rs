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

impl<'a> Into<DebugValue> for Value<'a> {
    fn into(self) -> DebugValue {
        match self {
            Value::Text(t) => DebugValue::Text(t.into()),
            Value::BlockStart => DebugValue::BlockStart,
            Value::VarStart => DebugValue::VarStart,
            Value::BlockEnd => DebugValue::BlockEnd,
            Value::VarEnd => DebugValue::VarEnd,
            Value::Name(n) => DebugValue::Name(n.into()),
            Value::Value(v) => DebugValue::Value(v.into()),
            Value::Operator(s) => DebugValue::Operator(s.into()),
            Value::Punctuation(s) => DebugValue::Punctuation(s.into()),
            Value::InterpolationStart => DebugValue::InterpolationStart,
            Value::InterpolationEnd => DebugValue::InterpolationEnd,
            Value::CommentStart => DebugValue::CommentStart,
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
