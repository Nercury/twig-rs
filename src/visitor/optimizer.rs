//! Twig: `Twig\NodeVisitor\OptimizerNodeVisitor` (TWIG/src/NodeVisitor/OptimizerNodeVisitor.php) and
//! `Twig\Extension\OptimizerExtension` (TWIG/src/Extension/OptimizerExtension.php).

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{NodeVisitor, Visit, VisitContext, VisitResult, priority};
use crate::ast::{ArgumentKey, ConstantValue, Expr, ExprKind, Node, NodeKind, NodeMut};
use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::{Error, Result};
use crate::extension::{Extension, ExtensionSet};

/// Optimizes the tree (priority 255, the last core visitor):
///
/// - always, whatever the mode: a print of a constant string becomes a `TextNode`, a print of `block()` or
///   `parent()` becomes the expression itself with `output = true` (the block is streamed);
/// - with [`OPTIMIZE_FOR`](Self::OPTIMIZE_FOR): `for` loops get `with_loop = false` unless their body can use the
///   `loop` variable (a `loop` reference, a block, an `include` without `only` or an `include()` without
///   `with_context = false`, an attribute access that may reach `loop.parent`), and the loop variables (`loop` and the
///   targets of the enclosing loops) inside the loops get `always_defined = true`. The sequence of a loop belongs to
///   the enclosing scope.
///
/// Twig: `Twig\NodeVisitor\OptimizerNodeVisitor`, registered by [`OptimizerExtension`].
#[derive(Debug)]
pub struct OptimizerNodeVisitor {
    optimizers: i64,
    /// For a visitor built by [`OptimizerExtension`]: what Twig's constructor raises, reported when the visitor
    /// first runs.
    deferred: Option<Deferred>,
}

/// The construction problems of a visitor built by an extension.
#[derive(Debug)]
struct Deferred {
    error: Option<Error>,
    deprecations: Vec<Deprecation>,
    reported: AtomicBool,
}

impl OptimizerNodeVisitor {
    /// Every optimization. Twig: `OptimizerNodeVisitor::OPTIMIZE_ALL`.
    pub const OPTIMIZE_ALL: i64 = -1;
    /// No optional optimization (print nodes are still optimized). Twig: `OptimizerNodeVisitor::OPTIMIZE_NONE`.
    pub const OPTIMIZE_NONE: i64 = 0;
    /// Creates the `loop` variable of `for` loops only when it can be used.
    /// Twig: `OptimizerNodeVisitor::OPTIMIZE_FOR`.
    pub const OPTIMIZE_FOR: i64 = 2;
    /// Deprecated since Twig 3.11, does nothing. Twig: `OptimizerNodeVisitor::OPTIMIZE_RAW_FILTER`.
    pub const OPTIMIZE_RAW_FILTER: i64 = 4;
    /// Deprecated since Twig 3.12, does nothing. Twig: `OptimizerNodeVisitor::OPTIMIZE_TEXT_NODES`.
    pub const OPTIMIZE_TEXT_NODES: i64 = 8;

    /// A visitor for the optimizer mode `optimizers` (a bitmask of the `OPTIMIZE_*` constants).
    ///
    /// Twig: `new OptimizerNodeVisitor($optimizers)`.
    ///
    /// # Errors
    ///
    /// `Optimizer mode "%s" is not valid.` (an `InvalidArgumentException`, here a `LogicException`-kind error) for a
    /// mode above 14. The deprecated [`OPTIMIZE_RAW_FILTER`](Self::OPTIMIZE_RAW_FILTER) and
    /// [`OPTIMIZE_TEXT_NODES`](Self::OPTIMIZE_TEXT_NODES) bits of a mode other than `-1` are reported to
    /// `deprecations`.
    pub fn new(optimizers: i64, deprecations: &mut dyn DeprecationSink) -> Result<Self> {
        check_mode(optimizers)?;
        for deprecation in mode_deprecations(optimizers) {
            deprecations.report(deprecation);
        }
        Ok(OptimizerNodeVisitor {
            optimizers,
            deferred: None,
        })
    }

