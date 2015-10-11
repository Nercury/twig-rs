use parser::Context;
use node::Body;
use Result;

pub trait TokenParserExtension
{
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>;
}

pub struct TokenParser {
    pub tag: &'static str,
    pub extension: Box<TokenParserExtension>,
}

impl TokenParser {
    pub fn new<E: 'static>(tag: &'static str, parser_extension: E)
        -> TokenParser
        where E: TokenParserExtension
    {
        TokenParser {
            tag: tag,
            extension: Box::new(parser_extension),
        }
    }
}
