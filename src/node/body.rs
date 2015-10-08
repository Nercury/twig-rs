use node::Expr;
use Token;
use TokenValue;
use Result;
use Expect;

use CompiledEnvironment;

#[derive(Debug)]
pub enum Body<'a> {
    List(Vec<Body<'a>>),
    Text(&'a str, usize),
    Print(Expr<'a>, usize),
}

impl<'a> Body<'a> {
    pub fn new() -> Body<'a> {
        Body::List(Vec::new())
    }

    pub fn expect_print<'r>(&'a self) -> &'r Expr<'a> {
        match *self {
            Body::Print(ref e, _) => e,
            ref what => panic!("Expected expect_print to return Expr but received {:?}", what),
        }
    }

    pub fn expect_list<'r>(&'a self) -> &'r Vec<Body<'a>> {
        match *self {
            Body::List(ref list) => list,
            ref what => panic!("Expected expect_list to return Vec but received {:?}", what),
        }
    }
}
