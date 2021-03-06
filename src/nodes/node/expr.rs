use uuid::Uuid;

#[derive(Debug, PartialEq, Clone)]
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

    pub fn new_array<'r>(value: Vec<Expr<'r>>, line: usize) -> Expr<'r> {
        Expr::new_at(ExprValue::Array(value), line)
    }

    pub fn new_hash<'r>(value: Vec<(Expr<'r>, Expr<'r>)>, line: usize) -> Expr<'r> {
        Expr::new_at(ExprValue::Hash(value), line)
    }

    pub fn new_str_constant<'r>(value: &'r str, line: usize) -> Expr<'r> {
        Expr::new_at(ExprValue::Constant(ExprConstant::Str(value)), line)
    }

    pub fn new_int_constant<'r>(value: i64, line: usize) -> Expr<'r> {
        Expr::new_at(ExprValue::Constant(ExprConstant::Int(value)), line)
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

    pub fn is_constant(&self) -> bool {
        match self.value {
            ExprValue::Array(ref items) => items.iter().all(|i| i.is_constant()),
            ExprValue::AssignName(_) => false,
            ExprValue::BinaryOperator { .. } => false,
            ExprValue::Concat { .. } => false,
            ExprValue::Conditional { .. } => false,
            ExprValue::Constant(_) => true,
            ExprValue::Name(_) => false,
            ExprValue::UnaryOperator { value: "-", ref expr } => expr.is_constant(),
            ExprValue::UnaryOperator { value: "+", ref expr } => expr.is_constant(),
            ExprValue::UnaryOperator { .. } => false,
            ExprValue::Hash(ref items) => items.iter().all(|&(ref k, ref v)| k.is_constant() && v.is_constant()),
            ExprValue::GetAttr { .. } => false,
            ExprValue::ImportedFunctionCall { .. } => false,
            ExprValue::FunctionCall { .. } => false,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum ExprConstant<'c> {
    Str(&'c str),
    Bool(bool),
    Int(i64),
    Float(f64),
    Big(&'c str),
    Null,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ExprValue<'c> {
    Constant(ExprConstant<'c>),
    Name(&'c str),
    AssignName(&'c str),
    Array(Vec<Expr<'c>>),
    Hash(Vec<(Expr<'c>, Expr<'c>)>),
    UnaryOperator { value: &'c str, expr: Box<Expr<'c>> },
    BinaryOperator { value: &'c str, left: Box<Expr<'c>>, right: Box<Expr<'c>> },
    Concat { left: Box<Expr<'c>>, right: Box<Expr<'c>> },
    Conditional { expr: Box<Expr<'c>>, yay: Box<Expr<'c>>, nay: Box<Expr<'c>> },
    GetAttr {
        node: Box<Expr<'c>>,
        arg: Box<Expr<'c>>,
        arguments: Vec<Expr<'c>>,
        call_type: ExprCallType
    },
    ImportedFunctionCall { uuid: Uuid, alias: &'c str, arguments: Vec<Expr<'c>> },
    FunctionCall { name: &'c str, arguments: Vec<(Option<&'c str>, Expr<'c>)> }
}

#[derive(Debug, PartialEq, Clone)]
pub enum ExprCallType {
    Any,
    Method,
    Array,
}
