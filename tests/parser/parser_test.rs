//! Port of Twig 3.31.1's `tests/ParserTest.php` (TWIG/tests/ParserTest.php), minus the tests of PHP internals
//! (`getVarName()`, reflection on private properties) and of later stages (the optimizer's removal of documentation).

use std::any::Any;
use std::sync::Arc;

use twig::ast::{
    ConstantValue, Expr, ExprKind, GetAttrExpr, MacroReferenceExpr, Node, NodeKey, NodeKind,
    NodeRef, Nodes,
};
use twig::deprecation::Deprecation;
use twig::extension::{Extension, ExtensionSet, TokenParser};

use super::support::{keep_parser_visitors, parser_set};
use twig::lexer::{Token, TokenType, TokenValue};
use twig::parser::{DocumentationTarget, ParseContext, Parser, TokenStream};
use twig::{Result, Source};

fn context() -> ParseContext {
    ParseContext::new(parser_set())
}

fn context_with(parsers: Vec<Arc<dyn TokenParser>>) -> ParseContext {
    struct Tags(Vec<Arc<dyn TokenParser>>);
    impl Extension for Tags {
        fn class_name(&self) -> &str {
            "Acme\\TagsExtension"
        }
        fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
            self.0.clone()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    let mut set = ExtensionSet::with_core_extensions();
    set.add_extension(Arc::new(Tags(parsers))).unwrap();
    ParseContext::new(keep_parser_visitors(set))
}

fn parse_with(context: &ParseContext, code: &str) -> (Result<Node>, Vec<String>) {
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let result = context.parse_source(&Source::new(code, "index"), &mut deprecations);
    (
        result,
        deprecations.iter().map(ToString::to_string).collect(),
    )
}

fn parse(code: &str) -> Node {
    let (result, _) = parse_with(&context(), code);
    result.unwrap_or_else(|error| panic!("{code:?}: {error}"))
}

/// `$module->getNode('body')->getNode('0')`.
fn body(module: &Node) -> &Node {
    module
        .as_module()
        .unwrap()
        .body
        .as_nodes()
        .unwrap()
        .get(&NodeKey::Index(0))
        .unwrap()
}

fn child(node: &Node, key: i64) -> &Node {
    node.as_nodes().unwrap().get(&NodeKey::Index(key)).unwrap()
}

fn stream(tokens: Vec<Token>) -> TokenStream {
    TokenStream::new(tokens, Source::new("", ""))
}

fn token(ty: TokenType, value: &str) -> Token {
    Token::new(ty, value, 1)
}

/// `ParserTest::testUnknownTag()`.
#[test]
fn unknown_tag() {
    let set = parser_set();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let tokens = stream(vec![
        token(TokenType::BlockStart, ""),
        token(TokenType::Name, "foo"),
        token(TokenType::BlockEnd, ""),
        token(TokenType::Eof, ""),
    ]);
    let error = Parser::new(&set, tokens, &mut deprecations)
        .parse(None, false)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Unknown \"foo\" tag. Did you mean \"for\" at line 1?"
    );
}

/// `ParserTest::testUnknownTagWithoutSuggestions()`.
#[test]
fn unknown_tag_without_suggestions() {
    let set = parser_set();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let tokens = stream(vec![
        token(TokenType::BlockStart, ""),
        token(TokenType::Name, "foobar"),
        token(TokenType::BlockEnd, ""),
        token(TokenType::Eof, ""),
    ]);
    let error = Parser::new(&set, tokens, &mut deprecations)
        .parse(None, false)
        .unwrap_err();
    assert_eq!(error.to_string(), "Unknown \"foobar\" tag at line 1.");
}

/// Parses another template in the middle of the current one (`ParserTest`'s `TestTokenParser`).
struct ReentrantTokenParser;

impl TokenParser for ReentrantTokenParser {
    fn tag(&self) -> &str {
        "test"
    }

    fn parse(&self, parser: &mut Parser<'_>, _token: &Token) -> Result<Option<Node>> {
        // simulate the parsing of another template right in the middle of the parsing of the current template
        let nested = parser.parse_stream(
            stream(vec![
                token(TokenType::BlockStart, ""),
                token(TokenType::Name, "extends"),
                token(TokenType::String, "base"),
                token(TokenType::BlockEnd, ""),
                token(TokenType::Eof, ""),
            ]),
            None,
            false,
        )?;
        assert!(nested.as_module().unwrap().parent.is_some());
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        Ok(Some(Node::empty(1)))
    }
}

