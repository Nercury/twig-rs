use parser::Context;
use token_parser::TokenParserExtension;
use nodes::Body;
use tokens::TokenRef;
use Result;

pub struct Flush;

impl Flush {
    pub fn new() -> Flush {
        Flush
    }
}

impl TokenParserExtension for Flush {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Flush::parse")
    }

    fn get_tag(&self) -> &'static str {
        "flush"
    }
}
