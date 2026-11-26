//! Statement and structural nodes: the classes under `Twig\Node` (TWIG/src/Node/) that are not expressions.
//!
//! Containers keep Twig's keys (`Nodes` lists keep the keys of removed children, block and macro maps are keyed by
//! name), so dumps and traversal order match Twig. Constructors port the PHP constructors.

use std::borrow::Cow;
use std::fmt;

use super::expr::{Expr, ExprKind};
use super::value::{AttrValue, ConstantValue, Lossy, TempName};
use super::visit::CustomNode;
use crate::error::{Error, Result};
use crate::source::Source;

/// A node. Twig: `Twig\Node\Node` and its subclasses.
#[derive(Clone, PartialEq)]
pub struct Node {
    /// The node class with its attributes and children.
    pub kind: NodeKind,
    /// The template line (`getTemplateLine()`). For [`NodeKind::Expression`] the expression's own line is used.
    pub line: i64,
    /// The tag of the token parser that created the node (`setNodeTag()`), e.g. `if`.
    pub tag: Option<String>,
    /// Documentation attached by `{## ... ##}` comments (`getDocumentation()`).
    pub documentation: Option<Vec<u8>>,
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&super::dump::dump_node(self))
    }
}

/// The class of a statement node with its own attributes and children.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// `ModuleNode`: a whole template.
    Module(Box<ModuleNode>),
    /// `Nodes`: a list of nodes (also keyed maps: blocks, traits).
    Nodes(Nodes),
    /// `BodyNode`.
    Body(Nodes),
    /// `MacrosNode`: macros keyed by name.
    Macros(Nodes),
    /// `EmptyNode`.
    Empty,
    /// `TextNode`.
    Text(TextNode),
    /// `PrintNode`.
    Print(Box<PrintNode>),
    /// `IfNode`.
    If(Box<IfNode>),
    /// `ForNode`.
    For(Box<ForNode>),
    /// `ForLoopNode`.
    ForLoop(ForLoopNode),
    /// `ForElseNode`.
    ForElse(Box<ForElseNode>),
    /// `SetNode`.
    Set(Box<SetNode>),
    /// `CaptureNode`.
    Capture(Box<CaptureNode>),
    /// `BlockNode`.
    Block(Box<BlockNode>),
    /// `BlockReferenceNode`.
    BlockReference(BlockReferenceNode),
    /// `MacroNode`.
    Macro(Box<MacroNode>),
    /// `MacroDeclarationNode` (a `ConfigNode` with a `name` attribute).
    MacroDeclaration(MacroDeclarationNode),
    /// `ConfigNode` (`extends`, `use`).
    Config,
    /// `ImportNode`.
    Import(Box<ImportNode>),
    /// `IncludeNode`.
    Include(Box<IncludeNode>),
    /// `EmbedNode`.
    Embed(Box<EmbedNode>),
    /// `WithNode`.
    With(Box<WithNode>),
    /// `AutoEscapeNode`.
    AutoEscape(Box<AutoEscapeNode>),
    /// `SandboxNode`.
    Sandbox(Box<SandboxNode>),
    /// `DoNode`.
    Do(Box<DoNode>),
    /// `FlushNode`.
    Flush,
    /// `DeprecatedNode`.
    Deprecated(Box<DeprecatedNode>),
    /// `TypesNode`.
    Types(Box<TypesNode>),
    /// `TypeNode`.
    Type(TypeNode),
    /// `CheckSecurityNode`, added by the sandbox visitor.
    CheckSecurity(Box<CheckSecurityNode>),
    /// `CheckSecurityCallNode`, added by the sandbox visitor.
    CheckSecurityCall,
    /// An expression in a node position (a child of a `Nodes` container, or what a visitor put in place of a
    /// statement, like the optimizer's `BlockReferenceExpression` in output mode).
    Expression(Box<Expr>),
    /// A node class defined by an extension.
    Custom(Box<dyn CustomNode>),
}

/// The key of a child in a [`Nodes`] container: an int (lists) or a name (blocks, macros, types, trait parts).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeKey {
    /// An int key.
    Index(i64),
    /// A string key.
    Name(Vec<u8>),
}

impl fmt::Display for NodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeKey::Index(i) => write!(f, "{i}"),
            NodeKey::Name(name) => write!(f, "{}", Lossy(name)),
        }
    }
}

/// The children of a container node (`Nodes`, `BodyNode`, `MacrosNode`, `TypesNode`), in insertion order with
/// their keys (PHP array semantics: setting an existing key replaces the child in place).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Nodes {
    entries: Vec<(NodeKey, Node)>,
}

impl Nodes {
    /// An empty container.
    pub fn new() -> Self {
        Nodes::default()
    }

    /// A list keyed 0, 1, ... (`new Nodes([$a, $b])`).
    pub fn from_list(nodes: impl IntoIterator<Item = Node>) -> Self {
        let mut list = Nodes::new();
        for node in nodes {
            list.push(node);
        }
        list
    }

