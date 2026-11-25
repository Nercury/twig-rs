//! The node traverser and the `CorrectnessNodeVisitor`: ports of Twig 3.31.1's `tests/NodeTraverserTest.php` and
//! `tests/NodeVisitor/CorrectnessTest.php` (TWIG/tests/).

use std::sync::{Arc, Mutex};

use twig::ast::{Expr, Node, NodeKey, NodeKind, NodeMut, Nodes};
use twig::deprecation::Deprecation;
use twig::extension::ExtensionSet;
use twig::visitor::{
    CorrectnessNodeVisitor, NodeTraverser, NodeVisitor, Visit, VisitContext, VisitResult, priority,
};
use twig::{ErrorKind, Result, Source};

/// `NodeTraverserTestNode`: a node with a name (kept in the tag) and children (a `Nodes` container, so children can
/// be removed).
fn named(name: &str, children: Vec<(&str, Node)>) -> Node {
    let mut nodes = Nodes::new();
    for (key, child) in children {
        nodes.set(NodeKey::Name(key.as_bytes().to_vec()), child);
    }
    let mut node = Node::nodes(nodes, 0);
    node.tag = Some(name.to_owned());
    node
}

fn leaf(name: &str) -> Node {
    named(name, Vec::new())
}

fn name_of(node: &NodeMut<'_>) -> String {
    match node {
        NodeMut::Node(node) => node.tag.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

type Hook = Box<dyn Fn(&mut Node) -> Option<VisitResult> + Send + Sync>;

/// `NodeTraverserTestVisitor`: logs every enter and leave; optional hooks replace or remove nodes.
struct LoggingVisitor {
    name: &'static str,
    priority: i64,
    log: Arc<Mutex<Vec<String>>>,
    on_enter: Option<Hook>,
    on_leave: Option<Hook>,
}

impl LoggingVisitor {
    fn new(name: &'static str, priority: i64, log: &Arc<Mutex<Vec<String>>>) -> Self {
        LoggingVisitor {
            name,
            priority,
            log: Arc::clone(log),
            on_enter: None,
            on_leave: None,
        }
    }
}

struct LoggingPass<'a>(&'a LoggingVisitor);

impl NodeVisitor for LoggingVisitor {
    fn priority(&self) -> i64 {
        self.priority
    }

    fn start<'a>(&'a self, _extensions: &'a ExtensionSet) -> Box<dyn Visit + 'a> {
        Box::new(LoggingPass(self))
    }
}

impl Visit for LoggingPass<'_> {
    fn enter(&mut self, mut node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<()> {
        let entry = format!("{} enters {}", self.0.name, name_of(&node));
        self.0.log.lock().unwrap().push(entry);
        if let (Some(hook), NodeMut::Node(node)) = (&self.0.on_enter, &mut node) {
            hook(node);
        }
        Ok(())
    }

    fn leave(&mut self, mut node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<VisitResult> {
        let entry = format!("{} leaves {}", self.0.name, name_of(&node));
        self.0.log.lock().unwrap().push(entry);
        if let (Some(hook), NodeMut::Node(node)) = (&self.0.on_leave, &mut node)
            && let Some(result) = hook(node)
        {
            return Ok(result);
        }
        Ok(VisitResult::Keep)
    }
}

fn traverse(tree: &mut Node, visitors: Vec<Arc<dyn NodeVisitor>>) -> Result<()> {
    let extensions = ExtensionSet::new();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    NodeTraverser::new(&extensions, visitors).traverse(tree, &mut deprecations)
}

fn children_names(node: &Node) -> Vec<(String, String)> {
    node.as_nodes()
        .unwrap()
        .entries()
        .iter()
        .map(|(key, child)| (key.to_string(), child.tag.clone().unwrap_or_default()))
        .collect()
}

/// `NodeTraverserTest::testVisitorsTraverseTheWholeTreeOneAfterTheOtherByPriority()`.
#[test]
fn visitors_traverse_the_whole_tree_one_after_the_other_by_priority() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut tree = named(
        "root",
        vec![("a", leaf("a")), ("b", named("b", vec![("0", leaf("c"))]))],
    );
    traverse(
        &mut tree,
        vec![
            Arc::new(LoggingVisitor::new("late", 10, &log)),
            Arc::new(LoggingVisitor::new("early", -10, &log)),
            Arc::new(LoggingVisitor::new("middle", 0, &log)),
        ],
    )
    .unwrap();
    let mut expected = Vec::new();
    for visitor in ["early", "middle", "late"] {
        for step in [
            "enters root",
            "enters a",
            "leaves a",
            "enters b",
            "enters c",
            "leaves c",
            "leaves b",
            "leaves root",
        ] {
            expected.push(format!("{visitor} {step}"));
        }
    }
    assert_eq!(*log.lock().unwrap(), expected);
}

/// Visitors with the same priority run in registration order.
#[test]
fn visitors_with_the_same_priority_run_in_registration_order() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut tree = leaf("root");
    traverse(
        &mut tree,
        vec![
            Arc::new(LoggingVisitor::new("first", 0, &log)),
            Arc::new(LoggingVisitor::new("before", -1, &log)),
            Arc::new(LoggingVisitor::new("second", 0, &log)),
        ],
    )
    .unwrap();
    assert_eq!(
        *log.lock().unwrap(),
        [
            "before enters root",
            "before leaves root",
            "first enters root",
            "first leaves root",
            "second enters root",
            "second leaves root"
        ]
    );
}

