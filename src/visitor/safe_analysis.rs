//! Twig: `Twig\NodeVisitor\SafeAnalysisNodeVisitor` (TWIG/src/NodeVisitor/SafeAnalysisNodeVisitor.php), the helper of
//! the escaper visitor that computes the escaping strategies an expression's value is already safe for.
//!
//! Twig runs the analysis as a node visitor over an expression (bottom-up, in `leaveNode()`) and caches the result of
//! every node in a `WeakMap`; `EscaperNodeVisitor::isSafeFor()` reads the cache and re-traverses an expression whose
//! cached list is empty. The rules only depend on a node's own attributes and on the lists of its children, and the
//! escaper never changes a subtree whose cached list is non-empty before reading it again, so the cache is not
//! observable: [`safe_strategies`] computes the list directly, recursively.

use crate::ast::{BinaryOp, ConstantValue, Expr, ExprKind};

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
/// | anything else (also expression classes of extensions) | nothing |
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
