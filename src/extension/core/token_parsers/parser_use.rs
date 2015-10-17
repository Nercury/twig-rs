use parser::Context;
use token_parser::TokenParserExtension;
use nodes::Body;
use tokens::TokenRef;
use Result;

pub struct Use;

impl Use {
    pub fn new() -> Use {
        Use
    }
}

impl TokenParserExtension for Use {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Use::parse")
    }

    fn get_tag(&self) -> &'static str {
        "use"
    }
}
