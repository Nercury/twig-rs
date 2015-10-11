use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use token::Token;
use Result;

pub struct Filter;

impl Filter {
    pub fn new() -> Filter {
        Filter
    }
}

impl TokenParserExtension for Filter {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Filter::parse")
    }

    fn get_tag(&self) -> &'static str {
        "filter"
    }
}
