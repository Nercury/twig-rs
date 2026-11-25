//! Twig: `Twig\TokenParser\IncludeTokenParser` (TWIG/src/TokenParser/IncludeTokenParser.php).

use super::{TokenParser, expect_block_end};
use crate::ast::{Expr, Node};
use crate::error::Result;
use crate::lexer::{Token, TokenType};
use crate::parser::Parser;

/// Includes a template: `{% include expr [ignore missing] [with expr] [only] %}` (the options in this order).
#[derive(Debug, Clone, Copy, Default)]
pub struct IncludeTokenParser;

impl TokenParser for IncludeTokenParser {
    fn tag(&self) -> &str {
        "include"
    }

    /// Twig: `IncludeTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let expr = parser.parse_expression(0)?;
        let arguments = parse_arguments(parser)?;
        Ok(Some(Node::include(
            expr,
            arguments.variables,
            arguments.only,
            arguments.ignore_missing,
            token.line(),
        )))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\IncludeTokenParser"
    }
}

/// The options of `include` and `embed`.
pub(crate) struct IncludeArguments {
    pub(crate) variables: Option<Expr>,
    pub(crate) only: bool,
    pub(crate) ignore_missing: bool,
}

/// Parses `[ignore missing] [with expr] [only]` and the end of the tag. Twig: `IncludeTokenParser::parseArguments()`.
pub(crate) fn parse_arguments(parser: &mut Parser<'_>) -> Result<IncludeArguments> {
    let mut ignore_missing = false;
    if parser
        .stream_mut()
        .next_if_value(TokenType::Name, "ignore")?
        .is_some()
    {
        parser
            .stream_mut()
            .expect(TokenType::Name, Some("missing"), None)?;
        ignore_missing = true;
    }

    let mut variables = None;
    if parser
        .stream_mut()
        .next_if_value(TokenType::Name, "with")?
        .is_some()
    {
        variables = Some(parser.parse_expression(0)?);
    }

    let only = parser
        .stream_mut()
        .next_if_value(TokenType::Name, "only")?
        .is_some();

    expect_block_end(parser)?;

    Ok(IncludeArguments {
        variables,
        only,
        ignore_missing,
    })
}
