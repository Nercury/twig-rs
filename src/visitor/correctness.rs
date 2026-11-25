//! Twig: `Twig\NodeVisitor\CorrectnessNodeVisitor` (TWIG/src/NodeVisitor/CorrectnessNodeVisitor.php).

use std::collections::HashSet;

use super::{NodeVisitor, Visit, VisitContext, VisitResult, priority};
use crate::ast::{
    ExprKind, MacroName, MacroReferenceExpr, ModuleNode, Node, NodeKey, NodeKind, NodeMut, NodeRef,
};
use crate::deprecation::Deprecation;
use crate::error::{Error, Result};
use crate::extension::ExtensionSet;
use crate::source::Source;

/// Checks the structure of a parsed template (priority -255, before any visitor changes the tree):
///
/// - a template that extends another one has no output outside blocks: `A template that extends another one cannot
///   include content outside Twig blocks. Did you forget to put the content inside a {% block %} tag?`;
/// - `extends` is not used in a block (`Cannot use "extend" in a block.`) or a macro (`Cannot use "extend" in a
///   macro.`) and only once (`Multiple extends tags are forbidden.`);
/// - `extends`, `use` and `macro` outside the root of the template are deprecated (since 3.28);
/// - at the root of a child template, a `block` is not nested under another tag: `A "block" tag cannot be under a
///   "%s" tag (line %d).`;
/// - a macro called without parentheses is deprecated (since 3.29), once per call site.
///
/// Twig: `Twig\NodeVisitor\CorrectnessNodeVisitor`, registered by `CoreExtension`.
#[derive(Debug, Clone, Copy, Default)]
pub struct CorrectnessNodeVisitor;

impl NodeVisitor for CorrectnessNodeVisitor {
    fn class_name(&self) -> &str {
        "Twig\\NodeVisitor\\CorrectnessNodeVisitor"
    }

    /// -255. Twig: `CorrectnessNodeVisitor::getPriority()`.
    fn priority(&self) -> i64 {
        priority::CORRECTNESS
    }

    fn start<'a>(&'a self, _extensions: &'a ExtensionSet) -> Box<dyn Visit + 'a> {
        Box::new(CorrectnessPass::default())
    }
}

/// The state of `CorrectnessNodeVisitor` during one traversal.
#[derive(Default)]
struct CorrectnessPass {
    /// Twig's `$rootNodes` weak map: the root nodes of the module being visited, by identity (the addresses do not
    /// change during the pass, which never modifies the tree). `None` outside a module.
    root_nodes: Option<Vec<*const Node>>,
    /// Twig's `$reportedMacroCallSites`.
    reported_macro_call_sites: HashSet<Vec<u8>>,
    /// Twig's `$tagStack`: the tag and line of each output-wrapping tag currently open.
    tag_stack: Vec<(String, i64)>,
    has_parent: bool,
    block_depth: i64,
    macro_depth: i64,
    capturing_node_depth: i64,
    has_extends: bool,
    /// The source of the module being visited (Twig reads it from each node, which the `ModuleNode` constructor
    /// gives the module's source).
    source: Option<Source>,
}

impl Visit for CorrectnessPass {
    /// Twig: `CorrectnessNodeVisitor::enterNode()`.
    fn enter(&mut self, node: NodeMut<'_>, cx: &mut VisitContext<'_>) -> Result<()> {
        if let NodeMut::Node(node) = &node
            && let NodeKind::Module(module) = &node.kind
        {
            return self.enter_module(module);
        }

        self.enter_scope(&node);

        if let NodeMut::Node(n) = &node
            && matches!(n.kind, NodeKind::Config | NodeKind::MacroDeclaration(_))
        {
            self.check_config_tag(n, cx)?;
        }

        if let Some(expr) = node.as_ref().as_expr()
            && let ExprKind::MacroReference(reference) = &expr.kind
        {
            self.check_macro_call_parentheses(reference, expr.line, cx);
        }

        if let NodeMut::Node(n) = &node
            && matches!(n.kind, NodeKind::BlockReference(_))
        {
            self.check_block_definition_nesting(n)?;
        }

        Ok(())
    }

