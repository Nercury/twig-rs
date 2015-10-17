use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use Result;

pub struct Do;

impl Do {
    pub fn new() -> Do {
        Do
    }
}

impl TokenParserExtension for Do {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Do::parse")
    }

    fn get_tag(&self) -> &'static str {
        "do"
    }
}