/// `ParserTest::testParseIsReentrant()`: the nested parse leaves the state of the outer one (here: no parent).
#[test]
fn parse_is_reentrant() {
    let context = context_with(vec![Arc::new(ReentrantTokenParser)]);
    let (module, _) = parse_with(&context, "{% test %}{{ foo }}");
    let module = module.unwrap();
    assert!(module.as_module().unwrap().parent.is_none());
    assert!(matches!(child(body(&module), 0).kind, NodeKind::Empty));
}

fn collect_expressions<'a>(
    node: NodeRef<'a>,
    macro_references: &mut Vec<&'a MacroReferenceExpr>,
    attributes: &mut Vec<&'a GetAttrExpr>,
) {
    if let Some(expr) = node.as_expr() {
        match &expr.kind {
            ExprKind::MacroReference(reference) => macro_references.push(reference),
            ExprKind::GetAttr(get_attr) => attributes.push(get_attr),
            _ => {}
        }
    }
    for (_, child) in node.children() {
        collect_expressions(child, macro_references, attributes);
    }
}

fn macro_reference_count(module: &Node) -> usize {
    let mut references = Vec::new();
    let mut attributes = Vec::new();
    collect_expressions(NodeRef::Node(module), &mut references, &mut attributes);
    assert!(attributes.is_empty(), "attribute expressions found");
    references.len()
}

const MACRO_TARGETS: [&str; 2] = ["_self", "macros"];

/// `ParserTest::testMacroTargetsOnlyCompileAsMacroReferences()`.
#[test]
fn macro_targets_only_compile_as_macro_references() {
    for target in MACRO_TARGETS {
        for expression in [
            format!("{target}.foo()"),
            format!("({target}.foo())"),
            format!("{target}.(name)()"),
            format!("({target}.(name)())"),
        ] {
            let (module, deprecations) = parse_with(
                &context(),
                &format!("{{% import _self as macros %}}{{{{ {expression} }}}}"),
            );
            assert_eq!(macro_reference_count(&module.unwrap()), 1, "{expression}");
            assert!(deprecations.is_empty(), "{expression}: {deprecations:?}");
        }
    }
}

/// `ParserTest::testMacroTargetsWithoutParenthesesAreDeprecated()`.
#[test]
fn macro_targets_without_parentheses_are_deprecated() {
    for target in MACRO_TARGETS {
        for (expression, name) in [
            (format!("{target}.foo"), Some("foo")),
            (format!("({target}.foo)"), Some("foo")),
            (format!("{target}.(name)"), None),
            (format!("({target}.(name))"), None),
        ] {
            let (module, deprecations) = parse_with(
                &context(),
                &format!("{{% import _self as macros %}}{{{{ {expression} }}}}"),
            );
            assert_eq!(macro_reference_count(&module.unwrap()), 1, "{expression}");
            let expected = match name {
                None => "Since twig/twig 3.29: Omitting parentheses when calling a macro is deprecated and will throw a SyntaxError in Twig 4.0; add parentheses after the macro name in \"index\" at line 1.".to_owned(),
                Some(name) => format!(
                    "Since twig/twig 3.29: Omitting parentheses when calling the macro \"{name}\" is deprecated and will throw a SyntaxError in Twig 4.0; add parentheses after the macro name in \"index\" at line 1."
                ),
            };
            assert_eq!(deprecations, [expected], "{expression}");
        }
    }
}

const FOO_WITHOUT_PARENTHESES: &str = "Since twig/twig 3.29: Omitting parentheses when calling the macro \"foo\" is deprecated and will throw a SyntaxError in Twig 4.0; add parentheses after the macro name in \"index\" at line 1.";

/// `ParserTest::testAMacroCallWithoutParenthesesIsReportedOncePerCallSite()`.
#[test]
fn a_macro_call_without_parentheses_is_reported_once_per_call_site() {
    for expression in [
        "macros.foo ?? \"x\"",
        "macros.foo ?: \"x\"",
        "name|default(macros.foo)",
    ] {
        let (_, deprecations) = parse_with(
            &context(),
            &format!("{{% import _self as macros %}}{{{{ {expression} }}}}"),
        );
        assert_eq!(deprecations, [FOO_WITHOUT_PARENTHESES], "{expression}");
    }
}

