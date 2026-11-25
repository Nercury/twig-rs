//! Twig: `Twig\TokenParser\SetTokenParser` (TWIG/src/TokenParser/SetTokenParser.php).

use super::{
    TokenParser, expect_block_end, parse_assignment_expression, subparse_until, syntax_error,
};
use crate::ast::{Node, Nodes};
use crate::error::Result;
use crate::lexer::{Token, TokenType, TokenValues};
use crate::parser::Parser;

/// Defines variables: `{% set a, b = expr, expr %}` or the capture `{% set a %}...{% endset %}`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SetTokenParser;

impl TokenParser for SetTokenParser {
    fn tag(&self) -> &str {
        "set"
    }

    /// Twig: `SetTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let lineno = token.line();
        let names = parse_assignment_expression(parser)?;
        let mut capture = false;
        let values = if parser
            .stream_mut()
            .next_if_value(TokenType::Operator, "=")?
            .is_some()
        {
            let values = parse_multitarget_expression(parser)?;
            expect_block_end(parser)?;
            if names.len() != values.len() {
                return Err(syntax_error(
                    parser,
                    "When using set, you must have the same number of variables and assignments.",
                    parser.current_token().line(),
                ));
            }
            Node::nodes(values, 0)
        } else {
            capture = true;
            if names.len() > 1 {
                return Err(syntax_error(
                    parser,
                    "When using set with a block, you cannot have a multi-target.",
                    parser.current_token().line(),
                ));
            }
            expect_block_end(parser)?;
            let values = subparse_until(parser, self.tag(), TokenValues::One(b"endset"), true)?;
            expect_block_end(parser)?;
            values
        };

        Ok(Some(Node::set(
            capture,
            Node::nodes(names, 0),
            values,
            lineno,
        )))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\SetTokenParser"
    }
}

/// `expr (, expr)*`. Twig: `SetTokenParser::parseMultitargetExpression()`.
fn parse_multitarget_expression(parser: &mut Parser<'_>) -> Result<Nodes> {
    let mut targets = Nodes::new();
    loop {
        targets.push(Node::expression(parser.parse_expression(0)?));
        if parser
            .stream_mut()
            .next_if_value(TokenType::Punctuation, ",")?
            .is_none()
        {
            break;
        }
    }
    Ok(targets)
}