    /// A visitor whose construction problems surface when it runs: Twig builds the visitor when the extensions are
    /// initialized, before the first template is parsed; here the deprecations are reported by the first traversal
    /// and an invalid mode fails every traversal.
    fn deferred(optimizers: i64) -> Self {
        OptimizerNodeVisitor {
            optimizers,
            deferred: Some(Deferred {
                error: check_mode(optimizers).err(),
                deprecations: mode_deprecations(optimizers),
                reported: AtomicBool::new(false),
            }),
        }
    }

    /// The optimizer mode.
    pub fn optimizers(&self) -> i64 {
        self.optimizers
    }
}

const OPTIMIZE_ALL: i64 = OptimizerNodeVisitor::OPTIMIZE_ALL;
const OPTIMIZE_FOR: i64 = OptimizerNodeVisitor::OPTIMIZE_FOR;
const OPTIMIZE_RAW_FILTER: i64 = OptimizerNodeVisitor::OPTIMIZE_RAW_FILTER;
const OPTIMIZE_TEXT_NODES: i64 = OptimizerNodeVisitor::OPTIMIZE_TEXT_NODES;

/// The check of Twig's constructor: `$optimizers > (OPTIMIZE_FOR | OPTIMIZE_RAW_FILTER | OPTIMIZE_TEXT_NODES)`.
fn check_mode(optimizers: i64) -> Result<()> {
    if optimizers > (OPTIMIZE_FOR | OPTIMIZE_RAW_FILTER | OPTIMIZE_TEXT_NODES) {
        return Err(Error::logic(format!(
            "Optimizer mode \"{optimizers}\" is not valid."
        )));
    }
    Ok(())
}

/// The deprecations of Twig's constructor for the no-op bits of a mode other than `-1`.
fn mode_deprecations(optimizers: i64) -> Vec<Deprecation> {
    let mut deprecations = Vec::new();
    if optimizers != OPTIMIZE_ALL && optimizers & OPTIMIZE_RAW_FILTER == OPTIMIZE_RAW_FILTER {
        deprecations.push(Deprecation::twig(
            "3.11",
            "The \"Twig\\NodeVisitor\\OptimizerNodeVisitor::OPTIMIZE_RAW_FILTER\" option is deprecated and does nothing.",
        ));
    }
    if optimizers != OPTIMIZE_ALL && optimizers & OPTIMIZE_TEXT_NODES == OPTIMIZE_TEXT_NODES {
        deprecations.push(Deprecation::twig(
            "3.12",
            "The \"Twig\\NodeVisitor\\OptimizerNodeVisitor::OPTIMIZE_TEXT_NODES\" option is deprecated and does nothing.",
        ));
    }
    deprecations
}

impl NodeVisitor for OptimizerNodeVisitor {
    /// 255. Twig: `OptimizerNodeVisitor::getPriority()`.
    fn priority(&self) -> i64 {
        priority::OPTIMIZER
    }

    fn start<'a>(&'a self, _extensions: &'a ExtensionSet) -> Box<dyn Visit + 'a> {
        Box::new(OptimizerPass {
            visitor: self,
            optimize_for: self.optimizers & OPTIMIZE_FOR == OPTIMIZE_FOR,
            loops: Vec::new(),
            suspended: Vec::new(),
        })
    }
}

/// A `for` loop being visited. Twig keeps the `ForNode` itself (`$loops`) and the target names (`$loopsTargets`);
/// the `with_loop` flag is written back to the node when the pass leaves it.
struct LoopState {
    with_loop: bool,
    key_target: Vec<u8>,
    value_target: Vec<u8>,
    /// The identity of the loop's `seq` child (never dereferenced).
    seq: *const Expr,
}

/// The state of `OptimizerNodeVisitor` during one traversal.
struct OptimizerPass<'a> {
    visitor: &'a OptimizerNodeVisitor,
    optimize_for: bool,
    /// Twig's `$loops` (the last entry is Twig's `$loops[0]`, the innermost loop).
    loops: Vec<LoopState>,
    /// Twig's `$suspendedLoops`: loops whose sequence is being visited (the last entry is the latest).
    suspended: Vec<LoopState>,
}