    /// Twig: `CorrectnessNodeVisitor::leaveNode()`.
    fn leave(&mut self, node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<VisitResult> {
        if let NodeMut::Node(n) = &node
            && matches!(n.kind, NodeKind::Module(_))
        {
            self.reset_state();
            return Ok(VisitResult::Keep);
        }
        self.leave_scope(&node);
        Ok(VisitResult::Keep)
    }
}

impl CorrectnessPass {
    /// Twig: `CorrectnessNodeVisitor::enterModule()`.
    fn enter_module(&mut self, module: &ModuleNode) -> Result<()> {
        self.reset_state();
        self.source = Some(module.source.clone());
        self.has_parent = module.parent.is_some();
        let mut root_nodes = Vec::new();
        for node in root_nodes_of(module)? {
            if self.has_parent && !is_empty_output_node(NodeRef::Node(node)) {
                return Err(self.syntax_error(
                    "A template that extends another one cannot include content outside Twig blocks. Did you forget to put the content inside a {% block %} tag?",
                    node.template_line(),
                ));
            }
            root_nodes.push(std::ptr::from_ref(node));
        }
        self.root_nodes = Some(root_nodes);
        Ok(())
    }

    /// Twig: `CorrectnessNodeVisitor::resetState()`.
    fn reset_state(&mut self) {
        self.root_nodes = None;
        self.reported_macro_call_sites.clear();
        self.tag_stack.clear();
        self.has_parent = false;
        self.block_depth = 0;
        self.macro_depth = 0;
        self.capturing_node_depth = 0;
        self.has_extends = false;
    }

    /// Twig: `CorrectnessNodeVisitor::enterScope()`.
    fn enter_scope(&mut self, node: &NodeMut<'_>) {
        let NodeMut::Node(node) = node else {
            return;
        };
        if node.is_capture() {
            self.capturing_node_depth += 1;
        }
        match &node.kind {
            NodeKind::Block(_) => self.block_depth += 1,
            NodeKind::Macro(_) => self.macro_depth += 1,
            NodeKind::BlockReference(_) => {}
            _ => {
                if let Some(tag) = truthy_tag(node) {
                    self.tag_stack.push((tag.to_owned(), node.template_line()));
                }
            }
        }
    }

    /// Twig: `CorrectnessNodeVisitor::leaveScope()`.
    fn leave_scope(&mut self, node: &NodeMut<'_>) {
        let NodeMut::Node(node) = node else {
            return;
        };
        if node.is_capture() {
            self.capturing_node_depth -= 1;
        }
        match &node.kind {
            NodeKind::Block(_) => self.block_depth -= 1,
            NodeKind::Macro(_) => self.macro_depth -= 1,
            NodeKind::BlockReference(_) => {}
            _ => {
                if truthy_tag(node).is_some() {
                    self.tag_stack.pop();
                }
            }
        }
    }

    /// Twig: `CorrectnessNodeVisitor::checkMacroCallParentheses()`.
    fn check_macro_call_parentheses(
        &mut self,
        reference: &MacroReferenceExpr,
        line: i64,
        cx: &mut VisitContext<'_>,
    ) {
        if reference.has_call_parentheses || reference.defined_test {
            return;
        }

        let source_name = self.source_name();
        // A dynamic macro name is only known at runtime.
        let name = match &reference.name {
            MacroName::Static(name) => Some(name.as_slice()),
            MacroName::Dynamic(_) => None,
        };

        // A single call site can be visited more than once: "??" reuses its left node, while "?:" and the
        // "default" filter put a clone of it in the compiled tree.
        let mut call_site = source_name.as_bytes().to_vec();
        call_site.push(0);
        call_site.extend_from_slice(line.to_string().as_bytes());
        call_site.push(0);
        call_site.extend_from_slice(name.unwrap_or_default());
        if !self.reported_macro_call_sites.insert(call_site) {
            return;
        }

        let message = match name {
            None => format!(
                "Omitting parentheses when calling a macro is deprecated and will throw a SyntaxError in Twig 4.0; add parentheses after the macro name in \"{source_name}\" at line {line}."
            ),
            Some(name) => format!(
                "Omitting parentheses when calling the macro \"{}\" is deprecated and will throw a SyntaxError in Twig 4.0; add parentheses after the macro name in \"{source_name}\" at line {line}.",
                String::from_utf8_lossy(name)
            ),
        };
        cx.report_deprecation(Deprecation::twig("3.29", message));
    }

