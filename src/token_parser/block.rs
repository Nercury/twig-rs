//! Twig: `Twig\TokenParser\BlockTokenParser` (TWIG/src/TokenParser/BlockTokenParser.php).

use super::{TokenParser, expect_block_end, subparse_until, syntax_error, token_value};
use crate::ast::{Node, NodeKind, Nodes};
use crate::error::Result;
use crate::lexer::{Token, TokenType, TokenValues};
use crate::parser::{DocumentationTarget, Parser};

/// Marks a section of a template as being reusable: `{% block name %}...{% endblock [name] %}` or the shorthand
/// `{% block name expr %}`.
///
/// The block is registered with [`Parser::set_block`] (it becomes the documentation target of the tag) and the tag
/// node is a `BlockReferenceNode`.
#[derive(Debug, Clone, Copy, Default)]
pub struct BlockTokenParser;

impl TokenParser for BlockTokenParser {
    fn tag(&self) -> &str {
        "block"
    }

    /// Twig: `BlockTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let lineno = token.line();
        let name = token_value(&parser.stream_mut().expect(TokenType::Name, None, None)?);
        parser.set_block(
            name.clone(),
            Node::block(name.clone(), Node::empty(0), lineno),
        )?;
        parser.set_documentation_target(DocumentationTarget::Block(name.clone()))?;
        parser.push_local_scope();
        parser.push_block_stack(name.clone());

        let body = if parser.stream_mut().next_if(TokenType::BlockEnd)?.is_some() {
            let body = subparse_until(parser, self.tag(), TokenValues::One(b"endblock"), true)?;
            if let Some(end) = parser.stream_mut().next_if(TokenType::Name)?
                && !end.value().loosely_equals(&name)
            {
                return Err(syntax_error(
                    parser,
                    format!(
                        "Expected endblock for block \"{}\" (but \"{}\" given).",
                        String::from_utf8_lossy(&name),
                        String::from_utf8_lossy(&token_value(&end))
                    ),
                    parser.current_token().line(),
                ));
            }
            body
        } else {
            let expr = parser.parse_expression(0)?;
            Node::nodes(Nodes::from_list([Node::print(expr, lineno)]), 0)
        };
        expect_block_end(parser)?;

        if let Some(NodeKind::Block(block)) = parser.block_mut(&name).map(|node| &mut node.kind) {
            block.body = body;
        }
        parser.pop_block_stack();
        parser.pop_local_scope();

        Ok(Some(Node::block_reference(name, lineno)))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\BlockTokenParser"
    }
}
