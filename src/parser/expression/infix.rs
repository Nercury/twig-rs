//! The core infix expression parsers (TWIG/src/ExpressionParser/Infix/).

use std::sync::Arc;

use super::{ExpressionParser, InfixAssociativity, InfixExpressionParser, PrecedenceChange};
use crate::ast::{
    Arguments, BinaryOp, CallType, ConstantValue, Expr, ExprKind, MacroName, SandboxedFunction,
    UnaryOp,
};
use crate::deprecation::Deprecation;
use crate::error::{Error, Result};
use crate::lexer::{Token, TokenType, matches_name};
use crate::parser::Parser;

use super::prefix::token_bytes;

/// Builds the node of a custom binary operator class (Twig's `new $nodeClass($left, $right, $lineno)`).
pub trait BinaryNodeFactory: Send + Sync {
    /// The class name.
    fn class_name(&self) -> &str;
    /// Creates the node.
    ///
    /// # Errors
    ///
    /// Whatever the node constructor reports.
    fn create(&self, left: Expr, right: Expr, line: i64) -> Result<Expr>;
}

/// The node class a binary operator creates.
#[derive(Clone)]
pub enum BinaryNodeClass {
    /// A core binary class.
    Core(BinaryOp),
    /// A class defined by an extension.
    Custom(Arc<dyn BinaryNodeFactory>),
}

/// A binary operator. Twig: `Twig\ExpressionParser\Infix\BinaryOperatorExpressionParser`.
#[derive(Clone)]
pub struct BinaryOperatorExpressionParser {
    node_class: BinaryNodeClass,
    name: String,
    precedence: i64,
    associativity: InfixAssociativity,
    precedence_change: Option<PrecedenceChange>,
    description: String,
    aliases: Vec<String>,
}

impl BinaryOperatorExpressionParser {
    /// Twig: `new BinaryOperatorExpressionParser($nodeClass, $name, $precedence, $associativity)`.
    pub fn new(
        node_class: BinaryNodeClass,
        name: &str,
        precedence: i64,
        associativity: InfixAssociativity,
    ) -> Self {
        BinaryOperatorExpressionParser {
            node_class,
            name: name.to_owned(),
            precedence,
            associativity,
            precedence_change: None,
            description: String::new(),
            aliases: Vec::new(),
        }
    }

    /// A binary operator of a core class.
    pub fn core(
        op: BinaryOp,
        name: &str,
        precedence: i64,
        associativity: InfixAssociativity,
    ) -> Self {
        Self::new(BinaryNodeClass::Core(op), name, precedence, associativity)
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

    fn right_precedence(&self) -> i64 {
        if self.associativity == InfixAssociativity::Left {
            self.precedence + 1
        } else {
            self.precedence
        }
    }
}

impl ExpressionParser for BinaryOperatorExpressionParser {
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

impl InfixExpressionParser for BinaryOperatorExpressionParser {
    /// Twig: `BinaryOperatorExpressionParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, token: &Token) -> Result<Expr> {
        let right = parser.parse_expression(self.right_precedence())?;
        match &self.node_class {
            BinaryNodeClass::Core(op) => {
                if *op == BinaryOp::Matches {
                    parser.check_matches_regexp(&right, token.line())?;
                }
                Expr::binary(*op, left, right, token.line())
            }
            BinaryNodeClass::Custom(factory) => factory.create(left, right, token.line()),
        }
    }

    fn associativity(&self) -> InfixAssociativity {
        self.associativity
    }
}

/// The assignment operator `=` and destructuring. Twig: `Twig\ExpressionParser\Infix\AssignmentExpressionParser`
/// (a subclass of the binary parser with class `SetBinary`, precedence 0, right associative).
#[derive(Debug, Clone)]
pub struct AssignmentExpressionParser {
    name: String,
}

impl AssignmentExpressionParser {
    /// Twig: `new AssignmentExpressionParser($name)`.
    pub fn new(name: &str) -> Self {
        AssignmentExpressionParser {
            name: name.to_owned(),
        }
    }
}

impl ExpressionParser for AssignmentExpressionParser {
    fn name(&self) -> &str {
        &self.name
    }

