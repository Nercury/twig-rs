//! The syntax tree API: node constructors (ports of the PHP constructors), generic children, traversal, custom node
//! classes and the dump format.

use std::any::Any;

use twig::Source;
use twig::ast::{
    AttrValue, BinaryOp, ChildKey, ConstantValue, CustomExpr, CustomNode, Expr, ExprKind, Node,
    NodeKey, NodeKind, NodeMut, NodeRef, Nodes, TempName, TextNode, dump::dump_node,
};
use twig::deprecation::Deprecation;
use twig::extension::ExtensionSet;
use twig::visitor::{Visit, VisitContext, VisitResult, traverse};
use twig::{ErrorKind, Result};

use super::support::parse;

fn local(name: &str) -> Expr {
    Expr::local_variable(name, 1).unwrap()
}

#[test]
fn set_node_rewrites_captures() {
    let names = || {
        Node::nodes(
            Nodes::from_list([Node::expression(
                Expr::assign_context_variable("a", 1).unwrap(),
            )]),
            0,
        )
    };
    // empty body
    let node = Node::set(true, names(), Node::nodes(Nodes::new(), 3), 1);
    let NodeKind::Set(set) = &node.kind else {
        panic!()
    };
    assert!(!set.capture && set.safe);
    assert_eq!(
        set.values.as_expr().and_then(Expr::as_constant),
        Some(&ConstantValue::string(""))
    );
    assert_eq!(set.values.template_line(), 3);
    // a single text
    let node = Node::set(true, names(), Node::text("abc", 2), 1);
    let NodeKind::Set(set) = &node.kind else {
        panic!()
    };
    assert!(!set.capture && set.safe);
    assert_eq!(
        set.values.as_expr().and_then(Expr::as_constant),
        Some(&ConstantValue::string("abc"))
    );
    // a print of a constant
    let node = Node::set(true, names(), Node::print(Expr::constant(5, 2), 2), 1);
    let NodeKind::Set(set) = &node.kind else {
        panic!()
    };
    assert!(!set.capture);
    assert_eq!(
        set.values.as_expr().and_then(Expr::as_constant),
        Some(&ConstantValue::Int(5))
    );
    // anything else is captured
    let body = Node::nodes(
        Nodes::from_list([Node::text("a", 1), Node::text("b", 1)]),
        1,
    );
    let node = Node::set(true, names(), body, 1);
    let NodeKind::Set(set) = &node.kind else {
        panic!()
    };
    assert!(set.capture && set.safe);
    assert!(matches!(set.values.kind, NodeKind::Capture(_)));
    // assignments are not rewritten
    let values = Node::nodes(
        Nodes::from_list([Node::expression(Expr::constant(1, 1))]),
        0,
    );
    let node = Node::set(false, names(), values, 1);
    let NodeKind::Set(set) = &node.kind else {
        panic!()
    };
    assert!(!set.capture && !set.safe);
}

#[test]
fn if_node_wraps_conditions() {
    let tests = Nodes::from_list([
        Node::expression(Expr::context_variable("a", 1)),
        Node::text("x", 1),
        Node::expression(Expr::constant(true, 2)),
        Node::text("y", 2),
    ]);
    let node = Node::if_(tests, None, 1);
    let NodeKind::If(if_node) = &node.kind else {
        panic!()
    };
    let tests = if_node.tests.as_nodes().unwrap();
    let first = tests.get(&NodeKey::Index(0)).unwrap().as_expr().unwrap();
    assert!(matches!(first.kind, ExprKind::Test(ref t) if t.test.name() == b"true"));
    let third = tests.get(&NodeKey::Index(2)).unwrap().as_expr().unwrap();
    assert!(
        matches!(third.kind, ExprKind::Constant(_)),
        "constants return a primitive type"
    );
}

#[test]
fn for_node_appends_the_loop_node() {
    let node = Node::for_(
        Expr::assign_context_variable("_key", 1).unwrap(),
        Expr::assign_context_variable("v", 1).unwrap(),
        Expr::context_variable("items", 1),
        Node::text("x", 1),
        None,
        1,
    );
    let NodeKind::For(for_node) = &node.kind else {
        panic!()
    };
    assert!(for_node.with_loop);
    let body = for_node.body.as_nodes().unwrap();
    assert_eq!(body.len(), 2);
    assert!(matches!(
        body.get(&NodeKey::Index(1)).unwrap().kind,
        NodeKind::ForLoop(_)
    ));
    let keys: Vec<String> = node.children().iter().map(|(k, _)| k.to_string()).collect();
    assert_eq!(keys, ["key_target", "value_target", "seq", "body"]);
}

