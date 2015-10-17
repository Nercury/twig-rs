use parser::Context;
use nodes::{ TokenParserExtension, Body };
use tokens::TokenRef;
use Result;

pub struct Embed;

impl Embed {
    pub fn new() -> Embed {
        Embed
    }
}

impl TokenParserExtension for Embed {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Embed::parse")
    }

    fn get_tag(&self) -> &'static str {
        "embed"
    }
}