    fn precedence(&self) -> i64 {
        0
    }

    fn description(&self) -> &str {
        "Assignment operator"
    }
}

impl InfixExpressionParser for AssignmentExpressionParser {
    /// Twig: `AssignmentExpressionParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, token: &Token) -> Result<Expr> {
        let is_array = matches!(left.kind, ExprKind::Array(_));
        if left.context_variable_name().is_none() && !is_array {
            return Err(Error::syntax(format!(
                "Cannot assign to \"{}\", only variables can be assigned.",
                left.class_name()
            ))
            .with_line(token.line())
            .with_source(parser.stream().source_context().clone()));
        }

        // right associative: the same precedence
        let right = parser.parse_expression(self.precedence())?;
        if self.name != "=" {
            return Err(Error::logic(format!("Unknown operator: {}.", self.name)));
        }

        if let ExprKind::Array(mut array) = left.kind {
            let is_sequence = array.is_sequence();
            if array.elements.is_empty()
                || (is_sequence
                    && array
                        .elements
                        .iter()
                        .all(|(_, value)| matches!(value.kind, ExprKind::Empty)))
            {
                return Err(
                    Error::syntax("Cannot destructure to an empty list of variables.")
                        .with_line(token.line())
                        .with_source(parser.stream().source_context().clone()),
                );
            }
            for (_, value) in &mut array.elements {
                if let ExprKind::ContextVariable(name) = &value.kind {
                    *value = Expr::assign_context_variable(name.name.clone(), value.line)?;
                }
            }
            let op = if is_sequence {
                BinaryOp::SequenceDestructuringSet
            } else {
                BinaryOp::ObjectDestructuringSet
            };
            let left = Expr {
                kind: ExprKind::Array(array),
                ..left
            };
            return Expr::binary(op, left, right, token.line());
        }

        Expr::binary(BinaryOp::Set, left, right, token.line())
    }

    fn associativity(&self) -> InfixAssociativity {
        InfixAssociativity::Right
    }
}

/// The conditional operator `a ? b : c` / `a ? b`.
/// Twig: `Twig\ExpressionParser\Infix\ConditionalTernaryExpressionParser`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConditionalTernaryExpressionParser;

impl ExpressionParser for ConditionalTernaryExpressionParser {
    fn name(&self) -> &str {
        "?"
    }

    fn precedence(&self) -> i64 {
        0
    }

    fn description(&self) -> &str {
        "Conditional operator (a ? b : c)"
    }
}

impl InfixExpressionParser for ConditionalTernaryExpressionParser {
    /// Twig: `ConditionalTernaryExpressionParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, token: &Token) -> Result<Expr> {
        let then = parser.parse_expression(self.precedence())?;
        let otherwise = if parser.skip_if(TokenType::Punctuation, ":")? {
            // Ternary operator (expr ? expr2 : expr3)
            parser.parse_expression(self.precedence())?
        } else {
            // Ternary without else (expr ? expr2)
            Expr::constant("", token.line())
        };
        Ok(Expr::conditional_ternary(
            left,
            then,
            otherwise,
            token.line(),
        ))
    }

    fn associativity(&self) -> InfixAssociativity {
        InfixAssociativity::Left
    }
}

/// Tests: `is` and `is not`. Twig: `Twig\ExpressionParser\Infix\IsExpressionParser` and `IsNotExpressionParser`.
#[derive(Debug, Clone, Copy)]
pub struct IsExpressionParser {
    negated: bool,
}

impl IsExpressionParser {
    /// The `is` parser.
    pub fn is() -> Self {
        IsExpressionParser { negated: false }
    }

    /// The `is not` parser (`IsNotExpressionParser`).
    pub fn is_not() -> Self {
        IsExpressionParser { negated: true }
    }
}

impl ExpressionParser for IsExpressionParser {
    fn name(&self) -> &str {
        if self.negated { "is not" } else { "is" }
    }

    fn precedence(&self) -> i64 {
        100
    }

