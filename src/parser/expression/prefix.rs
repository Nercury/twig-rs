//! The core prefix expression parsers (TWIG/src/ExpressionParser/Prefix/).

use std::sync::Arc;

use super::{ExpressionParser, PrecedenceChange, PrefixExpressionParser};
use crate::ast::{ArrayExpr, BinaryOp, ConstantValue, Expr, ExprKind, UnaryOp};
use crate::error::{Error, Result};
use crate::lexer::{Token, TokenType, TokenValue, match_name};
use crate::parser::Parser;

/// The literal fallback: names, numbers, strings with interpolation, sequences and mappings.
/// Twig: `Twig\ExpressionParser\Prefix\LiteralExpressionParser`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LiteralExpressionParser;

impl ExpressionParser for LiteralExpressionParser {
    fn name(&self) -> &str {
        "literal"
    }

    /// Not used.
    fn precedence(&self) -> i64 {
        0
    }

    /// No operator token.
    fn operator_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn description(&self) -> &str {
        "A literal value (boolean, string, number, sequence, mapping, ...)"
    }
}

impl PrefixExpressionParser for LiteralExpressionParser {
    /// Twig: `LiteralExpressionParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Expr> {
        if token.test(TokenType::Name) {
            parser.advance()?;
            let line = token.line();
            let name = token.value().as_bytes().unwrap_or(&[]);
            return Ok(match name {
                b"true" | b"TRUE" => Expr::constant(true, line),
                b"false" | b"FALSE" => Expr::constant(false, line),
                b"none" | b"NONE" | b"null" | b"NULL" => Expr::constant(ConstantValue::Null, line),
                _ => Expr::context_variable(name.to_vec(), line),
            });
        }
        if token.test(TokenType::Number) {
            parser.advance()?;
            return Ok(Expr::constant(
                ConstantValue::from_token_value(token.value()),
                token.line(),
            ));
        }
        if token.test(TokenType::String) || token.test(TokenType::InterpolationStart) {
            return parse_string_expression(parser);
        }
        // `case $token->test(Token::PUNCTUATION_TYPE)` falls through to the operator case for anything but `{`.
        if token.test(TokenType::Punctuation) && token.value().as_bytes() == Some(b"{") {
            return parse_mapping_expression(parser);
        }
        if token.test(TokenType::Punctuation) || token.test(TokenType::Operator) {
            let value = token.value().as_bytes().unwrap_or(&[]);
            if value == b"[" {
                return parse_sequence_expression(parser);
            }
            // in this context, string operators are variable names
            if match_name(value).is_some_and(|name| name == value) {
                parser.advance()?;
                return Ok(Expr::context_variable(value.to_vec(), token.line()));
            }
        }
        Err(unexpected_token(parser, token))
    }
}

/// `Unexpected token "%s" of value "%s".`
#[cold]
#[inline(never)]
fn unexpected_token(parser: &Parser<'_>, token: &Token) -> Error {
    Error::syntax(format!(
        "Unexpected token \"{}\" of value \"{}\".",
        token.to_english(),
        token.value()
    ))
    .with_line(token.line())
    .with_source(parser.stream().source_context().clone())
}

/// Twig: `LiteralExpressionParser::parseStringExpression()`: string parts and interpolations concatenated
/// left-nested, a string literal may only follow an interpolation.
fn parse_string_expression(parser: &mut Parser<'_>) -> Result<Expr> {
    let mut nodes: Vec<Expr> = Vec::new();
    // a string cannot be followed by another string in a single expression
    let mut next_can_be_string = true;
    loop {
        if next_can_be_string && let Some(token) = parser.stream_mut().next_if(TokenType::String)? {
            nodes.push(Expr::constant(
                ConstantValue::from_token_value(token.value()),
                token.line(),
            ));
            next_can_be_string = false;
        } else if parser.skip_if_type(TokenType::InterpolationStart)? {
            nodes.push(parser.parse_expression(0)?);
            parser.expect_token(TokenType::InterpolationEnd, None, None)?;
            next_can_be_string = true;
        } else {
            break;
        }
    }
    let mut nodes = nodes.into_iter();
    // The caller checked that the current token is a string or an interpolation start, so there is one node.
    let mut expr = nodes
        .next()
        .ok_or_else(|| Error::logic("A string expression must have at least one part."))?;
    for node in nodes {
        let line = node.line;
        expr = Expr::binary(BinaryOp::Concat, expr, node, line)?;
    }
    Ok(expr)
}

