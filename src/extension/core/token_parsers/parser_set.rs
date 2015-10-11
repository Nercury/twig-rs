use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use token::Token;
use Result;

pub struct Set;

impl Set {
    pub fn new() -> Set {
        Set
    }
}

impl TokenParserExtension for Set {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Set::parse")
    }

    fn get_tag(&self) -> &'static str {
        "set"
    }
}