    /// Appends a child with the next int key (`$nodes[] = $node`).
    pub fn push(&mut self, node: Node) {
        let next = self
            .entries
            .iter()
            .filter_map(|(key, _)| match key {
                NodeKey::Index(i) => Some(*i + 1),
                NodeKey::Name(_) => None,
            })
            .max()
            .unwrap_or(0);
        self.entries.push((NodeKey::Index(next), node));
    }

    /// Sets the child with the given key, replacing an existing one in place (`setNode()`).
    pub fn set(&mut self, key: NodeKey, node: Node) {
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            entry.1 = node;
        } else {
            self.entries.push((key, node));
        }
    }

    /// The child with the given key (`getNode()`).
    pub fn get(&self, key: &NodeKey) -> Option<&Node> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, n)| n)
    }

    /// The child with the given key, mutably.
    pub fn get_mut(&mut self, key: &NodeKey) -> Option<&mut Node> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, n)| n)
    }

    /// Removes the child with the given key (`removeNode()`); other keys are kept.
    pub fn remove(&mut self, key: &NodeKey) -> Option<Node> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(index).1)
    }

    /// The children with their keys.
    pub fn entries(&self) -> &[(NodeKey, Node)] {
        &self.entries
    }

    /// The children with their keys, mutably (keys cannot change).
    pub fn entries_mut(&mut self) -> impl Iterator<Item = (&NodeKey, &mut Node)> {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    /// Keeps only the children for which `keep` returns true (keys are kept).
    pub fn retain(&mut self, mut keep: impl FnMut(&NodeKey, &Node) -> bool) {
        self.entries.retain(|(k, n)| keep(k, n));
    }

    /// The children without their keys.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.entries.iter().map(|(_, n)| n)
    }

    /// The number of children (`count()`).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there is no child.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Consumes the container.
    pub fn into_entries(self) -> Vec<(NodeKey, Node)> {
        self.entries
    }
}

/// `ModuleNode`: the root of a template. Line 1.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleNode {
    /// The `body` child (a `BodyNode`).
    pub body: Node,
    /// The optional `parent` child (the `extends` expression).
    pub parent: Option<Expr>,
    /// The `blocks` child: `Nodes` of `BodyNode([BlockNode])` keyed by block name, or `EmptyNode`.
    pub blocks: Node,
    /// The `macros` child: a `MacrosNode` of `MacroNode`s keyed by name.
    pub macros: Node,
    /// The `traits` child: `Nodes` of `Nodes(['template' => expr, 'targets' => Nodes])`, or `EmptyNode`.
    pub traits: Node,
    /// The `display_start` child (`Nodes`).
    pub display_start: Node,
    /// The `display_end` child (`Nodes`).
    pub display_end: Node,
    /// The `constructor_start` child (`Nodes`).
    pub constructor_start: Node,
    /// The `constructor_end` child (`Nodes`).
    pub constructor_end: Node,
    /// The `class_end` child (`Nodes`).
    pub class_end: Node,
    /// The `index` attribute: the embedded-template index, `None` for Twig's `null`.
    pub index: Option<i64>,
    /// The `embedded_templates` attribute (not a child, so visitors do not traverse embedded templates again):
    /// `Nodes` of `ModuleNode`s, or `EmptyNode`.
    pub embedded_templates: Node,
    /// The `strategy` attribute (set by the escaper visitor): the default escaping strategy, `None` for `false`.
    pub strategy: Option<String>,
    /// The `escaper` attribute (set by the escaper visitor).
    pub escaper: bool,
    /// The template source (Twig sets it on every node of the module).
    pub source: Source,
}

/// `TextNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct TextNode {
    /// The `data` attribute.
    pub data: Vec<u8>,
}

impl TextNode {
    /// Twig: `TextNode::isBlank()`: only whitespace (`ctype_space()`), optionally after a UTF-8 BOM.
    pub fn is_blank(&self) -> bool {
        let is_space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C);
        if !self.data.is_empty() && self.data.iter().all(is_space) {
            return true;
        }
        // `str_contains((string) $this, BOM)`: the node's string form contains the data.
        if self.data.windows(3).any(|w| w == b"\xEF\xBB\xBF") {
            let rest = self.data.get(3..).unwrap_or(&[]);
            if rest.is_empty() || rest.iter().all(is_space) {
                return true;
            }
        }
        false
    }
}

/// `PrintNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintNode {
    /// The `expr` child.
    pub expr: Expr,
}

/// `IfNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct IfNode {
    /// The `tests` child: `Nodes` of alternating conditions ([`NodeKind::Expression`]) and bodies.
    pub tests: Node,
    /// The optional `else` child.
    pub else_: Option<Node>,
}

/// `ForNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct ForNode {
    /// The `key_target` child (an `AssignContextVariable`).
    pub key_target: Expr,
    /// The `value_target` child (an `AssignContextVariable`).
    pub value_target: Expr,
    /// The `seq` child.
    pub seq: Expr,
    /// The `body` child: `Nodes([<body>, ForLoopNode])`.
    pub body: Node,
    /// The optional `else` child (a `ForElseNode`).
    pub else_: Option<Node>,
    /// The `with_loop` attribute.
    pub with_loop: bool,
}

