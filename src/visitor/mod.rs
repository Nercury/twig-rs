//! Node visitors and the node traverser: Twig's `Twig\NodeVisitor\NodeVisitorInterface` and `Twig\NodeTraverser`
//! (TWIG/src/NodeVisitor/, TWIG/src/NodeTraverser.php).
//!
//! Extensions register [`NodeVisitor`]s. When a template has been parsed, the parser hands the `ModuleNode` to a
//! [`NodeTraverser`]: visitors run one after the other by ascending [priority](NodeVisitor::priority) (registration
//! order within a priority), and each one makes a complete pass over the tree, depth first: [`Visit::enter`] before
//! the children, [`Visit::leave`] after them. A pass replaces a node by assigning through the [`NodeMut`] it is
//! given and removes it by returning [`VisitResult::Remove`] from `leave`.
//!
//! Twig's visitors, by priority (`YieldNotReadyNodeVisitor` is not needed: every node twig-rs compiles is ready for
//! `use_yield`, so its extension registers nothing):
//!
//! | visitor | priority | registered by |
//! |---|---|---|
//! | [`CorrectnessNodeVisitor`] | [`priority::CORRECTNESS`] (-255) | [`CoreExtension`](crate::extension::CoreExtension) |
//! | [`EscaperNodeVisitor`] | [`priority::ESCAPER`] (0) | [`EscaperExtension`](crate::extension::EscaperExtension) |
//! | [`SandboxNodeVisitor`] | [`priority::SANDBOX`] (0) | [`SandboxExtension`](crate::extension::SandboxExtension) |
//! | `YieldNotReadyNodeVisitor` | [`priority::YIELD_NOT_READY`] (255) | `YieldNotReadyExtension` |
//! | [`OptimizerNodeVisitor`] | [`priority::OPTIMIZER`] (255) | [`OptimizerExtension`] |
//!
//! The [`Environment`](crate::runtime::Environment) registers the core, escaper, yield-not-ready and optimizer
//! extensions in Twig's order, so the effective pass order is Correctness, Escaper, the priority-0 visitors of later
//! extensions (Sandbox), then Optimizer.
//!
//! Twig reuses one visitor object for every template and resets its state when it enters a module; here a visitor
//! starts a fresh [`Visit`] pass per traversal, so one registry can serve several threads.

mod correctness;
mod escaper;
mod optimizer;
mod safe_analysis;
mod sandbox;

pub use correctness::CorrectnessNodeVisitor;
pub use escaper::EscaperNodeVisitor;
pub use optimizer::{OptimizerExtension, OptimizerNodeVisitor};
pub use safe_analysis::{is_safe_for, safe_strategies};
pub use sandbox::SandboxNodeVisitor;

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::ast::{ArgumentKey, ChildKey, Node, NodeKey, NodeKind, NodeMut};
use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::{Error, Result};
use crate::extension::ExtensionSet;

/// The priorities of Twig's node visitors (`getPriority()`).
pub mod priority {
    /// `CorrectnessNodeVisitor`: checks the parsed tree before any other visitor changes it.
    pub const CORRECTNESS: i64 = -255;
    /// `EscaperNodeVisitor`: adds the auto-escaping filters.
    pub const ESCAPER: i64 = 0;
    /// `SandboxNodeVisitor`: adds the security checks of the sandbox.
    pub const SANDBOX: i64 = 0;
    /// `YieldNotReadyNodeVisitor`: reports node classes not ready for `use_yield` (every twig-rs node is ready).
    pub const YIELD_NOT_READY: i64 = 255;
    /// `OptimizerNodeVisitor`: optimizes the tree.
    pub const OPTIMIZER: i64 = 255;
}

/// A node visitor. Twig: `Twig\NodeVisitor\NodeVisitorInterface` (TWIG/src/NodeVisitor/NodeVisitorInterface.php).
pub trait NodeVisitor: Send + Sync {
    /// The class name of the visitor (Twig's `$visitor::class`, e.g. `Twig\NodeVisitor\EscaperNodeVisitor`); by
    /// default the Rust type name.
    fn class_name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// The priority: visitors run by ascending priority, in registration order within a priority.
    /// Twig: `getPriority()`.
    fn priority(&self) -> i64;

