use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use error::TemplateResult;

pub struct Use;

impl Use {
    pub fn new() -> Use {
        Use
    }
}

impl TokenParserExtension for Use {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> TemplateResult<Option<Body<'c>>>
    {
        unreachable!("not implemented Use::parse")
    }

    fn get_tag(&self) -> &'static str {
        "use"
    }
}