/// Twig: `LiteralExpressionParser::parseSequenceExpression()`.
fn parse_sequence_expression(parser: &mut Parser<'_>) -> Result<Expr> {
    parser.expect_token(
        TokenType::Operator,
        Some("["),
        Some("A sequence element was expected"),
    )?;
    let line = parser.current_token().line();
    let mut array = ArrayExpr::new();
    let mut first = true;
    while !parser.stream().test_value(TokenType::Punctuation, "]") {
        if !first {
            parser.expect_token(
                TokenType::Punctuation,
                Some(","),
                Some("A sequence element must be followed by a comma"),
            )?;
            // trailing ,?
            if parser.stream().test_value(TokenType::Punctuation, "]") {
                break;
            }
        }
        first = false;

        // Check for empty slots (comma with no expression)
        let element = if parser.stream().test_value(TokenType::Punctuation, ",") {
            Expr::empty(parser.current_token().line())
        } else if parser.stream().test_value(TokenType::Operator, "...") {
            let line = parser.current_token().line();
            parser.advance()?;
            Expr::unary(UnaryOp::Spread, parser.parse_expression(0)?, line)
        } else {
            parser.parse_expression(0)?
        };
        array.add_element(element, None);
    }
    parser.expect_token(
        TokenType::Punctuation,
        Some("]"),
        Some("An opened sequence is not properly closed"),
    )?;
    Ok(array.into_expr(line))
}

/// Twig: `LiteralExpressionParser::parseMappingExpression()`.
fn parse_mapping_expression(parser: &mut Parser<'_>) -> Result<Expr> {
    parser.expect_token(
        TokenType::Punctuation,
        Some("{"),
        Some("A mapping element was expected"),
    )?;
    let line = parser.current_token().line();
    let mut array = ArrayExpr::new();
    let mut first = true;
    while !parser.stream().test_value(TokenType::Punctuation, "}") {
        if !first {
            parser.expect_token(
                TokenType::Punctuation,
                Some(","),
                Some("A mapping value must be followed by a comma"),
            )?;
            // trailing ,?
            if parser.stream().test_value(TokenType::Punctuation, "}") {
                break;
            }
        }
        first = false;

        if parser.stream().test_value(TokenType::Operator, "...") {
            let line = parser.current_token().line();
            parser.advance()?;
            let value = Expr::unary(UnaryOp::Spread, parser.parse_expression(0)?, line);
            array.add_element(value, None);
            continue;
        }

        // a mapping key can be:
        //
        //  * a number -- 12
        //  * a string -- 'a'
        //  * a name, which is equivalent to a string -- a
        //  * an expression, which must be enclosed in parentheses -- (1 + 2)
        let key = if let Some(token) = parser.stream_mut().next_if(TokenType::Name)? {
            let key = Expr::constant(ConstantValue::from_token_value(token.value()), token.line());
            // {a} is a shortcut for {a:a}
            if parser
                .stream()
                .test_value(TokenType::Punctuation, &[",", "}"])
            {
                let name = token.value().as_bytes().unwrap_or(&[]).to_vec();
                let value = Expr::context_variable(name, key.line);
                array.add_element(value, Some(key));
                continue;
            }
            key
        } else if let Some(token) = match parser.stream_mut().next_if(TokenType::String)? {
            Some(token) => Some(token),
            None => parser.stream_mut().next_if(TokenType::Number)?,
        } {
            Expr::constant(ConstantValue::from_token_value(token.value()), token.line())
        } else if parser.stream().test_value(TokenType::Operator, "(") {
            parser.parse_expression(0)?
        } else {
            return Err(mapping_key_error(parser));
        };

        parser.expect_token(
            TokenType::Punctuation,
            Some(":"),
            Some("A mapping key must be followed by a colon (:)"),
        )?;
        let value = parser.parse_expression(0)?;
        array.add_element(value, Some(key));
    }
    parser.expect_token(
        TokenType::Punctuation,
        Some("}"),
        Some("An opened mapping is not properly closed"),
    )?;
    Ok(array.into_expr(line))
}

