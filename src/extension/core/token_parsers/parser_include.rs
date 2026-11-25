use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

pub struct Include;

impl Include {
    pub fn new() -> Include {
        Include
    }
}

impl TokenParserExtension for Include {
    fn parse<'p, 'c>(
        &self,
        _parser: &mut Parser<'p, 'c>,
        _token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        unreachable!("not implemented Include::parse")
    }

    fn get_tag(&self) -> &'static str {
        "include"
    }
}
