use parser::Context;
use token_parser::TokenParserExtension;
use nodes::Body;
use nodes::node::ImportTarget;
use tokens::{ TokenRef, TokenValueRef };
use Result;

use parser::expr::parse_expression;

pub struct From;

impl From {
    pub fn new() -> From {
        From
    }
}

impl TokenParserExtension for From {
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>
    {
        trace!("From::parse {:?}", token);

        let macro_expr = try!(parse_expression(parser, 0));

        try!(parser.expect(TokenValueRef::Name("import")));

        let mut targets = Vec::new();
        loop {
            let name = try!(parser.expect_name());
            let mut alias = name;
            if try!(parser.skip_to_next_if(TokenValueRef::Name("as"))) {
                alias = try!(parser.expect_name());
            }
            targets.push((alias, name));
            if !try!(parser.skip_to_next_if(TokenValueRef::Punctuation(','))) {
                break;
            }
        }
        try!(parser.expect(TokenValueRef::BlockEnd));

        let mut target_slots = Vec::new();
        for (alias, name) in targets {
            target_slots.push(
                (
                    parser.add_imported_function(alias, name),
                    alias,
                    ImportTarget::Function { symbol: name }
                )
            );
        }

        Ok(Some(Body::Import {
            source: Box::new(macro_expr),
            targets: target_slots,
            line: token.line,
        }))
    }

    fn get_tag(&self) -> &'static str {
        "from"
    }
}
