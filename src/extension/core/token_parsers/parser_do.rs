use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct Do;

impl Do {
    pub fn new() -> Do {
        Do
    }
}

impl TokenParserExtension for Do {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Do::parse")
    }

    fn get_tag(&self) -> &'static str {
        "do"
    }
}
