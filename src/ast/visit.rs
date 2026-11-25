//! Generic access to the children of nodes and expressions, and the extension point for node classes defined by
//! extensions.
//!
//! [`Node::children`] and [`Expr::children`] list the children exactly as Twig's `$node->getNodes()` does (same keys,
//! same order), so a generic walk sees the tree the way Twig's visitors do; the node traverser
//! ([`crate::visitor`]) is built on [`Node::children_mut`] / [`Expr::children_mut`].

use std::any::Any;
use std::fmt;

use super::expr::{ArgumentKey, Arguments, Expr, ExprKind, MacroName};
use super::node::{Node, NodeKey, NodeKind, Nodes};
use super::value::AttrValue;

/// The key of a child: a fixed child name (`expr`, `body`, ...), an int key or a string key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChildKey<'a> {
    /// A named child of a node class, e.g. `left`.
    Name(&'static str),
    /// An int key of a container.
    Index(i64),
    /// A string key of a container (block names, argument names, ...).
    Key(&'a [u8]),
}

impl fmt::Display for ChildKey<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChildKey::Name(name) => f.write_str(name),
            ChildKey::Index(i) => write!(f, "{i}"),
            ChildKey::Key(key) => f.write_str(&String::from_utf8_lossy(key)),
        }
    }
}

/// A child: a statement node, an expression, or a call's argument container.
#[derive(Debug, Clone, Copy)]
pub enum NodeRef<'a> {
    /// A statement node.
    Node(&'a Node),
    /// An expression.
    Expr(&'a Expr),
    /// The arguments of a filter, function or test call (Twig's `Nodes`/`EmptyNode`).
    Arguments(&'a Arguments),
}

/// A mutable child: a statement node, an expression, or a call's argument container.
#[derive(Debug)]
pub enum NodeMut<'a> {
    /// A statement node.
    Node(&'a mut Node),
    /// An expression.
    Expr(&'a mut Expr),
    /// The arguments of a filter, function or test call.
    Arguments(&'a mut Arguments),
}

impl<'a> NodeRef<'a> {
    /// The children of the child.
    pub fn children(&self) -> Vec<(ChildKey<'a>, NodeRef<'a>)> {
        match *self {
            NodeRef::Node(node) => node.children(),
            NodeRef::Expr(expr) => expr.children(),
            NodeRef::Arguments(arguments) => arguments_children(arguments),
        }
    }

    /// The expression the child is: an expression child, or a statement node holding an expression
    /// ([`NodeKind::Expression`]). In Twig both are the same object, so `instanceof` checks on expression classes
    /// must look through the wrapper.
    pub fn as_expr(&self) -> Option<&'a Expr> {
        match *self {
            NodeRef::Expr(expr) => Some(expr),
            NodeRef::Node(node) => node.as_expr(),
            NodeRef::Arguments(_) => None,
        }
    }

    /// The template line (`getTemplateLine()`); an argument container (Twig's `Nodes`/`EmptyNode`) has line 0.
    pub fn template_line(&self) -> i64 {
        match *self {
            NodeRef::Node(node) => node.template_line(),
            NodeRef::Expr(expr) => expr.line,
            NodeRef::Arguments(_) => 0,
        }
    }
}

impl NodeMut<'_> {
    /// The child as a shared reference.
    pub fn as_ref(&self) -> NodeRef<'_> {
        match self {
            NodeMut::Node(node) => NodeRef::Node(node),
            NodeMut::Expr(expr) => NodeRef::Expr(expr),
            NodeMut::Arguments(arguments) => NodeRef::Arguments(arguments),
        }
    }

    /// The expression the child is (see [`NodeRef::as_expr`]), mutably.
    pub fn as_expr_mut(&mut self) -> Option<&mut Expr> {
        match self {
            NodeMut::Expr(expr) => Some(expr),
            NodeMut::Node(node) => node.as_expr_mut(),
            NodeMut::Arguments(_) => None,
        }
    }

    /// The children of the child, mutably.
    pub fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        match self {
            NodeMut::Node(node) => node.children_mut(),
            NodeMut::Expr(expr) => expr.children_mut(),
            NodeMut::Arguments(arguments) => arguments_children_mut(arguments),
        }
    }
}

