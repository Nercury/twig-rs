use value::{ TwigValueRef, TwigValue };

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
    Value(TwigValueRef<'a>),
    Operator(&'a str),
    Punctuation(char),
    InterpolationStart,
    InterpolationEnd,
    CommentStart, // Not in vanilla Twig.
}

impl<'a> Into<DebugValue> for TokenValueRef<'a> {
    fn into(self) -> DebugValue {
        match self {
            TokenValueRef::Text(t) => DebugValue::Text(t.into()),
            TokenValueRef::BlockStart => DebugValue::BlockStart,
            TokenValueRef::VarStart => DebugValue::VarStart,
            TokenValueRef::BlockEnd => DebugValue::BlockEnd,
            TokenValueRef::VarEnd => DebugValue::VarEnd,
            TokenValueRef::Name(n) => DebugValue::Name(n.into()),
            TokenValueRef::Value(v) => DebugValue::Value(v.into()),
            TokenValueRef::Operator(s) => DebugValue::Operator(s.into()),
            TokenValueRef::Punctuation(s) => DebugValue::Punctuation(s.into()),
            TokenValueRef::InterpolationStart => DebugValue::InterpolationStart,
            TokenValueRef::InterpolationEnd => DebugValue::InterpolationEnd,
            TokenValueRef::CommentStart => DebugValue::CommentStart,
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
