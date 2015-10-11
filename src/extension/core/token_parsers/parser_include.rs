use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct Include;

impl Include {
    pub fn new() -> Include {
        Include
    }
}

impl TokenParserExtension for Include {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>
    {
        unreachable!("not implemented Include::parse")
    }

    fn get_tag(&self) -> &'static str {
        "include"
    }
}
