//! Shared helpers: the test extension of the oracle generators and parsing shortcuts.

#![allow(dead_code)]

use std::any::Any;
use std::sync::Arc;

use twig::ast::{BinaryOp, ConstantValue, Node, UnaryOp};
use twig::deprecation::Deprecation;
use twig::extension::{
    CallableParameter, DeprecatedCallableInfo, Extension, ExtensionSet, FilterNodeClass,
    FilterOptions, FunctionOptions, PhpCallable, TestNodeClass, TestOptions, TwigFilter,
    TwigFunction, TwigTest,
};
use twig::lexer::LexerOptions;
use twig::parser::expression::infix::BinaryOperatorExpressionParser;
use twig::parser::expression::prefix::UnaryOperatorExpressionParser;
use twig::parser::expression::{ExpressionParserEntry, InfixAssociativity, PrecedenceChange};
use twig::{Error, Source};

use crate::common::Json;

/// The test extension of `expressions.gen.php` (`make_extension()`).
pub struct TestExtension {
    pub filters: Vec<TwigFilter>,
    pub functions: Vec<TwigFunction>,
    pub tests: Vec<TwigTest>,
    pub parsers: Vec<ExpressionParserEntry>,
}

impl Extension for TestExtension {
    fn class_name(&self) -> &str {
        "TestExtension"
    }

    fn filters(&self) -> Vec<TwigFilter> {
        self.filters.clone()
    }

    fn functions(&self) -> Vec<TwigFunction> {
        self.functions.clone()
    }

    fn tests(&self) -> Vec<TwigTest> {
        self.tests.clone()
    }

