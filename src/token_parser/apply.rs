//! Twig: `Twig\TokenParser\ApplyTokenParser` (TWIG/src/TokenParser/ApplyTokenParser.php).

use twig_value::PhpError;

use super::{TokenParser, expect_block_end, subparse_until};
use crate::ast::{Expr, Node, Nodes, TempNameInput};
use crate::error::{Error, Result};
use crate::lexer::{Token, TokenType, TokenValues};
use crate::parser::Parser;
use crate::parser::expression::ExpressionParserEntry;
use crate::parser::expression::infix::FilterExpressionParser;

/// Applies filters on a section of a template: `{% apply upper|escape %}...{% endapply %}`.
///
/// The node is `Nodes([SetNode(capture, $ref, $body), PrintNode($filters($ref))])`, where `$ref` is a
/// `LocalVariable` without a name; both occurrences are the same PHP object (the compiler names it once), which the
/// `apply` tag of the returned `Nodes` identifies.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApplyTokenParser;

impl TokenParser for ApplyTokenParser {
    fn tag(&self) -> &str {
        "apply"
    }

    /// Twig: `ApplyTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let lineno = token.line();
        let reference = Expr::local_variable(TempNameInput::Null, lineno)?;
        let mut filter = reference.clone();
        let env = parser.environment();
        let Some((_, ExpressionParserEntry::Infix(op))) = env
            .expression_parsers()
            .by_class::<FilterExpressionParser>()
        else {
            // `getByClass()` returns null and `$op->parse()` is PHP's `Error` on null (`zend_invalid_method_call()`,
            // PHPSRC/Zend/zend_execute.c), which `compileSource()` lets through unwrapped
            return Err(Error::php(PhpError::error(
                "Call to a member function parse() on null",
            )));
        };
        let op = std::sync::Arc::clone(op);
        loop {
            let current = parser.current_token().clone();
            filter = op.parse(parser, filter, &current)?;
            if !parser.stream().test_value(TokenType::Operator, "|") {
                break;
            }
            parser.stream_mut().next()?;
        }

        expect_block_end(parser)?;
        let body = subparse_until(parser, self.tag(), TokenValues::One(b"endapply"), true)?;
        expect_block_end(parser)?;

        Ok(Some(Node::nodes(
            Nodes::from_list([
                Node::set(true, Node::expression(reference), body, lineno),
                Node::print(filter, lineno),
            ]),
            lineno,
        )))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\ApplyTokenParser"
    }
}
