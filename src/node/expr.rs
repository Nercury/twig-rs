#[derive(Debug, Eq, PartialEq)]
pub enum Expr<'c> {
    Constant { value: &'c str, line: usize },
    Operator { value: &'c str, expr: Box<Expr<'c>>, line: usize },
}
