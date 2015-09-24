use node::Expr;

#[derive(Debug)]
pub enum BodyNode<'a> {
    Expr(Expr<'a>),
    List(Vec<BodyNode<'a>>),
}

impl<'a> BodyNode<'a> {
    pub fn new() -> BodyNode<'a> {
        BodyNode::List(Vec::new())
    }

    pub fn expect_expr<'r>(&'a self) -> &'r Expr<'a> {
        match *self {
            BodyNode::Expr(ref e) => e,
            ref what => panic!("Expected expect_expr to return Expr but received {:?}", what),
        }
    }
}
