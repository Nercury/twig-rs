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
