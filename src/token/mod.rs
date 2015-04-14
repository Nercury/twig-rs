#[derive(Eq, PartialEq, Debug, Clone)]
pub enum Value<'a> {
    Eof,
    Text(&'a str),
    BlockStart,
    VarStart,
    BlockEnd,
    VarEnd,
    Name,
    Number,
    String,
    Operator,
    Punctuation,
    InterpolationStart,
    InterpolationEnd,
    CommentStart, // Not in vanilla Twig.
}

#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub value: Value<'a>,
    pub line_num: u32,
}

#[derive(Debug)]
pub struct Unexpected<'a> {
    pub token: Option<Token<'a>>,
    pub message: Option<&'a str>,
}

#[derive(Copy, Clone)]
pub enum State {
    Data,
    Block,
    Var,
    String,
    Interpolation,
}
