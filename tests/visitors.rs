//! Tests of the node visitors of the escaper, the optimizer and the sandbox (`twig::visitor`), against oracle vectors:
//! the node trees Twig 3.31.1's `Environment::parse()` returns after all its node visitors ran
//! (`tests/vectors/visitors/visitors.json`), plus ports of `TWIG/tests/NodeVisitor/OptimizerTest.php` and of the
//! sandbox visitor tests that do not depend on custom node classes.
//!
//! Regenerate the vectors (from the repository root; Git Bash on Windows needs MSYS_NO_PATHCONV=1):
//!
//!     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/visitors/visitors.gen.php > tests/vectors/visitors/visitors.json

mod common;

use std::any::Any;
use std::sync::{Arc, Mutex};

use common::Json;
use twig::Source;
use twig::ast::dump::dump_node_bytes;
use twig::ast::{ConstantValue, Expr, ExprKind, ModuleNode, Node, NodeKind, NodeRef};
use twig::deprecation::Deprecation;
use twig::error::ErrorKind;
use twig::escaper::DefaultStrategy;
use twig::extension::{
    CallableParameter, CoreExtension, DebugExtension, DeprecatedCallableInfo, EscaperExtension,
    Extension, ExtensionSet, FilterNodeClass, FilterOptions, FunctionOptions, OptimizerExtension,
    PhpCallable, RegexValidator, SandboxExtension, TestOptions, TwigFilter, TwigFunction, TwigTest,
};
use twig::parser::ParseContext;
use twig::visitor::OptimizerNodeVisitor;

/// The validation of constant `matches` patterns at parse time (`MatchesBinary::__construct()`): the first warning of
/// `preg_match($regexp, '')` without its `preg_match(): ` prefix.
struct PcreValidator;

impl RegexValidator for PcreValidator {
    fn validate(&self, pattern: &[u8]) -> Option<String> {
        let mut first: Option<twig_pcre::Diagnostic> = None;
        twig_pcre::with_thread_state(|pcre| {
            pcre.preg_match(pattern, b"", &mut |diagnostic| {
                first.get_or_insert(diagnostic);
            });
        });
        first.map(|diagnostic| {
            String::from_utf8_lossy(diagnostic.text().get(14..).unwrap_or_default()).into_owned()
        })
    }
}