/// An expression node class defined by an extension (a PHP subclass of `AbstractExpression`).
pub trait CustomExpr: fmt::Debug + Send + Sync + 'static {
    /// The class name shown in dumps and error messages.
    fn class_name(&self) -> &str;

    /// The attributes in order.
    fn attributes(&self) -> Vec<(String, AttrValue)> {
        Vec::new()
    }

    /// The children in order.
    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        Vec::new()
    }

    /// The children in order, mutably.
    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        Vec::new()
    }

    /// A deep copy (`clone`).
    fn clone_box(&self) -> Box<dyn CustomExpr>;

    /// Equality with another custom expression.
    fn eq_dyn(&self, other: &dyn CustomExpr) -> bool;

    /// The node as `Any`, to downcast to the concrete type.
    fn as_any(&self) -> &dyn Any;

    /// The node as `Any`, mutably.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Whether the class implements `ReturnPrimitiveTypeInterface`.
    fn returns_primitive(&self) -> bool {
        false
    }

    /// Whether the class implements `SupportDefinedTestInterface`.
    fn supports_defined_test(&self) -> bool {
        false
    }

    /// Twig: `enableDefinedTest()`.
    fn enable_defined_test(&mut self) {}

    /// Twig: `isDefinedTestEnabled()`.
    fn is_defined_test_enabled(&self) -> bool {
        false
    }

    /// The children the compiled node converts to strings at runtime, for a class implementing
    /// `CoercesChildrenToStringInterface` (Twig: `getStringCoercedChildNames()`): the sandbox wraps them in
    /// `__toString()` checks (an argument container: each argument). None by default.
    fn string_coerced_child_names(&self) -> Vec<String> {
        Vec::new()
    }
}

/// A statement node class defined by an extension (a PHP subclass of `Node`).
pub trait CustomNode: fmt::Debug + Send + Sync + 'static {
    /// The class name shown in dumps and error messages.
    fn class_name(&self) -> &str;

    /// The attributes in order.
    fn attributes(&self) -> Vec<(String, AttrValue)> {
        Vec::new()
    }

    /// The children in order.
    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        Vec::new()
    }

    /// The children in order, mutably.
    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        Vec::new()
    }

    /// A deep copy (`clone`).
    fn clone_box(&self) -> Box<dyn CustomNode>;

    /// Equality with another custom node.
    fn eq_dyn(&self, other: &dyn CustomNode) -> bool;

    /// The node as `Any`, to downcast to the concrete type.
    fn as_any(&self) -> &dyn Any;

    /// The node as `Any`, mutably.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Whether the class implements `NodeOutputInterface`.
    fn is_output(&self) -> bool {
        false
    }

    /// Whether the class implements `NodeCaptureInterface`.
    fn is_capture(&self) -> bool {
        false
    }

    /// The children the compiled node converts to strings at runtime, for a class implementing
    /// `CoercesChildrenToStringInterface` (Twig: `getStringCoercedChildNames()`): the sandbox wraps them in
    /// `__toString()` checks (a container or an argument container: each element). None by default.
    fn string_coerced_child_names(&self) -> Vec<String> {
        Vec::new()
    }
}

impl Clone for Box<dyn CustomExpr> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

impl PartialEq for Box<dyn CustomExpr> {
    fn eq(&self, other: &Self) -> bool {
        self.eq_dyn(other.as_ref())
    }
}

impl Clone for Box<dyn CustomNode> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

impl PartialEq for Box<dyn CustomNode> {
    fn eq(&self, other: &Self) -> bool {
        self.eq_dyn(other.as_ref())
    }
}

fn arguments_children(arguments: &Arguments) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
    arguments
        .entries()
        .iter()
        .map(|(key, value)| (argument_key(key), NodeRef::Expr(value)))
        .collect()
}

fn arguments_children_mut(arguments: &mut Arguments) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
    arguments
        .entries_mut()
        .map(|(key, value)| (argument_key(key), NodeMut::Expr(value)))
        .collect()
}

fn argument_key(key: &ArgumentKey) -> ChildKey<'_> {
    match key {
        ArgumentKey::Index(i) => ChildKey::Index(*i),
        ArgumentKey::Name(name) => ChildKey::Key(name),
    }
}

fn node_key(key: &NodeKey) -> ChildKey<'_> {
    match key {
        NodeKey::Index(i) => ChildKey::Index(*i),
        NodeKey::Name(name) => ChildKey::Key(name),
    }
}