/// `ForLoopNode`; the attributes are copied from the `ForNode` at compile time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ForLoopNode {
    /// The `with_loop` attribute.
    pub with_loop: bool,
    /// The `ifexpr` attribute (always false since Twig 3).
    pub ifexpr: bool,
    /// The `else` attribute.
    pub else_: bool,
}

/// `ForElseNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct ForElseNode {
    /// The `body` child.
    pub body: Node,
}

/// `SetNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetNode {
    /// The `names` child: `Nodes` of `AssignContextVariable` expressions, or a single expression node (the
    /// `LocalVariable` of `apply`).
    pub names: Node,
    /// The `values` child: `Nodes` of expressions, a single expression node, or a `CaptureNode`.
    pub values: Node,
    /// The `capture` attribute.
    pub capture: bool,
    /// The `safe` attribute.
    pub safe: bool,
}

/// `CaptureNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureNode {
    /// The `body` child.
    pub body: Node,
    /// The `raw` attribute.
    pub raw: bool,
}

/// `BlockNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockNode {
    /// The `name` attribute.
    pub name: Vec<u8>,
    /// The `body` child.
    pub body: Node,
}

/// `BlockReferenceNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockReferenceNode {
    /// The `name` attribute.
    pub name: Vec<u8>,
}

/// `MacroNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroNode {
    /// The `name` attribute.
    pub name: Vec<u8>,
    /// The `body` child (a `BodyNode`).
    pub body: Node,
    /// The `arguments` child: an `ArrayExpression` of `LocalVariable` keys and default values.
    pub arguments: Expr,
    /// The `variadic_name` attribute.
    pub variadic_name: Option<Vec<u8>>,
}

/// Twig: `MacroNode::VARARGS_NAME`.
pub const VARARGS_NAME: &str = "varargs";

/// `MacroDeclarationNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroDeclarationNode {
    /// The `name` attribute.
    pub name: Vec<u8>,
}

/// `ImportNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportNode {
    /// The `expr` child (the template, or `_self`).
    pub expr: Expr,
    /// The `var` child (an `AssignMacroVariable`).
    pub var: Expr,
}

/// `IncludeNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct IncludeNode {
    /// The `expr` child.
    pub expr: Expr,
    /// The optional `variables` child.
    pub variables: Option<Expr>,
    /// The `only` attribute.
    pub only: bool,
    /// The `ignore_missing` attribute.
    pub ignore_missing: bool,
    /// The `sandboxed` attribute, appended by the deprecated `sandbox` tag.
    pub sandboxed: Option<bool>,
}

/// `EmbedNode`. The `expr` child is a dummy `ConstantExpression('not_used')`.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedNode {
    /// The `expr` child.
    pub expr: Expr,
    /// The optional `variables` child.
    pub variables: Option<Expr>,
    /// The `only` attribute.
    pub only: bool,
    /// The `ignore_missing` attribute.
    pub ignore_missing: bool,
    /// The `name` attribute: the name of the template that contains the embed.
    pub name: String,
    /// The `index` attribute: the index of the embedded template.
    pub index: i64,
    /// The `sandboxed` attribute, appended by the deprecated `sandbox` tag.
    pub sandboxed: Option<bool>,
}

/// `WithNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct WithNode {
    /// The `body` child.
    pub body: Node,
    /// The optional `variables` child.
    pub variables: Option<Expr>,
    /// The `only` attribute.
    pub only: bool,
}

/// `AutoEscapeNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoEscapeNode {
    /// The `value` attribute: the strategy (`'html'`, ... or `false`; any constant is accepted by the parser).
    pub value: ConstantValue,
    /// The `body` child.
    pub body: Node,
}

/// `SandboxNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct SandboxNode {
    /// The `body` child.
    pub body: Node,
}

/// `DoNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct DoNode {
    /// The `expr` child.
    pub expr: Expr,
}

/// `DeprecatedNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeprecatedNode {
    /// The `expr` child (the message).
    pub expr: Expr,
    /// The optional `package` child.
    pub package: Option<Expr>,
    /// The optional `version` child.
    pub version: Option<Expr>,
    /// The `version` child was set before the `package` child (children keep the order they were set in).
    pub version_first: bool,
}

impl DeprecatedNode {
    /// Sets the `package` child (`setNode('package', ...)`).
    pub fn set_package(&mut self, expr: Expr) {
        if self.package.is_none() && self.version.is_some() {
            self.version_first = true;
        }
        self.package = Some(expr);
    }

    /// Sets the `version` child (`setNode('version', ...)`).
    pub fn set_version(&mut self, expr: Expr) {
        if self.version.is_none() && self.package.is_none() {
            self.version_first = true;
        }
        self.version = Some(expr);
    }
}

/// `TypesNode`: the `{% types %}` declarations.
#[derive(Debug, Clone, PartialEq)]
pub struct TypesNode {
    /// The `TypeNode` children keyed by name.
    pub types: Nodes,
    /// The `mapping` attribute: name, type and optional flag of each declaration.
    pub mapping: Vec<(Vec<u8>, Vec<u8>, bool)>,
}

