#[derive(Debug, Eq, PartialEq)]
pub enum Expr<'a> {
    Constant { value: &'a str, line: usize },
}
