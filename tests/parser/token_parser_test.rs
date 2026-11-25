//! Ports of Twig 3.31.1's token-parser tests (TWIG/tests/TokenParser/{Block,Guard,Types}TokenParserTest.php); the
//! parts that render a template wait for the runtime.

use std::sync::Arc;

use twig::ast::{AttrValue, Node, NodeKey, NodeKind};
use twig::deprecation::Deprecation;
use twig::extension::ExtensionSet;
use twig::parser::ParseContext;
use twig::{Error, Result, Source};

fn parse_in(set: ExtensionSet, code: &str) -> Result<Node> {
    let mut deprecations: Vec<Deprecation> = Vec::new();
    ParseContext::new(set).parse_source(&Source::new(code, ""), &mut deprecations)
}

fn parse(code: &str) -> Node {
    parse_in(ExtensionSet::with_core_extensions(), code)
        .unwrap_or_else(|error| panic!("{code:?}: {error}"))
}

/// `$module->getNode('blocks')->getNode('content')->getNode('0')`.
fn content_block(module: &Node) -> &Node {
    module
        .as_module()
        .unwrap()
        .blocks()
        .find(|(name, _)| *name == b"content")
        .map(|(_, block)| block)
        .expect("a content block")
}

/// `BlockTokenParserTest::testDocumentation()`.
#[test]
fn block_documentation() {
    for (template, expected) in [
        (
            "{## The main content #}\n{% block content %}Hello{% endblock %}",
            Some("The main content"),
        ),
        (
            "{## The main content ##}\n{% block content %}Hello{% endblock %}",
            Some("The main content"),
        ),
        (
            "{## The main content #}\n{## Displayed on every page #}\n{% block content %}Hello{% endblock %}",
            Some("The main content\nDisplayed on every page"),
        ),
        ("{## #}\n{% block content %}Hello{% endblock %}", None),
        (
            "{## 0 #}\n{% block content %}Hello{% endblock %}",
            Some("0"),
        ),
        (
            "{# The main content #}\n{% block content %}Hello{% endblock %}",
            None,
        ),
        (
            "{## Not block documentation #}\nHello\n{% block content %}Hello{% endblock %}",
            None,
        ),
    ] {
        let module = parse(template);
        assert_eq!(
            content_block(&module).documentation.as_deref(),
            expected.map(str::as_bytes),
            "{template:?}"
        );
    }
}

/// The documentation part of `BlockTokenParserTest::testDocumentationWhitespaceControl()`.
#[test]
fn block_documentation_with_whitespace_control() {
    for template in [
        "A \n{##- The main content #}\n{% block content %}B{% endblock %}",
        "A \n  {##~ The main content #}\n{% block content %}B{% endblock %}",
        "A{## The main content -##}\n  {% block content %}B{% endblock %}",
        "A{## The main content ~##}  \n{% block content %}B{% endblock %}",
    ] {
        let module = parse(template);
        assert_eq!(
            content_block(&module).documentation.as_deref(),
            Some(&b"The main content"[..]),
            "{template:?}"
        );
    }
}

/// `BlockTokenParserTest::testDocumentationIsMovedFromTheBlockReference()`.
#[test]
fn block_documentation_is_moved_from_the_block_reference() {
    let module = parse("{## The main content #}\n{% block content %}Hello{% endblock %}");
    let reference = module
        .as_module()
        .unwrap()
        .body
        .as_nodes()
        .unwrap()
        .get(&NodeKey::Index(0))
        .unwrap();
    assert!(matches!(reference.kind, NodeKind::BlockReference(_)));
    assert_eq!(reference.documentation, None);
    assert_eq!(
        content_block(&module).documentation.as_deref(),
        Some(&b"The main content"[..])
    );
}

/// `BlockTokenParserTest::testDocumentationIsPreservedInChildTemplates()`.
#[test]
fn block_documentation_is_preserved_in_child_templates() {
    let module = parse(
        "{% extends 'base' %}\n{## The main content #}\n{% block content %}Hello{% endblock %}",
    );
    assert_eq!(
        content_block(&module).documentation.as_deref(),
        Some(&b"The main content"[..])
    );
}

/// `BlockTokenParserTest::testDocumentationDoesNotCrossSpecialConstructs()`.
#[test]
fn block_documentation_does_not_cross_special_constructs() {
    for template in [
        "{## Not do documentation #}\n{% line 10 %}{% do max() %}",
        "{## Not do documentation #}\n{% verbatim %}{% endverbatim %}{% do max() %}",
        "{## Not do documentation #}\n{% verbatim %} {% endverbatim %}{% do max() %}",
    ] {
        let module = parse(template);
        let body = module
            .as_module()
            .unwrap()
            .body
            .as_nodes()
            .unwrap()
            .get(&NodeKey::Index(0))
            .unwrap();
        let node = match &body.kind {
            NodeKind::Do(_) => body,
            _ => body.as_nodes().unwrap().get(&NodeKey::Index(1)).unwrap(),
        };
        assert!(matches!(node.kind, NodeKind::Do(_)), "{template:?}");
        assert_eq!(node.documentation, None, "{template:?}");
    }
}

