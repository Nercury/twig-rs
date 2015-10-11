use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use token::Token;
use Result;

pub struct Use;

impl Use {
    pub fn new() -> Use {
        Use
    }
}

impl TokenParserExtension for Use {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Use::parse")
    }

    fn get_tag(&self) -> &'static str {
        "use"
    }
}