    fn expression_parsers(&self) -> Vec<ExpressionParserEntry> {
        self.parsers.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn closure() -> Option<PhpCallable> {
    Some(PhpCallable::closure(vec![CallableParameter::variadic(
        "args",
    )]))
}

fn bool_option(options: &Json, key: &str) -> bool {
    matches!(options.get(key), Some(Json::Bool(true)))
}

fn string_option(value: Option<&Json>) -> Option<String> {
    value.and_then(Json::as_str).map(str::to_owned)
}

fn deprecation_option(options: &Json) -> Option<DeprecatedCallableInfo> {
    let list = options.get("deprecated")?.as_array();
    let item = |i: usize| list.get(i).and_then(Json::as_str).map(str::to_owned);
    Some(DeprecatedCallableInfo {
        package: item(0).unwrap_or_default(),
        version: item(1).unwrap_or_default(),
        alt_name: item(2),
        alt_package: item(3),
        alt_version: item(4),
    })
}

fn safe_option(options: &Json) -> Option<Vec<ConstantValue>> {
    options.get("is_safe").map(|list| {
        list.as_array()
            .iter()
            .map(|v| ConstantValue::String(v.as_str().unwrap_or_default().as_bytes().to_vec()))
            .collect()
    })
}

impl TestExtension {
    /// Builds the extension from the `ext` field of a vector case.
    pub fn from_json(ext: &Json) -> Self {
        let mut extension = TestExtension {
            filters: Vec::new(),
            functions: Vec::new(),
            tests: Vec::new(),
            parsers: Vec::new(),
        };
        let callables = |key: &str| -> Vec<(String, Json)> {
            ext.get(key)
                .map(|list| {
                    list.as_array()
                        .iter()
                        .map(|item| {
                            let item = item.as_array();
                            (item[0].as_str().unwrap().to_owned(), item[1].clone())
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        for (name, options) in callables("filters") {
            let node_class = match options.get("node_class").and_then(Json::as_str) {
                Some("default") => FilterNodeClass::Default,
                Some("escape") => FilterNodeClass::Escape,
                Some("raw") => FilterNodeClass::Raw,
                _ => FilterNodeClass::Filter,
            };
            extension.filters.push(TwigFilter::new(
                name,
                closure(),
                FilterOptions {
                    needs_environment: bool_option(&options, "needs_environment"),
                    needs_context: bool_option(&options, "needs_context"),
                    needs_charset: bool_option(&options, "needs_charset"),
                    needs_is_sandboxed: bool_option(&options, "needs_is_sandboxed"),
                    is_variadic: bool_option(&options, "is_variadic"),
                    deprecation_info: deprecation_option(&options),
                    is_safe: safe_option(&options),
                    pre_escape: string_option(options.get("pre_escape")),
                    node_class,
                    ..Default::default()
                },
            ));
        }
        for (name, options) in callables("functions") {
            extension.functions.push(TwigFunction::new(
                name,
                closure(),
                FunctionOptions {
                    needs_environment: bool_option(&options, "needs_environment"),
                    needs_context: bool_option(&options, "needs_context"),
                    needs_charset: bool_option(&options, "needs_charset"),
                    needs_is_sandboxed: bool_option(&options, "needs_is_sandboxed"),
                    is_variadic: bool_option(&options, "is_variadic"),
                    deprecation_info: deprecation_option(&options),
                    is_safe: safe_option(&options),
                    ..Default::default()
                },
            ));
        }
        for (name, options) in callables("tests") {
            let node_class = match options.get("node_class").and_then(Json::as_str) {
                Some("null") => TestNodeClass::Null,
                Some("defined") => TestNodeClass::Defined,
                _ => TestNodeClass::Test,
            };
            extension.tests.push(TwigTest::new(
                name,
                closure(),
                TestOptions {
                    needs_is_sandboxed: bool_option(&options, "needs_is_sandboxed"),
                    is_variadic: bool_option(&options, "is_variadic"),
                    deprecation_info: deprecation_option(&options),
                    one_mandatory_argument: bool_option(&options, "one_mandatory_argument"),
                    node_class,
                    ..Default::default()
                },
            ));
        }
        if let Some(list) = ext.get("unary") {
            for item in list.as_array() {
                let item = item.as_array();
                let op = match item[2].as_str() {
                    Some("not") => UnaryOp::Not,
                    Some("pos") => UnaryOp::Pos,
                    _ => UnaryOp::Neg,
                };
                let mut parser = UnaryOperatorExpressionParser::core(
                    op,
                    item[0].as_str().unwrap(),
                    item[1].as_i64().unwrap(),
                );
                if let Some(new) = item.get(3).and_then(Json::as_i64) {
                    parser = parser
                        .with_precedence_change(PrecedenceChange::new("acme/ops", "1.0", new));
                }
                extension
                    .parsers
                    .push(ExpressionParserEntry::prefix(parser));
            }
        }
        if let Some(list) = ext.get("binary") {
            for item in list.as_array() {
                let item = item.as_array();
                let op = match item[3].as_str() {
                    Some("sub") => BinaryOp::Sub,
                    Some("power") => BinaryOp::Power,
                    Some("concat") => BinaryOp::Concat,
                    _ => BinaryOp::Add,
                };
                let associativity = if item[2].as_str() == Some("left") {
                    InfixAssociativity::Left
                } else {
                    InfixAssociativity::Right
                };
                let mut parser = BinaryOperatorExpressionParser::core(
                    op,
                    item[0].as_str().unwrap(),
                    item[1].as_i64().unwrap(),
                    associativity,
                );
                if let Some(new) = item.get(4).and_then(Json::as_i64) {
                    parser = parser
                        .with_precedence_change(PrecedenceChange::new("acme/ops", "1.0", new));
                }
                if let Some(aliases) = item.get(5) {
                    let aliases: Vec<&str> =
                        aliases.as_array().iter().filter_map(Json::as_str).collect();
                    parser = parser.with_aliases(&aliases);
                }
                extension.parsers.push(ExpressionParserEntry::infix(parser));
            }
        }
        extension
    }
}

/// The node visitors of the later pipeline stages (auto-escaping, optimizing, sandboxing), which the parser tests
/// leave out.
const LATER_STAGE_VISITORS: [&str; 3] = [
    "Twig\\NodeVisitor\\EscaperNodeVisitor",
    "Twig\\NodeVisitor\\OptimizerNodeVisitor",
    "Twig\\NodeVisitor\\SandboxNodeVisitor",
];

/// Initializes the extensions and keeps only the parser's own node visitors (every visitor but the escaper,
/// optimizer and sandbox visitors), as the oracle's `keep_parser_visitors()` does in the generators of the parser
/// vectors: the trees compared are those of the parser and the `CorrectnessNodeVisitor`.
pub fn keep_parser_visitors(mut set: ExtensionSet) -> ExtensionSet {
    set.retain_node_visitors(|visitor| !LATER_STAGE_VISITORS.contains(&visitor.class_name()));
    set
}

/// The core extensions with the parser's own node visitors only ([`keep_parser_visitors`]).
pub fn parser_set() -> ExtensionSet {
    keep_parser_visitors(ExtensionSet::with_core_extensions())
}

/// The core extensions plus the optional extensions (`extensions`, e.g. `["sandbox"]`) and the test extension
/// (`ext`) of a vector case, in the order the generator registers them, with the parser's own node visitors only.
pub fn extension_set(case: &Json) -> ExtensionSet {
    let mut set = ExtensionSet::with_core_extensions();
    if let Some(extensions) = case.get("extensions") {
        for extension in extensions.as_array() {
            match extension.as_str() {
                Some("sandbox") => set
                    .add_extension(Arc::new(twig::extension::SandboxExtension::default()))
                    .expect("the set is not initialized yet"),
                other => panic!("unknown extension {other:?}"),
            }
        }
    }
    if let Some(ext) = case.get("ext") {
        set.add_extension(Arc::new(TestExtension::from_json(ext)))
            .expect("the set is not initialized yet");
    }
    keep_parser_visitors(set)
}

/// `$env->parse($env->tokenize($source))`: the module or the error, with every deprecation raised on the way
/// (registry initialization first, as in PHP where the lexer initializes the extensions).
pub fn parse_with(set: &ExtensionSet, source: &Source) -> (Result<Node, Error>, Vec<Deprecation>) {
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let lexer = set.lexer(LexerOptions::default());
    deprecations.extend(set.initialization_deprecations().iter().cloned());
    let result = lexer
        .tokenize(source, &mut deprecations)
        .and_then(|stream| {
            twig::parser::Parser::new(set, stream, &mut deprecations).parse(None, false)
        });
    (result, deprecations)
}

/// Parses a template named `index` with the core extensions (parser visitors only).
pub fn parse(code: &str) -> (Result<Node, Error>, Vec<Deprecation>) {
    parse_with(&parser_set(), &Source::new(code, "index"))
}

/// The error message of parsing `code` with the core extensions.
pub fn parse_error(code: &str) -> String {
    match parse(code).0 {
        Ok(_) => panic!("{code:?} parsed without error"),
        Err(error) => error.to_string(),
    }
}

/// The expression of the first print statement of a module (`body.0.expr` or `body.0.0.expr`).
pub fn first_print_expr(module: &Node) -> &twig::ast::Expr {
    let module = module.as_module().expect("a module");
    let body = module
        .body
        .as_nodes()
        .expect("a body")
        .nodes()
        .next()
        .expect("a node");
    let print = match &body.kind {
        twig::ast::NodeKind::Print(_) => body,
        _ => body
            .as_nodes()
            .expect("nodes")
            .nodes()
            .next()
            .expect("a print node"),
    };
    match &print.kind {
        twig::ast::NodeKind::Print(print) => &print.expr,
        _ => panic!("not a print node: {print:?}"),
    }
}
