//! Twig: `Twig\TokenParser\TypesTokenParser` (TWIG/src/TokenParser/TypesTokenParser.php).

use super::{TokenParser, expect_block_end, token_value};
use crate::ast::{Node, NodeKey, NodeKind, Nodes, TypeNode, TypesNode};
use crate::error::Result;
use crate::lexer::{Token, TokenType};
use crate::parser::{Parser, add_node_documentation};

/// Declares variable types (experimental, no effect on rendering): `{% types {name: 'type', optional?: 'type'} %}`
/// (the braces are optional).
#[derive(Debug, Clone, Copy, Default)]
pub struct TypesTokenParser;

impl TokenParser for TypesTokenParser {
    fn tag(&self) -> &str {
        "types"
    }

    /// Twig: `TypesTokenParser::parse()`: a `TypesNode` whose `mapping` attribute lists the declarations and whose
    /// children are the `TypeNode`s at the lines of their names.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let (mapping, types) = parse_simple_mapping_expression(parser)?;
        expect_block_end(parser)?;
        Ok(Some(Node::new(
            NodeKind::Types(Box::new(TypesNode { types, mapping })),
            token.line(),
        )))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\TypesTokenParser"
    }
}

/// The declarations: (name, type, optional) in order (a repeated name replaces the earlier declaration in place) and
/// the `TypeNode`s keyed by name. Twig: `TypesTokenParser::parseSimpleMappingExpression()`.
#[allow(clippy::type_complexity, reason = "mirrors the PHP return value")]
fn parse_simple_mapping_expression(
    parser: &mut Parser<'_>,
) -> Result<(Vec<(Vec<u8>, Vec<u8>, bool)>, Nodes)> {
    let enclosed = parser
        .stream_mut()
        .next_if_value(TokenType::Punctuation, "{")?
        .is_some();
    let mut types: Vec<(Vec<u8>, Vec<u8>, bool)> = Vec::new();
    let mut nodes = Nodes::new();
    let mut first = true;
    let at_end = |parser: &Parser<'_>| {
        parser.stream().test_value(TokenType::Punctuation, "}")
            || parser.stream().test(TokenType::BlockEnd)
    };
    while !at_end(parser) {
        if !first {
            parser.stream_mut().expect(
                TokenType::Punctuation,
                Some(","),
                Some("A type string must be followed by a comma"),
            )?;
            // trailing ,?
            if at_end(parser) {
                break;
            }
        }
        first = false;

        let name_token = parser.stream_mut().expect(TokenType::Name, None, None)?;
        let is_optional = if parser
            .stream_mut()
            .next_if_value(TokenType::Operator, "?:")?
            .is_some()
        {
            true
        } else {
            let is_optional = parser
                .stream_mut()
                .next_if_value(TokenType::Operator, "?")?
                .is_some();
            parser.stream_mut().expect(
                TokenType::Punctuation,
                Some(":"),
                Some("A type name must be followed by a colon (:)"),
            )?;
            is_optional
        };

        let value_token = parser.stream_mut().expect(TokenType::String, None, None)?;
        let name = token_value(&name_token);
        let type_ = token_value(&value_token);
        match types.iter_mut().find(|(n, _, _)| *n == name) {
            Some(slot) => *slot = (name.clone(), type_.clone(), is_optional),
            None => types.push((name.clone(), type_.clone(), is_optional)),
        }
        let mut node = Node::new(
            NodeKind::Type(TypeNode {
                name: name.clone(),
                type_,
                optional: is_optional,
            }),
            name_token.line(),
        );
        add_node_documentation(&mut node, &[&name_token]);
        nodes.set(NodeKey::Name(name), node);
    }

    if enclosed {
        parser.stream_mut().expect(
            TokenType::Punctuation,
            Some("}"),
            Some("An opened mapping is not properly closed"),
        )?;
    }

    Ok((types, nodes))
}
