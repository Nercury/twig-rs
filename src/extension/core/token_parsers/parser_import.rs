use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct Import;

impl Import {
    pub fn new() -> Import {
        Import
    }
}

impl TokenParserExtension for Import {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Import::parse")
    }

    fn get_tag(&self) -> &'static str {
        "import"
    }
}
