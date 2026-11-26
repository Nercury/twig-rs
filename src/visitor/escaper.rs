//! Twig: `Twig\NodeVisitor\EscaperNodeVisitor` (TWIG/src/NodeVisitor/EscaperNodeVisitor.php).

use std::collections::HashMap;

use twig_value::PhpError;

use super::safe_analysis::{is_operator_escape, is_safe_for};
use super::{NodeVisitor, Visit, VisitContext, VisitResult, priority};
use crate::ast::{
    Arguments, ConstantValue, Expr, ExprKind, FilterKind, ModuleNode, NodeKind, NodeMut,
};
use crate::error::{Error, Result};
use crate::extension::{EscaperExtension, ExtensionSet};

/// Adds the auto-escaping filters (priority 0, registered by `EscaperExtension`).
///
/// - Every `PrintNode` whose strategy is not `false` gets its expression wrapped in
///   `escape(<strategy>, null, true)` unless the expression is already safe for the strategy (see
///   [`safe_strategies`](super::safe_strategies)); for `?:`, `??` and ternaries each operand is escaped separately.
/// - The input of every filter with a `pre_escape` strategy (`nl2br`, `spaceless`) is escaped with that strategy
///   unless it is safe for it, whatever the auto-escaping state.
/// - The strategy is the innermost `{% autoescape %}` value, else the template's default strategy
///   ([`EscaperExtension::default_strategy`] for the template name). A block body uses the strategy in effect at its
///   `{% block %}` tag; macro bodies use the template's default strategy.
/// - It sets the module's `strategy` attribute (the default strategy when it is a string) and its `escaper`
///   attribute (whether an `EscapeFilter` compiles to the template's escaper), and marks such filters with
///   `template_escaper`.
///
/// Twig: `Twig\NodeVisitor\EscaperNodeVisitor`.
///
/// # Errors
///
/// A strategy that is neither a string nor a boolean nor a number (`{% autoescape null %}`, a default strategy
/// callback returning an array) makes Twig's `needEscaping(): string|bool` throw a PHP `TypeError` when a print, a
/// block or a block reference needs it: an [`ErrorKind::Php`](crate::error::ErrorKind::Php) error standing for that
/// `TypeError` (`Twig\NodeVisitor\EscaperNodeVisitor::needEscaping(): Return value must be of type string|bool, null
/// returned`). It is a PHP `\Error`, so `compileSource()` lets it through unwrapped. Numbers are coerced to strings as
/// PHP does (`{% autoescape 1 %}` escapes with the strategy `"1"`), and `true` becomes the strategy `"1"`.
#[derive(Debug, Clone, Copy, Default)]
pub struct EscaperNodeVisitor;

impl NodeVisitor for EscaperNodeVisitor {
    fn class_name(&self) -> &str {
        "Twig\\NodeVisitor\\EscaperNodeVisitor"
    }

    /// 0. Twig: `EscaperNodeVisitor::getPriority()`.
    fn priority(&self) -> i64 {
        priority::ESCAPER
    }

    fn start<'a>(&'a self, extensions: &'a ExtensionSet) -> Box<dyn Visit + 'a> {
        Box::new(EscaperPass {
            extensions,
            status_stack: Vec::new(),
            blocks: HashMap::new(),
            default_strategy: ConstantValue::Bool(false),
            uses_escaper: false,
        })
    }
}

/// The state of `EscaperNodeVisitor` during one traversal.
struct EscaperPass<'a> {
    extensions: &'a ExtensionSet,
    /// Twig's `$statusStack`: `{% autoescape %}` values and the strategies of the blocks being visited.
    status_stack: Vec<ConstantValue>,
    /// Twig's `$blocks`: the strategy at the `{% block %}` tag of each block, by name.
    blocks: HashMap<Vec<u8>, ConstantValue>,
    /// Twig's `$defaultStrategy`: `false`, or the truthy value `getDefaultStrategy()` returned.
    default_strategy: ConstantValue,
    /// Twig's `$usesEscaper`.
    uses_escaper: bool,
}

