use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use error::TemplateResult;

pub struct Do;

impl Do {
    pub fn new() -> Do {
        Do
    }
}

impl TokenParserExtension for Do {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> TemplateResult<Option<Body<'c>>>
    {
        unreachable!("not implemented Do::parse")
    }

    fn get_tag(&self) -> &'static str {
        "do"
    }
}