/// `TypeNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeNode {
    /// The `name` attribute.
    pub name: Vec<u8>,
    /// The `type` attribute.
    pub type_: Vec<u8>,
    /// The `optional` attribute.
    pub optional: bool,
}

/// `CheckSecurityNode`: the first line of use of every tag, filter, function and test (private properties).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CheckSecurityNode {
    /// Used filters with their first line.
    pub used_filters: Vec<(Vec<u8>, i64)>,
    /// Used tags with their first line.
    pub used_tags: Vec<(Vec<u8>, i64)>,
    /// Used functions with their first line.
    pub used_functions: Vec<(Vec<u8>, i64)>,
    /// Used tests with their first line.
    pub used_tests: Vec<(Vec<u8>, i64)>,
}

impl Node {
    /// A node of the given kind with no tag and no documentation.
    pub fn new(kind: NodeKind, line: i64) -> Self {
        Node {
            kind,
            line,
            tag: None,
            documentation: None,
        }
    }

    /// Twig: `new Nodes($nodes, $lineno)`.
    pub fn nodes(nodes: Nodes, line: i64) -> Self {
        Node::new(NodeKind::Nodes(nodes), line)
    }

    /// Twig: `new BodyNode($nodes, [], $lineno)`.
    pub fn body(nodes: Nodes, line: i64) -> Self {
        Node::new(NodeKind::Body(nodes), line)
    }

    /// Twig: `new EmptyNode($lineno)`.
    pub fn empty(line: i64) -> Self {
        Node::new(NodeKind::Empty, line)
    }

    /// A node of a class defined by an extension (`new MyNode(..., $lineno)`), what a token parser returns for a tag
    /// that compiles itself.
    pub fn custom(node: impl CustomNode, line: i64) -> Self {
        Node::new(NodeKind::Custom(Box::new(node)), line)
    }

    /// An expression in a node position; the node line is the expression's.
    pub fn expression(expr: Expr) -> Self {
        let line = expr.line;
        Node::new(NodeKind::Expression(Box::new(expr)), line)
    }

    /// Twig: `new TextNode($data, $lineno)`.
    pub fn text(data: impl Into<Vec<u8>>, line: i64) -> Self {
        Node::new(NodeKind::Text(TextNode { data: data.into() }), line)
    }

    /// Twig: `new PrintNode($expr, $lineno)`.
    pub fn print(expr: Expr, line: i64) -> Self {
        Node::new(NodeKind::Print(Box::new(PrintNode { expr })), line)
    }

    /// Twig: `new IfNode($tests, $else, $lineno)`: every condition (even key) of `tests` is wrapped by
    /// `TrueTest::wrap()`.
    pub fn if_(mut tests: Nodes, else_: Option<Node>, line: i64) -> Self {
        for (key, node) in tests.entries_mut() {
            if let NodeKey::Index(i) = key
                && i % 2 == 0
                && let NodeKind::Expression(expr) = &mut node.kind
            {
                let condition = std::mem::replace(&mut **expr, Expr::empty(0));
                **expr = Expr::true_test_wrap(condition);
                node.line = expr.line;
            }
        }
        Node::new(
            NodeKind::If(Box::new(IfNode {
                tests: Node::nodes(tests, 0),
                else_,
            })),
            line,
        )
    }

    /// Twig: `new ForNode($keyTarget, $valueTarget, $seq, null, $body, $else, $lineno)`: the body becomes
    /// `Nodes([$body, new ForLoopNode($lineno)])`.
    pub fn for_(
        key_target: Expr,
        value_target: Expr,
        seq: Expr,
        body: Node,
        else_: Option<Node>,
        line: i64,
    ) -> Self {
        let loop_node = Node::new(NodeKind::ForLoop(ForLoopNode::default()), line);
        Node::new(
            NodeKind::For(Box::new(ForNode {
                key_target,
                value_target,
                seq,
                body: Node::nodes(Nodes::from_list([body, loop_node]), 0),
                else_,
                with_loop: true,
            })),
            line,
        )
    }

    /// Twig: `new ForElseNode($body, $lineno)`.
    pub fn for_else(body: Node, line: i64) -> Self {
        Node::new(NodeKind::ForElse(Box::new(ForElseNode { body })), line)
    }

    /// Twig: `new SetNode($capture, $names, $values, $lineno)`, including the constructor's rewrite of captures: an
    /// empty body becomes `ConstantExpression('')`, a single text becomes a constant, a print of a constant
    /// becomes that constant (all without capture), anything else is wrapped in a `CaptureNode`; captures are safe.
    pub fn set(capture: bool, names: Node, values: Node, line: i64) -> Self {
        let mut capture = capture;
        let mut values = values;
        let mut safe = false;
        if capture {
            safe = true;
            let values_line = values.line;
            values = match values.kind {
                NodeKind::Nodes(ref list) if list.is_empty() => {
                    capture = false;
                    Node::expression(Expr::constant("", values_line))
                }
                NodeKind::Text(text) => {
                    capture = false;
                    Node::expression(Expr::constant(
                        ConstantValue::String(text.data),
                        values_line,
                    ))
                }
                NodeKind::Print(print) if matches!(print.expr.kind, ExprKind::Constant(_)) => {
                    capture = false;
                    Node::expression(print.expr)
                }
                kind => {
                    let body = Node {
                        kind,
                        line: values.line,
                        tag: values.tag,
                        documentation: values.documentation,
                    };
                    Node::new(
                        NodeKind::Capture(Box::new(CaptureNode { body, raw: false })),
                        values_line,
                    )
                }
            };
        }
        Node::new(
            NodeKind::Set(Box::new(SetNode {
                names,
                values,
                capture,
                safe,
            })),
            line,
        )
    }

