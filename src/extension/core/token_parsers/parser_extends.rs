use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use error::TemplateResult;

pub struct Extends;

impl Extends {
    pub fn new() -> Extends {
        Extends
    }
}

impl TokenParserExtension for Extends {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> TemplateResult<Option<Body<'c>>>
    {
        unreachable!("not implemented Extends::parse")
    }

    fn get_tag(&self) -> &'static str {
        "extends"
    }
}
