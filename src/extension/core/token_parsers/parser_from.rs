use crate::error::TemplateResult;
use crate::nodes::body::{Body, ImportTarget};
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::{TokenRef, TokenValueRef};

use crate::nodes::expr_parser::parse_expression;

pub struct From;

impl From {
    pub fn new() -> From {
        From
    }
}

impl TokenParserExtension for From {
    fn parse<'p, 'c>(
        &self,
        parser: &mut Parser<'p, 'c>,
        token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        let macro_expr = parse_expression(parser, 0)?;

        parser.expect(TokenValueRef::Name("import"))?;

        let mut targets = Vec::new();
        loop {
            let name = parser.expect_name()?;
            let mut alias = name;
            if parser.skip_to_next_if(TokenValueRef::Name("as"))? {
                alias = parser.expect_name()?;
            }
            targets.push((alias, name));
            if !parser.skip_to_next_if(TokenValueRef::Punctuation(','))? {
                break;
            }
        }
        parser.expect(TokenValueRef::BlockEnd)?;

        let mut target_slots = Vec::new();
        for (alias, name) in targets {
            target_slots.push((
                parser.add_imported_function(alias, name),
                alias,
                ImportTarget::Function { symbol: name },
            ));
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
