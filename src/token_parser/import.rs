//! Twig: `Twig\TokenParser\ImportTokenParser` (TWIG/src/TokenParser/ImportTokenParser.php).

use super::{TokenParser, expect_block_end, token_value};
use crate::ast::{Expr, Node};
use crate::error::Result;
use crate::lexer::{Token, TokenType};
use crate::parser::Parser;

/// Imports macros: `{% import expr as name %}`; `name` becomes a `template` imported symbol, so `name.macro()` is a
/// macro call.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImportTokenParser;

impl TokenParser for ImportTokenParser {
    fn tag(&self) -> &str {
        "import"
    }

    /// Twig: `ImportTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let macro_ = parser.parse_expression(0)?;
        parser
            .stream_mut()
            .expect(TokenType::Name, Some("as"), None)?;
        let name = token_value(&parser.stream_mut().expect(TokenType::Name, None, None)?);
        let id = parser.next_macro_variable_id();
        let variable = Expr::macro_variable(name.clone(), token.line(), id)?;
        let var = Expr::assign_macro_variable(variable, parser.is_main_scope());
        expect_block_end(parser)?;
        parser.add_imported_symbol("template", name, None, None);
        Ok(Some(Node::import(macro_, var, token.line())))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\ImportTokenParser"
    }
}