impl Visit for EscaperPass<'_> {
    /// Twig: `EscaperNodeVisitor::enterNode()`.
    fn enter(&mut self, mut node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<()> {
        let NodeMut::Node(node) = &mut node else {
            return Ok(());
        };
        match &mut node.kind {
            NodeKind::Module(module) => self.enter_module(module)?,
            NodeKind::AutoEscape(autoescape) => self.status_stack.push(autoescape.value.clone()),
            NodeKind::Block(block) => {
                // `$this->blocks[$name] ?? $this->needEscaping()`
                let strategy = match self.blocks.get(&block.name) {
                    Some(strategy) => strategy.clone(),
                    None => self.need_escaping()?,
                };
                self.status_stack.push(strategy);
            }
            _ => {}
        }
        Ok(())
    }

    /// Twig: `EscaperNodeVisitor::leaveNode()`.
    fn leave(&mut self, mut node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<VisitResult> {
        if let NodeMut::Node(n) = &mut node
            && let NodeKind::Module(module) = &mut n.kind
        {
            module.escaper = self.uses_escaper;
            self.default_strategy = ConstantValue::Bool(false);
            self.blocks.clear();
            return Ok(VisitResult::Keep);
        }

        if let Some(expr) = node.as_expr_mut()
            && matches!(expr.kind, ExprKind::Filter(_))
        {
            self.use_template_escaper(expr);
            self.pre_escape_filter_node(expr)?;
            return Ok(VisitResult::Keep);
        }

        if let NodeMut::Node(n) = &mut node
            && let NodeKind::Print(print) = &mut n.kind
            && let Some(strategy) = strategy_of(self.need_escaping()?)
        {
            if is_operator_escape_expr(&print.expr) {
                self.escape_conditional(&mut print.expr, &strategy)?;
            } else {
                self.escape_expression(&mut print.expr, &strategy)?;
            }
            return Ok(VisitResult::Keep);
        }

        if let NodeMut::Node(n) = &mut node {
            match &n.kind {
                NodeKind::AutoEscape(_) | NodeKind::Block(_) => {
                    self.status_stack.pop();
                }
                NodeKind::BlockReference(reference) => {
                    let strategy = self.need_escaping()?;
                    self.blocks.insert(reference.name.clone(), strategy);
                }
                _ => {}
            }
        }
        Ok(VisitResult::Keep)
    }
}

impl EscaperPass<'_> {
    /// The `ModuleNode` branch of `enterNode()`: resolves the default strategy of the template, sets the module's
    /// `strategy` attribute and resets the state.
    fn enter_module(&mut self, module: &mut ModuleNode) -> Result<()> {
        self.default_strategy = ConstantValue::Bool(false);
        // `$env->hasExtension(EscaperExtension::class) && $defaultStrategy = ...->getDefaultStrategy($name)`
        if let Some(extension) = self.extensions.extension::<EscaperExtension>() {
            let strategy = extension.default_strategy(module.source.name())?;
            if strategy.is_truthy() {
                self.default_strategy = strategy;
            }
        }
        // `\is_string($this->defaultStrategy) ? $this->defaultStrategy : false`
        module.strategy = self
            .default_strategy
            .as_bytes()
            .map(|strategy| String::from_utf8_lossy(strategy).into_owned());
        self.blocks.clear();
        self.status_stack.clear();
        self.uses_escaper = false;
        Ok(())
    }

    /// The current strategy: the top of the status stack, else the default strategy (`false` when falsy), passed
    /// through the `string|bool` return type of Twig's `needEscaping()`.
    ///
    /// Twig: `EscaperNodeVisitor::needEscaping()`.
    fn need_escaping(&self) -> Result<ConstantValue> {
        let value = match self.status_stack.last() {
            Some(value) => value.clone(),
            None if self.default_strategy.is_truthy() => self.default_strategy.clone(),
            None => ConstantValue::Bool(false),
        };
        coerce_need_escaping_return(value)
    }

    /// Twig: `EscaperNodeVisitor::escapeConditional()`: escapes each operand of a `?:`, `??` or ternary, recursing
    /// into operands that are themselves such operators.
    fn escape_conditional(&mut self, expr: &mut Expr, strategy: &[u8]) -> Result<()> {
        let (left, right) = match &mut expr.kind {
            ExprKind::ConditionalTernary(ternary) => {
                let ternary = &mut **ternary;
                (&mut ternary.left, &mut ternary.right)
            }
            ExprKind::Binary(binary) if is_operator_escape(binary.op) => {
                let binary = &mut **binary;
                (&mut binary.left, &mut binary.right)
            }
            _ => return Ok(()),
        };
        for operand in [left, right] {
            if is_operator_escape_expr(operand) {
                self.escape_conditional(operand, strategy)?;
            } else {
                self.escape_expression(operand, strategy)?;
            }
        }
        Ok(())
    }

    /// Twig: `EscaperNodeVisitor::escapeExpression()`: wraps the expression in the escape filter unless it is safe
    /// for the strategy.
    fn escape_expression(&mut self, slot: &mut Expr, strategy: &[u8]) -> Result<()> {
        if is_safe_for(strategy, slot) {
            return Ok(());
        }
        let expr = std::mem::replace(slot, Expr::empty(0));
        *slot = self.escaper_filter(strategy, expr)?;
        Ok(())
    }

    /// Twig: `EscaperNodeVisitor::preEscapeFilterNode()`: escapes the input of a filter with a `pre_escape`
    /// strategy unless the input is safe for it.
    fn pre_escape_filter_node(&mut self, expr: &mut Expr) -> Result<()> {
        let ExprKind::Filter(filter) = &mut expr.kind else {
            return Ok(());
        };
        let Some(strategy) = filter.filter.pre_escape().map(|s| s.as_bytes().to_vec()) else {
            return Ok(());
        };
        if is_safe_for(&strategy, &filter.node) {
            return Ok(());
        }
        let input = std::mem::replace(&mut filter.node, Expr::empty(0));
        filter.node = self.escaper_filter(&strategy, input)?;
        Ok(())
    }

    /// `new $class($node, $filter, new Nodes([strategy, null, true]), $line)` for the environment's `escape` filter
    /// (whatever its node class), at the line of the escaped expression; an `EscapeFilter` is marked to use the
    /// template's escaper.
    ///
    /// Twig: `EscaperNodeVisitor::getEscaperFilter()`.
    fn escaper_filter(&mut self, strategy: &[u8], node: Expr) -> Result<Expr> {
        let line = node.line;
        // `$env->getFilter('escape')->getNodeClass()` (a PHP Error on null)
        let filter = self
            .extensions
            .get_filter(b"escape")?
            .ok_or_else(|| Error::logic("Call to a member function getNodeClass() on null"))?;
        let mut arguments = Arguments::new();
        arguments.push(Expr::constant(
            ConstantValue::String(strategy.to_vec()),
            line,
        ));
        arguments.push(Expr::constant(ConstantValue::Null, line));
        arguments.push(Expr::constant(true, line));
        let mut expr = Expr::filter(node, filter, arguments, line)?;
        self.use_template_escaper(&mut expr);
        Ok(expr)
    }

    /// Twig: `EscaperNodeVisitor::useTemplateEscaper()` for an `EscapeFilter` node.
    fn use_template_escaper(&mut self, expr: &mut Expr) {
        if let ExprKind::Filter(filter) = &mut expr.kind
            && let FilterKind::Escape { template_escaper } = &mut filter.kind
        {
            *template_escaper = true;
            self.uses_escaper = true;
        }
    }
}

