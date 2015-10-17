use parser::Context;
use nodes::{ TokenParserExtension, Body };
use tokens::TokenRef;
use Result;

pub struct Filter;

impl Filter {
    pub fn new() -> Filter {
        Filter
    }
}

impl TokenParserExtension for Filter {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        unreachable!("not implemented Filter::parse")
    }

    fn get_tag(&self) -> &'static str {
        "filter"
    }
}
