use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use tokens::TokenRef;
use Result;

pub struct Include;

impl Include {
    pub fn new() -> Include {
        Include
    }
}

impl TokenParserExtension for Include {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Include::parse")
    }

    fn get_tag(&self) -> &'static str {
        "include"
    }
}