/// `NodeTraverserTest::testVisitorsCanReplaceAndRemoveNodes()`.
#[test]
fn visitors_can_replace_and_remove_nodes() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut tree = named(
        "root",
        vec![
            (
                "replacedOnEnter",
                named("old", vec![("0", leaf("old child"))]),
            ),
            ("removed", leaf("removed")),
            ("replacedOnLeave", leaf("leaving")),
        ],
    );
    let mut visitor = LoggingVisitor::new("visitor", 0, &log);
    visitor.on_enter = Some(Box::new(|node: &mut Node| {
        if node.tag.as_deref() == Some("old") {
            *node = named("new", vec![("0", leaf("new child"))]);
        }
        None
    }));
    visitor.on_leave = Some(Box::new(|node: &mut Node| match node.tag.as_deref() {
        Some("removed") => Some(VisitResult::Remove),
        Some("leaving") => {
            *node = leaf("left");
            None
        }
        _ => None,
    }));
    traverse(&mut tree, vec![Arc::new(visitor)]).unwrap();
    assert_eq!(
        *log.lock().unwrap(),
        [
            "visitor enters root",
            "visitor enters old",
            "visitor enters new child",
            "visitor leaves new child",
            "visitor leaves new",
            "visitor enters removed",
            "visitor leaves removed",
            "visitor enters leaving",
            "visitor leaves leaving",
            "visitor leaves root",
        ]
    );
    assert_eq!(
        children_names(&tree),
        [
            ("replacedOnEnter".to_owned(), "new".to_owned()),
            ("replacedOnLeave".to_owned(), "left".to_owned()),
        ]
    );
}

/// `NodeTraverserTest::testChildrenAreTheOnesTheNodeHadAfterBeingEntered()`, for the changes a visitor can make to
/// the node it enters (a visitor cannot reach the ancestors of a node in Rust).
#[test]
fn children_are_the_ones_the_node_had_after_being_entered() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut tree = named("root", vec![("a", leaf("a")), ("b", leaf("b"))]);
    let mut visitor = LoggingVisitor::new("visitor", 0, &log);
    visitor.on_enter = Some(Box::new(|node: &mut Node| {
        if node.tag.as_deref() == Some("root") {
            let nodes = node.as_nodes_mut().unwrap();
            nodes.remove(&NodeKey::Name(b"b".to_vec()));
            nodes.set(NodeKey::Name(b"c".to_vec()), leaf("c"));
        }
        None
    }));
    traverse(&mut tree, vec![Arc::new(visitor)]).unwrap();
    assert_eq!(
        *log.lock().unwrap(),
        [
            "visitor enters root",
            "visitor enters a",
            "visitor leaves a",
            "visitor enters c",
            "visitor leaves c",
            "visitor leaves root",
        ]
    );
    let keys: Vec<String> = children_names(&tree).into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["a", "c"]);
}

