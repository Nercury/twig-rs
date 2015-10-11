use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use token::Token;
use Result;

pub struct If;

impl If {
    pub fn new() -> If {
        If
    }
}

impl TokenParserExtension for If {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented If::parse")
    }

    fn get_tag(&self) -> &'static str {
        "if"
    }
}
