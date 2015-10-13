use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use token::Token;
use Result;

use super::parse_assignment_expression;

pub struct Set;

impl Set {
    pub fn new() -> Set {
        Set
    }
}

impl TokenParserExtension for Set {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        println!("Set::parse");

        let _line = token.line;
        let _targets = try!(parse_assignment_expression(parser));

        unreachable!("not fully implemented Set::parse")
    }

    fn get_tag(&self) -> &'static str {
        "set"
    }
}