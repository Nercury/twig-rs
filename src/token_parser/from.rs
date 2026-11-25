//! Twig: `Twig\TokenParser\FromTokenParser` (TWIG/src/TokenParser/FromTokenParser.php).

use super::{TokenParser, expect_block_end, token_value};
use crate::ast::{Expr, Node, TempNameInput};
use crate::error::Result;
use crate::lexer::{Token, TokenType};
use crate::parser::Parser;

/// Imports macros: `{% from expr import name [as alias] (, name [as alias])* %}`.
///
/// Each alias becomes a `function` imported symbol mapping the alias to the macro name and the import's macro
/// variable (one variable without a name, shared by the `ImportNode` and every call it enables); a macro imported
/// twice keeps only its last alias.
#[derive(Debug, Clone, Copy, Default)]
pub struct FromTokenParser;

impl TokenParser for FromTokenParser {
    fn tag(&self) -> &str {
        "from"
    }

    /// Twig: `FromTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let macro_ = parser.parse_expression(0)?;
        parser
            .stream_mut()
            .expect(TokenType::Name, Some("import"), None)?;

        // `$targets[$name] = $alias`: keyed by macro name, a later alias replaces an earlier one in place
        let mut targets: Vec<(Vec<u8>, Expr)> = Vec::new();
        loop {
            let name = token_value(&parser.stream_mut().expect(TokenType::Name, None, None)?);
            let alias = if parser
                .stream_mut()
                .next_if_value(TokenType::Name, "as")?
                .is_some()
            {
                let alias =
                    token_value(&parser.stream_mut().expect(TokenType::Name, None, None)?);
                Expr::assign_context_variable(alias, token.line())?
            } else {
                Expr::assign_context_variable(name.clone(), token.line())?
            };
            match targets.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = alias,
                None => targets.push((name, alias)),
            }
            if parser
                .stream_mut()
                .next_if_value(TokenType::Punctuation, ",")?
                .is_none()
            {
                break;
            }
        }

        expect_block_end(parser)?;

        let id = parser.next_macro_variable_id();
        let variable = Expr::macro_variable(TempNameInput::Null, token.line(), id)?;
        let internal_ref = Expr::assign_macro_variable(variable, parser.is_main_scope());
        let node = Node::import(macro_, internal_ref.clone(), token.line());

        // Each alias is registered as a "function" imported symbol mapping the local name to the macro name and the
        // internal macro variable: a bare call like "my_macro()" is syntactically a function call, so
        // FunctionExpressionParser resolves it through this symbol into a MacroReferenceExpression.
        for (name, alias) in targets {
            let alias = alias.context_variable_name().unwrap_or_default().to_vec();
            parser.add_imported_symbol("function", alias, Some(name), Some(internal_ref.clone()));
        }

        Ok(Some(node))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\FromTokenParser"
    }
}
