#[derive(Debug, Eq, PartialEq)]
pub enum Expr<'a> {
    Constant(&'a str, usize) // value, lineno
}
