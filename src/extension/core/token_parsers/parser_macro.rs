use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct Macro;

impl Macro {
    pub fn new() -> Macro {
        Macro
    }
}

impl TokenParserExtension for Macro {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>
    {
        unreachable!("not implemented Macro::parse")
    }

    fn get_tag(&self) -> &'static str {
        "macro"
    }
}
