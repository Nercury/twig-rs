use parser::Context;
use node::Body;
use Result;

pub trait TokenParserExtension
{
    fn get_tag<'r>(&self) -> &'r str;
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>;
}

pub struct TokenParser {
    pub tag: String,
    pub extension: Box<TokenParserExtension>,
}

impl TokenParser {
    pub fn new<E: 'static>(parser_extension: E)
        -> TokenParser
        where E: TokenParserExtension
    {
        TokenParser {
            tag: parser_extension.get_tag().into(),
            extension: Box::new(parser_extension),
        }
    }
}
