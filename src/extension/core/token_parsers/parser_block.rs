use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension, Body };
use Result;

pub struct Block;

impl Block {
    pub fn new() -> Block {
        Block
    }
}

impl TokenParserExtension for Block {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Block::parse")
    }

    fn get_tag(&self) -> &'static str {
        "block"
    }
}