/// The test extension of `visitors.gen.php` (`make_extension()`).
struct TestExtension {
    filters: Vec<TwigFilter>,
    functions: Vec<TwigFunction>,
    tests: Vec<TwigTest>,
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

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn flag(options: &Json, key: &str) -> bool {
    matches!(options.get(key), Some(Json::Bool(true)))
}

fn strategies(options: &Json, key: &str) -> Option<Vec<ConstantValue>> {
    options.get(key).map(|list| {
        list.as_array()
            .iter()
            .map(|v| ConstantValue::String(v.as_bytes()))
            .collect()
    })
}

fn deprecation(options: &Json) -> Option<DeprecatedCallableInfo> {
    let list = options.get("deprecated")?.as_array();
    let item = |i: usize| list.get(i).and_then(Json::as_str).unwrap_or_default();
    Some(DeprecatedCallableInfo::new(item(0), item(1)))
}

/// `static fn (...$args) => null`
fn closure() -> Option<PhpCallable> {
    Some(PhpCallable::closure(vec![CallableParameter::variadic(
        "args",
    )]))
}

impl TestExtension {
    fn from_json(ext: &Json) -> Self {
        let entries = |key: &str| -> Vec<(String, Json)> {
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
        let filters = entries("filters")
            .into_iter()
            .map(|(name, o)| {
                let node_class = match o.get("node_class").and_then(Json::as_str) {
                    Some("default") => FilterNodeClass::Default,
                    Some("escape") => FilterNodeClass::Escape,
                    Some("raw") => FilterNodeClass::Raw,
                    _ => FilterNodeClass::Filter,
                };
                TwigFilter::new(
                    name,
                    closure(),
                    FilterOptions {
                        needs_environment: flag(&o, "needs_environment"),
                        needs_context: flag(&o, "needs_context"),
                        needs_charset: flag(&o, "needs_charset"),
                        needs_is_sandboxed: flag(&o, "needs_is_sandboxed"),
                        is_variadic: flag(&o, "is_variadic"),
                        always_allowed_in_sandbox: flag(&o, "always_allowed_in_sandbox"),
                        deprecation_info: deprecation(&o),
                        is_safe: strategies(&o, "is_safe"),
                        pre_escape: o
                            .get("pre_escape")
                            .and_then(Json::as_str)
                            .map(str::to_owned),
                        preserves_safety: strategies(&o, "preserves_safety"),
                        node_class,
                        ..Default::default()
                    },
                )
            })
            .collect();
        let functions = entries("functions")
            .into_iter()
            .map(|(name, o)| {
                TwigFunction::new(
                    name,
                    closure(),
                    FunctionOptions {
                        needs_environment: flag(&o, "needs_environment"),
                        needs_context: flag(&o, "needs_context"),
                        needs_charset: flag(&o, "needs_charset"),
                        needs_is_sandboxed: flag(&o, "needs_is_sandboxed"),
                        is_variadic: flag(&o, "is_variadic"),
                        always_allowed_in_sandbox: flag(&o, "always_allowed_in_sandbox"),
                        deprecation_info: deprecation(&o),
                        is_safe: strategies(&o, "is_safe"),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let tests = entries("tests")
            .into_iter()
            .map(|(name, o)| {
                TwigTest::new(
                    name,
                    closure(),
                    TestOptions {
                        needs_is_sandboxed: flag(&o, "needs_is_sandboxed"),
                        is_variadic: flag(&o, "is_variadic"),
                        always_allowed_in_sandbox: flag(&o, "always_allowed_in_sandbox"),
                        deprecation_info: deprecation(&o),
                        ..Default::default()
                    },
                )
            })
            .collect();
        TestExtension {
            filters,
            functions,
            tests,
        }
    }
}

/// A PHP value of the oracle's JSON (`autoescape_map` values).
fn constant(value: &Json) -> ConstantValue {
    match value {
        Json::Null => ConstantValue::Null,
        Json::Bool(b) => ConstantValue::Bool(*b),
        Json::Number(n) => match n.parse::<i64>() {
            Ok(i) => ConstantValue::Int(i),
            Err(_) => ConstantValue::Float(n.parse().unwrap()),
        },
        Json::String(s) => ConstantValue::String(s.as_bytes().to_vec()),
        Json::Array(items) => ConstantValue::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (twig::ast::ArrayKey::Int(i as i64), constant(v)))
                .collect(),
        ),
        Json::Object(_) => panic!("unsupported value {value:?}"),
    }
}

/// The `autoescape` option of a case.
fn default_strategy(case: &Json) -> DefaultStrategy {
    if let Some(map) = case.get("autoescape_map") {
        let map: Vec<(String, ConstantValue)> = map
            .as_object()
            .iter()
            .map(|(name, value)| (name.clone(), constant(value)))
            .collect();
        return DefaultStrategy::Callback(Arc::new(move |name: &str| {
            Ok(map
                .iter()
                .find(|(n, _)| n == name)
                .map_or(ConstantValue::Bool(false), |(_, v)| v.clone()))
        }));
    }
    match case.get("config").and_then(|c| c.get("autoescape")) {
        None => DefaultStrategy::html(),
        Some(Json::Bool(false)) => DefaultStrategy::Disabled,
        Some(Json::String(s)) => DefaultStrategy::Strategy(s.clone()),
        Some(other) => panic!("unsupported autoescape {other:?}"),
    }
}

/// The extensions of `new Environment(new ArrayLoader(), $config)` (Core, Escaper, YieldNotReady, Optimizer) and of
/// the case.
fn environment(case: &Json) -> ExtensionSet {
    let mut set = ExtensionSet::new();
    let mut add = |extension: Arc<dyn Extension>| set.add_extension(extension).unwrap();
    add(Arc::new(CoreExtension::new()));
    add(Arc::new(EscaperExtension::with_default_strategy(
        default_strategy(case),
    )));
    let optimizations = case
        .get("config")
        .and_then(|c| c.get("optimizations"))
        .and_then(Json::as_i64)
        .unwrap_or(-1);
    add(Arc::new(OptimizerExtension::new(optimizations)));
    if let Some(extensions) = case.get("extensions") {
        for extension in extensions.as_array() {
            match extension.as_str().unwrap() {
                "sandbox" => add(Arc::new(SandboxExtension::default())),
                "debug" => add(Arc::new(DebugExtension::new())),
                other => panic!("unknown extension {other}"),
            }
        }
    }
    if let Some(ext) = case.get("ext") {
        add(Arc::new(TestExtension::from_json(ext)));
    }
    set.set_regex_validator(Arc::new(PcreValidator));
    set
}

/// The used tags, filters, functions and tests of the `CheckSecurityNode`s of a module and its embedded templates.
fn security(module: &ModuleNode, out: &mut Vec<String>) {
    for node in module
        .class_end
        .as_nodes()
        .into_iter()
        .flat_map(|n| n.nodes())
    {
        if let NodeKind::CheckSecurity(check) = &node.kind {
            let pairs = |list: &[(Vec<u8>, i64)]| {
                list.iter()
                    .map(|(name, line)| format!("{}:{line}", String::from_utf8_lossy(name)))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            out.push(format!(
                "tags[{}] filters[{}] functions[{}] tests[{}]",
                pairs(&check.used_tags),
                pairs(&check.used_filters),
                pairs(&check.used_functions),
                pairs(&check.used_tests)
            ));
        }
    }
    for embedded in module.embedded_templates() {
        security(embedded.as_module().unwrap(), out);
    }
}

fn expected_security(case: &Json) -> Vec<String> {
    let pairs = |list: &Json| {
        list.as_array()
            .iter()
            .map(|pair| {
                let pair = pair.as_array();
                format!(
                    "{}:{}",
                    pair[0].as_lossy_string(),
                    pair[1].as_i64().unwrap()
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    case.get("security")
        .map(|list| {
            list.as_array()
                .iter()
                .map(|module| {
                    format!(
                        "tags[{}] filters[{}] functions[{}] tests[{}]",
                        pairs(module.field("tags")),
                        pairs(module.field("filters")),
                        pairs(module.field("functions")),
                        pairs(module.field("tests"))
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `ErrorKind` standing for a PHP exception class of the oracle.
fn kind_of(class: &str) -> ErrorKind {
    match class {
        "Twig\\Error\\SyntaxError" => ErrorKind::Syntax,
        "Twig\\Error\\RuntimeError" => ErrorKind::Runtime,
        "Twig\\Error\\LoaderError" => ErrorKind::Loader,
        // PHP's LogicException and InvalidArgumentException; the TypeError of `needEscaping()` has a
        // LogicException-kind stand-in
        "LogicException" | "InvalidArgumentException" | "TypeError" => ErrorKind::Logic,
        other => panic!("unexpected class {other}"),
    }
}

/// Runs one vector case; `Err` describes the mismatch.
fn check_case(case: &Json) -> Result<(), String> {
    let code = match case.get("source") {
        Some(source) => source.as_bytes(),
        None => common::base64_decode(case.field("source_b64").as_str().unwrap()),
    };
    let name = case.get("name").and_then(Json::as_str).unwrap_or("index");
    let context = ParseContext::new(environment(case));
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let result = context.parse_source(&Source::new(code, name), &mut deprecations);
    let mut problems = Vec::new();
    match (result, case.get("error")) {
        (Ok(module), None) => {
            let expected = case.field("dump").as_bytes();
            let actual = dump_node_bytes(&module);
            if actual != expected {
                problems.push(format!(
                    "dump differs:\n--- expected\n{}\n--- actual\n{}",
                    String::from_utf8_lossy(&expected),
                    String::from_utf8_lossy(&actual)
                ));
            }
            let mut actual_security = Vec::new();
            if case.get("security").is_some() {
                security(module.as_module().unwrap(), &mut actual_security);
            }
            let expected_security = expected_security(case);
            if actual_security != expected_security {
                problems.push(format!(
                    "security lists differ:\n  expected {expected_security:?}\n  actual   {actual_security:?}"
                ));
            }
        }
        (Ok(_), Some(error)) => problems.push(format!("expected error {error:?}, parsed fine")),
        (Err(error), None) => problems.push(format!("unexpected error {error:?}")),
        (Err(error), Some(expected)) => {
            let class = expected.field("class").as_str().unwrap();
            let message = expected.field("message").as_lossy_string();
            if error.kind() != kind_of(class) || error.to_string() != message {
                problems.push(format!(
                    "error differs: expected {class}: {message}, got {:?}: {error}",
                    error.kind()
                ));
            }
        }
    }
    let expected_deprecations: Vec<String> = case
        .field("deprecations")
        .as_array()
        .iter()
        .map(Json::as_lossy_string)
        .collect();
    let actual_deprecations: Vec<String> = deprecations.iter().map(ToString::to_string).collect();
    if expected_deprecations != actual_deprecations {
        problems.push(format!(
            "deprecations differ:\n  expected {expected_deprecations:?}\n  actual   {actual_deprecations:?}"
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[test]
fn visitor_vectors() {
    check_vector_file("visitors/visitors.json", 450);
}

/// The templates of the parser's statement and random batches through every visitor, under rotating environment
/// configurations (`random.gen.py`).
#[test]
fn random_visitor_vectors() {
    check_vector_file("visitors/random.json", 1700);
}

/// Runs an external vector file produced by `visitors.gen.php` (e.g. an ad-hoc batch), given in the
/// `TWIG_VISITOR_VECTORS` environment variable: `cargo test --test visitors -- --ignored external`.
#[test]
#[ignore = "needs an external vector file in TWIG_VISITOR_VECTORS"]
fn external_vectors() {
    let path = std::env::var("TWIG_VISITOR_VECTORS").expect("TWIG_VISITOR_VECTORS is not set");
    check_vectors(&Json::load_path(&path), 0);
}

fn check_vector_file(file: &str, minimum: usize) {
    check_vectors(&Json::load_vectors(file), minimum);
}

fn check_vectors(vectors: &Json, minimum: usize) {
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in vectors.as_array() {
        checked += 1;
        if let Err(problem) = check_case(case) {
            failures.push(format!("{}: {problem}", case.field("id").as_str().unwrap()));
        }
    }
    assert!(checked >= minimum, "only {checked} cases");
    let ids: Vec<&str> = failures
        .iter()
        .map(|f| f.split(':').next().unwrap_or(""))
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {checked} cases failed: {ids:?}\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

// ---- ports of TWIG/tests/NodeVisitor/OptimizerTest.php --------------------------------------------------------------

/// `new Environment(new ArrayLoader(), ['cache' => false, 'autoescape' => $autoescape])` and `$env->parse()`.
fn parse(template: &str, autoescape: DefaultStrategy) -> Node {
    let mut set = ExtensionSet::new();
    set.add_extension(Arc::new(CoreExtension::new())).unwrap();
    set.add_extension(Arc::new(EscaperExtension::with_default_strategy(
        autoescape,
    )))
    .unwrap();
    set.add_extension(Arc::new(OptimizerExtension::default()))
        .unwrap();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    ParseContext::new(set)
        .parse_source(&Source::new(template, "index"), &mut deprecations)
        .unwrap()
}

#[test]
fn optimizer_constructor() {
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let visitor =
        OptimizerNodeVisitor::new(OptimizerNodeVisitor::OPTIMIZE_FOR, &mut deprecations).unwrap();
    assert_eq!(visitor.optimizers(), 2);
    assert!(deprecations.is_empty());
    assert_eq!(
        OptimizerNodeVisitor::new(15, &mut deprecations)
            .unwrap_err()
            .to_string(),
        "Optimizer mode \"15\" is not valid."
    );
    OptimizerNodeVisitor::new(OptimizerNodeVisitor::OPTIMIZE_RAW_FILTER, &mut deprecations)
        .unwrap();
    assert_eq!(
        deprecations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "Since twig/twig 3.11: The \"Twig\\NodeVisitor\\OptimizerNodeVisitor::OPTIMIZE_RAW_FILTER\" option is deprecated and does nothing."
        ]
    );
}

#[test]
fn render_block_optimizer() {
    let module = parse("{{ block(\"foo\") }}", DefaultStrategy::Disabled);
    let body = module.as_module().unwrap().body.as_nodes().unwrap();
    let node = body.nodes().next().unwrap();
    let ExprKind::BlockReference(reference) = &node.as_expr().unwrap().kind else {
        panic!("expected a BlockReferenceExpression, got {node:?}");
    };
    assert!(reference.output);
}

#[test]
fn render_parent_block_optimizer() {
    let module = parse(
        "{% extends \"foo\" %}{% block content %}{{ parent() }}{% endblock %}",
        DefaultStrategy::Disabled,
    );
    let module = module.as_module().unwrap();
    let (_, block) = module.blocks().next().unwrap();
    let NodeKind::Block(block) = &block.kind else {
        panic!("expected a block");
    };
    let ExprKind::Parent(parent) = &block.body.as_expr().unwrap().kind else {
        panic!("expected a ParentExpression, got {:?}", block.body);
    };
    assert!(parent.output);
}

/// Every `ContextVariable` named `target` is always defined.
fn check_for_var_configuration(node: NodeRef<'_>, target: &[u8]) {
    for (_, child) in node.children() {
        if let Some(expr) = child.as_expr()
            && let ExprKind::ContextVariable(name) = &expr.kind
            && name.name == target
        {
            assert!(
                expr.always_defined,
                "{} is not always defined",
                String::from_utf8_lossy(target)
            );
        } else {
            check_for_var_configuration(child, target);
        }
    }
}

#[test]
fn for_var_optimizer() {
    let module = parse(
        "{% for i, j in foo %}{{ loop.index }}{{ i }}{{ j }}{% endfor %}",
        DefaultStrategy::Disabled,
    );
    for target in [b"loop".as_slice(), b"i", b"j"] {
        check_for_var_configuration(NodeRef::Node(&module), target);
    }
}

/// The `with_loop` attribute of the loop whose value target is `target`.
fn with_loop_of(node: NodeRef<'_>, target: &[u8]) -> Option<bool> {
    for (_, child) in node.children() {
        if let NodeRef::Node(n) = child
            && let NodeKind::For(for_node) = &n.kind
            && for_node.value_target.context_variable_name() == Some(target)
        {
            return Some(for_node.with_loop);
        }
        if let Some(found) = with_loop_of(child, target) {
            return Some(found);
        }
    }
    None
}

#[test]
fn for_loop_optimizer() {
    let cases: &[(&str, &[(&str, bool)])] = &[
        ("{% for i in foo %}{% endfor %}", &[("i", false)]),
        (
            "{% for i in foo %}{{ loop.index }}{% endfor %}",
            &[("i", true)],
        ),
        (
            "{% for i in foo %}{% for j in foo %}{% endfor %}{% endfor %}",
            &[("i", false), ("j", false)],
        ),
        (
            "{% for i in foo %}{% include \"foo\" %}{% endfor %}",
            &[("i", true)],
        ),
        (
            "{% for i in foo %}{% include \"foo\" only %}{% endfor %}",
            &[("i", false)],
        ),
        (
            "{% for i in foo %}{% include \"foo\" with { \"foo\": \"bar\" } only %}{% endfor %}",
            &[("i", false)],
        ),
        (
            "{% for i in foo %}{% include \"foo\" with { \"foo\": loop.index } only %}{% endfor %}",
            &[("i", true)],
        ),
        (
            "{% for i in foo %}{% for j in foo %}{{ loop.index }}{% endfor %}{% endfor %}",
            &[("i", false), ("j", true)],
        ),
        (
            "{% for i in foo %}{% for j in foo %}{{ loop.parent.loop.index }}{% endfor %}{% endfor %}",
            &[("i", true), ("j", true)],
        ),
        (
            "{% for i in foo %}{% for j in [loop.index] %}{% endfor %}{% endfor %}",
            &[("i", true), ("j", false)],
        ),
        (
            "{% for i in foo %}{% for j in loop %}{% endfor %}{% endfor %}",
            &[("i", true), ("j", false)],
        ),
        (
            "{% for i in foo %}{% set l = loop %}{% for j in foo %}{{ l.index }}{% endfor %}{% endfor %}",
            &[("i", true), ("j", false)],
        ),
        (
            "{% for i in foo %}{% for j in foo %}{{ foo.parent.loop.index }}{% endfor %}{% endfor %}",
            &[("i", false), ("j", false)],
        ),
        (
            "{% for i in foo %}{% for j in foo %}{{ loop[\"parent\"].loop.index }}{% endfor %}{% endfor %}",
            &[("i", true), ("j", true)],
        ),
        (
            "{% for i in foo %}{{ include(\"foo\") }}{% endfor %}",
            &[("i", true)],
        ),
        (
            "{% for i in foo %}{{ include(\"foo\", with_context = false) }}{% endfor %}",
            &[("i", false)],
        ),
        (
            "{% for i in foo %}{{ include(\"foo\", with_context = true) }}{% endfor %}",
            &[("i", true)],
        ),
        (
            "{% for i in foo %}{{ include(\"foo\", { \"foo\": \"bar\" }, with_context = false) }}{% endfor %}",
            &[("i", false)],
        ),
        (
            "{% for i in foo %}{{ include(\"foo\", { \"foo\": loop.index }, with_context = false) }}{% endfor %}",
            &[("i", true)],
        ),
    ];
    for (template, expected) in cases {
        let module = parse(template, DefaultStrategy::html());
        for (target, with_loop) in *expected {
            assert_eq!(
                with_loop_of(NodeRef::Node(&module), target.as_bytes()),
                Some(*with_loop),
                "variable {target} in {template}"
            );
        }
    }
}

// ---- the default strategy ------------------------------------------------------------------------------------------

#[test]
fn default_strategies_are_resolved_once_per_name() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&calls);
    let extension = EscaperExtension::with_default_strategy(DefaultStrategy::Callback(Arc::new(
        move |name: &str| {
            seen.lock().unwrap().push(name.to_owned());
            Ok(ConstantValue::from("js"))
        },
    )));
    assert_eq!(
        extension.default_strategy("a").unwrap(),
        ConstantValue::from("js")
    );
    assert_eq!(
        extension.default_strategy("a").unwrap(),
        ConstantValue::from("js")
    );
    assert_eq!(
        extension.default_strategy("b").unwrap(),
        ConstantValue::from("js")
    );
    assert_eq!(*calls.lock().unwrap(), ["a", "b"]);

    // the 1000 most recent names are remembered
    for i in 0..1000 {
        extension.default_strategy(&format!("n{i}")).unwrap();
    }
    calls.lock().unwrap().clear();
    extension.default_strategy("n999").unwrap();
    extension.default_strategy("a").unwrap();
    assert_eq!(*calls.lock().unwrap(), ["a"]);

    // a new default forgets the resolved strategies
    extension.set_default_strategy(DefaultStrategy::Strategy("name".to_owned()));
    assert_eq!(
        extension.default_strategy("x.txt").unwrap(),
        ConstantValue::Bool(false)
    );
    assert_eq!(
        extension.default_strategy("x.js.twig").unwrap(),
        ConstantValue::from("js")
    );
    extension.set_default_strategy(DefaultStrategy::Disabled);
    assert_eq!(
        extension.default_strategy("x.js.twig").unwrap(),
        ConstantValue::Bool(false)
    );
    assert_eq!(
        EscaperExtension::new().default_strategy("x.txt").unwrap(),
        ConstantValue::from("html")
    );
}

#[test]
fn callback_errors_stop_the_compilation() {
    let mut set = ExtensionSet::new();
    set.add_extension(Arc::new(CoreExtension::new())).unwrap();
    set.add_extension(Arc::new(EscaperExtension::with_default_strategy(
        DefaultStrategy::Callback(Arc::new(|_: &str| {
            Err(twig::Error::logic("Unable to choose a strategy."))
        })),
    )))
    .unwrap();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let error = ParseContext::new(set)
        .parse_template(&Source::new("{{ a }}", "index"), &mut deprecations)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the compilation of a template (\"Unable to choose a strategy.\") in \"index\"."
    );
}

// ---- the sandbox visitor --------------------------------------------------------------------------------------------

/// Port of `SandboxTest::testSelfIsNeverWrapped()` (on a print instead of a custom node class) and of the
/// generator-expression test's expectation that a plain print of a variable is wrapped.
#[test]
fn sandbox_wraps_coerced_variables_but_not_self() {
    let mut set = ExtensionSet::new();
    set.add_extension(Arc::new(CoreExtension::new())).unwrap();
    set.add_extension(Arc::new(EscaperExtension::with_default_strategy(
        DefaultStrategy::Disabled,
    )))
    .unwrap();
    set.add_extension(Arc::new(SandboxExtension::default()))
        .unwrap();
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let module = ParseContext::new(set)
        .parse_source(
            &Source::new("{{ _self }}{{ foo }}", "foo"),
            &mut deprecations,
        )
        .unwrap();
    let body = module.as_module().unwrap().body.as_nodes().unwrap();
    let prints: Vec<&Expr> = body
        .nodes()
        .next()
        .unwrap()
        .as_nodes()
        .unwrap()
        .nodes()
        .map(|print| match &print.kind {
            NodeKind::Print(print) => &print.expr,
            other => panic!("expected a print, got {other:?}"),
        })
        .collect();
    assert!(matches!(prints[0].kind, ExprKind::ContextVariable(_)));
    assert!(matches!(prints[1].kind, ExprKind::CheckToString(_)));
}

/// The first printed expression of a sandboxed template: `{{ ... }}` parsed with the core, escaper (auto-escaping off)
/// and sandbox extensions plus `extra`.
fn sandboxed_print(code: &str, extra: impl FnOnce(&mut ExtensionSet)) -> Expr {
    let mut set = ExtensionSet::new();
    set.add_extension(Arc::new(CoreExtension::new())).unwrap();
    set.add_extension(Arc::new(EscaperExtension::with_default_strategy(
        DefaultStrategy::Disabled,
    )))
    .unwrap();
    set.add_extension(Arc::new(SandboxExtension::default()))
        .unwrap();
    extra(&mut set);
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let module = ParseContext::new(set)
        .parse_source(&Source::new(code, "index"), &mut deprecations)
        .unwrap();
    let body = module.as_module().unwrap().body.as_nodes().unwrap();
    let print = body.nodes().next().unwrap();
    match &print.kind {
        NodeKind::Print(print) => print.expr.clone(),
        other => panic!("expected a print, got {other:?}"),
    }
}

/// The argument `index` of the function call under the `CheckToStringNode` of a print.
fn printed_call_argument(print: &Expr, index: i64) -> &Expr {
    let ExprKind::CheckToString(check) = &print.kind else {
        panic!("the printed call is not wrapped: {print:?}");
    };
    let ExprKind::Function(function) = &check.expr.kind else {
        panic!("not a function call: {:?}", check.expr);
    };
    function
        .arguments
        .get(&twig::ast::ArgumentKey::Index(index))
        .expect("the argument")
}

/// Twig: `SandboxNodeVisitor::wrapArguments()` skips the arguments of parameters whose PHP type cannot string-coerce
/// (`CallableParameters::isStringCoercionSafe()`); host callables declare it on their parameters.
#[test]
fn sandbox_skips_arguments_of_coercion_safe_parameters() {
    let add = |safe: bool| {
        move |set: &mut ExtensionSet| {
            let mut parameter = CallableParameter::required("n");
            if safe {
                parameter = parameter.string_coercion_safe();
            }
            set.add_function(TwigFunction::new(
                "f",
                Some(PhpCallable::closure(vec![parameter])),
                FunctionOptions::default(),
            ))
            .unwrap();
        }
    };
    let untyped = sandboxed_print("{{ f(x) }}", add(false));
    assert!(matches!(
        printed_call_argument(&untyped, 0).kind,
        ExprKind::CheckToString(_)
    ));
    let typed = sandboxed_print("{{ f(x) }}", add(true));
    assert!(matches!(
        printed_call_argument(&typed, 0).kind,
        ExprKind::ContextVariable(_)
    ));
}

/// An expression class defined by an extension that implements `CoercesChildrenToStringInterface`: the sandbox wraps
/// the children it lists in `getStringCoercedChildNames()`.
#[derive(Debug, Clone, PartialEq)]
struct Shout {
    node: Expr,
    coerces: bool,
}

impl twig::ast::CustomExpr for Shout {
    fn class_name(&self) -> &str {
        "ShoutExpression"
    }

    fn children(&self) -> Vec<(twig::ast::ChildKey<'_>, NodeRef<'_>)> {
        vec![(twig::ast::ChildKey::Name("node"), NodeRef::Expr(&self.node))]
    }

    fn children_mut(&mut self) -> Vec<(twig::ast::ChildKey<'_>, twig::ast::NodeMut<'_>)> {
        vec![(
            twig::ast::ChildKey::Name("node"),
            twig::ast::NodeMut::Expr(&mut self.node),
        )]
    }

    fn clone_box(&self) -> Box<dyn twig::ast::CustomExpr> {
        Box::new(self.clone())
    }

    fn eq_dyn(&self, other: &dyn twig::ast::CustomExpr) -> bool {
        other.as_any().downcast_ref::<Shout>() == Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn string_coerced_child_names(&self) -> Vec<String> {
        if self.coerces {
            vec!["node".to_owned()]
        } else {
            Vec::new()
        }
    }
}

/// The node class of the `shout` function: a [`Shout`] around its first argument.
struct ShoutFactory {
    coerces: bool,
}

impl twig::extension::FunctionNodeFactory for ShoutFactory {
    fn class_name(&self) -> &str {
        "ShoutExpression"
    }

    fn create(
        &self,
        _function: TwigFunction,
        arguments: twig::ast::Arguments,
        line: i64,
    ) -> twig::Result<Expr> {
        let node = arguments
            .get(&twig::ast::ArgumentKey::Index(0))
            .cloned()
            .expect("an argument");
        Ok(Expr::new(
            ExprKind::Custom(Box::new(Shout {
                node,
                coerces: self.coerces,
            })),
            line,
        ))
    }
}

#[test]
fn sandbox_wraps_the_children_custom_nodes_coerce() {
    let add = |coerces: bool| {
        move |set: &mut ExtensionSet| {
            set.add_function(TwigFunction::new(
                "shout",
                Some(PhpCallable::closure(vec![CallableParameter::required(
                    "value",
                )])),
                FunctionOptions {
                    node_class: twig::extension::FunctionNodeClass::Custom(Arc::new(
                        ShoutFactory { coerces },
                    )),
                    ..Default::default()
                },
            ))
            .unwrap();
        }
    };
    let child = |print: &Expr| -> ExprKind {
        match &print.kind {
            ExprKind::Custom(custom) => {
                let shout = custom.as_any().downcast_ref::<Shout>().expect("a Shout");
                shout.node.kind.clone()
            }
            other => panic!("expected the custom node, got {other:?}"),
        }
    };
    let plain = sandboxed_print("{{ shout(x) }}", add(false));
    assert!(matches!(child(&plain), ExprKind::ContextVariable(_)));
    let coercing = sandboxed_print("{{ shout(x) }}", add(true));
    assert!(matches!(child(&coercing), ExprKind::CheckToString(_)));
}
