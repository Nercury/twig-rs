#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TwigNumber {
    Float(f64),
    Int(u64),
}

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Value<'a> {
    Eof,
    Text(&'a str),
    BlockStart,
    VarStart,
    BlockEnd,
    VarEnd,
    Name(&'a str),
    Number(TwigNumber),
    String,
    Operator,
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
