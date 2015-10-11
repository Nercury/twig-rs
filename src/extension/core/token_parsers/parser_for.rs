use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct For;

impl For {
    pub fn new() -> For {
        For
    }
}

impl TokenParserExtension for For {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>
    {
        unreachable!("not implemented TokenParserExtension::parse")
    }
}
