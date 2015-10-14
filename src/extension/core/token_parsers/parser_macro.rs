use parser::Context;
use token_parser::TokenParserExtension;
use node::Body;
use { Token, TokenValue };
use Result;

use parser::expr::parse_named_arguments;

pub struct Macro;

impl Macro {
    pub fn new() -> Macro {
        Macro
    }
}

impl TokenParserExtension for Macro {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: Token<'c>)
        -> Result<Option<Body<'c>>>
    {
        println!("Macro::parse, {:?}", token);

        let name = try!(parser.expect_name());
        let arguments = try!(parse_named_arguments(parser, true));

        try!(parser.expect(TokenValue::BlockEnd));

        unreachable!("not implemented Macro::parse")
    }

    fn get_tag(&self) -> &'static str {
        "macro"
    }
}
