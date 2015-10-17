use parser::Context;
use nodes::{ Body, TokenParserExtension };
use tokens::TokenRef;
use Result;

use super::parse_assignment_expression;

pub struct Set;

impl Set {
    pub fn new() -> Set {
        Set
    }
}

impl TokenParserExtension for Set {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        trace!("Set::parse");

        let _line = token.line;
        let _targets = try!(parse_assignment_expression(parser));

        unreachable!("not fully implemented Set::parse")
    }

    fn get_tag(&self) -> &'static str {
        "set"
    }
}
