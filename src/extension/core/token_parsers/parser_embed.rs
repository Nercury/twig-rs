use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

pub struct Embed;

impl Embed {
    pub fn new() -> Embed {
        Embed
    }
}

impl TokenParserExtension for Embed {
    fn parse<'p, 'c>(
        &self,
        _parser: &mut Parser<'p, 'c>,
        _token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        unreachable!("not implemented Embed::parse")
    }

    fn get_tag(&self) -> &'static str {
        "embed"
    }
}
