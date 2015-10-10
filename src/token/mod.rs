use value::{ TwigValue, OwnedTwigValue };
use operator::OperatorKind;

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
    Value(TwigValue<'a>),
    Operator { value: &'a str, kind: OperatorKind },
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
            Value::Value(v) => DebugValue::Value(v.into()),
            Value::Operator { value: s, kind: k } => DebugValue::Operator { value: s.into(), kind: k },
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
    Value(OwnedTwigValue),
    Operator { value: String, kind: OperatorKind },
    Punctuation(char),
    InterpolationStart,
    InterpolationEnd,
    CommentStart, // Not in vanilla Twig.
}
