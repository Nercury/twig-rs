use parser::Context;
use node::Body;
use Result;

pub trait TokenParserExtension
{
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body>;
}

pub struct TokenParser {
    pub tag: &'static str,
    pub extension: Box<TokenParserExtension>,
}
