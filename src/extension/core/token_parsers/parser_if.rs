use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

pub struct If;

impl If {
    pub fn new() -> If {
        If
    }
}

impl TokenParserExtension for If {
    fn parse<'p, 'c>(
        &self,
        _parser: &mut Parser<'p, 'c>,
        _token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        unreachable!("not implemented If::parse")
    }

    fn get_tag(&self) -> &'static str {
        "if"
    }
}