    fn description(&self) -> &str {
        "Twig tests"
    }

    /// `is` and `is not` are distinct PHP classes.
    fn class_id(&self) -> std::any::TypeId {
        if self.negated {
            std::any::TypeId::of::<IsNotMarker>()
        } else {
            std::any::TypeId::of::<Self>()
        }
    }
}

/// The class identity of `IsNotExpressionParser`.
struct IsNotMarker;

impl InfixExpressionParser for IsExpressionParser {
    /// Twig: `IsExpressionParser::parse()`, wrapped in a `NotUnary` for `is not`.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, token: &Token) -> Result<Expr> {
        let mut expr = left;
        let test = parser.get_test(token.line())?;
        let arguments = if parser.stream().test_value(TokenType::Operator, "(") {
            Some(parser.parse_named_arguments(true)?)
        } else if test.has_one_mandatory_argument() {
            let mut arguments = Arguments::new();
            arguments.push(parser.parse_expression(self.precedence())?);
            Some(arguments)
        } else {
            None
        };

        if test.name() == b"defined" {
            if let Some(name) = expr.context_variable_name()
                && let Some(alias) = parser.get_imported_symbol("function", name)
            {
                let template = alias.macro_variable()?;
                let macro_name = alias.name.clone().unwrap_or_default();
                let line = expr.line;
                let mut reference = Expr::macro_reference(
                    template,
                    MacroName::Static(macro_name),
                    Expr::array(line),
                    line,
                );
                set_call_parentheses(&mut reference, false);
                expr = reference;
            }
            if let ExprKind::MacroReference(reference) = &expr.kind
                && reference.has_call_parentheses
            {
                let message = format!(
                    "Using parentheses when testing a macro with the \"defined\" test is deprecated and will throw a SyntaxError in Twig 4.0; remove the parentheses after the macro name in \"{}\" at line {}.",
                    parser.stream().source_context().name(),
                    expr.line
                );
                parser.report_deprecation(Deprecation::twig("3.29", message));
            }
        }

        let line = parser.current_token().line();
        let node = Expr::test_for(expr, test, arguments, line)?;
        if self.negated {
            return Ok(Expr::unary(UnaryOp::Not, node, token.line()));
        }
        Ok(node)
    }

    fn associativity(&self) -> InfixAssociativity {
        InfixAssociativity::Left
    }
}

fn set_call_parentheses(expr: &mut Expr, value: bool) {
    if let ExprKind::MacroReference(reference) = &mut expr.kind {
        reference.has_call_parentheses = value;
    }
}

/// Filters `|`. Twig: `Twig\ExpressionParser\Infix\FilterExpressionParser`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FilterExpressionParser;

impl ExpressionParser for FilterExpressionParser {
    fn name(&self) -> &str {
        "|"
    }

    fn precedence(&self) -> i64 {
        512
    }

    fn precedence_change(&self) -> Option<&PrecedenceChange> {
        static CHANGE: std::sync::OnceLock<PrecedenceChange> = std::sync::OnceLock::new();
        Some(CHANGE.get_or_init(|| PrecedenceChange::new("twig/twig", "3.21", 300)))
    }

    fn description(&self) -> &str {
        "Twig filter call"
    }
}

impl InfixExpressionParser for FilterExpressionParser {
    /// Twig: `FilterExpressionParser::parse()`: the filter name, its arguments (parsed before the lookup), then the
    /// node of the filter's class.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, _token: &Token) -> Result<Expr> {
        let token = parser.stream_mut().expect(TokenType::Name, None, None)?;
        let line = token.line();
        let arguments = if parser.stream().test_value(TokenType::Operator, "(") {
            parser.parse_named_arguments(true)?
        } else {
            Arguments::empty_node()
        };
        let filter = parser.get_filter(&token_bytes(&token), line)?;
        Expr::filter(left, filter, arguments, line)
    }

    fn associativity(&self) -> InfixAssociativity {
        InfixAssociativity::Left
    }
}

/// Function calls `(`. Twig: `Twig\ExpressionParser\Infix\FunctionExpressionParser`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FunctionExpressionParser;

