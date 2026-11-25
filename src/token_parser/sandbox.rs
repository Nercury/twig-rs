//! Twig: `Twig\TokenParser\SandboxTokenParser` (TWIG/src/TokenParser/SandboxTokenParser.php).

use super::{TokenParser, ctype_space, expect_block_end, subparse_until, syntax_error};
use crate::ast::{Node, NodeKind, NodeMut};
use crate::deprecation::Deprecation;
use crate::error::Result;
use crate::lexer::{Token, TokenValues};
use crate::parser::Parser;

/// Marks a section of a template as untrusted code that must be evaluated in the sandbox mode (deprecated since 3.15):
/// `{% sandbox %}{% include ... %}{% endsandbox %}`. Only `include` (and `embed`) tags are allowed in the body; they
/// get the `sandboxed` attribute.
///
/// Registered by [`SandboxExtension`](crate::extension::SandboxExtension).
#[derive(Debug, Clone, Copy, Default)]
pub struct SandboxTokenParser;

impl TokenParser for SandboxTokenParser {
    fn tag(&self) -> &str {
        "sandbox"
    }

    /// Twig: `SandboxTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let message = format!(
            "The \"sandbox\" tag is deprecated in \"{}\" at line {}.",
            parser.stream().source_context().name(),
            token.line()
        );
        parser.report_deprecation(Deprecation::twig("3.15", message));
        expect_block_end(parser)?;
        let mut body = subparse_until(parser, self.tag(), TokenValues::One(b"endsandbox"), true)?;
        expect_block_end(parser)?;

        // in a sandbox tag, only include tags are allowed
        if let Some(sandboxed) = sandboxed_attribute(&mut body) {
            *sandboxed = Some(true);
        } else {
            // `foreach ($body as $node)`: the children of the body (of the single node, when the body is one node)
            let mut offending = None;
            for (_, mut child) in body.children_mut() {
                if let NodeMut::Node(node) = &mut child {
                    if let NodeKind::Text(text) = &node.kind
                        && ctype_space(&text.data)
                    {
                        continue;
                    }
                    if let Some(sandboxed) = sandboxed_attribute(node) {
                        *sandboxed = Some(true);
                        continue;
                    }
                }
                offending = Some(child.as_ref().template_line());
                break;
            }
            if let Some(line) = offending {
                return Err(syntax_error(
                    parser,
                    "Only \"include\" tags are allowed within a \"sandbox\" section.",
                    line,
                ));
            }
        }

        Ok(Some(Node::sandbox(body, token.line())))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\SandboxTokenParser"
    }
}

/// The `sandboxed` attribute of an `IncludeNode` (or of its subclass `EmbedNode`).
fn sandboxed_attribute(node: &mut Node) -> Option<&mut Option<bool>> {
    match &mut node.kind {
        NodeKind::Include(include) => Some(&mut include.sandboxed),
        NodeKind::Embed(embed) => Some(&mut embed.sandboxed),
        _ => None,
    }
}
