use parser::Context;
use token_parser::TokenParserExtension;
use nodes::Body;
use tokens::TokenRef;
use Result;

pub struct If;

impl If {
    pub fn new() -> If {
        If
    }
}

impl TokenParserExtension for If {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented If::parse")
    }

    fn get_tag(&self) -> &'static str {
        "if"
    }
}
