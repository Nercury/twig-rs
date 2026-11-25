use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

pub struct For;

impl For {
    pub fn new() -> For {
        For
    }
}

impl TokenParserExtension for For {
    fn parse<'p, 'c>(
        &self,
        _parser: &mut Parser<'p, 'c>,
        _token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        unreachable!("not implemented For::parse")
    }

    fn get_tag(&self) -> &'static str {
        "for"
    }
}
