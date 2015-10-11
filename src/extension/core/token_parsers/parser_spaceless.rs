use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct Spaceless;

impl Spaceless {
    pub fn new() -> Spaceless {
        Spaceless
    }
}

impl TokenParserExtension for Spaceless {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>
    {
        unreachable!("not implemented Spaceless::parse")
    }

    fn get_tag(&self) -> &'static str {
        "spaceless"
    }
}
