use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension, Body };
use Result;

pub struct Spaceless;

impl Spaceless {
    pub fn new() -> Spaceless {
        Spaceless
    }
}

impl TokenParserExtension for Spaceless {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Spaceless::parse")
    }

    fn get_tag(&self) -> &'static str {
        "spaceless"
    }
}
