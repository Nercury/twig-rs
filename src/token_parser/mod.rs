use parser::Context;
use node::Body;
use token::Token;
use Result;

pub trait TokenParserExtension
{
    fn get_tag(&self) -> &'static str;
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>;
}

pub struct TokenParser {
    pub tag: &'static str,
    pub extension: Box<TokenParserExtension>,
}

impl TokenParser {
    pub fn new<E: 'static>(parser_extension: E)
        -> TokenParser
        where E: TokenParserExtension
    {
        TokenParser {
            tag: parser_extension.get_tag(),
            extension: Box::new(parser_extension),
        }
    }
}
