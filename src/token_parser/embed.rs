//! Twig: `Twig\TokenParser\EmbedTokenParser` (TWIG/src/TokenParser/EmbedTokenParser.php).

use super::TokenParser;
use super::include::parse_arguments;
use crate::ast::Node;
use crate::error::{Error, Result};
use crate::lexer::{Token, TokenType, TokenValue};
use crate::parser::{EndTagTest, Parser};

/// Embeds a template: `{% embed expr [ignore missing] [with expr] [only] %}...{% endembed %}`.
///
/// The body is parsed as a child template of `expr` (a nested [`Parser::parse_embedded_template`], with the four
/// tokens of `{% extends "__parent__" %}` injected first so that `parent()` works), registered with
/// [`Parser::embed_template`]; the tag node is an `EmbedNode` with the template's index.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmbedTokenParser;

impl TokenParser for EmbedTokenParser {
    fn tag(&self) -> &str {
        "embed"
    }

    /// Twig: `EmbedTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let parent = parser.parse_expression(0)?;
        let arguments = parse_arguments(parser)?;

        // inject a fake parent to make the parent() function work
        let line = token.line();
        parser.stream_mut().inject_tokens([
            Token::new(TokenType::BlockStart, TokenValue::empty(), line),
            Token::new(TokenType::Name, "extends", line),
            Token::new(TokenType::String, "__parent__", line),
            Token::new(TokenType::BlockEnd, TokenValue::empty(), line),
        ]);

        let test = |token: &Token| token.test_value(TokenType::Name, "endembed");
        let module = parser.parse_embedded_template(
            parent,
            &EndTagTest {
                tag: Some(self.tag()),
                test: &test,
            },
        )?;
        let name = module
            .as_module()
            .map(|module| module.source.name().to_owned())
            .ok_or_else(|| Error::logic("An embedded template must be a module."))?;
        let index = parser.embed_template(module);

        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;

        Ok(Some(Node::embed(
            name,
            index,
            arguments.variables,
            arguments.only,
            arguments.ignore_missing,
            line,
        )))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\EmbedTokenParser"
    }
}
