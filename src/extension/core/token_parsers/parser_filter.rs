use tokens::TokenRef;
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use error::TemplateResult;

pub struct Filter;

impl Filter {
    pub fn new() -> Filter {
        Filter
    }
}

impl TokenParserExtension for Filter {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> TemplateResult<Option<Body<'c>>>
    {
        unreachable!("not implemented Filter::parse")
    }

    fn get_tag(&self) -> &'static str {
        "filter"
    }
}