fn macro_arguments(names: &[&str]) -> Expr {
    let mut arguments = twig::ast::ArrayExpr::new();
    for name in names {
        arguments.add_element(Expr::constant(ConstantValue::Null, 4), Some(local(name)));
    }
    arguments.into_expr(1)
}

#[test]
fn macro_node_checks_argument_names() {
    let body = || Node::body(Nodes::new(), 0);
    let error = Node::macro_("m", body(), macro_arguments(&["varargs"]), 1, None).unwrap_err();
    assert_eq!(
        error.raw_message(),
        "The argument \"varargs\" in macro \"m\" cannot be defined because the variable \"varargs\" is reserved for arbitrary arguments."
    );
    assert_eq!(error.template_line(), 4);
    let error =
        Node::macro_("m", body(), macro_arguments(&["a"]), 1, Some(b"a".to_vec())).unwrap_err();
    assert_eq!(
        error.raw_message(),
        "The variadic argument \"a\" in macro \"m\" cannot have the same name as another argument."
    );
    let error = Node::macro_("m", body(), macro_arguments(&["a", "a"]), 1, None).unwrap_err();
    assert_eq!(
        error.raw_message(),
        "Argument \"a\" is defined twice for macro \"m\"."
    );
    // reserved names are prefixed and compared without the prefix
    let error = Node::macro_(
        "m",
        body(),
        macro_arguments(&["context", "context"]),
        1,
        None,
    )
    .unwrap_err();
    assert_eq!(
        error.raw_message(),
        "Argument \"context\" is defined twice for macro \"m\"."
    );
    let error = Node::macro_(
        "m",
        body(),
        macro_arguments(&["context"]),
        1,
        Some(b"context".to_vec()),
    )
    .unwrap_err();
    assert_eq!(
        error.raw_message(),
        "The variadic argument \"context\" in macro \"m\" cannot have the same name as another argument."
    );
    assert!(
        Node::macro_(
            "m",
            body(),
            macro_arguments(&["a", "b"]),
            1,
            Some(b"c".to_vec())
        )
        .is_ok()
    );
}

#[test]
fn temp_names_are_checked_and_normalized() {
    let error = Expr::local_variable("NULL", 7).unwrap_err();
    assert_eq!(
        error.to_string(),
        "You cannot assign a value to \"NULL\" at line 7."
    );
    assert!(matches!(
        local("12").kind,
        ExprKind::LocalVariable(TempName::Int(12))
    ));
    let ExprKind::LocalVariable(TempName::String(name)) = local("this").kind else {
        panic!()
    };
    assert_eq!(name, "\u{035C}this".as_bytes());
    let error = Expr::assign_context_variable("False", 2).unwrap_err();
    assert_eq!(
        error.raw_message(),
        "You cannot assign a value to \"False\"."
    );
}

#[test]
fn text_nodes_know_when_they_are_blank() {
    let blank = |data: &[u8]| {
        TextNode {
            data: data.to_vec(),
        }
        .is_blank()
    };
    assert!(blank(b" \t\n\r\x0b\x0c"));
    assert!(!blank(b""));
    assert!(!blank(b" \0"));
    assert!(blank(b"\xEF\xBB\xBF"));
    assert!(blank(b"\xEF\xBB\xBF \n"));
    assert!(!blank(b"\xEF\xBB\xBFx"));
}

#[test]
fn null_coalesce_shares_the_always_defined_left_operand() {
    let node = Expr::binary(
        BinaryOp::NullCoalesce,
        Expr::context_variable("a", 1),
        Expr::constant(1, 1),
        1,
    )
    .unwrap();
    let ExprKind::Binary(binary) = &node.kind else {
        panic!()
    };
    assert!(binary.left.always_defined);
    let ExprKind::Binary(test) = &binary.test.as_ref().unwrap().kind else {
        panic!()
    };
    let ExprKind::Test(defined) = &test.left.kind else {
        panic!()
    };
    assert!(
        !defined.node.always_defined,
        "the defined test uses a clone taken before"
    );
    assert!(defined.node.is_defined_test_enabled());
    let error = Expr::binary(
        BinaryOp::NullCoalesce,
        Expr::list(Vec::new(), 1),
        Expr::constant(1, 1),
        1,
    )
    .unwrap_err();
    assert_eq!(
        error.raw_message(),
        "The \"defined\" test only works with simple variables."
    );
}

