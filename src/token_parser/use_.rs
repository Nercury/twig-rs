//! Twig: `Twig\TokenParser\UseTokenParser` (TWIG/src/TokenParser/UseTokenParser.php).

use super::{TokenParser, expect_block_end, syntax_error, token_value};
use crate::ast::{Expr, ExprKind, Node, NodeKey, Nodes};
use crate::error::Result;
use crate::lexer::{Token, TokenType};
use crate::parser::Parser;

/// Imports the blocks of another template (horizontal reuse): `{% use 'template' [with block [as alias], ...] %}`.
///
/// The trait is registered with [`Parser::add_trait`] as `Nodes(['template' => expr, 'targets' => Nodes(block =>
/// ConstantExpression(alias))])`; the tag node is a `ConfigNode`.
#[derive(Debug, Clone, Copy, Default)]
pub struct UseTokenParser;

impl TokenParser for UseTokenParser {
    fn tag(&self) -> &str {
        "use"
    }

    /// Twig: `UseTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let template = parser.parse_expression(0)?;
        if !matches!(template.kind, ExprKind::Constant(_)) {
            return Err(syntax_error(
                parser,
                "The template references in a \"use\" statement must be a string.",
                parser.current_token().line(),
            ));
        }

        // `$targets[$name] = new ConstantExpression($alias, -1)`: a repeated block replaces the earlier entry in place
        let mut targets = Nodes::new();
        if parser
            .stream_mut()
            .next_if_value(TokenType::Name, "with")?
            .is_some()
        {
            loop {
                let name = token_value(&parser.stream_mut().expect(TokenType::Name, None, None)?);
                let mut alias = name.clone();
                if parser
                    .stream_mut()
                    .next_if_value(TokenType::Name, "as")?
                    .is_some()
                {
                    alias =
                        token_value(&parser.stream_mut().expect(TokenType::Name, None, None)?);
                }
                targets.set(
                    NodeKey::Name(name),
                    Node::expression(Expr::constant(crate::ast::ConstantValue::String(alias), -1)),
                );
                if parser
                    .stream_mut()
                    .next_if_value(TokenType::Punctuation, ",")?
                    .is_none()
                {
                    break;
                }
            }
        }

        expect_block_end(parser)?;

        let mut trait_ = Nodes::new();
        trait_.set(
            NodeKey::Name(b"template".to_vec()),
            Node::expression(template),
        );
        trait_.set(NodeKey::Name(b"targets".to_vec()), Node::nodes(targets, 0));
        parser.add_trait(Node::nodes(trait_, 0));

        Ok(Some(Node::config(token.line())))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\UseTokenParser"
    }
}
