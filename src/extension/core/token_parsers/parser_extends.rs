use parser::Context;
use token_parser::TokenParserExtension;
use nodes::Body;
use tokens::TokenRef;
use Result;

pub struct Extends;

impl Extends {
    pub fn new() -> Extends {
        Extends
    }
}

impl TokenParserExtension for Extends {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Extends::parse")
    }

    fn get_tag(&self) -> &'static str {
        "extends"
    }
}