    /// Twig: `CorrectnessNodeVisitor::checkConfigTag()`.
    fn check_config_tag(&mut self, node: &Node, cx: &mut VisitContext<'_>) -> Result<()> {
        if node.tag.as_deref() == Some("extends") {
            self.check_extends_tag(node)?;
        }

        let is_root = self
            .root_nodes
            .as_ref()
            .is_some_and(|roots| roots.contains(&std::ptr::from_ref(node)));
        if !is_root {
            let message = format!(
                "Using the \"{}\" tag outside the root of a template is deprecated in {} at line {}.",
                node.tag.as_deref().unwrap_or_default(),
                self.source_name(),
                node.template_line()
            );
            cx.report_deprecation(Deprecation::twig("3.28", message));
        }
        Ok(())
    }

    /// Twig: `CorrectnessNodeVisitor::checkExtendsTag()`.
    fn check_extends_tag(&mut self, node: &Node) -> Result<()> {
        // "extends" inside a "block" or a "macro" has always been a hard error; keep it
        if self.block_depth != 0 {
            return Err(
                self.syntax_error("Cannot use \"extend\" in a block.", node.template_line())
            );
        }
        if self.macro_depth != 0 {
            return Err(
                self.syntax_error("Cannot use \"extend\" in a macro.", node.template_line())
            );
        }
        if self.has_extends {
            return Err(
                self.syntax_error("Multiple extends tags are forbidden.", node.template_line())
            );
        }
        self.has_extends = true;
        Ok(())
    }

    /// Twig: `CorrectnessNodeVisitor::checkBlockDefinitionNesting()`.
    fn check_block_definition_nesting(&self, node: &Node) -> Result<()> {
        // A "block" definition nested under an output-wrapping tag is registered globally regardless of that tag, so
        // the nesting is misleading. This only matters at the root of a child template's body: once inside a block,
        // a macro, an output capture, or in a standalone template, the block is rendered in place and behaves like
        // any other.
        if !self.has_parent
            || self.block_depth != 0
            || self.macro_depth != 0
            || self.capturing_node_depth != 0
        {
            return Ok(());
        }
        let Some((tag, line)) = self.tag_stack.last() else {
            return Ok(());
        };
        Err(self.syntax_error(
            format!("A \"block\" tag cannot be under a \"{tag}\" tag (line {line})."),
            node.template_line(),
        ))
    }

    /// `$node->getSourceContext()->getName()`.
    fn source_name(&self) -> String {
        self.source
            .as_ref()
            .map(|source| source.name().to_owned())
            .unwrap_or_default()
    }

    /// `new SyntaxError($message, $line, $node->getSourceContext())`.
    fn syntax_error(&self, message: impl Into<String>, line: i64) -> Error {
        let error = Error::syntax(message).with_line(line);
        match &self.source {
            Some(source) => error.with_source(source.clone()),
            None => error,
        }
    }
}

/// The tag of a node when it is truthy in PHP (`if ($node->getNodeTag())`).
fn truthy_tag(node: &Node) -> Option<&str> {
    node.tag
        .as_deref()
        .filter(|tag| !tag.is_empty() && *tag != "0")
}

/// Twig: `CorrectnessNodeVisitor::getRootNodes()`: `$module->getNode('body')->getNode('0')`, iterated when it is a
/// `Nodes` container (`Parser::subparse()` does not wrap a single node), else the only root node.
///
/// # Errors
///
/// The `LogicException` of `getNode()` when the body has no node `0`.
fn root_nodes_of(module: &ModuleNode) -> Result<Vec<&Node>> {
    let body = module
        .body
        .as_nodes()
        .and_then(|nodes| nodes.get(&NodeKey::Index(0)))
        .ok_or_else(|| {
            Error::logic(format!(
                "Node \"0\" does not exist for Node \"{}\".",
                module.body.class_name()
            ))
        })?;
    Ok(match &body.kind {
        NodeKind::Nodes(nodes) => nodes.nodes().collect(),
        _ => vec![body],
    })
}

/// Twig: `CorrectnessNodeVisitor::isEmptyOutputNode()`: whether the node never outputs anything, or only blank text.
fn is_empty_output_node(node: NodeRef<'_>) -> bool {
    if let NodeRef::Node(node) = node {
        if node.is_capture() {
            // a "block" tag in such a node will serve as a block definition AND be displayed in place as well
            return true;
        }
        // Can the text be considered "empty" (only whitespace)?
        if let NodeKind::Text(text) = &node.kind {
            return text.is_blank();
        }
        if !matches!(node.kind, NodeKind::BlockReference(_)) && node.is_output() {
            return false;
        }
    }
    node.children()
        .into_iter()
        .all(|(_, child)| is_empty_output_node(child))
}
