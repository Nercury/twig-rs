use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use error::Result;

pub struct For;

impl For {
    pub fn new() -> For {
        For
    }
}

impl TokenParserExtension for For {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented For::parse")
    }

    fn get_tag(&self) -> &'static str {
        "for"
    }
}