/// `ParserTest::testEachMacroCallWithoutParenthesesIsReportedOnItsOwnLine()`.
#[test]
fn each_macro_call_without_parentheses_is_reported_on_its_own_line() {
    let (_, deprecations) = parse_with(
        &context(),
        "{% import _self as macros %}{{ macros.foo ?: 'x' }}\n{{ macros.foo ?: 'x' }}",
    );
    assert_eq!(
        deprecations,
        [
            FOO_WITHOUT_PARENTHESES.to_owned(),
            FOO_WITHOUT_PARENTHESES.replace("line 1.", "line 2."),
        ]
    );
}

/// `ParserTest::testDistinctMacroCallsWithoutParenthesesOnTheSameLineAreBothReported()`.
#[test]
fn distinct_macro_calls_without_parentheses_on_the_same_line_are_both_reported() {
    let (_, deprecations) = parse_with(
        &context(),
        "{% import _self as macros %}{{ macros.foo }}{{ macros.bar }}",
    );
    assert_eq!(
        deprecations,
        [
            FOO_WITHOUT_PARENTHESES.to_owned(),
            FOO_WITHOUT_PARENTHESES.replace("\"foo\"", "\"bar\""),
        ]
    );
}

/// `ParserTest::testADynamicMacroCallWithoutParenthesesIsReportedWithoutAName()`.
#[test]
fn a_dynamic_macro_call_without_parentheses_is_reported_without_a_name() {
    let (_, deprecations) = parse_with(
        &context(),
        "{% import _self as macros %}{{ macros.(name) ?: \"x\" }}",
    );
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Omitting parentheses when calling a macro is deprecated and will throw a SyntaxError in Twig 4.0; add parentheses after the macro name in \"index\" at line 1."
        ]
    );
}

/// `ParserTest::testAMacroCallWithParenthesesDoesNotSilenceOneWithoutOnTheSameLine()`.
#[test]
fn a_macro_call_with_parentheses_does_not_silence_one_without_on_the_same_line() {
    let (_, deprecations) = parse_with(
        &context(),
        "{% import _self as macros %}{{ macros.foo() }}{{ macros.foo }}",
    );
    assert_eq!(deprecations, [FOO_WITHOUT_PARENTHESES]);
}

/// `ParserTest::testMacroTargetsWithoutParenthesesAreAllowedInDefinedTest()`.
#[test]
fn macro_targets_without_parentheses_are_allowed_in_defined_test() {
    for target in MACRO_TARGETS {
        for expression in [
            format!("{target}.foo"),
            format!("({target}.foo)"),
            format!("{target}.(name)"),
            format!("({target}.(name))"),
        ] {
            let (module, deprecations) = parse_with(
                &context(),
                &format!(
                    "{{% import _self as macros %}}{{{{ {expression} is defined }}}}{{{{ {expression} is not defined }}}}"
                ),
            );
            assert_eq!(macro_reference_count(&module.unwrap()), 2, "{expression}");
            assert!(deprecations.is_empty(), "{expression}: {deprecations:?}");
        }
    }
}

/// `ParserTest::testMacroTargetsWithParenthesesAreDeprecatedInDefinedTest()`.
#[test]
fn macro_targets_with_parentheses_are_deprecated_in_defined_test() {
    for target in MACRO_TARGETS {
        for expression in [
            format!("{target}.foo()"),
            format!("({target}.foo())"),
            format!("{target}.(name)()"),
            format!("({target}.(name)())"),
        ] {
            let (module, deprecations) = parse_with(
                &context(),
                &format!("{{% import _self as macros %}}{{{{ {expression} is defined }}}}"),
            );
            assert_eq!(macro_reference_count(&module.unwrap()), 1, "{expression}");
            assert_eq!(
                deprecations,
                [
                    "Since twig/twig 3.29: Using parentheses when testing a macro with the \"defined\" test is deprecated and will throw a SyntaxError in Twig 4.0; remove the parentheses after the macro name in \"index\" at line 1."
                ],
                "{expression}"
            );
        }
    }
}

