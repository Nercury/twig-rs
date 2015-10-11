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