fn nodes_children(nodes: &Nodes) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
    nodes
        .entries()
        .iter()
        .map(|(key, node)| (node_key(key), NodeRef::Node(node)))
        .collect()
}

fn nodes_children_mut(nodes: &mut Nodes) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
    nodes
        .entries_mut()
        .map(|(key, node)| (node_key(key), NodeMut::Node(node)))
        .collect()
}

impl Expr {
    /// The children in Twig's order with Twig's keys (`$node->getNodes()`).
    pub fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        use ChildKey::Name;
        let e = NodeRef::Expr;
        match &self.kind {
            ExprKind::Constant(_)
            | ExprKind::ContextVariable(_)
            | ExprKind::AssignContextVariable(_)
            | ExprKind::LocalVariable(_)
            | ExprKind::MacroVariable(_)
            | ExprKind::Parent(_)
            | ExprKind::Empty => Vec::new(),
            ExprKind::AssignMacroVariable(a) => vec![(Name("var"), e(&a.var))],
            ExprKind::Array(a) => {
                let mut children = Vec::with_capacity(a.elements.len() * 2);
                for (i, (key, value)) in a.elements.iter().enumerate() {
                    children.push((ChildKey::Index(2 * i as i64), e(key)));
                    children.push((ChildKey::Index(2 * i as i64 + 1), e(value)));
                }
                children
            }
            ExprKind::ArrowFunction(a) => {
                vec![(Name("expr"), e(&a.expr)), (Name("names"), e(&a.names))]
            }
            ExprKind::List(l) => l
                .items
                .iter()
                .enumerate()
                .map(|(i, item)| (ChildKey::Index(i as i64), e(item)))
                .collect(),
            ExprKind::Unary(u) => vec![(Name("node"), e(&u.node))],
            ExprKind::Binary(b) => {
                let mut children = vec![(Name("left"), e(&b.left)), (Name("right"), e(&b.right))];
                if let Some(test) = &b.test {
                    children.push((Name("test"), e(test)));
                }
                children
            }
            ExprKind::ConditionalTernary(t) => vec![
                (Name("test"), e(&t.test)),
                (Name("left"), e(&t.left)),
                (Name("right"), e(&t.right)),
            ],
            ExprKind::GetAttr(g) => {
                let mut children = vec![
                    (Name("node"), e(&g.node)),
                    (Name("attribute"), e(&g.attribute)),
                ];
                if let Some(arguments) = &g.arguments {
                    children.push((Name("arguments"), e(arguments)));
                }
                children
            }
            ExprKind::MacroReference(m) => {
                let mut children = vec![
                    (Name("template"), e(&m.template)),
                    (Name("arguments"), e(&m.arguments)),
                ];
                if let MacroName::Dynamic(name) = &m.name {
                    children.push((Name("name"), e(name)));
                }
                children
            }
            ExprKind::BlockReference(b) => {
                let mut children = vec![(Name("name"), e(&b.name))];
                if let Some(template) = &b.template {
                    children.push((Name("template"), e(template)));
                }
                children
            }
            ExprKind::Filter(f) => vec![
                (Name("node"), e(&f.node)),
                (Name("filter"), e(&f.filter_name)),
                (Name("arguments"), NodeRef::Arguments(&f.arguments)),
            ],
            ExprKind::Function(f) => vec![(Name("arguments"), NodeRef::Arguments(&f.arguments))],
            ExprKind::Test(t) => {
                let mut children = vec![(Name("node"), e(&t.node))];
                if let Some(arguments) = &t.arguments {
                    children.push((Name("arguments"), NodeRef::Arguments(arguments)));
                }
                children
            }
            ExprKind::CheckToString(c) => vec![(Name("expr"), e(&c.expr))],
            ExprKind::Custom(custom) => custom.children(),
        }
    }

    /// The children in Twig's order, mutably.
    pub fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        use ChildKey::Name;
        let e = NodeMut::Expr;
        match &mut self.kind {
            ExprKind::Constant(_)
            | ExprKind::ContextVariable(_)
            | ExprKind::AssignContextVariable(_)
            | ExprKind::LocalVariable(_)
            | ExprKind::MacroVariable(_)
            | ExprKind::Parent(_)
            | ExprKind::Empty => Vec::new(),
            ExprKind::AssignMacroVariable(a) => vec![(Name("var"), e(&mut a.var))],
            ExprKind::Array(a) => {
                let mut children = Vec::with_capacity(a.elements.len() * 2);
                for (i, (key, value)) in a.elements.iter_mut().enumerate() {
                    children.push((ChildKey::Index(2 * i as i64), e(key)));
                    children.push((ChildKey::Index(2 * i as i64 + 1), e(value)));
                }
                children
            }
            ExprKind::ArrowFunction(a) => {
                vec![
                    (Name("expr"), e(&mut a.expr)),
                    (Name("names"), e(&mut a.names)),
                ]
            }
            ExprKind::List(l) => l
                .items
                .iter_mut()
                .enumerate()
                .map(|(i, item)| (ChildKey::Index(i as i64), e(item)))
                .collect(),
            ExprKind::Unary(u) => vec![(Name("node"), e(&mut u.node))],
            ExprKind::Binary(b) => {
                let b = &mut **b;
                let mut children = vec![
                    (Name("left"), e(&mut b.left)),
                    (Name("right"), e(&mut b.right)),
                ];
                if let Some(test) = &mut b.test {
                    children.push((Name("test"), e(test)));
                }
                children
            }
            ExprKind::ConditionalTernary(t) => {
                let t = &mut **t;
                vec![
                    (Name("test"), e(&mut t.test)),
                    (Name("left"), e(&mut t.left)),
                    (Name("right"), e(&mut t.right)),
                ]
            }
            ExprKind::GetAttr(g) => {
                let g = &mut **g;
                let mut children = vec![
                    (Name("node"), e(&mut g.node)),
                    (Name("attribute"), e(&mut g.attribute)),
                ];
                if let Some(arguments) = &mut g.arguments {
                    children.push((Name("arguments"), e(arguments)));
                }
                children
            }
            ExprKind::MacroReference(m) => {
                let m = &mut **m;
                let mut children = vec![
                    (Name("template"), e(&mut m.template)),
                    (Name("arguments"), e(&mut m.arguments)),
                ];
                if let MacroName::Dynamic(name) = &mut m.name {
                    children.push((Name("name"), e(name)));
                }
                children
            }
            ExprKind::BlockReference(b) => {
                let b = &mut **b;
                let mut children = vec![(Name("name"), e(&mut b.name))];
                if let Some(template) = &mut b.template {
                    children.push((Name("template"), e(template)));
                }
                children
            }
            ExprKind::Filter(f) => {
                let f = &mut **f;
                vec![
                    (Name("node"), e(&mut f.node)),
                    (Name("filter"), e(&mut f.filter_name)),
                    (Name("arguments"), NodeMut::Arguments(&mut f.arguments)),
                ]
            }
            ExprKind::Function(f) => {
                vec![(Name("arguments"), NodeMut::Arguments(&mut f.arguments))]
            }
            ExprKind::Test(t) => {
                let t = &mut **t;
                let mut children = vec![(Name("node"), e(&mut t.node))];
                if let Some(arguments) = &mut t.arguments {
                    children.push((Name("arguments"), NodeMut::Arguments(arguments)));
                }
                children
            }
            ExprKind::CheckToString(c) => vec![(Name("expr"), e(&mut c.expr))],
            ExprKind::Custom(custom) => custom.children_mut(),
        }
    }
}

