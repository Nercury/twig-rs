use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use { Token, TokenValue };
use token::DebugValue;
use error::{ Error, ErrorMessage, Received };
use Result;

use parser::expr::parse_expression;

pub struct From;

impl From {
    pub fn new() -> From {
        From
    }
}

impl TokenParserExtension for From {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        println!("From::parse");

        let macro_expr = try!(parse_expression(parser, 0));
        try!(parser.expect(TokenValue::Name("import")));

        //let mut targets = Vec::new();
        loop {
            let name = try!(parser.expect_name());
        }
        unreachable!("not implemented From::parse");
    }

    fn get_tag(&self) -> &'static str {
        "from"
    }
}
