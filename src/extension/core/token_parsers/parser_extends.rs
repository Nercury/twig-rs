use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension, Body };
use Result;

pub struct Extends;

impl Extends {
    pub fn new() -> Extends {
        Extends
    }
}

impl TokenParserExtension for Extends {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Extends::parse")
    }

    fn get_tag(&self) -> &'static str {
        "extends"
    }
}
