//! Twig: `Twig\TokenParser\ForTokenParser` (TWIG/src/TokenParser/ForTokenParser.php).

use super::{TokenParser, expect_block_end, parse_assignment_expression, subparse_until};
use crate::ast::{Expr, Node, NodeKind};
use crate::error::{Error, Result};
use crate::lexer::{Token, TokenType, TokenValues};
use crate::parser::Parser;

/// Loops over each item of a sequence: `{% for [key,] value in seq %}...[{% else %}...]{% endfor %}`. With more than
/// two targets, the third and later ones are ignored.
#[derive(Debug, Clone, Copy, Default)]
pub struct ForTokenParser;

impl TokenParser for ForTokenParser {
    fn tag(&self) -> &str {
        "for"
    }

    /// Twig: `ForTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let lineno = token.line();
        let targets = parse_assignment_expression(parser)?;
        parser
            .stream_mut()
            .expect(TokenType::Operator, Some("in"), None)?;
        let seq = parser.parse_expression(0)?;
        expect_block_end(parser)?;

        let body = subparse_until(
            parser,
            self.tag(),
            TokenValues::AnyOf(&["else", "endfor"]),
            false,
        )?;
        let else_ = if parser.stream_mut().next()?.value().loosely_equals(b"else") {
            let else_lineno = parser.current_token().line();
            expect_block_end(parser)?;
            let body = subparse_until(parser, self.tag(), TokenValues::One(b"endfor"), true)?;
            Some(Node::for_else(body, else_lineno))
        } else {
            None
        };
        expect_block_end(parser)?;

        let count = targets.len();
        let mut targets = targets
            .into_entries()
            .into_iter()
            .map(|(_, node)| into_expr(node));
        let (key_target, value_target) = if count > 1 {
            let key = targets.next().transpose()?;
            let value = targets.next().transpose()?;
            (key, value)
        } else {
            (
                Some(Expr::assign_context_variable("_key", lineno)?),
                targets.next().transpose()?,
            )
        };
        let (Some(key_target), Some(value_target)) = (key_target, value_target) else {
            return Err(Error::logic(
                "Node \"0\" does not exist for Node \"Twig\\Node\\Nodes\".",
            ));
        };

        Ok(Some(Node::for_(
            key_target,
            value_target,
            seq,
            body,
            else_,
            lineno,
        )))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\ForTokenParser"
    }
}

/// The expression of an assignment target node.
fn into_expr(node: Node) -> Result<Expr> {
    match node.kind {
        NodeKind::Expression(expr) => Ok(*expr),
        _ => Err(Error::logic("An assignment target must be an expression.")),
    }
}