    /// Starts a pass over a tree (Twig resets a visitor's state when it enters a module; a pass starts fresh).
    fn start<'a>(&'a self, extensions: &'a ExtensionSet) -> Box<dyn Visit + 'a>;
}

/// What [`Visit::leave`] returns for a node. Twig: the return value of `NodeVisitorInterface::leaveNode()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisitResult {
    /// Keep the node (possibly modified in place, or replaced through the [`NodeMut`]).
    Keep,
    /// Remove the node from its parent (Twig's `null`). Only children of containers (`Nodes`, `BodyNode`,
    /// `MacrosNode`, `TypesNode`) and call arguments can be removed.
    Remove,
}

/// The environment of a pass: Twig's `$env` argument of `enterNode()`/`leaveNode()`, and the sink of the
/// deprecations the pass raises (Twig's `trigger_deprecation()`).
pub struct VisitContext<'a> {
    extensions: &'a ExtensionSet,
    deprecations: &'a mut dyn DeprecationSink,
}

impl<'a> VisitContext<'a> {
    /// A context over the given extensions and deprecation sink.
    pub fn new(extensions: &'a ExtensionSet, deprecations: &'a mut dyn DeprecationSink) -> Self {
        VisitContext {
            extensions,
            deprecations,
        }
    }

    /// The extensions (Twig's environment).
    pub fn extensions(&self) -> &'a ExtensionSet {
        self.extensions
    }

    /// Reports a deprecation.
    pub fn report_deprecation(&mut self, deprecation: Deprecation) {
        self.deprecations.report(deprecation);
    }

    /// The deprecation sink.
    pub fn deprecations(&mut self) -> &mut dyn DeprecationSink {
        &mut *self.deprecations
    }
}

/// One pass of a visitor over a tree. Twig: `NodeVisitorInterface::enterNode()` and `leaveNode()`.
pub trait Visit {
    /// Called before the children are visited; the pass replaces the node by assigning through `node`, and the
    /// children of the replacement are visited.
    ///
    /// # Errors
    ///
    /// Stops the traversal.
    fn enter(&mut self, node: NodeMut<'_>, cx: &mut VisitContext<'_>) -> Result<()>;

    /// Called after the children were visited; the pass replaces the node by assigning through `node`, or removes it
    /// by returning [`VisitResult::Remove`].
    ///
    /// # Errors
    ///
    /// Stops the traversal.
    fn leave(&mut self, node: NodeMut<'_>, cx: &mut VisitContext<'_>) -> Result<VisitResult>;
}

/// Runs node visitors over trees. Twig: `Twig\NodeTraverser` (TWIG/src/NodeTraverser.php).
pub struct NodeTraverser<'a> {
    extensions: &'a ExtensionSet,
    visitors: BTreeMap<i64, Vec<Arc<dyn NodeVisitor>>>,
}

impl<'a> NodeTraverser<'a> {
    /// A traverser with the given visitors. Twig: `new NodeTraverser($env, $visitors)`.
    pub fn new(
        extensions: &'a ExtensionSet,
        visitors: impl IntoIterator<Item = Arc<dyn NodeVisitor>>,
    ) -> Self {
        let mut traverser = NodeTraverser {
            extensions,
            visitors: BTreeMap::new(),
        };
        for visitor in visitors {
            traverser.add_visitor(visitor);
        }
        traverser
    }

    /// Twig: `NodeTraverser::addVisitor()`.
    pub fn add_visitor(&mut self, visitor: Arc<dyn NodeVisitor>) {
        self.visitors
            .entry(visitor.priority())
            .or_default()
            .push(visitor);
    }

    /// Runs every visitor over `node`, by ascending priority, each one over the whole tree; `node` is updated in
    /// place. Twig: `NodeTraverser::traverse()`.
    ///
    /// # Errors
    ///
    /// The first error of a visitor; a `LogicException`-kind error when a visitor removes the root node or a child
    /// that is not in a container.
    pub fn traverse(&self, node: &mut Node, deprecations: &mut dyn DeprecationSink) -> Result<()> {
        for visitor in self.visitors.values().flatten() {
            let mut pass = visitor.start(self.extensions);
            let mut cx = VisitContext::new(self.extensions, &mut *deprecations);
            if traverse(NodeMut::Node(node), pass.as_mut(), &mut cx)? == VisitResult::Remove {
                return Err(Error::logic(
                    "The root node of a traversal cannot be removed.",
                ));
            }
        }
        Ok(())
    }
}

