use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct From;

impl From {
    pub fn new() -> From {
        From
    }
}

impl TokenParserExtension for From {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented From::parse")
    }

    fn get_tag(&self) -> &'static str {
        "from"
    }
}