/// `ParserTest::testImplicitMacroArgumentDefaultValues()`.
#[test]
fn implicit_macro_argument_default_values() {
    let module = parse("{% macro marco (po, lo = true) %}{% endmacro %}");
    let (_, marco) = module.as_module().unwrap().macros().next().unwrap();
    let NodeKind::Macro(marco) = &marco.kind else {
        panic!("not a macro");
    };
    let arguments = marco.arguments.as_array().unwrap();
    let ExprKind::Constant(po) = &arguments.elements[0].1.kind else {
        panic!("not a constant");
    };
    assert!(po.is_implicit);
    assert_eq!(po.value, ConstantValue::Null);
    let ExprKind::Constant(lo) = &arguments.elements[1].1.kind else {
        panic!("not a constant");
    };
    assert!(!lo.is_implicit);
    assert_eq!(lo.value, ConstantValue::Bool(true));
}

/// `ParserTest::testMacroDeclarationIsRepresentedInTheTemplateBody()`.
#[test]
fn macro_declaration_is_represented_in_the_template_body() {
    let module = parse("{% macro input() %}{% endmacro %}");
    let declaration = body(&module);
    let NodeKind::MacroDeclaration(declaration_node) = &declaration.kind else {
        panic!("not a macro declaration: {declaration:?}");
    };
    assert_eq!(declaration_node.name, b"input");
    assert_eq!(declaration.tag.as_deref(), Some("macro"));
    let macros: Vec<_> = module.as_module().unwrap().macros().collect();
    assert_eq!(macros.len(), 1);
    assert!(matches!(macros[0].1.kind, NodeKind::Macro(_)));
}

/// `ParserTest::testEmbeddedTemplatesHaveSequentialIndices()`.
#[test]
fn embedded_templates_have_sequential_indices() {
    let module = parse("{% embed \"first\" %}{% endembed %}{% embed \"second\" %}{% endembed %}");
    let indices: Vec<Option<i64>> = module
        .as_module()
        .unwrap()
        .embedded_templates()
        .map(|template| template.as_module().unwrap().index)
        .collect();
    assert_eq!(indices, [Some(1), Some(2)]);
}

/// `ParserTest::testBodyForChildTemplates()`: blank texts and block references are removed from the body of a child
/// template; the other keys are kept.
#[test]
fn body_for_child_templates() {
    let module = parse(
        "{% extends \"base\" %}\n\n{% block header %}\n    header\n{% endblock %}\n\n{% set foo = 'bar' %}\n\n{% block footer %}\n    footer\n{% endblock %}\n",
    );
    let body = body(&module).as_nodes().unwrap();
    assert_eq!(body.len(), 2);
    assert_eq!(
        body.get(&NodeKey::Index(0)).unwrap().tag.as_deref(),
        Some("extends")
    );
    assert_eq!(
        body.get(&NodeKey::Index(4)).unwrap().tag.as_deref(),
        Some("set")
    );
}

/// Sets a parent and returns a blank text (`ParserTest`'s `ParentSettingTokenParser`).
struct ParentSettingTokenParser;

impl TokenParser for ParentSettingTokenParser {
    fn tag(&self) -> &str {
        "set_parent"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        parser.set_parent(Expr::constant("base", token.line()), false)?;
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        // returns a blank text node so the whole child body is a single removable node
        Ok(Some(Node::text("   ", token.line())))
    }
}

/// `ParserTest::testCleanupBodyForChildTemplatesWithASingleNodeBody()`.
#[test]
fn cleanup_body_for_child_templates_with_a_single_node_body() {
    let context = context_with(vec![Arc::new(ParentSettingTokenParser)]);
    let (module, _) = parse_with(&context, "{% set_parent %}");
    assert!(matches!(body(&module.unwrap()).kind, NodeKind::Empty));
}

/// Sets a parent and returns a reference to a block that does not exist (`UnregisteredBlockReferenceTokenParser`).
struct UnregisteredBlockReferenceTokenParser;

impl TokenParser for UnregisteredBlockReferenceTokenParser {
    fn tag(&self) -> &str {
        "unregistered_block_reference"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        parser.set_parent(Expr::constant("base", token.line()), false)?;
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        Ok(Some(Node::block_reference("missing", token.line())))
    }
}

/// `ParserTest::testCleanupBodyAcceptsAnUnregisteredBlockReference()`.
#[test]
fn cleanup_body_accepts_an_unregistered_block_reference() {
    let context = context_with(vec![Arc::new(UnregisteredBlockReferenceTokenParser)]);
    let (module, _) = parse_with(&context, "{% unregistered_block_reference %}");
    assert!(matches!(body(&module.unwrap()).kind, NodeKind::Empty));
}

