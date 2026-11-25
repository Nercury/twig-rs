//! Twig: `Twig\TokenParser\WithTokenParser` (TWIG/src/TokenParser/WithTokenParser.php).

use super::{TokenParser, expect_block_end, subparse_until};
use crate::ast::Node;
use crate::error::Result;
use crate::lexer::{Token, TokenType, TokenValues};
use crate::parser::Parser;

/// Creates a nested scope: `{% with [expr [only]] %}...{% endwith %}`.
#[derive(Debug, Clone, Copy, Default)]
pub struct WithTokenParser;

impl TokenParser for WithTokenParser {
    fn tag(&self) -> &str {
        "with"
    }

    /// Twig: `WithTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let mut variables = None;
        let mut only = false;
        if !parser.stream().test(TokenType::BlockEnd) {
            variables = Some(parser.parse_expression(0)?);
            only = parser
                .stream_mut()
                .next_if_value(TokenType::Name, "only")?
                .is_some();
        }

        expect_block_end(parser)?;
        let body = subparse_until(parser, self.tag(), TokenValues::One(b"endwith"), true)?;
        expect_block_end(parser)?;

        Ok(Some(Node::with(body, variables, only, token.line())))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\WithTokenParser"
    }
}