#[test]
fn deprecated_node_children_keep_their_order() {
    let mut node = Node::deprecated(Expr::constant("m", 1), 1);
    let NodeKind::Deprecated(deprecated) = &mut node.kind else {
        panic!()
    };
    deprecated.set_version(Expr::constant("1.0", 1));
    deprecated.set_package(Expr::constant("a/b", 1));
    let keys: Vec<String> = node.children().iter().map(|(k, _)| k.to_string()).collect();
    assert_eq!(keys, ["expr", "version", "package"]);
}

#[test]
fn types_node_has_one_child_per_declaration() {
    let node = Node::types(
        vec![
            (b"a".to_vec(), b"string".to_vec(), false),
            (b"b".to_vec(), b"int".to_vec(), true),
        ],
        3,
    );
    let children = node.children();
    assert_eq!(children.len(), 2);
    let NodeRef::Node(child) = children[1].1 else {
        panic!()
    };
    assert_eq!(child.line, 3);
    assert_eq!(
        child
            .attributes()
            .into_iter()
            .map(|(k, v)| (k.into_owned(), v))
            .collect::<Vec<_>>(),
        [
            ("name".to_owned(), AttrValue::string("b")),
            ("type".to_owned(), AttrValue::string("int")),
            ("optional".to_owned(), AttrValue::Bool(true)),
        ]
    );
}

#[test]
fn modules_list_twigs_children() {
    let (module, _) = parse("x");
    let module = module.unwrap();
    let keys: Vec<String> = module
        .children()
        .iter()
        .map(|(k, _)| k.to_string())
        .collect();
    assert_eq!(
        keys,
        [
            "body",
            "blocks",
            "macros",
            "traits",
            "display_start",
            "display_end",
            "constructor_start",
            "constructor_end",
            "class_end"
        ]
    );
    assert_eq!(module.line, 1);
    assert_eq!(module.as_module().unwrap().source.name(), "index");
}

/// One pass of `visit` over `node`, without extensions; the deprecations are dropped.
fn visit(node: &mut Node, visit: &mut dyn Visit) -> Result<VisitResult> {
    let extensions = ExtensionSet::new();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let mut cx = VisitContext::new(&extensions, &mut deprecations);
    traverse(NodeMut::Node(node), visit, &mut cx)
}

/// A visitor renaming variables and removing texts.
struct Rename {
    entered: usize,
}

