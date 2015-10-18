use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use error::TemplateResult;

pub struct Import;

impl Import {
    pub fn new() -> Import {
        Import
    }
}

impl TokenParserExtension for Import {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> TemplateResult<Option<Body<'c>>>
    {
        unreachable!("not implemented Import::parse")
    }

    fn get_tag(&self) -> &'static str {
        "import"
    }
}
