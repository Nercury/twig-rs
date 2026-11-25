//! Twig: `Twig\TokenParser\AutoEscapeTokenParser` (TWIG/src/TokenParser/AutoEscapeTokenParser.php).

use super::{TokenParser, expect_block_end, subparse_until, syntax_error};
use crate::ast::{ConstantValue, ExprKind, Node};
use crate::error::Result;
use crate::lexer::{Token, TokenType, TokenValues};
use crate::parser::Parser;

/// Marks a section of a template to be escaped or not: `{% autoescape 'js' %}...{% endautoescape %}`. Any constant
/// is accepted as the strategy (the escaper checks it later); without one the strategy is `html`.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutoEscapeTokenParser;

impl TokenParser for AutoEscapeTokenParser {
    fn tag(&self) -> &str {
        "autoescape"
    }

    /// Twig: `AutoEscapeTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let lineno = token.line();
        let value = if parser.stream().test(TokenType::BlockEnd) {
            ConstantValue::string("html")
        } else {
            let expr = parser.parse_expression(0)?;
            let ExprKind::Constant(constant) = expr.kind else {
                return Err(syntax_error(
                    parser,
                    "An escaping strategy must be a string or false.",
                    parser.current_token().line(),
                ));
            };
            constant.value
        };

        expect_block_end(parser)?;
        let body = subparse_until(parser, self.tag(), TokenValues::One(b"endautoescape"), true)?;
        expect_block_end(parser)?;

        Ok(Some(Node::auto_escape(value, body, lineno)))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\AutoEscapeTokenParser"
    }
}
