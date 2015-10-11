use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use token::Token;
use Result;

pub struct Block;

impl Block {
    pub fn new() -> Block {
        Block
    }
}

impl TokenParserExtension for Block {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Block::parse")
    }

    fn get_tag(&self) -> &'static str {
        "block"
    }
}