impl Visit for OptimizerPass<'_> {
    /// Twig: `OptimizerNodeVisitor::enterNode()`.
    fn enter(&mut self, node: NodeMut<'_>, cx: &mut VisitContext<'_>) -> Result<()> {
        if let Some(deferred) = &self.visitor.deferred {
            if let Some(error) = &deferred.error {
                return Err(error.clone());
            }
            if !deferred.reported.swap(true, Ordering::AcqRel) {
                for deprecation in &deferred.deprecations {
                    cx.report_deprecation(deprecation.clone());
                }
            }
        }
        if self.optimize_for {
            self.enter_optimize_for(node)?;
        }
        Ok(())
    }

    /// Twig: `OptimizerNodeVisitor::leaveNode()`.
    fn leave(&mut self, mut node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<VisitResult> {
        if self.optimize_for {
            self.leave_optimize_for(&mut node);
        }
        if let NodeMut::Node(node) = node {
            optimize_print_node(node);
        }
        Ok(VisitResult::Keep)
    }
}

impl OptimizerPass<'_> {
    /// Twig: `OptimizerNodeVisitor::enterOptimizeFor()`.
    fn enter_optimize_for(&mut self, mut node: NodeMut<'_>) -> Result<()> {
        if let NodeMut::Node(n) = &mut node
            && let NodeKind::For(for_node) = &mut n.kind
        {
            // disable the loop variable by default
            for_node.with_loop = false;
            self.loops.push(LoopState {
                with_loop: false,
                key_target: target_name(&for_node.key_target)?,
                value_target: target_name(&for_node.value_target)?,
                seq: std::ptr::from_ref(&for_node.seq),
            });
            return Ok(());
        }

        // the sequence of a loop is evaluated in the scope enclosing the loop
        if let NodeMut::Expr(expr) = &node
            && self
                .loops
                .last()
                .is_some_and(|current| std::ptr::eq(current.seq, &**expr))
            && let Some(current) = self.loops.pop()
        {
            self.suspended.push(current);
        }

        if self.loops.is_empty() {
            // we are outside a loop
            return Ok(());
        }

        // when do we need to add the loop variable back?
        if let NodeMut::Node(n) = &node {
            match &n.kind {
                // block reference
                NodeKind::BlockReference(_) => {
                    self.add_loop_to_current();
                    return Ok(());
                }
                // include without the only attribute (an embed is an include)
                NodeKind::Include(include) if !include.only => {
                    self.add_loop_to_all();
                    return Ok(());
                }
                NodeKind::Embed(embed) if !embed.only => {
                    self.add_loop_to_all();
                    return Ok(());
                }
                _ => {}
            }
        }

        let Some(expr) = node.as_expr_mut() else {
            return Ok(());
        };
        if let Some(name) = expr.context_variable_name() {
            if name == b"loop" {
                // the loop variable is referenced for the current loop
                expr.always_defined = true;
                self.add_loop_to_current();
            } else if self
                .loops
                .iter()
                .any(|l| l.key_target == name || l.value_target == name)
            {
                // optimize access to loop targets
                expr.always_defined = true;
            }
            return Ok(());
        }
        match &expr.kind {
            // block reference
            ExprKind::BlockReference(_) => self.add_loop_to_current(),
            // include function without the with_context=false parameter
            ExprKind::Function(function) if function.function.name() == b"include" => {
                let with_context = function
                    .arguments
                    .get(&ArgumentKey::Name(b"with_context".to_vec()));
                let disabled = match with_context {
                    None => false,
                    // `->getAttribute('value')`: only constants have the attribute
                    Some(argument) => match &argument.kind {
                        ExprKind::Constant(constant) => {
                            constant.value == ConstantValue::Bool(false)
                        }
                        _ => {
                            return Err(Error::logic(format!(
                                "Attribute \"value\" does not exist for Node \"{}\".",
                                argument.class_name()
                            )));
                        }
                    },
                };
                if !disabled {
                    self.add_loop_to_all();
                }
            }
            // the loop variable is referenced via an attribute
            ExprKind::GetAttr(get_attr) => {
                let dynamic_or_parent = match &get_attr.attribute.kind {
                    ExprKind::Constant(constant) => {
                        constant.value == ConstantValue::string("parent")
                    }
                    _ => true,
                };
                let current_with_loop = self.loops.last().is_some_and(|l| l.with_loop);
                let on_loop = get_attr.node.context_variable_name() == Some(b"loop".as_slice());
                if dynamic_or_parent && (current_with_loop || on_loop) {
                    self.add_loop_to_all();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Twig: `OptimizerNodeVisitor::leaveOptimizeFor()`.
    fn leave_optimize_for(&mut self, node: &mut NodeMut<'_>) {
        if let NodeMut::Expr(expr) = node
            && self
                .suspended
                .last()
                .is_some_and(|suspended| std::ptr::eq(suspended.seq, &**expr))
            && let Some(suspended) = self.suspended.pop()
        {
            self.loops.push(suspended);
        }

        if let NodeMut::Node(n) = node
            && let NodeKind::For(for_node) = &mut n.kind
            && let Some(state) = self.loops.pop()
        {
            for_node.with_loop = state.with_loop;
        }
    }

    /// Twig: `OptimizerNodeVisitor::addLoopToCurrent()`.
    fn add_loop_to_current(&mut self) {
        if let Some(current) = self.loops.last_mut() {
            current.with_loop = true;
        }
    }

    /// Twig: `OptimizerNodeVisitor::addLoopToAll()` (the suspended loops are not concerned).
    fn add_loop_to_all(&mut self) {
        for state in &mut self.loops {
            state.with_loop = true;
        }
    }
}

/// `$node->getNode('key_target')->getAttribute('name')` (the targets are `AssignContextVariable`s).
fn target_name(target: &Expr) -> Result<Vec<u8>> {
    target
        .context_variable_name()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| {
            Error::logic(format!(
                "Attribute \"name\" does not exist for Node \"{}\".",
                target.class_name()
            ))
        })
}

/// Replaces a print of a constant string by a `TextNode`, and a print of `block()` or `parent()` by the expression
/// with `output = true`.
///
/// Twig: `OptimizerNodeVisitor::optimizePrintNode()`.
fn optimize_print_node(node: &mut Node) {
    let NodeKind::Print(print) = &mut node.kind else {
        return;
    };
    match &mut print.expr.kind {
        ExprKind::Constant(constant) => {
            if let ConstantValue::String(data) = &constant.value {
                *node = Node::text(data.clone(), print.expr.line);
            }
        }
        ExprKind::BlockReference(_) | ExprKind::Parent(_) => {
            let mut expr = std::mem::replace(&mut print.expr, Expr::empty(0));
            match &mut expr.kind {
                ExprKind::BlockReference(reference) => reference.output = true,
                ExprKind::Parent(parent) => parent.output = true,
                _ => {}
            }
            *node = Node::expression(expr);
        }
        _ => {}
    }
}

/// `Twig\Extension\OptimizerExtension`: registers the [`OptimizerNodeVisitor`] with the environment's
/// `optimizations` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptimizerExtension {
    optimizers: i64,
}

impl OptimizerExtension {
    /// The extension for the optimizer mode `optimizers` (the `optimizations` option, `-1` by default). An invalid
    /// mode is reported when templates are parsed, as Twig reports it when the extensions are initialized.
    /// Twig: `new OptimizerExtension($optimizers)`.
    pub fn new(optimizers: i64) -> Self {
        OptimizerExtension { optimizers }
    }

    /// The optimizer mode.
    pub fn optimizers(&self) -> i64 {
        self.optimizers
    }
}

impl Default for OptimizerExtension {
    fn default() -> Self {
        OptimizerExtension::new(OPTIMIZE_ALL)
    }
}

impl Extension for OptimizerExtension {
    fn class_name(&self) -> &str {
        "Twig\\Extension\\OptimizerExtension"
    }

    /// Twig: `OptimizerExtension::getNodeVisitors()`.
    fn node_visitors(&self) -> Vec<Arc<dyn NodeVisitor>> {
        vec![Arc::new(OptimizerNodeVisitor::deferred(self.optimizers))]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
