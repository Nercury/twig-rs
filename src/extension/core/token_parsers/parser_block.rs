use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::TokenRef;

pub struct Block;

impl Block {
    pub fn new() -> Block {
        Block
    }
}

impl TokenParserExtension for Block {
    fn parse<'p, 'c>(
        &self,
        _parser: &mut Parser<'p, 'c>,
        _token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        unreachable!("not implemented Block::parse")
    }

    fn get_tag(&self) -> &'static str {
        "block"
    }
}