    /// Twig: `new CaptureNode($body, $lineno)`.
    pub fn capture(body: Node, line: i64) -> Self {
        Node::new(
            NodeKind::Capture(Box::new(CaptureNode { body, raw: false })),
            line,
        )
    }

    /// Twig: `new BlockNode($name, $body, $lineno)`.
    pub fn block(name: impl Into<Vec<u8>>, body: Node, line: i64) -> Self {
        Node::new(
            NodeKind::Block(Box::new(BlockNode {
                name: name.into(),
                body,
            })),
            line,
        )
    }

    /// Twig: `new BlockReferenceNode($name, $lineno)`.
    pub fn block_reference(name: impl Into<Vec<u8>>, line: i64) -> Self {
        Node::new(
            NodeKind::BlockReference(BlockReferenceNode { name: name.into() }),
            line,
        )
    }

    /// Twig: `new MacroNode($name, $body, $arguments, $lineno, $variadicName)`.
    ///
    /// # Errors
    ///
    /// The constructor's checks of the argument names (at the line of the argument's default value):
    /// `The argument "varargs" in macro "%s" cannot be defined because the variable "varargs" is reserved for
    /// arbitrary arguments.`, `The variadic argument "%s" in macro "%s" cannot have the same name as another
    /// argument.`, `Argument "%s" is defined twice for macro "%s".`
    pub fn macro_(
        name: impl Into<Vec<u8>>,
        body: Node,
        arguments: Expr,
        line: i64,
        variadic_name: Option<Vec<u8>>,
    ) -> Result<Self> {
        let name = name.into();
        if let ExprKind::Array(array) = &arguments.kind {
            let mut seen: Vec<TempName> = Vec::new();
            let mut reserved_varargs = super::value::RESERVED_NAME_PREFIX.as_bytes().to_vec();
            reserved_varargs.extend_from_slice(VARARGS_NAME.as_bytes());
            for (key, value) in &array.elements {
                let arg_name = match &key.kind {
                    ExprKind::LocalVariable(name) => name.clone(),
                    _ => {
                        return Err(Error::logic(format!(
                            "Attribute \"name\" does not exist for Node \"{}\".",
                            key.class_name()
                        )));
                    }
                };
                let arg_bytes: Cow<'_, [u8]> = match &arg_name {
                    TempName::String(s) => Cow::Borrowed(s),
                    TempName::Int(i) => Cow::Owned(i.to_string().into_bytes()),
                    TempName::Null => Cow::Borrowed(&[]),
                };
                if arg_bytes.as_ref() == reserved_varargs.as_slice() {
                    return Err(Error::syntax(format!(
                        "The argument \"{VARARGS_NAME}\" in macro \"{}\" cannot be defined because the variable \"{VARARGS_NAME}\" is reserved for arbitrary arguments.",
                        Lossy(&name)
                    ))
                    .with_line(value.line));
                }
                let stripped = TempName::strip_reserved_prefix(&arg_bytes);
                if let Some(variadic) = &variadic_name
                    && variadic.as_slice() == stripped
                {
                    return Err(Error::syntax(format!(
                        "The variadic argument \"{}\" in macro \"{}\" cannot have the same name as another argument.",
                        Lossy(variadic),
                        Lossy(&name)
                    ))
                    .with_line(value.line));
                }
                if seen.contains(&arg_name) {
                    return Err(Error::syntax(format!(
                        "Argument \"{}\" is defined twice for macro \"{}\".",
                        Lossy(stripped),
                        Lossy(&name)
                    ))
                    .with_line(value.line));
                }
                seen.push(arg_name);
            }
        }
        Ok(Node::new(
            NodeKind::Macro(Box::new(MacroNode {
                name,
                body,
                arguments,
                variadic_name,
            })),
            line,
        ))
    }

    /// Twig: `new MacroDeclarationNode($name, $lineno)`.
    pub fn macro_declaration(name: impl Into<Vec<u8>>, line: i64) -> Self {
        Node::new(
            NodeKind::MacroDeclaration(MacroDeclarationNode { name: name.into() }),
            line,
        )
    }

    /// Twig: `new ConfigNode($lineno)`.
    pub fn config(line: i64) -> Self {
        Node::new(NodeKind::Config, line)
    }

    /// Twig: `new ImportNode($expr, $var, $lineno)`.
    pub fn import(expr: Expr, var: Expr, line: i64) -> Self {
        Node::new(NodeKind::Import(Box::new(ImportNode { expr, var })), line)
    }

    /// Twig: `new IncludeNode($expr, $variables, $only, $ignoreMissing, $lineno)`.
    pub fn include(
        expr: Expr,
        variables: Option<Expr>,
        only: bool,
        ignore_missing: bool,
        line: i64,
    ) -> Self {
        Node::new(
            NodeKind::Include(Box::new(IncludeNode {
                expr,
                variables,
                only,
                ignore_missing,
                sandboxed: None,
            })),
            line,
        )
    }

    /// Twig: `new EmbedNode($name, $index, $variables, $only, $ignoreMissing, $lineno)`.
    pub fn embed(
        name: impl Into<String>,
        index: i64,
        variables: Option<Expr>,
        only: bool,
        ignore_missing: bool,
        line: i64,
    ) -> Self {
        Node::new(
            NodeKind::Embed(Box::new(EmbedNode {
                expr: Expr::constant("not_used", line),
                variables,
                only,
                ignore_missing,
                name: name.into(),
                index,
                sandboxed: None,
            })),
            line,
        )
    }

    /// Twig: `new WithNode($body, $variables, $only, $lineno)`.
    pub fn with(body: Node, variables: Option<Expr>, only: bool, line: i64) -> Self {
        Node::new(
            NodeKind::With(Box::new(WithNode {
                body,
                variables,
                only,
            })),
            line,
        )
    }

    /// Twig: `new AutoEscapeNode($value, $body, $lineno)`.
    pub fn auto_escape(value: ConstantValue, body: Node, line: i64) -> Self {
        Node::new(
            NodeKind::AutoEscape(Box::new(AutoEscapeNode { value, body })),
            line,
        )
    }

    /// Twig: `new SandboxNode($body, $lineno)`.
    pub fn sandbox(body: Node, line: i64) -> Self {
        Node::new(NodeKind::Sandbox(Box::new(SandboxNode { body })), line)
    }

    /// Twig: `new DoNode($expr, $lineno)`.
    pub fn do_(expr: Expr, line: i64) -> Self {
        Node::new(NodeKind::Do(Box::new(DoNode { expr })), line)
    }

    /// Twig: `new FlushNode($lineno)`.
    pub fn flush(line: i64) -> Self {
        Node::new(NodeKind::Flush, line)
    }

    /// Twig: `new DeprecatedNode($expr, $lineno)`.
    pub fn deprecated(expr: Expr, line: i64) -> Self {
        Node::new(
            NodeKind::Deprecated(Box::new(DeprecatedNode {
                expr,
                package: None,
                version: None,
                version_first: false,
            })),
            line,
        )
    }

    /// Twig: `new TypesNode($types, $lineno)`: one `TypeNode` child per declaration, at the `TypesNode` line.
    pub fn types(mapping: Vec<(Vec<u8>, Vec<u8>, bool)>, line: i64) -> Self {
        let mut types = Nodes::new();
        for (name, type_, optional) in &mapping {
            types.set(
                NodeKey::Name(name.clone()),
                Node::new(
                    NodeKind::Type(TypeNode {
                        name: name.clone(),
                        type_: type_.clone(),
                        optional: *optional,
                    }),
                    line,
                ),
            );
        }
        Node::new(
            NodeKind::Types(Box::new(TypesNode { types, mapping })),
            line,
        )
    }

    /// Twig: `new ModuleNode($body, $parent, $blocks, $macros, $traits, $embeddedTemplates, $source)` (line 1).
    pub fn module(
        body: Node,
        parent: Option<Expr>,
        blocks: Node,
        macros: Node,
        traits: Node,
        embedded_templates: Node,
        source: Source,
    ) -> Self {
        Node::new(
            NodeKind::Module(Box::new(ModuleNode {
                body,
                parent,
                blocks,
                macros,
                traits,
                display_start: Node::nodes(Nodes::new(), 0),
                display_end: Node::nodes(Nodes::new(), 0),
                constructor_start: Node::nodes(Nodes::new(), 0),
                constructor_end: Node::nodes(Nodes::new(), 0),
                class_end: Node::nodes(Nodes::new(), 0),
                index: None,
                embedded_templates,
                strategy: None,
                escaper: false,
                source,
            })),
            1,
        )
    }

    /// The template line: [`line`](Self::line), or the expression's line for [`NodeKind::Expression`].
    pub fn template_line(&self) -> i64 {
        match &self.kind {
            NodeKind::Expression(expr) => expr.line,
            _ => self.line,
        }
    }

    /// The expression, for a [`NodeKind::Expression`] node.
    pub fn as_expr(&self) -> Option<&Expr> {
        match &self.kind {
            NodeKind::Expression(expr) => Some(expr),
            _ => None,
        }
    }

    /// The expression, for a [`NodeKind::Expression`] node.
    pub fn as_expr_mut(&mut self) -> Option<&mut Expr> {
        match &mut self.kind {
            NodeKind::Expression(expr) => Some(expr),
            _ => None,
        }
    }

    /// The children of a container (`Nodes`, `BodyNode`, `MacrosNode`).
    pub fn as_nodes(&self) -> Option<&Nodes> {
        match &self.kind {
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) | NodeKind::Macros(nodes) => Some(nodes),
            _ => None,
        }
    }

    /// The children of a container (`Nodes`, `BodyNode`, `MacrosNode`), mutably.
    pub fn as_nodes_mut(&mut self) -> Option<&mut Nodes> {
        match &mut self.kind {
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) | NodeKind::Macros(nodes) => Some(nodes),
            _ => None,
        }
    }

    /// The module, for a `ModuleNode`.
    pub fn as_module(&self) -> Option<&ModuleNode> {
        match &self.kind {
            NodeKind::Module(module) => Some(module),
            _ => None,
        }
    }

    /// The module, for a `ModuleNode`, mutably.
    pub fn as_module_mut(&mut self) -> Option<&mut ModuleNode> {
        match &mut self.kind {
            NodeKind::Module(module) => Some(module),
            _ => None,
        }
    }

    /// Twig: `setNodeTag()`.
    ///
    /// # Errors
    ///
    /// `The tag of a node can only be set once.` (a `LogicException`).
    pub fn set_node_tag(&mut self, tag: &str) -> Result<()> {
        if self
            .tag
            .as_deref()
            .is_some_and(|t| !t.is_empty() && t != "0")
        {
            return Err(Error::logic("The tag of a node can only be set once."));
        }
        self.tag = Some(tag.to_owned());
        Ok(())
    }

    /// The fully qualified PHP class name, e.g. `Twig\Node\PrintNode`.
    pub fn class_name(&self) -> Cow<'static, str> {
        Cow::Borrowed(match &self.kind {
            NodeKind::Module(_) => "Twig\\Node\\ModuleNode",
            NodeKind::Nodes(_) => "Twig\\Node\\Nodes",
            NodeKind::Body(_) => "Twig\\Node\\BodyNode",
            NodeKind::Macros(_) => "Twig\\Node\\MacrosNode",
            NodeKind::Empty => "Twig\\Node\\EmptyNode",
            NodeKind::Text(_) => "Twig\\Node\\TextNode",
            NodeKind::Print(_) => "Twig\\Node\\PrintNode",
            NodeKind::If(_) => "Twig\\Node\\IfNode",
            NodeKind::For(_) => "Twig\\Node\\ForNode",
            NodeKind::ForLoop(_) => "Twig\\Node\\ForLoopNode",
            NodeKind::ForElse(_) => "Twig\\Node\\ForElseNode",
            NodeKind::Set(_) => "Twig\\Node\\SetNode",
            NodeKind::Capture(_) => "Twig\\Node\\CaptureNode",
            NodeKind::Block(_) => "Twig\\Node\\BlockNode",
            NodeKind::BlockReference(_) => "Twig\\Node\\BlockReferenceNode",
            NodeKind::Macro(_) => "Twig\\Node\\MacroNode",
            NodeKind::MacroDeclaration(_) => "Twig\\Node\\MacroDeclarationNode",
            NodeKind::Config => "Twig\\Node\\ConfigNode",
            NodeKind::Import(_) => "Twig\\Node\\ImportNode",
            NodeKind::Include(_) => "Twig\\Node\\IncludeNode",
            NodeKind::Embed(_) => "Twig\\Node\\EmbedNode",
            NodeKind::With(_) => "Twig\\Node\\WithNode",
            NodeKind::AutoEscape(_) => "Twig\\Node\\AutoEscapeNode",
            NodeKind::Sandbox(_) => "Twig\\Node\\SandboxNode",
            NodeKind::Do(_) => "Twig\\Node\\DoNode",
            NodeKind::Flush => "Twig\\Node\\FlushNode",
            NodeKind::Deprecated(_) => "Twig\\Node\\DeprecatedNode",
            NodeKind::Types(_) => "Twig\\Node\\TypesNode",
            NodeKind::Type(_) => "Twig\\Node\\TypeNode",
            NodeKind::CheckSecurity(_) => "Twig\\Node\\CheckSecurityNode",
            NodeKind::CheckSecurityCall => "Twig\\Node\\CheckSecurityCallNode",
            NodeKind::Expression(expr) => return expr.class_name(),
            NodeKind::Custom(custom) => return Cow::Owned(custom.class_name().to_owned()),
        })
    }

    /// Whether the class implements `NodeOutputInterface` (text, print, block reference, include, embed).
    pub fn is_output(&self) -> bool {
        match &self.kind {
            NodeKind::Text(_)
            | NodeKind::Print(_)
            | NodeKind::BlockReference(_)
            | NodeKind::Include(_)
            | NodeKind::Embed(_) => true,
            NodeKind::Custom(custom) => custom.is_output(),
            _ => false,
        }
    }

    /// Whether the class implements `NodeCaptureInterface` (`SetNode`).
    pub fn is_capture(&self) -> bool {
        match &self.kind {
            NodeKind::Set(_) => true,
            NodeKind::Custom(custom) => custom.is_capture(),
            _ => false,
        }
    }

    /// The node's attributes in Twig's attribute order, without node-valued attributes (`embedded_templates`).
    pub fn attributes(&self) -> Vec<(Cow<'static, str>, AttrValue)> {
        let mut attrs: Vec<(Cow<'static, str>, AttrValue)> = Vec::new();
        let mut push =
            |name: &'static str, value: AttrValue| attrs.push((Cow::Borrowed(name), value));
        match &self.kind {
            NodeKind::Module(m) => {
                push("index", m.index.map_or(AttrValue::Null, AttrValue::Int));
                push(
                    "strategy",
                    m.strategy
                        .as_ref()
                        .map_or(AttrValue::Bool(false), AttrValue::string),
                );
                push("escaper", m.escaper.into());
            }
            NodeKind::Text(t) => push("data", AttrValue::string(&t.data)),
            NodeKind::For(f) => push("with_loop", f.with_loop.into()),
            NodeKind::ForLoop(f) => {
                push("with_loop", f.with_loop.into());
                push("ifexpr", f.ifexpr.into());
                push("else", f.else_.into());
            }
            NodeKind::Set(s) => {
                push("capture", s.capture.into());
                push("safe", s.safe.into());
            }
            NodeKind::Capture(c) => push("raw", c.raw.into()),
            NodeKind::Block(b) => push("name", AttrValue::string(&b.name)),
            NodeKind::BlockReference(b) => push("name", AttrValue::string(&b.name)),
            NodeKind::Macro(m) => {
                push("name", AttrValue::string(&m.name));
                push(
                    "variadic_name",
                    m.variadic_name
                        .as_ref()
                        .map_or(AttrValue::Null, AttrValue::string),
                );
            }
            NodeKind::MacroDeclaration(m) => push("name", AttrValue::string(&m.name)),
            NodeKind::Include(i) => {
                push("only", i.only.into());
                push("ignore_missing", i.ignore_missing.into());
                if let Some(sandboxed) = i.sandboxed {
                    push("sandboxed", sandboxed.into());
                }
            }
            NodeKind::Embed(e) => {
                push("only", e.only.into());
                push("ignore_missing", e.ignore_missing.into());
                push("name", AttrValue::string(&e.name));
                push("index", AttrValue::Int(e.index));
                if let Some(sandboxed) = e.sandboxed {
                    push("sandboxed", sandboxed.into());
                }
            }
            NodeKind::With(w) => push("only", w.only.into()),
            NodeKind::AutoEscape(a) => push("value", AttrValue::from(&a.value)),
            NodeKind::Types(t) => push(
                "mapping",
                AttrValue::Array(
                    t.mapping
                        .iter()
                        .map(|(name, type_, optional)| {
                            (
                                AttrValue::string(name),
                                AttrValue::Array(vec![
                                    (AttrValue::string("type"), AttrValue::string(type_)),
                                    (AttrValue::string("optional"), AttrValue::Bool(*optional)),
                                ]),
                            )
                        })
                        .collect(),
                ),
            ),
            NodeKind::Type(t) => {
                push("name", AttrValue::string(&t.name));
                push("type", AttrValue::string(&t.type_));
                push("optional", t.optional.into());
            }
            NodeKind::Expression(expr) => return expr.attributes(),
            NodeKind::Custom(custom) => {
                for (name, value) in custom.attributes() {
                    attrs.push((Cow::Owned(name), value));
                }
            }
            NodeKind::Nodes(_)
            | NodeKind::Body(_)
            | NodeKind::Macros(_)
            | NodeKind::Empty
            | NodeKind::Print(_)
            | NodeKind::If(_)
            | NodeKind::ForElse(_)
            | NodeKind::Config
            | NodeKind::Import(_)
            | NodeKind::Sandbox(_)
            | NodeKind::Do(_)
            | NodeKind::Flush
            | NodeKind::Deprecated(_)
            | NodeKind::CheckSecurity(_)
            | NodeKind::CheckSecurityCall => {}
        }
        attrs
    }
}

