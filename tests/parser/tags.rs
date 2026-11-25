//! The parser's tag plumbing (`subparse()`, scopes, imported symbols, blocks, the parent template, documentation
//! targets) through the core `from`, `import`, `block` and `extends` token parsers, against
//! `tests/vectors/parser/tags.json`, and two custom tags (a `null` return and a discarded body).
//!
//! Regenerate (from the repository root; Git Bash on Windows needs MSYS_NO_PATHCONV=1):
//!
//!     MSYS_NO_PATHCONV=1 docker run --rm -i -v "$PWD/tests/vectors/parser:/v" -e CASES_FILE=/v/tags.gen.json twig-oracle:3.31.1 php < tests/vectors/parser/expressions.gen.php > tests/vectors/parser/tags.json

use std::any::Any;
use std::sync::Arc;

use twig::ast::{Node, NodeKind, Nodes, PrintNode, dump::dump_node_bytes};
use twig::deprecation::Deprecation;
use twig::extension::{Extension, ExtensionSet, TokenParser};
use twig::lexer::{Token, TokenType};
use twig::parser::{DocumentationTarget, EndTagTest, Parser};
use twig::{Result, Source};

use super::support::parse_with;
use crate::common::Json;

/// A tag returning `null` (Twig's deprecated behaviour).
pub struct Nothing;

impl TokenParser for Nothing {
    fn tag(&self) -> &str {
        "nothing"
    }

    fn parse(&self, parser: &mut Parser<'_>, _token: &Token) -> Result<Option<Node>> {
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        Ok(None)
    }

    fn class_name(&self) -> &str {
        "Acme\\NothingTokenParser"
    }
}

/// A tag whose body is parsed while ignoring unknown callables and then discarded (like a false `guard`).
pub struct Discard;

impl TokenParser for Discard {
    fn tag(&self) -> &str {
        "discard"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        let test = |token: &Token| token.test_value(TokenType::Name, "enddiscard");
        parser.subparse_ignore_unknown_twig_callables(
            Some(&EndTagTest {
                tag: Some("discard"),
                test: &test,
            }),
            true,
        )?;
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        Ok(Some(Node::nodes(Nodes::new(), token.line())))
    }
}

struct TagExtension;

