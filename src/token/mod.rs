use std::fmt;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TwigNumber<'a> {
    Big(&'a str),
    Float(f64),
    Int(u64),
}

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

#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub value: Value<'a>,
    pub line_num: usize,
}

#[derive(Debug)]
pub struct Unexpected<'a> {
    pub token: Option<Token<'a>>,
    pub message: Option<&'a str>,
}

#[derive(Debug, Copy, Clone)]
pub enum State {
    Data,
    Block,
    Var,
    String,
    Interpolation,
}
