use node::Expr;

#[derive(Debug)]
pub enum Body<'c> {
    List { items: Vec<Body<'c>> },
    Text { value: &'c str, line: usize },
    Print { expr: Box<Expr<'c>>, line: usize },
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
