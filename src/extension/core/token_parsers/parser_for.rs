use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use tokens::TokenRef;
use Result;

pub struct For;

impl For {
    pub fn new() -> For {
        For
    }
}

impl TokenParserExtension for For {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented For::parse")
    }

    fn get_tag(&self) -> &'static str {
        "for"
    }
}
