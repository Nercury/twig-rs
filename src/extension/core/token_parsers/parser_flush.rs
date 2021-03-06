use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use error::TemplateResult;

pub struct Flush;

impl Flush {
    pub fn new() -> Flush {
        Flush
    }
}

impl TokenParserExtension for Flush {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> TemplateResult<Option<Body<'c>>>
    {
        unreachable!("not implemented Flush::parse")
    }

    fn get_tag(&self) -> &'static str {
        "flush"
    }
}