impl Extension for TagExtension {
    fn class_name(&self) -> &str {
        "TagExtension"
    }

    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        vec![Arc::new(Nothing), Arc::new(Discard)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn tag_set() -> ExtensionSet {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_extension(Arc::new(TagExtension)).unwrap();
    set
}

fn parse_tags(code: &str) -> (Result<Node>, Vec<Deprecation>) {
    parse_with(&tag_set(), &Source::new(code, "index"))
}

#[test]
fn tag_vectors() {
    let vectors = Json::load_vectors("parser/tags.json");
    let set = tag_set();
    let mut failures = Vec::new();
    for case in vectors.as_array() {
        let id = case.field("id").as_str().unwrap();
        let code = case.field("source").as_bytes();
        let (result, deprecations) = parse_with(&set, &Source::new(code, "index"));
        let expected_deprecations: Vec<String> = case
            .field("deprecations")
            .as_array()
            .iter()
            .map(Json::as_lossy_string)
            .collect();
        let actual_deprecations: Vec<String> =
            deprecations.iter().map(ToString::to_string).collect();
        if expected_deprecations != actual_deprecations {
            failures.push(format!(
                "{id}: deprecations {expected_deprecations:?} != {actual_deprecations:?}"
            ));
        }
        match (case.get("dump"), case.get("error"), result) {
            (Some(dump), _, Ok(module)) => {
                let expected = String::from_utf8_lossy(&dump.as_bytes()).into_owned();
                let actual = String::from_utf8_lossy(&dump_node_bytes(&module)).into_owned();
                if expected != actual {
                    failures.push(format!(
                        "{id}: dump\n--- expected\n{expected}\n--- actual\n{actual}"
                    ));
                }
            }
            (None, Some(error), Err(actual)) => {
                let expected = error.field("message").as_lossy_string();
                if expected != actual.to_string() {
                    failures.push(format!(
                        "{id}: error\n  expected {expected}\n  actual   {actual}"
                    ));
                }
            }
            (_, _, result) => failures.push(format!("{id}: unexpected result {result:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn null_from_a_token_parser_is_deprecated() {
    let (module, deprecations) = parse_tags("a{% nothing %}b");
    let module = module.unwrap();
    assert_eq!(
        deprecations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "Since twig/twig 3.12: Returning \"null\" from \"Acme\\NothingTokenParser\" is deprecated and forbidden by \"TokenParserInterface\"."
        ]
    );
    // the body has the two texts only
    let body = module.as_module().unwrap().body.as_nodes().unwrap();
    let inner = body.nodes().next().unwrap().as_nodes().unwrap();
    assert_eq!(inner.len(), 2);
}

#[test]
fn tags_are_set_on_tag_nodes() {
    let (module, _) = parse_tags("{% extends 'x' %}");
    let module = module.unwrap();
    let body = module.as_module().unwrap().body.as_nodes().unwrap();
    let node = body.nodes().next().unwrap();
    assert_eq!(node.tag.as_deref(), Some("extends"));
    assert!(matches!(node.kind, NodeKind::Config));
}

#[test]
fn discarded_bodies_ignore_unknown_callables_and_registrations() {
    let (module, _) = parse_tags(
        "{% discard %}{{ nope() }}{{ x|nope }}{{ x is nope }}{% block b %}{% endblock %}{% from _self import m %}{% enddiscard %}{{ m() }}",
    );
    // the import of the discarded body is gone
    let error = module.unwrap_err().to_string();
    assert!(error.starts_with("Unknown \"m\" function."), "{error}");

    let (module, _) = parse_tags(
        "{% discard %}{% block b %}{% endblock %}{% enddiscard %}{% block b %}{% endblock %}",
    );
    let module = module.unwrap();
    assert_eq!(module.as_module().unwrap().blocks().count(), 1);

    // unknown tags still fail
    let (module, _) = parse_tags("{% discard %}{% nope %}{% enddiscard %}");
    assert_eq!(
        module.unwrap_err().to_string(),
        "Unexpected \"nope\" tag (expecting closing tag for the \"discard\" tag defined near line 1) in \"index\" at line 1."
    );
}

#[test]
fn print_nodes_get_the_documentation_of_their_opening_token() {
    let (module, _) = parse_tags("{## a ##}{{ x }}");
    let module = module.unwrap();
    let body = module.as_module().unwrap().body.as_nodes().unwrap();
    let print = body.nodes().next().unwrap();
    assert!(matches!(print.kind, NodeKind::Print(ref p) if matches!(**p, PrintNode { .. })));
    assert_eq!(print.documentation.as_deref(), Some(&b"a"[..]));
}

#[test]
fn documentation_targets_can_only_be_set_while_parsing_a_tag() {
    let set = tag_set();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let stream = set
        .lexer(Default::default())
        .tokenize(&Source::new("", "index"), &mut deprecations)
        .unwrap();
    let mut parser = Parser::new(&set, stream, &mut deprecations);
    let error = parser
        .set_documentation_target(DocumentationTarget::Block(b"b".to_vec()))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "A documentation target can only be set while parsing a tag."
    );
    assert_eq!(error.kind(), twig::ErrorKind::Logic);
}

#[test]
fn embedded_templates_get_increasing_indices() {
    let set = tag_set();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let stream = set
        .lexer(Default::default())
        .tokenize(&Source::new("", "index"), &mut deprecations)
        .unwrap();
    let mut parser = Parser::new(&set, stream, &mut deprecations);
    let module = Node::module(
        Node::body(Nodes::new(), 0),
        None,
        Node::empty(0),
        Node::new(NodeKind::Macros(Nodes::new()), 0),
        Node::empty(0),
        Node::empty(0),
        Source::new("", "index"),
    );
    assert_eq!(parser.embed_template(module.clone()), 1);
    assert_eq!(parser.embed_template(module), 2);
}
