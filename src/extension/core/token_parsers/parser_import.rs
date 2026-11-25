use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

pub struct Import;

impl Import {
    pub fn new() -> Import {
        Import
    }
}

impl TokenParserExtension for Import {
    fn parse<'p, 'c>(
        &self,
        _parser: &mut Parser<'p, 'c>,
        _token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        unreachable!("not implemented Import::parse")
    }

    fn get_tag(&self) -> &'static str {
        "import"
    }
}
