use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

pub struct Use;

impl Use {
    pub fn new() -> Use {
        Use
    }
}

impl TokenParserExtension for Use {
    fn parse<'p, 'c>(
        &self,
        _parser: &mut Parser<'p, 'c>,
        _token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        unreachable!("not implemented Use::parse")
    }

    fn get_tag(&self) -> &'static str {
        "use"
    }
}
