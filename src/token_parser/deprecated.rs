//! Twig: `Twig\TokenParser\DeprecatedTokenParser` (TWIG/src/TokenParser/DeprecatedTokenParser.php).

use super::{TokenParser, expect_block_end, syntax_error, token_value};
use crate::ast::{Node, NodeKind};
use crate::error::Result;
use crate::lexer::{Token, TokenType};
use crate::parser::Parser;

/// Deprecates a section of a template: `{% deprecated 'message' [package = expr] [version = expr] %}` (the options
/// in any order, repeatable).
#[derive(Debug, Clone, Copy, Default)]
pub struct DeprecatedTokenParser;

impl TokenParser for DeprecatedTokenParser {
    fn tag(&self) -> &str {
        "deprecated"
    }

    /// Twig: `DeprecatedTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let expr = parser.parse_expression(0)?;
        let mut node = Node::deprecated(expr, token.line());

        while parser.stream().test(TokenType::Name) {
            let k = token_value(parser.current_token());
            parser.stream_mut().next()?;
            parser
                .stream_mut()
                .expect(TokenType::Operator, Some("="), None)?;
            // `switch ($k)`: a name is never a numeric string, so the loose comparison is plain equality.
            match k.as_slice() {
                b"package" => {
                    let value = parser.parse_expression(0)?;
                    if let NodeKind::Deprecated(deprecated) = &mut node.kind {
                        deprecated.set_package(value);
                    }
                }
                b"version" => {
                    let value = parser.parse_expression(0)?;
                    if let NodeKind::Deprecated(deprecated) = &mut node.kind {
                        deprecated.set_version(value);
                    }
                }
                _ => {
                    return Err(syntax_error(
                        parser,
                        format!("Unknown \"{}\" option.", String::from_utf8_lossy(&k)),
                        parser.current_token().line(),
                    ));
                }
            }
        }

        expect_block_end(parser)?;

        Ok(Some(node))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\DeprecatedTokenParser"
    }
}
