use std::collections::HashMap;
use parser::Context;
use token_parser::TokenParserExtension;
use node::{ Body, ImportTarget };
use { Token, TokenValue };
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
        println!("From::parse {:?}", token);

        let macro_expr = try!(parse_expression(parser, 0));

        try!(parser.expect(TokenValue::Name("import")));

        let mut targets = HashMap::new();
        loop {
            let name = try!(parser.expect_name());
            let mut alias = name;
            if try!(parser.skip_to_next_if(TokenValue::Name("as"))) {
                alias = try!(parser.expect_name());
            }
            targets.insert(alias, ImportTarget::Macro { symbol: name });
            if !try!(parser.skip_to_next_if(TokenValue::Punctuation(','))) {
                break;
            }
        }
        try!(parser.expect(TokenValue::BlockEnd));

        Ok(Some(Body::Import {
            source: Box::new(macro_expr),
            targets: targets,
            line: token.line,
        }))
    }

    fn get_tag(&self) -> &'static str {
        "from"
    }
}