impl Visit for Rename {
    fn enter(&mut self, _node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<()> {
        self.entered += 1;
        Ok(())
    }

    fn leave(&mut self, node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<VisitResult> {
        match node {
            NodeMut::Expr(expr) => {
                if let ExprKind::ContextVariable(name) = &mut expr.kind {
                    name.name = b"renamed".to_vec();
                }
                Ok(VisitResult::Keep)
            }
            NodeMut::Node(node) if matches!(node.kind, NodeKind::Text(_)) => {
                Ok(VisitResult::Remove)
            }
            _ => Ok(VisitResult::Keep),
        }
    }
}

#[test]
fn traverses_and_rewrites_like_the_node_traverser() {
    let (module, _) = parse("a{{ x + y|upper }}b{{ z }}");
    let mut module = module.unwrap();
    let mut visitor = Rename { entered: 0 };
    visit(&mut module, &mut visitor).unwrap();
    assert!(visitor.entered > 10);
    let dump = dump_node(&module);
    assert!(!dump.contains("TextNode"), "{dump}");
    assert!(!dump.contains("\"x\""), "{dump}");
    assert_eq!(dump.matches("\"renamed\"").count(), 3, "{dump}");
    // removed children keep the keys of the others
    let body = module.as_module().unwrap().body.as_nodes().unwrap();
    let keys: Vec<NodeKey> = body
        .nodes()
        .next()
        .unwrap()
        .as_nodes()
        .unwrap()
        .entries()
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    assert_eq!(keys, [NodeKey::Index(1), NodeKey::Index(3)]);
}

struct RemoveExpressions;

impl Visit for RemoveExpressions {
    fn enter(&mut self, _node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<()> {
        Ok(())
    }

    fn leave(&mut self, node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<VisitResult> {
        Ok(if matches!(node, NodeMut::Expr(_)) {
            VisitResult::Remove
        } else {
            VisitResult::Keep
        })
    }
}

#[test]
fn only_container_children_can_be_removed() {
    let (module, _) = parse("{{ x }}");
    let mut module = module.unwrap();
    let error = visit(&mut module, &mut RemoveExpressions).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "Removing the child \"expr\" of a \"Twig\\Node\\PrintNode\" node is not supported."
    );
}

#[derive(Debug, Clone, PartialEq)]
struct Upper {
    inner: Expr,
}

impl CustomExpr for Upper {
    fn class_name(&self) -> &str {
        "Acme\\UpperExpression"
    }

    fn attributes(&self) -> Vec<(String, AttrValue)> {
        vec![("mode".to_owned(), AttrValue::string("upper"))]
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![(ChildKey::Name("node"), NodeRef::Expr(&self.inner))]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![(ChildKey::Name("node"), NodeMut::Expr(&mut self.inner))]
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

    fn returns_primitive(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Cache {
    body: Node,
}

impl CustomNode for Cache {
    fn class_name(&self) -> &str {
        "Acme\\CacheNode"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![(ChildKey::Name("body"), NodeRef::Node(&self.body))]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![(ChildKey::Name("body"), NodeMut::Node(&mut self.body))]
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

    fn is_output(&self) -> bool {
        true
    }
}

#[test]
fn custom_nodes_take_part_in_dumps_traversal_and_equality() {
    let custom = Expr::new(
        ExprKind::Custom(Box::new(Upper {
            inner: Expr::context_variable("x", 2),
        })),
        2,
    );
    assert_eq!(custom.class_name(), "Acme\\UpperExpression");
    assert!(
        Expr::true_test_wrap(custom.clone()) == custom,
        "a primitive custom expression is not wrapped"
    );
    let node = Node::new(
        NodeKind::Custom(Box::new(Cache {
            body: Node::print(custom.clone(), 2),
        })),
        2,
    );
    assert!(node.is_output());
    assert_eq!(node.clone(), node);
    let dump = dump_node(&node);
    assert_eq!(
        dump,
        "Acme\\CacheNode @2\n  [body] PrintNode @2\n    [expr] Acme\\UpperExpression @2\n      .mode = \"upper\"\n      [node] Expression\\Variable\\ContextVariable @2\n        .name = \"x\"\n        .ignore_strict_check = false\n        .always_defined = false"
    );
    let mut node = node;
    let mut visitor = Rename { entered: 0 };
    visit(&mut node, &mut visitor).unwrap();
    assert!(dump_node(&node).contains("\"renamed\""));
}

#[test]
fn dumps_values_like_the_oracle_generator() {
    let (module, _) = parse("{{ 'a\"b\\\\c\n' }}{{ 1.5 }}{{ 1e100 }}{{ -0.0 }}");
    let dump = dump_node(&module.unwrap());
    assert!(dump.contains(".value = \"a\\\"b\\\\c\\x0a\""), "{dump}");
    assert!(dump.contains(".value = float(1.5)"), "{dump}");
    assert!(dump.contains(".value = float(1.0E+100)"), "{dump}");
    assert!(
        dump.contains("[.embedded_templates] EmptyNode @0"),
        "{dump}"
    );
}

#[test]
fn embed_nodes_have_a_dummy_expression() {
    let node = Node::embed("index", 1, None, true, false, 3);
    let NodeKind::Embed(embed) = &node.kind else {
        panic!()
    };
    assert_eq!(
        embed.expr.as_constant(),
        Some(&ConstantValue::string("not_used"))
    );
    let attributes: Vec<String> = node
        .attributes()
        .into_iter()
        .map(|(k, _)| k.into_owned())
        .collect();
    assert_eq!(attributes, ["only", "ignore_missing", "name", "index"]);
}

#[test]
fn module_nodes_expose_blocks_and_macros() {
    let mut blocks = Nodes::new();
    blocks.set(
        NodeKey::Name(b"b".to_vec()),
        Node::body(Nodes::from_list([Node::block("b", Node::empty(0), 2)]), 2),
    );
    let module = Node::module(
        Node::body(Nodes::new(), 0),
        None,
        Node::nodes(blocks, 0),
        Node::new(NodeKind::Macros(Nodes::new()), 0),
        Node::empty(0),
        Node::empty(0),
        Source::new("", "t"),
    );
    let module = module.as_module().unwrap();
    let names: Vec<&[u8]> = module.blocks().map(|(name, _)| name).collect();
    assert_eq!(names, [b"b".as_slice()]);
    assert_eq!(module.macros().count(), 0);
}
