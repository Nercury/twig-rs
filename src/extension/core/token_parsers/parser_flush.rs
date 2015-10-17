use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use tokens::Token;
use Result;

pub struct Flush;

impl Flush {
    pub fn new() -> Flush {
        Flush
    }
}

impl TokenParserExtension for Flush {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Flush::parse")
    }

    fn get_tag(&self) -> &'static str {
        "flush"
    }
}
