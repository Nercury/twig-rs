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

    pub fn new_str_constant<'r>(value: &'r str, line: usize) -> Expr<'r> {
        Expr::new_at(ExprValue::Constant(ExprConstant::Str(value)), line)
    }

    pub fn new_bool<'r>(value: bool, line: usize) -> Expr<'r> {
        Expr::new_at(ExprValue::Constant(ExprConstant::Bool(value)), line)
    }

    pub fn new_null<'r>(line: usize) -> Expr<'r> {
        Expr::new_at(ExprValue::Constant(ExprConstant::Null), line)
    }

    pub fn new_name<'r>(name: &'r str, line: usize) -> Expr<'r> {
        Expr::new_at(ExprValue::Name(name), line)
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum ExprConstant<'c> {
    Str(&'c str),
    Bool(bool),
    Null,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum ExprValue<'c> {
    Constant(ExprConstant<'c>),
    Name(&'c str),
    AssignName(&'c str),
    UnaryOperator { value: &'c str, expr: Box<Expr<'c>> },
    BinaryOperator { value: &'c str, left: Box<Expr<'c>>, right: Box<Expr<'c>> },
    Concat { left: Box<Expr<'c>>, right: Box<Expr<'c>> },
    Conditional { expr: Box<Expr<'c>>, yay: Box<Expr<'c>>, nay: Box<Expr<'c>> },
}