impl ExpressionParser for FunctionExpressionParser {
    fn name(&self) -> &str {
        "("
    }

    fn precedence(&self) -> i64 {
        512
    }

    fn description(&self) -> &str {
        "Twig function call"
    }
}

impl InfixExpressionParser for FunctionExpressionParser {
    /// Twig: `FunctionExpressionParser::parse()`: macros imported with `from` first, then the function (arguments
    /// parsed before the lookup), its parser callable or its node class.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, token: &Token) -> Result<Expr> {
        let line = token.line();
        let Some(name) = left.context_variable_name().map(<[u8]>::to_vec) else {
            return Err(function_name_error(parser, line));
        };

        // A bare call to a macro imported via "from" is syntactically a function call; it is resolved through the
        // "function" imported symbol registered by FromTokenParser, which maps the local alias to the macro name
        // and the template it comes from.
        if let Some(alias) = parser.get_imported_symbol("function", &name) {
            let template = alias.macro_variable()?;
            let macro_name = alias.name.clone().unwrap_or_default();
            let arguments = parser.parse_callable_arguments(line, false, true)?;
            let mut node =
                Expr::macro_reference(template, MacroName::Static(macro_name), arguments, line);
            set_call_parentheses(&mut node, true);
            return Ok(node);
        }

        let arguments = parser.parse_named_arguments(false)?;
        let function = parser.get_function(&name, line)?;
        if let Some(callable) = function.parser_callable().cloned() {
            let mut node = callable(parser, arguments, line)?;
            // remember the original function name so the sandbox can enforce the `allowedFunctions` allow-list
            // even though the parser callable returned a specialized node (e.g. `parent`, `block`, `attribute`).
            node.sandboxed_function = Some(Box::new(SandboxedFunction { name, function }));
            return Ok(node);
        }
        Expr::function(function, arguments, line)
    }

    fn associativity(&self) -> InfixAssociativity {
        InfixAssociativity::Left
    }
}

/// `Function name must be an identifier.`
#[cold]
#[inline(never)]
fn function_name_error(parser: &Parser<'_>, line: i64) -> Error {
    Error::syntax("Function name must be an identifier.")
        .with_line(line)
        .with_source(parser.stream().source_context().clone())
}

/// `Expected name or number, got value "%s" of type "%s".`
#[cold]
#[inline(never)]
fn attribute_name_error(parser: &Parser<'_>, token: &Token) -> Error {
    Error::syntax(format!(
        "Expected name or number, got value \"{}\" of type \"{}\".",
        token.value(),
        token.to_english()
    ))
    .with_line(token.line())
    .with_source(parser.stream().source_context().clone())
}

/// Attribute access `.` and `?.`. Twig: `Twig\ExpressionParser\Infix\DotExpressionParser`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DotExpressionParser;

impl ExpressionParser for DotExpressionParser {
    fn name(&self) -> &str {
        "."
    }

    fn precedence(&self) -> i64 {
        512
    }

    fn aliases(&self) -> &[String] {
        static ALIASES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        ALIASES.get_or_init(|| vec!["?.".to_owned()])
    }

    fn description(&self) -> &str {
        "Get an attribute on a variable"
    }
}

impl InfixExpressionParser for DotExpressionParser {
    /// Twig: `DotExpressionParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, token: &Token) -> Result<Expr> {
        let null_safe = token.value().as_bytes() == Some(b"?.");
        let mut token = parser.current_token().clone();
        let lineno = token.line();
        let mut arguments = Expr::array(lineno);
        let mut call_type = CallType::Any;
        let attribute = if parser.skip_if(TokenType::Operator, "(")? {
            let attribute = parser.parse_expression(0)?;
            parser.expect_token(TokenType::Punctuation, Some(")"), None)?;
            attribute
        } else {
            token = parser.stream_mut().next()?;
            if token.test(TokenType::Name)
                || token.test(TokenType::Number)
                || (token.test(TokenType::Operator)
                    && matches_name(token.value().as_bytes().unwrap_or(&[])))
            {
                Expr::constant(ConstantValue::from_token_value(token.value()), token.line())
            } else {
                return Err(attribute_name_error(parser, &token));
            }
        };

