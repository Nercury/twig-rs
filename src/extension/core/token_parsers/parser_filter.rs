use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use Result;

pub struct Filter;

impl Filter {
    pub fn new() -> Filter {
        Filter
    }
}

impl TokenParserExtension for Filter {
    fn parse<'p, 'c>(&'p self, parser: &mut Context<'p, 'c>)
        -> Result<Body<'c>>
    {
        unreachable!("not implemented Filter::parse")
    }

    fn get_tag(&self) -> &'static str {
        "filter"
    }
}
