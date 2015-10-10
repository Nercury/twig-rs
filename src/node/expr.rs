#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Expr<'c> {
    Constant { value: &'c str, line: usize },
    UnaryOperator { value: &'c str, expr: Box<Expr<'c>>, line: usize },
    BinaryOperator { value: &'c str, left: Box<Expr<'c>>, right: Box<Expr<'c>>, line: usize },
}
