//! Twig: `Twig\TokenParser\IfTokenParser` (TWIG/src/TokenParser/IfTokenParser.php).

use super::{TokenParser, expect_block_end, subparse_until, syntax_error};
use crate::ast::{Node, Nodes};
use crate::error::Result;
use crate::lexer::{Token, TokenValues};
use crate::parser::Parser;

/// Tests a condition: `{% if expr %}...[{% elseif expr %}...]*[{% else %}...]{% endif %}`.
#[derive(Debug, Clone, Copy, Default)]
pub struct IfTokenParser;

/// `decideIfFork()`.
const FORK: TokenValues<'static> = TokenValues::AnyOf(&["elseif", "else", "endif"]);

impl TokenParser for IfTokenParser {
    fn tag(&self) -> &str {
        "if"
    }

    /// Twig: `IfTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let lineno = token.line();
        let expr = parser.parse_expression(0)?;
        expect_block_end(parser)?;
        let body = subparse_until(parser, self.tag(), FORK, false)?;
        let mut tests = Nodes::from_list([Node::expression(expr), body]);
        let mut else_ = None;

        loop {
            // `switch ($stream->next()->getValue())` (loose comparisons, in case order)
            let next = parser.stream_mut().next()?;
            let value = next.value();
            if value.loosely_equals(b"else") {
                expect_block_end(parser)?;
                else_ = Some(subparse_until(
                    parser,
                    self.tag(),
                    TokenValues::AnyOf(&["endif"]),
                    false,
                )?);
            } else if value.loosely_equals(b"elseif") {
                let expr = parser.parse_expression(0)?;
                expect_block_end(parser)?;
                let body = subparse_until(parser, self.tag(), FORK, false)?;
                tests.push(Node::expression(expr));
                tests.push(body);
            } else if value.loosely_equals(b"endif") {
                break;
            } else {
                return Err(syntax_error(
                    parser,
                    format!(
                        "Unexpected end of template. Twig was looking for the following tags \"else\", \"elseif\", or \"endif\" to close the \"if\" block started at line {lineno})."
                    ),
                    parser.current_token().line(),
                ));
            }
        }

        expect_block_end(parser)?;

        Ok(Some(Node::if_(tests, else_, lineno)))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\IfTokenParser"
    }
}
