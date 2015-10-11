use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use token::Token;
use Result;

pub struct For;

impl For {
    pub fn new() -> For {
        For
    }
}

impl TokenParserExtension for For {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented For::parse")
    }

    fn get_tag(&self) -> &'static str {
        "for"
    }
}
