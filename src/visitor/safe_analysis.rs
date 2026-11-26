//! Twig: `Twig\NodeVisitor\SafeAnalysisNodeVisitor` (TWIG/src/NodeVisitor/SafeAnalysisNodeVisitor.php), the helper of
//! the escaper visitor that computes the escaping strategies an expression's value is already safe for.
//!
//! Twig runs the analysis as a node visitor over an expression (bottom-up, in `leaveNode()`) and caches the result of
//! every node in a `WeakMap`; `EscaperNodeVisitor::isSafeFor()` reads the cache and re-traverses an expression whose
//! cached list is empty. The rules only depend on a node's own attributes and on the lists of its children, and the
//! escaper never changes a subtree whose cached list is non-empty before reading it again, so the cache is not
//! observable: [`safe_strategies`] computes the list directly, recursively. The exceptions the traversal throws for
//! node classes of extensions come from [`check_analysis`], which the escaper runs first.

use crate::ast::{
    Arguments, BinaryOp, ChildKey, ConstantValue, CustomExpr, Expr, ExprKind, NodeRef,
};
use crate::error::{Error, Result};
use crate::extension::TwigCallable;

/// The strategy that stands for every strategy (`'all'`).
pub const ALL: &[u8] = b"all";

/// The strategies `expr` is safe for, with the implied strategies (`html_attr` implies `html` and
/// `html_attr_relaxed`, `html_attr_relaxed` implies `html`).
///
/// Twig: `SafeAnalysisNodeVisitor::getSafe()` after `NodeTraverser::traverse()` of the expression with the analysis
/// visitor; the per-node rules are `SafeAnalysisNodeVisitor::leaveNode()`:
///
/// | node | safe for |
/// |---|---|
/// | `ConstantExpression`, `BlockReferenceExpression`, `ParentExpression`, `MacroReferenceExpression` | `all` |
/// | `?:`, `??`, ternary (`OperatorEscapeInterface`) | the intersection of the two operands |
/// | `FilterExpression` | the filter's `is_safe`/`is_safe_callback`, else the input's list ∩ `preserves_safety` |
/// | `FunctionExpression` | the function's `is_safe`/`is_safe_callback` |
/// | anything else | nothing |
///
/// An expression class defined by an extension counts as the class it declares: an `OperatorEscapeInterface`
/// ([`CustomExpr::operand_names_to_escape`], two operands), else a `FilterExpression` or `FunctionExpression`
/// ([`CustomExpr::callable`], with its `node` and `arguments` children); a missing child counts as no input and no
/// arguments.
pub fn safe_strategies(expr: &Expr) -> Vec<ConstantValue> {
    with_implied_strategies(stored_safe(expr))
}

/// Whether `expr` is safe for the strategy `strategy`: `in_array($type, $safe, true) || in_array('all', $safe, true)`.
///
/// Twig: `EscaperNodeVisitor::isSafeFor()`.
pub fn is_safe_for(strategy: &[u8], expr: &Expr) -> bool {
    let safe = safe_strategies(expr);
    contains_string(&safe, strategy) || contains_string(&safe, ALL)
}

/// The exceptions Twig's traversal of `expr` with the analysis visitor throws, which only node classes of extensions
/// cause, in the order the traversal leaves the nodes (children first): an `OperatorEscapeInterface` with more than two
/// operands (`Operators with more than 2 operands are not supported yet, got %d.`), and the children a class lacks
/// (`Node "%s" does not exist for Node "%s".`, from `getNode()`). Both are `LogicException`s.
///
/// Twig: `SafeAnalysisNodeVisitor::leaveNode()` as `EscaperNodeVisitor::isSafeFor()` traverses an expression.
pub(crate) fn check_analysis(expr: &Expr) -> Result<()> {
    check_children(expr.children())?;
    if let ExprKind::Custom(custom) = &expr.kind {
        check_custom(custom.as_ref())?;
    }
    Ok(())
}