/// `ParserTest::testDocumentationIsAttachedToNodes()`.
#[test]
fn documentation_is_attached_to_nodes() {
    let module = parse(
        "{## Printed expression #}\n{{ answer }}\n{## Conditional tag #}\n{% if enabled %}Enabled{% endif %}\n{## Do tag #}\n{% do max() %}\n{## Text node #}\nText",
    );
    let body = body(&module);
    let print = child(body, 0);
    assert!(matches!(print.kind, NodeKind::Print(_)));
    assert_eq!(
        print.documentation.as_deref(),
        Some(&b"Printed expression"[..])
    );
    let NodeKind::Print(print_node) = &print.kind else {
        unreachable!()
    };
    assert_eq!(print_node.expr.documentation, None);
    let if_node = child(body, 2);
    assert!(matches!(if_node.kind, NodeKind::If(_)));
    assert_eq!(
        if_node.documentation.as_deref(),
        Some(&b"Conditional tag"[..])
    );
    let do_node = child(body, 3);
    assert!(matches!(do_node.kind, NodeKind::Do(_)));
    assert_eq!(do_node.documentation.as_deref(), Some(&b"Do tag"[..]));
    let text = child(body, 4);
    assert!(matches!(text.kind, NodeKind::Text(_)));
    assert_eq!(text.documentation, None);
}

/// `ParserTest::testEmptyCommentIsNotLexedAsDocumentation()`.
#[test]
fn empty_comment_is_not_lexed_as_documentation() {
    let module =
        parse("{##}{{ answer }}\n{% block a %}x{##}{% endblock %}{% block b %}y{% endblock %}");
    let print = child(body(&module), 0);
    assert!(matches!(print.kind, NodeKind::Print(_)));
    assert_eq!(print.documentation, None);
}

/// `ParserTest::testInlineDocumentationBeforeATagNameIsIgnored()`.
#[test]
fn inline_documentation_before_a_tag_name_is_ignored() {
    let module = parse("{% ## Not tag documentation\ndo max() %}");
    assert_eq!(body(&module).documentation, None);
}

/// `ParserTest::testDocumentationIsAvailableToNodeVisitors()`: the documentation of blocks and macros is on the
/// `BlockNode` and the `MacroNode` when the visitors run.
#[test]
fn documentation_is_available_to_node_visitors() {
    let module = parse(
        "{## Block documentation #}{% block content %}{% endblock %}{## Macro documentation #}{% macro input() %}{% endmacro %}",
    );
    let module = module.as_module().unwrap();
    let (_, block) = module.blocks().next().unwrap();
    assert_eq!(
        block.documentation.as_deref(),
        Some(&b"Block documentation"[..])
    );
    let (_, macro_node) = module.macros().next().unwrap();
    assert_eq!(
        macro_node.documentation.as_deref(),
        Some(&b"Macro documentation"[..])
    );
}

/// Moves the documentation of its tag to a child of the node it returns (`DocumentationTargetTokenParser`).
struct DocumentationTargetTokenParser;

impl TokenParser for DocumentationTargetTokenParser {
    fn tag(&self) -> &str {
        "documentation_target"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        parser.set_documentation_target(DocumentationTarget::Child(vec![b"target".to_vec()]))?;
        let mut nodes = Nodes::new();
        nodes.set(NodeKey::Name(b"target".to_vec()), Node::empty(token.line()));
        Ok(Some(Node::nodes(nodes, token.line())))
    }
}

/// `ParserTest::testCustomTokenParserCanSetDocumentationTarget()`.
#[test]
fn custom_token_parser_can_set_documentation_target() {
    let context = context_with(vec![Arc::new(DocumentationTargetTokenParser)]);
    let (module, _) = parse_with(
        &context,
        "{## Target documentation #}{% documentation_target %}",
    );
    let module = module.unwrap();
    let node = body(&module);
    assert_eq!(node.documentation, None);
    let target = node
        .as_nodes()
        .unwrap()
        .get(&NodeKey::Name(b"target".to_vec()))
        .unwrap();
    assert_eq!(
        target.documentation.as_deref(),
        Some(&b"Target documentation"[..])
    );
}

