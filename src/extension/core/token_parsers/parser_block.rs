use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct Block;

impl Block {
    pub fn new() -> Block {
        Block
    }
}

impl TokenParserExtension for Block {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>
    {
        unreachable!("not implemented Block::parse")
    }

    fn get_tag(&self) -> &'static str {
        "block"
    }
}