/// The root of a traversal cannot be removed (Twig would hand `null` to the next visitor).
#[test]
fn the_root_cannot_be_removed() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut visitor = LoggingVisitor::new("visitor", 0, &log);
    visitor.on_leave = Some(Box::new(|_: &mut Node| Some(VisitResult::Remove)));
    let error = traverse(&mut leaf("root"), vec![Arc::new(visitor)]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
}

/// `CorrectnessTest::traverse()`: a child template (parent `parent`) whose body is `input`, visited by the
/// `CorrectnessNodeVisitor` only.
fn correctness(input: Node) -> Result<Node> {
    let mut module = Node::module(
        Node::body(Nodes::from_list([input]), 0),
        Some(Expr::constant("parent", 1)),
        Node::empty(0),
        Node::new(NodeKind::Macros(Nodes::new()), 0),
        Node::empty(0),
        Node::empty(0),
        Source::new("", "index"),
    );
    traverse(&mut module, vec![Arc::new(CorrectnessNodeVisitor)])?;
    let NodeKind::Module(module) = module.kind else {
        unreachable!()
    };
    let NodeKind::Body(body) = module.body.kind else {
        unreachable!()
    };
    Ok(body.into_entries().remove(0).1)
}

/// `CorrectnessTest::testFilterBodyNodes()`.
#[test]
fn correctness_accepts_bodies_without_output() {
    let input = Node::nodes(
        Nodes::from_list([Node::set(false, Node::empty(0), Node::empty(0), 1)]),
        0,
    );
    assert_eq!(correctness(input.clone()).unwrap(), input);

    let input = Node::nodes(
        Nodes::from_list([Node::set(
            true,
            Node::empty(0),
            Node::nodes(
                Nodes::from_list([Node::nodes(Nodes::from_list([Node::text("foo", 1)]), 0)]),
                0,
            ),
            1,
        )]),
        0,
    );
    assert_eq!(correctness(input.clone()).unwrap(), input);
}

/// `CorrectnessTest::testFilterBodyNodesThrowsException()`.
#[test]
fn correctness_rejects_output_outside_blocks() {
    for input in [
        Node::text("foo", 1),
        Node::print(Expr::constant("foo", 1), 1),
        Node::include(Expr::constant("foo", 1), None, false, false, 1),
        Node::nodes(
            Nodes::from_list([Node::nodes(Nodes::from_list([Node::text("foo", 1)]), 0)]),
            0,
        ),
    ] {
        let error = correctness(input).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Syntax);
        assert!(error.to_string().starts_with(
            "A template that extends another one cannot include content outside Twig blocks."
        ));
    }
}

/// `CorrectnessTest::testFilterBodyNodesWithBOM()`.
#[test]
fn correctness_accepts_a_bom_followed_by_blanks() {
    for blank in [" ", "\t", "\n", "\n\t\n   "] {
        let mut data = b"\xEF\xBB\xBF".to_vec();
        data.extend_from_slice(blank.as_bytes());
        let input = Node::text(data, 1);
        assert_eq!(correctness(input.clone()).unwrap(), input);
    }
}

/// A module whose body has no node `0` fails like `getNode('0')`.
#[test]
fn correctness_needs_a_body_node() {
    let mut module = Node::module(
        Node::body(Nodes::new(), 0),
        None,
        Node::empty(0),
        Node::new(NodeKind::Macros(Nodes::new()), 0),
        Node::empty(0),
        Node::empty(0),
        Source::new("", "index"),
    );
    let error = traverse(&mut module, vec![Arc::new(CorrectnessNodeVisitor)]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "Node \"0\" does not exist for Node \"Twig\\Node\\BodyNode\"."
    );
}

/// The priority slots of Twig's visitors.
#[test]
fn visitor_priorities() {
    assert_eq!(CorrectnessNodeVisitor.priority(), -255);
    assert_eq!(priority::CORRECTNESS, -255);
    assert_eq!(priority::ESCAPER, 0);
    assert_eq!(priority::SANDBOX, 0);
    assert_eq!(priority::YIELD_NOT_READY, 255);
    assert_eq!(priority::OPTIMIZER, 255);
}