/// `ParserTest::testDocumentationIsAttachedToAssignmentTargets()`.
#[test]
fn documentation_is_attached_to_assignment_targets() {
    let module = parse(
        "{% set ## First name\n    first_name, ## Last name\n    last_name = user.first_name, user.last_name\n%}\n{% for ## Product identifier\n    product_id, ## Product\n    product in products\n%}{% endfor %}",
    );
    let body = body(&module);
    let NodeKind::Set(set) = &child(body, 0).kind else {
        panic!("not a set node");
    };
    let names = set.names.as_nodes().unwrap();
    let name = |i: i64| {
        names
            .get(&NodeKey::Index(i))
            .unwrap()
            .as_expr()
            .unwrap()
            .documentation
            .clone()
    };
    assert_eq!(name(0).as_deref(), Some(&b"First name"[..]));
    assert_eq!(name(1).as_deref(), Some(&b"Last name"[..]));
    let NodeKind::For(for_node) = &child(body, 1).kind else {
        panic!("not a for node");
    };
    assert_eq!(
        for_node.key_target.documentation.as_deref(),
        Some(&b"Product identifier"[..])
    );
    assert_eq!(
        for_node.value_target.documentation.as_deref(),
        Some(&b"Product"[..])
    );
}

/// `ParserTest::testDocumentationIsAttachedToMacroNode()`.
#[test]
fn documentation_is_attached_to_macro_node() {
    let module = parse("{## Input macro #}\n{% macro input() %}{% endmacro %}");
    let (_, macro_node) = module.as_module().unwrap().macros().next().unwrap();
    assert_eq!(
        macro_node.documentation.as_deref(),
        Some(&b"Input macro"[..])
    );

    let module = parse(
        "{## Input macro #}\n{% macro input(## Name argument\nname, ## Value argument\nvalue) %}{% endmacro %}",
    );
    assert_eq!(body(&module).documentation, None);
    let (_, macro_node) = module.as_module().unwrap().macros().next().unwrap();
    assert_eq!(
        macro_node.documentation.as_deref(),
        Some(&b"Input macro"[..])
    );
    let NodeKind::Macro(macro_node) = &macro_node.kind else {
        unreachable!()
    };
    let arguments = macro_node.arguments.as_array().unwrap();
    assert_eq!(
        arguments.elements[0].0.documentation.as_deref(),
        Some(&b"Name argument"[..])
    );
    assert_eq!(arguments.elements[0].0.line, 3);
    assert_eq!(
        arguments.elements[1].0.documentation.as_deref(),
        Some(&b"Value argument"[..])
    );
    assert_eq!(arguments.elements[1].0.line, 4);
}

/// `ParserTest::testDuplicateMacroKeepsOnlyTheLastDocumentation()`.
#[test]
fn duplicate_macro_keeps_only_the_last_documentation() {
    let (module, deprecations) = parse_with(
        &context(),
        "{## First #}{% macro input() %}{% endmacro %}{## Second #}{% macro input() %}{% endmacro %}",
    );
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Defining the macro \"input\" more than once in \"index\" is deprecated and will throw a SyntaxError in Twig 4.0 (previous definition at line 1, new definition at line 1). The last definition is used in Twig 3."
        ]
    );
    let module = module.unwrap();
    let (_, macro_node) = module.as_module().unwrap().macros().next().unwrap();
    assert_eq!(macro_node.documentation.as_deref(), Some(&b"Second"[..]));
    for declaration in body(&module).as_nodes().unwrap().nodes() {
        assert_eq!(declaration.documentation, None);
    }
}

/// `ParserTest::testDuplicateMacroDeprecationsReferToPreviousDefinition()`.
#[test]
fn duplicate_macro_deprecations_refer_to_previous_definition() {
    let (_, deprecations) = parse_with(
        &context(),
        "{% macro input() %}{% endmacro %}\n{% macro input() %}{% endmacro %}\n{% macro input() %}{% endmacro %}",
    );
    assert_eq!(
        deprecations,
        [
            "Since twig/twig 3.29: Defining the macro \"input\" more than once in \"index\" is deprecated and will throw a SyntaxError in Twig 4.0 (previous definition at line 1, new definition at line 2). The last definition is used in Twig 3.",
            "Since twig/twig 3.29: Defining the macro \"input\" more than once in \"index\" is deprecated and will throw a SyntaxError in Twig 4.0 (previous definition at line 2, new definition at line 3). The last definition is used in Twig 3."
        ]
    );
}