        let is_macro_target = left.context_variable_name().is_some_and(|name| {
            name == b"_self" || parser.get_imported_symbol("template", name).is_some()
        });

        if parser.stream().test_value(TokenType::Operator, "(") {
            call_type = CallType::Method;
            arguments = parser.parse_callable_arguments(token.line(), true, is_macro_target)?;
        }

        if is_macro_target {
            let name = match &attribute.kind {
                ExprKind::Constant(constant) => MacroName::Static(constant.value.to_php_string()),
                _ => MacroName::Dynamic(attribute),
            };
            let target_name = left.context_variable_name().unwrap_or_default().to_vec();
            let id = parser.next_macro_variable_id();
            let template = Expr::macro_variable(target_name, left.line, id)?;
            let mut node = Expr::macro_reference(template, name, arguments, left.line);
            set_call_parentheses(&mut node, call_type == CallType::Method);
            return Ok(node);
        }

        Ok(Expr::get_attr(
            left,
            attribute,
            Some(arguments),
            call_type,
            lineno,
            null_safe,
        ))
    }

    fn associativity(&self) -> InfixAssociativity {
        InfixAssociativity::Left
    }
}

/// Subscripts and slices `[`. Twig: `Twig\ExpressionParser\Infix\SquareBracketExpressionParser`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SquareBracketExpressionParser;

impl ExpressionParser for SquareBracketExpressionParser {
    fn name(&self) -> &str {
        "["
    }

    fn precedence(&self) -> i64 {
        512
    }

    fn description(&self) -> &str {
        "Array access"
    }
}

impl InfixExpressionParser for SquareBracketExpressionParser {
    /// Twig: `SquareBracketExpressionParser::parse()`: `a[b]`, or a slice `a[b:c]` as the `slice` filter.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, token: &Token) -> Result<Expr> {
        let lineno = token.line();
        let arguments = Expr::array(lineno);

        // slice?
        let mut slice = false;
        let attribute = if parser.stream().test_value(TokenType::Punctuation, ":") {
            slice = true;
            Expr::constant(0, token.line())
        } else {
            parser.parse_expression(0)?
        };

        if parser.skip_if(TokenType::Punctuation, ":")? {
            slice = true;
        }

        if slice {
            let length = if parser.stream().test_value(TokenType::Punctuation, "]") {
                Expr::constant(ConstantValue::Null, token.line())
            } else {
                parser.parse_expression(0)?
            };
            let filter = parser.get_filter(b"slice", token.line())?;
            let mut slice_arguments = Arguments::new();
            slice_arguments.push(attribute);
            slice_arguments.push(length);
            let node = Expr::filter(left, filter, slice_arguments, token.line())?;
            parser.expect_token(TokenType::Punctuation, Some("]"), None)?;
            return Ok(node);
        }

        parser.expect_token(TokenType::Punctuation, Some("]"), None)?;
        Ok(Expr::get_attr(
            left,
            attribute,
            Some(arguments),
            CallType::Array,
            lineno,
            false,
        ))
    }

    fn associativity(&self) -> InfixAssociativity {
        InfixAssociativity::Left
    }
}

/// Arrow functions `=>`. Twig: `Twig\ExpressionParser\Infix\ArrowExpressionParser`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArrowExpressionParser;

impl ExpressionParser for ArrowExpressionParser {
    fn name(&self) -> &str {
        "=>"
    }

    fn precedence(&self) -> i64 {
        250
    }

    fn description(&self) -> &str {
        "Arrow function (x => expr)"
    }
}

impl InfixExpressionParser for ArrowExpressionParser {
    /// Twig: `ArrowExpressionParser::parse()`: the body is parsed with precedence 0, independent of the context.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, token: &Token) -> Result<Expr> {
        let body = parser.parse_expression(0)?;
        Expr::arrow_function(body, left, token.line())
    }

    fn associativity(&self) -> InfixAssociativity {
        InfixAssociativity::Left
    }
}