/// `A mapping key must be a quoted string, a number, a name, or an expression enclosed in parentheses (unexpected
/// token "%s" of value "%s".` (sic: the parenthesis is not closed).
#[cold]
#[inline(never)]
fn mapping_key_error(parser: &Parser<'_>) -> Error {
    let current = parser.current_token();
    Error::syntax(format!(
        "A mapping key must be a quoted string, a number, a name, or an expression enclosed in parentheses (unexpected token \"{}\" of value \"{}\".",
        current.to_english(),
        current.value()
    ))
    .with_line(current.line())
    .with_source(parser.stream().source_context().clone())
}

/// Parentheses: a grouped expression, or the parameter list of an arrow function.
/// Twig: `Twig\ExpressionParser\Prefix\GroupingExpressionParser`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GroupingExpressionParser;

impl ExpressionParser for GroupingExpressionParser {
    fn name(&self) -> &str {
        "("
    }

    fn precedence(&self) -> i64 {
        0
    }

    fn description(&self) -> &str {
        "Explicit group expression (a)"
    }
}

impl PrefixExpressionParser for GroupingExpressionParser {
    /// Twig: `GroupingExpressionParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Expr> {
        let expr = parser.parse_expression(self.precedence())?;

        if parser.skip_if(TokenType::Punctuation, ")")? {
            if !parser.stream().test_value(TokenType::Operator, "=>") {
                return Ok(expr.with_explicit_parentheses());
            }
            let item = to_assign_context_variable(expr)?;
            return Ok(Expr::list(vec![item], token.line()));
        }

        // determine if we are parsing an arrow function arguments
        if !parser.stream().test_value(TokenType::Punctuation, ",") {
            parser.expect_token(
                TokenType::Punctuation,
                Some(")"),
                Some("An opened parenthesis is not properly closed"),
            )?;
        }

        let mut names = vec![expr];
        let mut last_token = token.clone();
        loop {
            if parser.skip_if(TokenType::Punctuation, ")")? {
                break;
            }
            parser.expect_token(TokenType::Punctuation, Some(","), None)?;
            last_token = parser.stream_mut().expect(TokenType::Name, None, None)?;
            let name = last_token.value().as_bytes().unwrap_or(&[]).to_vec();
            names.push(Expr::context_variable(name, last_token.line()));
        }

        if !parser.stream().test_value(TokenType::Operator, "=>") {
            return Err(
                Error::syntax("A list of variables must be followed by an arrow.")
                    .with_line(parser.current_token().line())
                    .with_source(parser.stream().source_context().clone()),
            );
        }

        let mut items = Vec::with_capacity(names.len());
        for name in names {
            items.push(to_assign_context_variable(name)?);
        }
        Ok(Expr::list(items, last_token.line()))
    }
}

/// Twig: `GroupingExpressionParser::toAssignContextVariable()`. The error has no source yet (`$expr->getSourceContext()`
/// is only set when the module is built); `Parser::parse()` adds the stream's source.
fn to_assign_context_variable(expr: Expr) -> Result<Expr> {
    let Some(name) = expr.context_variable_name() else {
        return Err(Error::syntax("A list must only contain variables.").with_line(expr.line));
    };
    if matches!(expr.kind, ExprKind::AssignContextVariable(_)) {
        return Ok(expr);
    }
    Expr::assign_context_variable(name.to_vec(), expr.line)
}

/// The spread operator outside sequences, mappings and arguments: always an error.
/// Twig: `Twig\ExpressionParser\Prefix\SpreadExpressionParser`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpreadExpressionParser;

impl ExpressionParser for SpreadExpressionParser {
    fn name(&self) -> &str {
        "..."
    }