impl Node {
    /// The children in Twig's order with Twig's keys (`$node->getNodes()`). The `embedded_templates` attribute of
    /// a module is not a child (Twig visits embedded templates separately).
    pub fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        use ChildKey::Name;
        let n = NodeRef::Node;
        let e = NodeRef::Expr;
        match &self.kind {
            NodeKind::Module(m) => {
                let mut children = vec![
                    (Name("body"), n(&m.body)),
                    (Name("blocks"), n(&m.blocks)),
                    (Name("macros"), n(&m.macros)),
                    (Name("traits"), n(&m.traits)),
                    (Name("display_start"), n(&m.display_start)),
                    (Name("display_end"), n(&m.display_end)),
                    (Name("constructor_start"), n(&m.constructor_start)),
                    (Name("constructor_end"), n(&m.constructor_end)),
                    (Name("class_end"), n(&m.class_end)),
                ];
                if let Some(parent) = &m.parent {
                    children.push((Name("parent"), e(parent)));
                }
                children
            }
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) | NodeKind::Macros(nodes) => {
                nodes_children(nodes)
            }
            NodeKind::Types(t) => nodes_children(&t.types),
            NodeKind::Empty
            | NodeKind::Text(_)
            | NodeKind::ForLoop(_)
            | NodeKind::BlockReference(_)
            | NodeKind::MacroDeclaration(_)
            | NodeKind::Config
            | NodeKind::Flush
            | NodeKind::Type(_)
            | NodeKind::CheckSecurity(_)
            | NodeKind::CheckSecurityCall => Vec::new(),
            NodeKind::Print(p) => vec![(Name("expr"), e(&p.expr))],
            NodeKind::If(i) => {
                let mut children = vec![(Name("tests"), n(&i.tests))];
                if let Some(else_) = &i.else_ {
                    children.push((Name("else"), n(else_)));
                }
                children
            }
            NodeKind::For(f) => {
                let mut children = vec![
                    (Name("key_target"), e(&f.key_target)),
                    (Name("value_target"), e(&f.value_target)),
                    (Name("seq"), e(&f.seq)),
                    (Name("body"), n(&f.body)),
                ];
                if let Some(else_) = &f.else_ {
                    children.push((Name("else"), n(else_)));
                }
                children
            }
            NodeKind::ForElse(f) => vec![(Name("body"), n(&f.body))],
            NodeKind::Set(s) => vec![(Name("names"), n(&s.names)), (Name("values"), n(&s.values))],
            NodeKind::Capture(c) => vec![(Name("body"), n(&c.body))],
            NodeKind::Block(b) => vec![(Name("body"), n(&b.body))],
            NodeKind::Macro(m) => vec![
                (Name("body"), n(&m.body)),
                (Name("arguments"), e(&m.arguments)),
            ],
            NodeKind::Import(i) => vec![(Name("expr"), e(&i.expr)), (Name("var"), e(&i.var))],
            NodeKind::Include(i) => {
                let mut children = vec![(Name("expr"), e(&i.expr))];
                if let Some(variables) = &i.variables {
                    children.push((Name("variables"), e(variables)));
                }
                children
            }
            NodeKind::Embed(i) => {
                let mut children = vec![(Name("expr"), e(&i.expr))];
                if let Some(variables) = &i.variables {
                    children.push((Name("variables"), e(variables)));
                }
                children
            }
            NodeKind::With(w) => {
                let mut children = vec![(Name("body"), n(&w.body))];
                if let Some(variables) = &w.variables {
                    children.push((Name("variables"), e(variables)));
                }
                children
            }
            NodeKind::AutoEscape(a) => vec![(Name("body"), n(&a.body))],
            NodeKind::Sandbox(s) => vec![(Name("body"), n(&s.body))],
            NodeKind::Do(d) => vec![(Name("expr"), e(&d.expr))],
            NodeKind::Deprecated(d) => {
                let mut children = vec![(Name("expr"), e(&d.expr))];
                let package = d.package.as_ref().map(|p| (Name("package"), e(p)));
                let version = d.version.as_ref().map(|v| (Name("version"), e(v)));
                if d.version_first {
                    children.extend(version);
                    children.extend(package);
                } else {
                    children.extend(package);
                    children.extend(version);
                }
                children
            }
            NodeKind::Expression(expr) => expr.children(),
            NodeKind::Custom(custom) => custom.children(),
        }
    }

    /// The children in Twig's order, mutably.
    pub fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        use ChildKey::Name;
        let n = NodeMut::Node;
        let e = NodeMut::Expr;
        match &mut self.kind {
            NodeKind::Module(m) => {
                let m = &mut **m;
                let mut children = vec![
                    (Name("body"), n(&mut m.body)),
                    (Name("blocks"), n(&mut m.blocks)),
                    (Name("macros"), n(&mut m.macros)),
                    (Name("traits"), n(&mut m.traits)),
                    (Name("display_start"), n(&mut m.display_start)),
                    (Name("display_end"), n(&mut m.display_end)),
                    (Name("constructor_start"), n(&mut m.constructor_start)),
                    (Name("constructor_end"), n(&mut m.constructor_end)),
                    (Name("class_end"), n(&mut m.class_end)),
                ];
                if let Some(parent) = &mut m.parent {
                    children.push((Name("parent"), e(parent)));
                }
                children
            }
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) | NodeKind::Macros(nodes) => {
                nodes_children_mut(nodes)
            }
            NodeKind::Types(t) => nodes_children_mut(&mut t.types),
            NodeKind::Empty
            | NodeKind::Text(_)
            | NodeKind::ForLoop(_)
            | NodeKind::BlockReference(_)
            | NodeKind::MacroDeclaration(_)
            | NodeKind::Config
            | NodeKind::Flush
            | NodeKind::Type(_)
            | NodeKind::CheckSecurity(_)
            | NodeKind::CheckSecurityCall => Vec::new(),
            NodeKind::Print(p) => vec![(Name("expr"), e(&mut p.expr))],
            NodeKind::If(i) => {
                let i = &mut **i;
                let mut children = vec![(Name("tests"), n(&mut i.tests))];
                if let Some(else_) = &mut i.else_ {
                    children.push((Name("else"), n(else_)));
                }
                children
            }
            NodeKind::For(f) => {
                let f = &mut **f;
                let mut children = vec![
                    (Name("key_target"), e(&mut f.key_target)),
                    (Name("value_target"), e(&mut f.value_target)),
                    (Name("seq"), e(&mut f.seq)),
                    (Name("body"), n(&mut f.body)),
                ];
                if let Some(else_) = &mut f.else_ {
                    children.push((Name("else"), n(else_)));
                }
                children
            }
            NodeKind::ForElse(f) => vec![(Name("body"), n(&mut f.body))],
            NodeKind::Set(s) => {
                let s = &mut **s;
                vec![
                    (Name("names"), n(&mut s.names)),
                    (Name("values"), n(&mut s.values)),
                ]
            }
            NodeKind::Capture(c) => vec![(Name("body"), n(&mut c.body))],
            NodeKind::Block(b) => vec![(Name("body"), n(&mut b.body))],
            NodeKind::Macro(m) => {
                let m = &mut **m;
                vec![
                    (Name("body"), n(&mut m.body)),
                    (Name("arguments"), e(&mut m.arguments)),
                ]
            }
            NodeKind::Import(i) => {
                let i = &mut **i;
                vec![(Name("expr"), e(&mut i.expr)), (Name("var"), e(&mut i.var))]
            }
            NodeKind::Include(i) => {
                let i = &mut **i;
                let mut children = vec![(Name("expr"), e(&mut i.expr))];
                if let Some(variables) = &mut i.variables {
                    children.push((Name("variables"), e(variables)));
                }
                children
            }
            NodeKind::Embed(i) => {
                let i = &mut **i;
                let mut children = vec![(Name("expr"), e(&mut i.expr))];
                if let Some(variables) = &mut i.variables {
                    children.push((Name("variables"), e(variables)));
                }
                children
            }
            NodeKind::With(w) => {
                let w = &mut **w;
                let mut children = vec![(Name("body"), n(&mut w.body))];
                if let Some(variables) = &mut w.variables {
                    children.push((Name("variables"), e(variables)));
                }
                children
            }
            NodeKind::AutoEscape(a) => vec![(Name("body"), n(&mut a.body))],
            NodeKind::Sandbox(s) => vec![(Name("body"), n(&mut s.body))],
            NodeKind::Do(d) => vec![(Name("expr"), e(&mut d.expr))],
            NodeKind::Deprecated(d) => {
                let d = &mut **d;
                let version_first = d.version_first;
                let mut children = vec![(Name("expr"), e(&mut d.expr))];
                let package = d.package.as_mut().map(|p| (Name("package"), e(p)));
                let version = d.version.as_mut().map(|v| (Name("version"), e(v)));
                if version_first {
                    children.extend(version);
                    children.extend(package);
                } else {
                    children.extend(package);
                    children.extend(version);
                }
                children
            }
            NodeKind::Expression(expr) => expr.children_mut(),
            NodeKind::Custom(custom) => custom.children_mut(),
        }
    }
}

impl Arguments {
    /// The arguments as children (keys are positions or names).
    pub fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        arguments_children(self)
    }

    /// The arguments as children, mutably.
    pub fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        arguments_children_mut(self)
    }
}