impl ModuleNode {
    /// The blocks as (name, `BlockNode`) pairs, in definition order.
    pub fn blocks(&self) -> impl Iterator<Item = (&[u8], &Node)> {
        self.blocks.as_nodes().into_iter().flat_map(|nodes| {
            nodes.entries().iter().filter_map(|(key, body)| {
                let NodeKey::Name(name) = key else {
                    return None;
                };
                let block = body.as_nodes()?.nodes().next()?;
                Some((name.as_slice(), block))
            })
        })
    }

    /// The macros as (name, `MacroNode`) pairs, in definition order.
    pub fn macros(&self) -> impl Iterator<Item = (&[u8], &Node)> {
        self.macros.as_nodes().into_iter().flat_map(|nodes| {
            nodes.entries().iter().filter_map(|(key, node)| match key {
                NodeKey::Name(name) => Some((name.as_slice(), node)),
                NodeKey::Index(_) => None,
            })
        })
    }

    /// The embedded templates (`ModuleNode`s), in index order.
    pub fn embedded_templates(&self) -> impl Iterator<Item = &Node> {
        self.embedded_templates
            .as_nodes()
            .into_iter()
            .flat_map(Nodes::nodes)
    }

    /// Twig: `ModuleNode::setIndex()`.
    pub fn set_index(&mut self, index: i64) {
        self.index = Some(index);
    }
}
