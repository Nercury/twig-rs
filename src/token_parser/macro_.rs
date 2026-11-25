//! Twig: `Twig\TokenParser\MacroTokenParser` (TWIG/src/TokenParser/MacroTokenParser.php).

use super::{TokenParser, expect_block_end, subparse_until, syntax_error, token_value};
use crate::ast::{
    ArrayExpr, ConstantValue, Expr, ExprKind, Node, NodeRef, Nodes, TempName, UnaryOp,
};
use crate::error::Result;
use crate::lexer::{Token, TokenType, TokenValues};
use crate::parser::{DocumentationTarget, Parser, add_expr_documentation};

/// Defines a macro: `{% macro name(arg [= constant], ..., [...variadic]) %}...{% endmacro [name] %}`.
///
/// The `MacroNode` is registered with [`Parser::set_macro`] (it becomes the documentation target of the tag) and the
/// tag node is a `MacroDeclarationNode`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacroTokenParser;

impl TokenParser for MacroTokenParser {
    fn tag(&self) -> &str {
        "macro"
    }

    /// Twig: `MacroTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let lineno = token.line();
        let name = token_value(&parser.stream_mut().expect(TokenType::Name, None, None)?);
        let (arguments, variadic_name) = parse_definition(parser, &name)?;

        expect_block_end(parser)?;
        parser.push_local_scope();
        let body = subparse_until(parser, self.tag(), TokenValues::One(b"endmacro"), true)?;
        if let Some(end) = parser.stream_mut().next_if(TokenType::Name)?
            && !end.value().loosely_equals(&name)
        {
            return Err(syntax_error(
                parser,
                format!(
                    "Expected endmacro for macro \"{}\" (but \"{}\" given).",
                    String::from_utf8_lossy(&name),
                    String::from_utf8_lossy(&token_value(&end))
                ),
                parser.current_token().line(),
            ));
        }
        parser.pop_local_scope();
        expect_block_end(parser)?;

        let macro_node = Node::macro_(
            name.clone(),
            Node::body(Nodes::from_list([body]), 0),
            arguments,
            lineno,
            variadic_name,
        )?;
        parser.set_macro(name.clone(), macro_node);
        parser.set_documentation_target(DocumentationTarget::Macro(name.clone()))?;

        Ok(Some(Node::macro_declaration(name, lineno)))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\MacroTokenParser"
    }
}

/// Parses the argument list: an `ArrayExpression` of default values keyed by `LocalVariable`s, and the name of the
/// variadic argument. Twig: `MacroTokenParser::parseDefinition()`.
fn parse_definition(parser: &mut Parser<'_>, macro_name: &[u8]) -> Result<(Expr, Option<Vec<u8>>)> {
    let arguments_line = parser.current_token().line();
    let mut arguments = ArrayExpr::new();
    let mut variadic_name = None;
    parser.stream_mut().expect(
        TokenType::Operator,
        Some("("),
        Some("A list of arguments must begin with an opening parenthesis"),
    )?;
    while !parser.stream().test_value(TokenType::Punctuation, ")") {
        if !arguments.elements.is_empty() {
            parser.stream_mut().expect(
                TokenType::Punctuation,
                Some(","),
                Some("Arguments must be separated by a comma"),
            )?;
            // if the comma above was a trailing comma, early exit the argument parse loop
            if parser.stream().test_value(TokenType::Punctuation, ")") {
                break;
            }
        }

        if parser
            .stream_mut()
            .next_if_value(TokenType::Operator, "...")?
            .is_some()
        {
            let token = parser.stream_mut().expect(
                TokenType::Name,
                None,
                Some("A variadic argument must be a name"),
            )?;
            let local = Expr::local_variable(token_value(&token), token.line())?;
            let name = match &local.kind {
                ExprKind::LocalVariable(TempName::String(name)) => {
                    TempName::strip_reserved_prefix(name).to_vec()
                }
                ExprKind::LocalVariable(TempName::Int(int)) => int.to_string().into_bytes(),
                _ => Vec::new(),
            };
            if parser.stream().test_value(TokenType::Operator, "=") {
                return Err(syntax_error(
                    parser,
                    format!(
                        "The variadic argument \"{}\" in macro \"{}\" cannot have a default value.",
                        String::from_utf8_lossy(&name),
                        String::from_utf8_lossy(macro_name)
                    ),
                    token.line(),
                ));
            }
            if parser
                .stream_mut()
                .next_if_value(TokenType::Punctuation, ",")?
                .is_some()
                && !parser.stream().test_value(TokenType::Punctuation, ")")
            {
                return Err(syntax_error(
                    parser,
                    format!(
                        "The variadic argument \"{}\" in macro \"{}\" must be the last one.",
                        String::from_utf8_lossy(&name),
                        String::from_utf8_lossy(macro_name)
                    ),
                    token.line(),
                ));
            }
            variadic_name = Some(name);
            break;
        }

        let token = parser.stream_mut().expect(
            TokenType::Name,
            None,
            Some("An argument must be a name"),
        )?;
        let mut name = Expr::local_variable(token_value(&token), token.line())?;
        add_expr_documentation(&mut name, &[&token]);
        let (default, equals) = match parser
            .stream_mut()
            .next_if_value(TokenType::Operator, "=")?
        {
            Some(equals) => (parser.parse_expression(0)?, Some(equals)),
            None => {
                let mut default =
                    Expr::constant(ConstantValue::Null, parser.current_token().line());
                if let ExprKind::Constant(constant) = &mut default.kind {
                    constant.is_implicit = true;
                }
                (default, None)
            }
        };
        if !check_constant_expression(&default) {
            // only an explicit default can fail the check, so the token is the "=" operator
            let line = equals.map_or(token.line(), |equals| equals.line());
            return Err(syntax_error(
                parser,
                "A default value for an argument must be a constant (a boolean, a string, a number, a sequence, or a mapping).",
                line,
            ));
        }
        arguments.add_element(default, Some(name));
    }
    parser.stream_mut().expect(
        TokenType::Punctuation,
        Some(")"),
        Some("A list of arguments must be closed by a parenthesis"),
    )?;

    Ok((arguments.into_expr(arguments_line), variadic_name))
}

/// Whether the node only contains "constant" elements: constants, sequences and mappings, `-x` and `+x`.
/// Twig: `MacroTokenParser::checkConstantExpression()`.
fn check_constant_expression(expr: &Expr) -> bool {
    let allowed = match &expr.kind {
        ExprKind::Constant(_) | ExprKind::Array(_) => true,
        ExprKind::Unary(unary) => matches!(unary.op, UnaryOp::Neg | UnaryOp::Pos),
        _ => false,
    };
    allowed
        && expr.children().into_iter().all(|(_, child)| match child {
            NodeRef::Expr(child) => check_constant_expression(child),
            NodeRef::Node(_) | NodeRef::Arguments(_) => false,
        })
}
