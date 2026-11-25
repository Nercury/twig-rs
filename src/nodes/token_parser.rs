use super::TokenParserExtension;
use std::fmt;

pub struct TokenParser {
    pub tag: &'static str,
    pub extension: Box<dyn TokenParserExtension>,
}

impl fmt::Debug for TokenParser {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({:?}, extension)", self.tag)
    }
}

impl TokenParser {
    pub fn new<E>(parser_extension: E) -> TokenParser
    where
        E: TokenParserExtension + 'static,
    {
        TokenParser {
            tag: parser_extension.get_tag(),
            extension: Box::new(parser_extension),
        }
    }
}