fn check_children(children: Vec<(ChildKey<'_>, NodeRef<'_>)>) -> Result<()> {
    for (_, child) in children {
        match child {
            NodeRef::Expr(expr) => check_analysis(expr)?,
            NodeRef::Node(node) => match node.as_expr() {
                Some(expr) => check_analysis(expr)?,
                None => check_children(node.children())?,
            },
            NodeRef::Arguments(arguments) => check_children(arguments.children())?,
        }
    }
    Ok(())
}

/// The checks of `leaveNode()` for one node class of an extension.
fn check_custom(custom: &dyn CustomExpr) -> Result<()> {
    if let Some(operands) = custom.operand_names_to_escape() {
        if operands.len() > 2 {
            return Err(Error::logic(format!(
                "Operators with more than 2 operands are not supported yet, got {}.",
                operands.len()
            )));
        }
        if operands.len() == 2 {
            for name in &operands {
                if named_expr(custom, name).is_none() {
                    return Err(missing_child(name, custom));
                }
            }
        }
        return Ok(());
    }
    match custom.callable() {
        Some(TwigCallable::Filter(filter)) => {
            let Some(arguments) = named_arguments(custom) else {
                return Err(missing_child("arguments", custom));
            };
            if filter.safe(arguments).is_empty() && named_expr(custom, "node").is_none() {
                return Err(missing_child("node", custom));
            }
        }
        Some(TwigCallable::Function(_)) if named_arguments(custom).is_none() => {
            return Err(missing_child("arguments", custom));
        }
        _ => {}
    }
    Ok(())
}

fn missing_child(name: &str, custom: &dyn CustomExpr) -> Error {
    Error::logic(format!(
        "Node \"{name}\" does not exist for Node \"{}\".",
        custom.class_name()
    ))
}

/// The expression child `name` of a node class of an extension.
fn named_expr<'a>(custom: &'a dyn CustomExpr, name: &str) -> Option<&'a Expr> {
    custom
        .children()
        .into_iter()
        .find(|(key, _)| key.to_string() == name)
        .and_then(|(_, child)| child.as_expr())
}

/// The `arguments` child (an argument container) of a node class of an extension.
fn named_arguments(custom: &dyn CustomExpr) -> Option<&Arguments> {
    custom
        .children()
        .into_iter()
        .find_map(|(key, child)| match child {
            NodeRef::Arguments(arguments) if key == ChildKey::Name("arguments") => Some(arguments),
            _ => None,
        })
}