    fn precedence(&self) -> i64 {
        512
    }

    fn description(&self) -> &str {
        "Spread operator"
    }
}

impl PrefixExpressionParser for SpreadExpressionParser {
    /// Twig: `SpreadExpressionParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Expr> {
        Err(Error::syntax(
            "The spread operator can only be used on sequence elements, mapping elements, and call arguments.",
        )
        .with_line(token.line())
        .with_source(parser.stream().source_context().clone()))
    }
}

/// Builds the node of a custom unary operator class (Twig's `new $nodeClass($node, $lineno)`).
pub trait UnaryNodeFactory: Send + Sync {
    /// The class name.
    fn class_name(&self) -> &str;
    /// Creates the node.
    ///
    /// # Errors
    ///
    /// Whatever the node constructor reports.
    fn create(&self, node: Expr, line: i64) -> Result<Expr>;
}

/// The node class a unary operator creates.
#[derive(Clone)]
pub enum UnaryNodeClass {
    /// A core unary class.
    Core(UnaryOp),
    /// A class defined by an extension.
    Custom(Arc<dyn UnaryNodeFactory>),
}

/// A unary operator. Twig: `Twig\ExpressionParser\Prefix\UnaryOperatorExpressionParser`.
#[derive(Clone)]
pub struct UnaryOperatorExpressionParser {
    node_class: UnaryNodeClass,
    name: String,
    precedence: i64,
    precedence_change: Option<PrecedenceChange>,
    description: String,
    aliases: Vec<String>,
    operand_precedence: Option<i64>,
}

impl UnaryOperatorExpressionParser {
    /// Twig: `new UnaryOperatorExpressionParser($nodeClass, $name, $precedence)`.
    pub fn new(node_class: UnaryNodeClass, name: &str, precedence: i64) -> Self {
        UnaryOperatorExpressionParser {
            node_class,
            name: name.to_owned(),
            precedence,
            precedence_change: None,
            description: String::new(),
            aliases: Vec::new(),
            operand_precedence: None,
        }
    }

    /// A unary operator of a core class.
    pub fn core(op: UnaryOp, name: &str, precedence: i64) -> Self {
        Self::new(UnaryNodeClass::Core(op), name, precedence)
    }

    /// Sets the precedence change.
    #[must_use]
    pub fn with_precedence_change(mut self, change: PrecedenceChange) -> Self {
        self.precedence_change = Some(change);
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: &str) -> Self {
        self.description = description.to_owned();
        self
    }

    /// Sets the aliases.
    #[must_use]
    pub fn with_aliases(mut self, aliases: &[&str]) -> Self {
        self.aliases = aliases.iter().map(|a| (*a).to_owned()).collect();
        self
    }

    /// Sets the precedence of the operand (by default the operator's precedence).
    #[must_use]
    pub fn with_operand_precedence(mut self, precedence: i64) -> Self {
        self.operand_precedence = Some(precedence);
        self
    }
}

impl ExpressionParser for UnaryOperatorExpressionParser {
    fn name(&self) -> &str {
        &self.name
    }

    fn precedence(&self) -> i64 {
        self.precedence
    }

    fn precedence_change(&self) -> Option<&PrecedenceChange> {
        self.precedence_change.as_ref()
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
    }

    fn description(&self) -> &str {
        &self.description
    }
}

impl PrefixExpressionParser for UnaryOperatorExpressionParser {
    /// Twig: `UnaryOperatorExpressionParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Expr> {
        let operand =
            parser.parse_expression(self.operand_precedence.unwrap_or(self.precedence))?;
        match &self.node_class {
            UnaryNodeClass::Core(op) => Ok(Expr::unary(*op, operand, token.line())),
            UnaryNodeClass::Custom(factory) => factory.create(operand, token.line()),
        }
    }
}

/// The value of a token as a byte string (string tokens; empty for numbers, which callers do not pass).
pub(crate) fn token_bytes(token: &Token) -> Vec<u8> {
    match token.value() {
        TokenValue::Bytes(bytes) => bytes.clone(),
        value => value.to_php_string(),
    }
}
