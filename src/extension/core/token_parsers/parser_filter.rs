use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

pub struct Filter;

impl Filter {
    pub fn new() -> Filter {
        Filter
    }
}

impl TokenParserExtension for Filter {
    fn parse<'p, 'c>(
        &self,
        _parser: &mut Parser<'p, 'c>,
        _token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        unreachable!("not implemented Filter::parse")
    }

    fn get_tag(&self) -> &'static str {
        "filter"
    }
}
