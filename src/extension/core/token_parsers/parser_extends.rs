use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct Extends;

impl Extends {
    pub fn new() -> Extends {
        Extends
    }
}

impl TokenParserExtension for Extends {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>
    {
        unreachable!("not implemented Extends::parse")
    }

    fn get_tag<'r>(&self) -> &'r str {
        "extends"
    }
}
