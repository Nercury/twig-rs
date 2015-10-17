use parser::Context;
use nodes::{ Body, TokenParserExtension };
use tokens::TokenRef;
use Result;

pub struct Import;

impl Import {
    pub fn new() -> Import {
        Import
    }
}

impl TokenParserExtension for Import {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Import::parse")
    }

    fn get_tag(&self) -> &'static str {
        "import"
    }
}
