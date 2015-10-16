use node::Expr;
use uuid::Uuid;

#[derive(Debug)]
pub enum ImportTarget<'c> {
    Function { symbol: &'c str },
}

#[derive(Debug)]
pub enum Body<'c> {
    List { items: Vec<Body<'c>> },
    Text { value: &'c str, line: usize },
    Print { expr: Box<Expr<'c>>, line: usize },
    Import {
        /// Target template to import, which can be evaluated at runtime from
        /// provided expression.
        source: Box<Expr<'c>>,
        /// Target list alias => name.
        targets: Vec<(Uuid, &'c str, ImportTarget<'c>)>,
        line: usize
    },
    Macro {
        name: &'c str,
        body: Box<Body<'c>>,
        arguments: Vec<(&'c str, Expr<'c>)>,
        line: usize
    }
}

impl<'c> Body<'c> {
    pub fn new() -> Body<'c> {
        Body::List { items: Vec::new() }
    }

    pub fn expect_print<'r>(&'r self) -> &'r Expr<'c> {
        match *self {
            Body::Print { expr: ref e, .. } => e,
            ref what => panic!("Expected expect_print to return Expr but received {:?}", what),
        }
    }

    pub fn expect_list<'r>(&'r self) -> &'r Vec<Body<'c>> {
        match *self {
            Body::List { items: ref list } => list,
            ref what => panic!("Expected expect_list to return Vec but received {:?}", what),
        }
    }
}
