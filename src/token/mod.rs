use std::fmt;
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
