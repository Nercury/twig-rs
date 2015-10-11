#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Expr<'c> {
    pub line: usize,
    pub value: ExprValue<'c>,
}

impl<'c> Expr<'c> {
    pub fn new_at<'r>(value: ExprValue<'r>, line: usize) -> Expr<'r> {
        Expr {
            line: line,
            value: value
        }
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum ExprValue<'c> {
    Constant { value: &'c str },
    UnaryOperator { value: &'c str, expr: Box<Expr<'c>> },
    BinaryOperator { value: &'c str, left: Box<Expr<'c>>, right: Box<Expr<'c>> },
    Concat { left: Box<Expr<'c>>, right: Box<Expr<'c>> },
    Conditional { expr: Box<Expr<'c>>, yay: Box<Expr<'c>>, nay: Box<Expr<'c>> },
}
