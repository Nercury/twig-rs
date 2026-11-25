//! Twig: `Twig\TokenParser\DoTokenParser` (TWIG/src/TokenParser/DoTokenParser.php).

use super::{TokenParser, expect_block_end};
use crate::ast::Node;
use crate::error::Result;
use crate::lexer::Token;
use crate::parser::Parser;

/// Evaluates an expression, discarding the returned value: `{% do expr %}`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DoTokenParser;

impl TokenParser for DoTokenParser {
    fn tag(&self) -> &str {
        "do"
    }

    /// Twig: `DoTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let expr = parser.parse_expression(0)?;
        expect_block_end(parser)?;
        Ok(Some(Node::do_(expr, token.line())))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\DoTokenParser"
    }
}