/// What an expression class defined by an extension is safe for (see [`safe_strategies`]).
fn custom_safe(custom: &dyn CustomExpr) -> Vec<ConstantValue> {
    if let Some(operands) = custom.operand_names_to_escape() {
        // intersect safeness of operands
        return match operands.as_slice() {
            [left, right] => match (named_expr(custom, left), named_expr(custom, right)) {
                (Some(left), Some(right)) => {
                    intersect_safe(&safe_strategies(left), &safe_strategies(right))
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
    }
    let empty = Arguments::new();
    match custom.callable() {
        // filter expression is safe when the filter is safe
        Some(TwigCallable::Filter(filter)) => {
            let safe = filter.safe(named_arguments(custom).unwrap_or(&empty));
            if !safe.is_empty() {
                return safe;
            }
            match named_expr(custom, "node") {
                Some(node) => intersect_safe(&safe_strategies(node), filter.preserves_safety()),
                None => Vec::new(),
            }
        }
        // function expression is safe when the function is safe
        Some(TwigCallable::Function(function)) => {
            function.safe(named_arguments(custom).unwrap_or(&empty))
        }
        _ => Vec::new(),
    }
}

/// The list `leaveNode()` stores for a node (`setSafe()`), without the implied strategies.
fn stored_safe(expr: &Expr) -> Vec<ConstantValue> {
    match &expr.kind {
        // constants are marked safe for all; blocks and parent blocks are safe by definition; all macro calls are
        // safe
        ExprKind::Constant(_)
        | ExprKind::BlockReference(_)
        | ExprKind::Parent(_)
        | ExprKind::MacroReference(_) => vec![all()],
        // intersect safeness of operands (`getOperandNamesToEscape()` is `['left', 'right']` for every core class)
        ExprKind::ConditionalTernary(ternary) => intersect_safe(
            &safe_strategies(&ternary.left),
            &safe_strategies(&ternary.right),
        ),
        ExprKind::Binary(binary) if is_operator_escape(binary.op) => intersect_safe(
            &safe_strategies(&binary.left),
            &safe_strategies(&binary.right),
        ),
        // filter expression is safe when the filter is safe
        ExprKind::Filter(filter) => {
            let safe = filter.filter.safe(&filter.arguments);
            if safe.is_empty() {
                intersect_safe(
                    &safe_strategies(&filter.node),
                    filter.filter.preserves_safety(),
                )
            } else {
                safe
            }
        }
        // function expression is safe when the function is safe
        ExprKind::Function(function) => function.function.safe(&function.arguments),
        ExprKind::Custom(custom) => custom_safe(custom.as_ref()),
        _ => Vec::new(),
    }
}

/// Whether a binary class implements `OperatorEscapeInterface` (`ElvisBinary`, `NullCoalesceBinary`).
pub(crate) fn is_operator_escape(op: BinaryOp) -> bool {
    matches!(op, BinaryOp::Elvis | BinaryOp::NullCoalesce)
}

/// Adds the implied strategies the way `getSafe()` does: `html` and `html_attr_relaxed` after a list containing
/// `html_attr`, then `html` after a list containing `html_attr_relaxed`.
fn with_implied_strategies(mut safe: Vec<ConstantValue>) -> Vec<ConstantValue> {
    if contains_string(&safe, b"html_attr") {
        safe.push(ConstantValue::string("html"));
        safe.push(ConstantValue::string("html_attr_relaxed"));
    }
    if contains_string(&safe, b"html_attr_relaxed") {
        safe.push(ConstantValue::string("html"));
    }
    safe
}

/// Twig: `SafeAnalysisNodeVisitor::intersectSafe()`: nothing when either list is empty, the other list when one
/// contains `all`, else PHP's `array_intersect()` (values compared as strings).
fn intersect_safe(a: &[ConstantValue], b: &[ConstantValue]) -> Vec<ConstantValue> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    if contains_string(a, ALL) {
        return b.to_vec();
    }
    if contains_string(b, ALL) {
        return a.to_vec();
    }
    // array_intersect(): the values of $a whose string form equals the string form of a value of $b
    let b_strings: Vec<Vec<u8>> = b.iter().map(ConstantValue::to_php_string).collect();
    a.iter()
        .filter(|value| b_strings.contains(&value.to_php_string()))
        .cloned()
        .collect()
}

/// `in_array($needle, $list, true)` for a string needle.
fn contains_string(list: &[ConstantValue], needle: &[u8]) -> bool {
    list.iter().any(|value| value.as_bytes() == Some(needle))
}

fn all() -> ConstantValue {
    ConstantValue::String(ALL.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(value: &str) -> ConstantValue {
        ConstantValue::string(value)
    }

    #[test]
    fn intersects_like_twig() {
        assert_eq!(
            intersect_safe(&[], &[s("html")]),
            Vec::<ConstantValue>::new()
        );
        assert_eq!(intersect_safe(&[s("all")], &[s("js")]), vec![s("js")]);
        assert_eq!(intersect_safe(&[s("css")], &[s("all")]), vec![s("css")]);
        assert_eq!(
            intersect_safe(&[s("html"), s("js"), s("css")], &[s("css"), s("html")]),
            vec![s("html"), s("css")]
        );
        // array_intersect() compares string forms
        assert_eq!(
            intersect_safe(&[ConstantValue::Int(1)], &[ConstantValue::Bool(true)]),
            vec![ConstantValue::Int(1)]
        );
    }

    #[test]
    fn adds_implied_strategies() {
        assert_eq!(
            with_implied_strategies(vec![s("html_attr")]),
            vec![s("html_attr"), s("html"), s("html_attr_relaxed"), s("html")]
        );
        assert_eq!(
            with_implied_strategies(vec![s("html_attr_relaxed")]),
            vec![s("html_attr_relaxed"), s("html")]
        );
        assert_eq!(with_implied_strategies(vec![s("js")]), vec![s("js")]);
    }
}
