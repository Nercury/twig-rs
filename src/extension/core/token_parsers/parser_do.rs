use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

pub struct Do;

impl Do {
    pub fn new() -> Do {
        Do
    }
}

impl TokenParserExtension for Do {
    fn parse<'p, 'c>(
        &self,
        _parser: &mut Parser<'p, 'c>,
        _token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        unreachable!("not implemented Do::parse")
    }

    fn get_tag(&self) -> &'static str {
        "do"
    }
}
