use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension, Body };
use Result;

pub struct Flush;

impl Flush {
    pub fn new() -> Flush {
        Flush
    }
}

impl TokenParserExtension for Flush {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Flush::parse")
    }

    fn get_tag(&self) -> &'static str {
        "flush"
    }
}
