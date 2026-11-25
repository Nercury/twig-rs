use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

use super::parse_assignment_expression;

pub struct Set;

impl Set {
    pub fn new() -> Set {
        Set
    }
}

impl TokenParserExtension for Set {
    fn parse<'p, 'c>(
        &self,
        parser: &mut Parser<'p, 'c>,
        token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        let _line = token.line;
        let _targets = parse_assignment_expression(parser)?;

        unreachable!("not fully implemented Set::parse")
    }

    fn get_tag(&self) -> &'static str {
        "set"
    }
}
