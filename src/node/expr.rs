use Token;
use Result;

#[derive(Debug, Eq, PartialEq)]
pub enum Expr<'a> {
    Constant(&'a str, usize) // value, lineno
}

impl<'a> Expr<'a> {
    pub fn from_tokens<'code, I>(tokens: &mut I)
        -> Result<Expr<'code>>
            where I: Iterator<Item=Result<Token<'code>>>
    {
        Ok(Expr::Constant("", 1))
    }
}