/// `ParserTest::testBodyForParentTemplates()`.
#[test]
fn body_for_parent_templates() {
    let module = parse(
        "{% block header %}\n    header\n{% endblock %}\n\n{% set foo = 'bar' %}\n\n{% block footer %}\n    footer\n{% endblock %}\n",
    );
    let body = body(&module).as_nodes().unwrap();
    assert_eq!(body.len(), 5);
    let tag = |i: i64| body.get(&NodeKey::Index(i)).unwrap().tag.as_deref();
    assert_eq!(tag(0), Some("block"));
    assert!(matches!(
        body.get(&NodeKey::Index(1)).unwrap().kind,
        NodeKind::Text(_)
    ));
    assert_eq!(tag(2), Some("set"));
    assert!(matches!(
        body.get(&NodeKey::Index(3)).unwrap().kind,
        NodeKind::Text(_)
    ));
    assert_eq!(tag(4), Some("block"));
}

/// A token parser cannot return a node whose tag is already set (`Node::setNodeTag()`).
#[test]
fn a_returned_node_gets_the_tag_once() {
    struct Wrap;
    impl TokenParser for Wrap {
        fn tag(&self) -> &str {
            "wrap"
        }
        fn parse(&self, parser: &mut Parser<'_>, _token: &Token) -> Result<Option<Node>> {
            parser
                .stream_mut()
                .expect(TokenType::BlockEnd, None, None)?;
            let test = |token: &Token| token.test_value(TokenType::Name, "endwrap");
            let body = parser.subparse(
                Some(&twig::parser::EndTagTest {
                    tag: Some("wrap"),
                    test: &test,
                }),
                true,
            )?;
            parser
                .stream_mut()
                .expect(TokenType::BlockEnd, None, None)?;
            Ok(Some(body))
        }
    }
    let context = context_with(vec![Arc::new(Wrap)]);
    let (result, _) = parse_with(&context, "{% wrap %}{% if a %}{% endif %}{% endwrap %}");
    let error = result.unwrap_err();
    assert_eq!(error.kind(), twig::ErrorKind::Logic);
    assert_eq!(error.to_string(), "The tag of a node can only be set once.");
    // `compileSource()` turns it into a syntax error without a line
    let source = Source::new("{% wrap %}{% if a %}{% endif %}{% endwrap %}", "index");
    let error = context
        .parse_template(&source, &mut Vec::new())
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the compilation of a template (\"The tag of a node can only be set once.\") in \"index\"."
    );
}

/// Tokens a token parser injects have no offset, so their errors have no column (the four tokens of `embed`).
#[test]
fn injected_tokens_have_no_column() {
    let token = Token::new(TokenType::String, TokenValue::from("__parent__"), 3);
    assert_eq!(token.offset(), None);
    let (result, _) = parse_with(&context(), "{% embed 'b' %}{% endembed %}");
    assert!(result.is_ok());
}

/// `registerUndefinedTokenParserCallback()`: a callback can provide a token parser for an unknown tag; its error (Twig's
/// exception) stops the parse, and `Parser::parse()` gives it the source (its line `-1` is kept).
#[test]
fn undefined_token_parser_callbacks() {
    let mut set = ExtensionSet::with_core_extensions();
    set.register_undefined_token_parser_callback(Arc::new(|tag: &[u8]| match tag {
        b"set_parent" => Ok(Some(
            Arc::new(ParentSettingTokenParser) as Arc<dyn TokenParser>
        )),
        b"boom" => Err(twig::Error::syntax("This tag is undefined in the tests.")),
        _ => Ok(None),
    }));
    let context = ParseContext::new(keep_parser_visitors(set));
    let (module, _) = parse_with(&context, "{% set_parent %}");
    assert!(module.unwrap().as_module().unwrap().parent.is_some());
    let (error, _) = parse_with(&context, "{% boom %}");
    assert_eq!(
        error.unwrap_err().to_string(),
        "This tag is undefined in the tests in \"index\"."
    );
    let (error, _) = parse_with(&context, "{% nope %}");
    assert!(
        error
            .unwrap_err()
            .to_string()
            .starts_with("Unknown \"nope\" tag")
    );
}
