use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use tokens::Token;
use Result;

pub struct Embed;

impl Embed {
    pub fn new() -> Embed {
        Embed
    }
}

impl TokenParserExtension for Embed {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Embed::parse")
    }

    fn get_tag(&self) -> &'static str {
        "embed"
    }
}
