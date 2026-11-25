use crate::error::TemplateResult;
use crate::extension::core::error::*;
use crate::nodes::body::Body;
use crate::nodes::body_parser::{BlockEnd, subparse};
use crate::nodes::expr_parser::parse_named_arguments;
use crate::nodes::{Parser, TokenParserExtension};
use crate::tokens::{TokenRef, TokenValueRef};

pub struct Macro;

impl Macro {
    pub fn new() -> Macro {
        Macro
    }
}

impl TokenParserExtension for Macro {
    fn parse<'p, 'c>(
        &self,
        parser: &mut Parser<'p, 'c>,
        token: TokenRef<'c>,
    ) -> TemplateResult<Option<Body<'c>>> {
        let name = parser.expect_name()?;
        let arguments = parse_named_arguments(parser, true)?;
        let line = token.line;

        parser.expect(TokenValueRef::BlockEnd)?;
        parser.push_local_scope();

        let body = subparse(parser, |token| match token.value {
            TokenValueRef::Name("endmacro") => Some(BlockEnd { drop_needle: true }),
            _ => None,
        })?;
        let token = parser.current()?;
        if let TokenValueRef::Name(value) = token.value {
            parser.next()?;

            if value != name {
                return Err(CoreTemplateError::ExpectedEndmacroName {
                    given: value.into(),
                    expected: name.into(),
                }
                .at(parser.current()?.line)
                .into());
            }
        }

        parser.pop_local_scope();
        parser.expect(TokenValueRef::BlockEnd)?;

        Ok(Some(Body::Macro {
            name,
            body: Box::new(body),
            arguments,
            line,
        }))
    }

    fn get_tag(&self) -> &'static str {
        "macro"
    }
}
