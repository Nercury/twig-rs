use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct Use;

impl Use {
    pub fn new() -> Use {
        Use
    }
}

impl TokenParserExtension for Use {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>
    {
        unreachable!("not implemented Use::parse")
    }

    fn get_tag(&self) -> &'static str {
        "use"
    }
}