/// Traverses `node` depth first with one pass: `enter`, the children (those the node has after `enter`) in order,
/// `leave`. Children for which `leave` returns [`VisitResult::Remove`] are removed from their container.
///
/// Port of `NodeTraverser::traverseForVisitor()` (TWIG/src/NodeTraverser.php).
///
/// # Errors
///
/// The first error of the pass; a `LogicException`-kind error when the pass removes a child that is not in a
/// container (Twig would leave a node without a required child).
pub fn traverse(
    mut node: NodeMut<'_>,
    visit: &mut dyn Visit,
    cx: &mut VisitContext<'_>,
) -> Result<VisitResult> {
    visit.enter(reborrow(&mut node), cx)?;
    traverse_children(reborrow(&mut node), visit, cx)?;
    visit.leave(node, cx)
}

fn reborrow<'b>(node: &'b mut NodeMut<'_>) -> NodeMut<'b> {
    match node {
        NodeMut::Node(node) => NodeMut::Node(node),
        NodeMut::Expr(expr) => NodeMut::Expr(expr),
        NodeMut::Arguments(arguments) => NodeMut::Arguments(arguments),
    }
}

fn traverse_children(
    node: NodeMut<'_>,
    visit: &mut dyn Visit,
    cx: &mut VisitContext<'_>,
) -> Result<()> {
    match node {
        NodeMut::Node(node) => {
            let removable = matches!(
                node.kind,
                NodeKind::Nodes(_) | NodeKind::Body(_) | NodeKind::Macros(_) | NodeKind::Types(_)
            );
            let class = node.class_name();
            let mut removed: Vec<OwnedKey> = Vec::new();
            for (key, child) in node.children_mut() {
                if traverse(child, visit, cx)? == VisitResult::Remove {
                    if !removable {
                        return Err(not_removable(key, &class));
                    }
                    removed.push(OwnedKey::from(key));
                }
            }
            for key in removed {
                let key = match key {
                    OwnedKey::Index(i) => NodeKey::Index(i),
                    OwnedKey::Key(k) => NodeKey::Name(k),
                    OwnedKey::Name(n) => NodeKey::Name(n.as_bytes().to_vec()),
                };
                match &mut node.kind {
                    NodeKind::Nodes(nodes) | NodeKind::Body(nodes) | NodeKind::Macros(nodes) => {
                        nodes.remove(&key);
                    }
                    NodeKind::Types(types) => {
                        types.types.remove(&key);
                    }
                    _ => {}
                }
            }
        }
        NodeMut::Expr(expr) => {
            let class = expr.class_name();
            for (key, child) in expr.children_mut() {
                if traverse(child, visit, cx)? == VisitResult::Remove {
                    return Err(not_removable(key, &class));
                }
            }
        }
        NodeMut::Arguments(arguments) => {
            let mut removed: Vec<OwnedKey> = Vec::new();
            for (key, child) in arguments.children_mut() {
                if traverse(child, visit, cx)? == VisitResult::Remove {
                    removed.push(OwnedKey::from(key));
                }
            }
            for key in removed {
                let key = match key {
                    OwnedKey::Index(i) => ArgumentKey::Index(i),
                    OwnedKey::Key(k) => ArgumentKey::Name(k),
                    OwnedKey::Name(n) => ArgumentKey::Name(n.as_bytes().to_vec()),
                };
                arguments.remove(&key);
            }
        }
    }
    Ok(())
}

fn not_removable(key: ChildKey<'_>, class: &str) -> Error {
    Error::logic(format!(
        "Removing the child \"{key}\" of a \"{class}\" node is not supported."
    ))
}

/// A child key detached from the borrow of its parent.
#[derive(Debug, Clone, PartialEq, Eq)]
enum OwnedKey {
    Index(i64),
    Key(Vec<u8>),
    Name(&'static str),
}

impl From<ChildKey<'_>> for OwnedKey {
    fn from(key: ChildKey<'_>) -> Self {
        match key {
            ChildKey::Name(name) => OwnedKey::Name(name),
            ChildKey::Index(i) => OwnedKey::Index(i),
            ChildKey::Key(k) => OwnedKey::Key(k.to_vec()),
        }
    }
}
