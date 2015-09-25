use node::Expr;

#[derive(Debug)]
pub enum Body<'a> {
    Expr(Expr<'a>),
    List(Vec<Body<'a>>),
    Text(&'a str, usize),
}

impl<'a> Body<'a> {
    pub fn new() -> Body<'a> {
        Body::List(Vec::new())
    }

    pub fn expect_expr<'r>(&'a self) -> &'r Expr<'a> {
        match *self {
            Body::Expr(ref e) => e,
            ref what => panic!("Expected expect_expr to return Expr but received {:?}", what),
        }
    }

    pub fn expect_list<'r>(&'a self) -> &'r Vec<Body<'a>> {
        match *self {
            Body::List(ref list) => list,
            ref what => panic!("Expected expect_list to return Vec but received {:?}", what),
        }
    }
}
