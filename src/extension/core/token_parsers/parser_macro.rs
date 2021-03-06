use tokens::{ TokenRef, TokenValueRef };
use nodes::{ Parser, TokenParserExtension };
use nodes::body::Body;
use error::TemplateResult;
use nodes::expr_parser::parse_named_arguments;
use nodes::body_parser::{ subparse, BlockEnd };
use extension::core::error::*;

pub struct Macro;

impl Macro {
    pub fn new() -> Macro {
        Macro
    }
}

impl TokenParserExtension for Macro {
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> TemplateResult<Option<Body<'c>>>
    {
        trace!("Macro::parse, {:?}", token);

        let name = try!(parser.expect_name());
        let arguments = try!(parse_named_arguments(parser, true));
        let line = token.line;

        try!(parser.expect(TokenValueRef::BlockEnd));
        parser.push_local_scope();

        let body = try!(subparse(parser, |token| match token.value {
            TokenValueRef::Name("endmacro") => Some(BlockEnd { drop_needle: true }),
            _ => None,
        }));
        let token = try!(parser.current());
        if let TokenValueRef::Name(value) = token.value {
            try!(parser.next());

            if value != name {
                return Err(
                    CoreTemplateError::ExpectedEndmacroName { given: value.into(), expected: name.into() }
                        .at(try!(parser.current()).line)
                        .into()
                )
            }
        }

        parser.pop_local_scope();
        try!(parser.expect(TokenValueRef::BlockEnd));

        Ok(Some(Body::Macro {
            name: name,
            body: Box::new(body),
            arguments: arguments,
            line: line,
        }))
    }

    fn get_tag(&self) -> &'static str {
        "macro"
    }
}
