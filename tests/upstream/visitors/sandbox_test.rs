//! Twig 3.31.1's `tests/NodeVisitor/SandboxTest.php`: the `SandboxNodeVisitor` alone over hand-built module trees
//! (`new NodeTraverser($env, [new SandboxNodeVisitor($env)])`).

use std::any::Any;
use std::sync::Arc;

use twig::Source;
use twig::ast::{
    ChildKey, CustomExpr, CustomNode, Expr, ExprKind, Node, NodeKind, NodeMut, NodeRef, Nodes,
};
use twig::deprecation::Deprecation;
use twig::extension::ExtensionSet;
use twig::visitor::{NodeTraverser, SandboxNodeVisitor};

/// `new ModuleNode(new BodyNode([$node]), null, new EmptyNode(), new MacrosNode(), new EmptyNode(), new EmptyNode(),
/// new Source('foo', 'foo'))` after the sandbox visitor ran; returns `$node->getNode('body')->getNode(0)`.
fn traverse(node: Node) -> Node {
    let mut module = Node::module(
        Node::body(Nodes::from_list([node]), 0),
        None,
        Node::empty(0),
        Node::new(NodeKind::Macros(Nodes::new()), 0),
        Node::empty(0),
        Node::empty(0),
        Source::new("foo", "foo"),
    );
    let extensions = ExtensionSet::with_core_extensions();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    NodeTraverser::new(&extensions, [Arc::new(SandboxNodeVisitor) as _])
        .traverse(&mut module, &mut deprecations)
        .unwrap();
    let NodeKind::Module(module) = module.kind else {
        unreachable!()
    };
    let NodeKind::Body(body) = module.body.kind else {
        panic!("the body is not a BodyNode: {:?}", module.body)
    };
    body.into_entries().remove(0).1
}

fn is_check_to_string(expr: &Expr) -> bool {
    matches!(expr.kind, ExprKind::CheckToString(_))
}

/// `CustomCoercingExpression`: an expression with `left` and `right` children that implements
/// `CoercesChildrenToStringInterface` for both.
#[derive(Debug, Clone, PartialEq)]
struct CustomCoercingExpression {
    left: Expr,
    right: Expr,
}

impl CustomExpr for CustomCoercingExpression {
    fn class_name(&self) -> &str {
        "Twig\\Tests\\NodeVisitor\\CustomCoercingExpression"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![
            (ChildKey::Name("left"), NodeRef::Expr(&self.left)),
            (ChildKey::Name("right"), NodeRef::Expr(&self.right)),
        ]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![
            (ChildKey::Name("left"), NodeMut::Expr(&mut self.left)),
            (ChildKey::Name("right"), NodeMut::Expr(&mut self.right)),
        ]
    }

    fn clone_box(&self) -> Box<dyn CustomExpr> {
        Box::new(self.clone())
    }

    fn eq_dyn(&self, other: &dyn CustomExpr) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn string_coerced_child_names(&self) -> Vec<String> {
        vec!["left".to_owned(), "right".to_owned()]
    }
}

/// `CustomCoercingNode`: a statement node with an `expr` child that implements
/// `CoercesChildrenToStringInterface` for it.
#[derive(Debug, Clone, PartialEq)]
struct CustomCoercingNode {
    expr: Expr,
}

impl CustomNode for CustomCoercingNode {
    fn class_name(&self) -> &str {
        "Twig\\Tests\\NodeVisitor\\CustomCoercingNode"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![(ChildKey::Name("expr"), NodeRef::Expr(&self.expr))]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![(ChildKey::Name("expr"), NodeMut::Expr(&mut self.expr))]
    }

    fn clone_box(&self) -> Box<dyn CustomNode> {
        Box::new(self.clone())
    }

    fn eq_dyn(&self, other: &dyn CustomNode) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn string_coerced_child_names(&self) -> Vec<String> {
        vec!["expr".to_owned()]
    }
}

/// The `expr` child of a `CustomCoercingNode` in the body.
fn custom_node_expr(node: &Node) -> &Expr {
    let NodeKind::Custom(custom) = &node.kind else {
        panic!("expected the custom node, got {node:?}");
    };
    &custom
        .as_any()
        .downcast_ref::<CustomCoercingNode>()
        .expect("a CustomCoercingNode")
        .expr
}

/// `testCustomNodeImplementingCoercesChildrenToStringInterfaceIsWrapped()`: the custom expression is printed, and
/// the wrapping happens on the custom node itself.
#[test]
fn custom_node_implementing_coerces_children_to_string_interface_is_wrapped() {
    let custom = Expr::new(
        ExprKind::Custom(Box::new(CustomCoercingExpression {
            left: Expr::context_variable("foo", 1),
            right: Expr::context_variable("bar", 1),
        })),
        1,
    );
    let print = traverse(Node::print(custom, 1));
    let NodeKind::Print(print) = &print.kind else {
        panic!("expected a print, got {print:?}");
    };
    let ExprKind::Custom(custom) = &print.expr.kind else {
        panic!("expected the custom expression, got {:?}", print.expr);
    };
    let custom = custom
        .as_any()
        .downcast_ref::<CustomCoercingExpression>()
        .expect("a CustomCoercingExpression");
    assert!(is_check_to_string(&custom.left), "{:?}", custom.left);
    assert!(is_check_to_string(&custom.right), "{:?}", custom.right);
}

/// `testCustomNonExpressionNodeImplementingCoercesChildrenToStringInterfaceIsWrapped()`.
#[test]
fn custom_non_expression_node_implementing_coerces_children_to_string_interface_is_wrapped() {
    let custom = Node::new(
        NodeKind::Custom(Box::new(CustomCoercingNode {
            expr: Expr::context_variable("foo", 1),
        })),
        1,
    );
    let custom = traverse(custom);
    assert!(is_check_to_string(custom_node_expr(&custom)));
}

/// `testSelfIsNeverWrapped()`.
#[test]
fn self_is_never_wrapped() {
    let custom = Node::new(
        NodeKind::Custom(Box::new(CustomCoercingNode {
            expr: Expr::context_variable("_self", 1),
        })),
        1,
    );
    let custom = traverse(custom);
    assert!(!is_check_to_string(custom_node_expr(&custom)));
}
