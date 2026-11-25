//! Twig: `Twig\TokenParser\ExtendsTokenParser` (TWIG/src/TokenParser/ExtendsTokenParser.php).

use super::{TokenParser, expect_block_end};
use crate::ast::Node;
use crate::error::Result;
use crate::lexer::Token;
use crate::parser::Parser;

/// Extends a template by another one: `{% extends expr %}`. A second `extends` is ignored here and reported by the
/// [`CorrectnessNodeVisitor`](crate::visitor::CorrectnessNodeVisitor); the tag node is a `ConfigNode`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExtendsTokenParser;

impl TokenParser for ExtendsTokenParser {
    fn tag(&self) -> &str {
        "extends"
    }

    /// Twig: `ExtendsTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let parent = parser.parse_expression(0)?;
        parser.set_parent(parent, false)?;
        expect_block_end(parser)?;
        Ok(Some(Node::config(token.line())))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\ExtendsTokenParser"
    }
}
