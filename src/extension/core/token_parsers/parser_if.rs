use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use error::TemplateResult;

pub struct If;

impl If {
    pub fn new() -> If {
        If
    }
}

impl TokenParserExtension for If {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> TemplateResult<Option<Body<'c>>>
    {
        unreachable!("not implemented If::parse")
    }

    fn get_tag(&self) -> &'static str {
        "if"
    }
}