/// Whether the expression class implements `OperatorEscapeInterface` (ternary, `?:`, `??`).
fn is_operator_escape_expr(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::ConditionalTernary(_) => true,
        ExprKind::Binary(binary) => is_operator_escape(binary.op),
        _ => false,
    }
}

/// The strategy to escape with for a `needEscaping()` result: `None` for `false`; a string as is; `true` passed to
/// the `string $type` parameters of the escaping methods becomes `"1"`.
fn strategy_of(value: ConstantValue) -> Option<Vec<u8>> {
    match value {
        ConstantValue::Bool(false) => None,
        ConstantValue::Bool(true) => Some(b"1".to_vec()),
        other => Some(other.to_php_string()),
    }
}

/// The return value of `needEscaping(): string|bool` in coercive typing mode (EscaperNodeVisitor.php declares no
/// `strict_types`): strings and booleans are kept, ints and floats become strings (the first type of the union they
/// coerce to), `null` and arrays are a `TypeError`.
///
/// Port of the return type check of a user function: `zend_verify_scalar_type_hint()` (PHPSRC/Zend/zend_execute.c:818;
/// `null` passes only a nullable type) and `zend_verify_weak_scalar_type_hint()` (:733; the first of int, float, string
/// and bool that the union has and that accepts the value).
fn coerce_need_escaping_return(value: ConstantValue) -> Result<ConstantValue> {
    match value {
        ConstantValue::String(_) | ConstantValue::Bool(_) => Ok(value),
        ConstantValue::Int(_) | ConstantValue::Float(_) => {
            Ok(ConstantValue::String(value.to_php_string()))
        }
        ConstantValue::Null => Err(need_escaping_type_error("null")),
        ConstantValue::Array(_) => Err(need_escaping_type_error("array")),
    }
}

/// PHP's `TypeError` of a `needEscaping()` return value that does not coerce to `string|bool`; `given` is the
/// `zend_zval_value_name()` of the value.
///
/// Port of `zend_verify_return_error()` (PHPSRC/Zend/zend_execute.c:1421).
fn need_escaping_type_error(given: &str) -> Error {
    Error::php(PhpError::type_error(format!(
        "Twig\\NodeVisitor\\EscaperNodeVisitor::needEscaping(): Return value must be of type string|bool, {given} returned"
    )))
}
