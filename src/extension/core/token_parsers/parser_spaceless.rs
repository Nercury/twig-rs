use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use tokens::TokenRef;
use Result;

pub struct Spaceless;

impl Spaceless {
    pub fn new() -> Spaceless {
        Spaceless
    }
}

impl TokenParserExtension for Spaceless {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Spaceless::parse")
    }

    fn get_tag(&self) -> &'static str {
        "spaceless"
    }
}
