//! Twig: `Twig\TokenParser\FlushTokenParser` (TWIG/src/TokenParser/FlushTokenParser.php).

use super::{TokenParser, expect_block_end};
use crate::ast::Node;
use crate::error::Result;
use crate::lexer::Token;
use crate::parser::Parser;

/// Flushes the output to the client: `{% flush %}`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FlushTokenParser;

impl TokenParser for FlushTokenParser {
    fn tag(&self) -> &str {
        "flush"
    }

    /// Twig: `FlushTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        expect_block_end(parser)?;
        Ok(Some(Node::flush(token.line())))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\FlushTokenParser"
    }
}
