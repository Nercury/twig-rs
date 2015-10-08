use node::Expr;

#[derive(Debug)]
pub enum Body<'a> {
    List { items: Vec<Body<'a>> },
    Text { value: &'a str, line: usize },
    Print { expr: Expr<'a>, line: usize },
}

impl<'a> Body<'a> {
    pub fn new() -> Body<'a> {
        Body::List { items: Vec::new() }
    }

    pub fn expect_print<'r>(&'a self) -> &'r Expr<'a> {
        match *self {
            Body::Print { expr: ref e, .. } => e,
            ref what => panic!("Expected expect_print to return Expr but received {:?}", what),
        }
    }

    pub fn expect_list<'r>(&'a self) -> &'r Vec<Body<'a>> {
        match *self {
            Body::List { items: ref list } => list,
            ref what => panic!("Expected expect_list to return Vec but received {:?}", what),
        }
    }
}