/// The parse part of `BlockTokenParserTest::testShortcutAssignmentNamedDocsRemainsValid()`: `docs="..."` in the
/// shorthand is an assignment expression.
#[test]
fn block_shortcut_assignment_named_docs_remains_valid() {
    let module = parse("{% block title docs=\"The page title\" %}");
    let (_, block) = module.as_module().unwrap().blocks().next().unwrap();
    let dump = twig::ast::dump::dump_node(block);
    assert!(dump.contains("Binary\\SetBinary"), "{dump}");
}

/// `BlockTokenParserTest::testDocumentationCommentRequiresALineBreak()`.
#[test]
fn block_documentation_comment_requires_a_line_break() {
    let error = parse_in(
        ExtensionSet::with_core_extensions(),
        "{% block ## Description %}Hello{% endblock %}",
    )
    .unwrap_err();
    assert_eq!(error.kind(), twig::ErrorKind::Syntax);
}

/// `GuardTokenParserTest::testUndefinedHandlers()`: a syntax error of an undefined-callable callback means that the
/// guarded callable does not exist.
#[test]
fn guard_undefined_handlers() {
    let mut set = ExtensionSet::with_core_extensions();
    set.register_undefined_function_callback(Arc::new(|_: &[u8]| Err(Error::syntax("boom."))));
    parse_in(set, "{% guard function boom %}{% endguard %}").unwrap();

    // other errors of a callback are not swallowed
    let mut set = ExtensionSet::with_core_extensions();
    set.register_undefined_function_callback(Arc::new(|_: &[u8]| Err(Error::runtime("boom."))));
    let error = parse_in(set, "{% guard function boom %}{% endguard %}").unwrap_err();
    assert_eq!(error.to_string(), "boom.");
}

fn types_mapping(code: &str) -> AttrValue {
    let module = parse(code);
    let types = module
        .as_module()
        .unwrap()
        .body
        .as_nodes()
        .unwrap()
        .get(&NodeKey::Index(0))
        .unwrap();
    types
        .attributes()
        .into_iter()
        .find(|(name, _)| name == "mapping")
        .map(|(_, value)| value)
        .expect("a mapping attribute")
}

fn mapping(entries: &[(&str, &str, bool)]) -> AttrValue {
    AttrValue::Array(
        entries
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
    )
}

/// A template and its expected `(name, type, optional)` declarations.
type MappingCase = (&'static str, &'static [(&'static str, &'static str, bool)]);

/// `TypesTokenParserTest::testMappingParsing()`.
#[test]
fn types_mapping_parsing() {
    let cases: &[MappingCase] = &[
        ("{% types {} %}", &[]),
        ("{% types {foo: \"bar\"} %}", &[("foo", "bar", false)]),
        ("{% types {foo: \"bar\",} %}", &[("foo", "bar", false)]),
        ("{% types {foo?: \"bar\"} %}", &[("foo", "bar", true)]),
        (
            "{% types {foo: \"foo\", bar?: \"foo\", baz: \"baz\"} %}",
            &[
                ("foo", "foo", false),
                ("bar", "foo", true),
                ("baz", "baz", false),
            ],
        ),
        (
            "{% types foo: \"foo\", bar: \"bar\" %}",
            &[("foo", "foo", false), ("bar", "bar", false)],
        ),
        (
            "{% types {\n## The foo variable\n## Used by the header\nfoo: \"bar\"\n} %}",
            &[("foo", "bar", false)],
        ),
        (
            "{% types {\n## The foo variable\nfoo: \"foo\",\n## The bar variable\nbar?: \"bar\",\n} %}",
            &[("foo", "foo", false), ("bar", "bar", true)],
        ),
        (
            "{% types {\n# Not documentation\nfoo: \"bar\",\n} %}",
            &[("foo", "bar", false)],
        ),
    ];
    for (template, expected) in cases {
        assert_eq!(types_mapping(template), mapping(expected), "{template:?}");
    }
}

/// `TypesTokenParserTest::testDocumentationIsAttachedToTypeNodes()`.
#[test]
fn types_documentation_is_attached_to_type_nodes() {
    let module = parse(
        "{% types {\n## The foo variable\nfoo: 'foo',\n## The bar variable\nbar?: 'bar',\n} %}",
    );
    let types = module
        .as_module()
        .unwrap()
        .body
        .as_nodes()
        .unwrap()
        .get(&NodeKey::Index(0))
        .unwrap();
    let NodeKind::Types(types) = &types.kind else {
        panic!("not a types node");
    };
    let foo = types.types.get(&NodeKey::Name(b"foo".to_vec())).unwrap();
    assert_eq!(foo.documentation.as_deref(), Some(&b"The foo variable"[..]));
    assert_eq!(foo.line, 3);
    let bar = types.types.get(&NodeKey::Name(b"bar".to_vec())).unwrap();
    assert_eq!(bar.documentation.as_deref(), Some(&b"The bar variable"[..]));
    assert_eq!(bar.line, 5);
}
