//! The core extension metadata against the registry of a PHP Twig 3.31.1 environment
//! (`tests/vectors/parser/registry.json`).
//!
//! Regenerate (from the repository root; Git Bash on Windows needs MSYS_NO_PATHCONV=1):
//!
//!     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/parser/registry.gen.php > tests/vectors/parser/registry.json

use std::sync::Arc;

use twig::ast::{ArrayKey, ConstantValue};
use twig::extension::{
    CallableKind, DebugExtension, ExtensionSet, FilterNodeClass, FunctionNodeClass, PhpCallable,
    SandboxBridgeExtension, SandboxExtension, StringLoaderExtension, TestNodeClass, TwigCallable,
};
use twig::lexer::DEFAULT_OPERATORS;
use twig::loader::ArrayLoader;
use twig::parser::expression::{ExpressionParserEntry, InfixAssociativity};
use twig::runtime::Environment;

use super::support::keep_parser_visitors;
use crate::common::Json;

fn full_set() -> ExtensionSet {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_extension(Arc::new(DebugExtension::new())).unwrap();
    set.add_extension(Arc::new(StringLoaderExtension::new()))
        .unwrap();
    set.add_extension(Arc::new(SandboxExtension::default()))
        .unwrap();
    set.add_extension(Arc::new(SandboxBridgeExtension::new()))
        .unwrap();
    set
}

/// PHP `var_export()` of a default value, with newlines removed.
fn var_export(value: &ConstantValue) -> String {
    match value {
        ConstantValue::Null => "NULL".into(),
        ConstantValue::Bool(b) => b.to_string(),
        ConstantValue::Int(i) => i.to_string(),
        ConstantValue::Float(f) => twig::ast::php_var_export_float(*f),
        ConstantValue::String(s) => {
            let s = String::from_utf8_lossy(s)
                .replace('\\', "\\\\")
                .replace('\'', "\\'");
            format!("'{s}'")
        }
        ConstantValue::Array(items) => {
            let mut out = String::from("array (");
            for (key, value) in items {
                let key = match key {
                    ArrayKey::Int(i) => i.to_string(),
                    ArrayKey::String(s) => format!("'{}'", String::from_utf8_lossy(s)),
                };
                out.push_str(&format!("  {key} => {},", var_export(value)));
            }
            out.push(')');
            out
        }
    }
}

fn json_bool(value: &Json) -> bool {
    matches!(value, Json::Bool(true))
}

fn json_strings(value: &Json) -> Option<Vec<String>> {
    if value.is_null() {
        return None;
    }
    Some(
        value
            .as_array()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect(),
    )
}

fn compare_callable(expected: &Json, callable: &TwigCallable, failures: &mut Vec<String>) {
    let name = String::from_utf8_lossy(callable.name()).into_owned();
    let (node_class, options) = match callable {
        TwigCallable::Filter(f) => {
            let class = match f.node_class() {
                FilterNodeClass::Filter => "FilterExpression",
                FilterNodeClass::Default => "DefaultFilter",
                FilterNodeClass::Escape => "EscapeFilter",
                FilterNodeClass::Raw => "RawFilter",
                FilterNodeClass::Custom(_) => "custom",
            };
            let o = f.options();
            (
                class,
                (
                    o.needs_environment,
                    o.needs_context,
                    o.needs_charset,
                    o.needs_is_sandboxed,
                    o.is_variadic,
                    o.always_allowed_in_sandbox,
                ),
            )
        }
        TwigCallable::Function(f) => {
            let class = match f.node_class() {
                FunctionNodeClass::Function => "FunctionExpression",
                FunctionNodeClass::Enum => "EnumFunction",
                FunctionNodeClass::EnumCases => "EnumCasesFunction",
                FunctionNodeClass::RenderSandboxed => "RenderSandboxedFunction",
                FunctionNodeClass::Custom(_) => "custom",
            };
            let o = f.options();
            (
                class,
                (
                    o.needs_environment,
                    o.needs_context,
                    o.needs_charset,
                    o.needs_is_sandboxed,
                    o.is_variadic,
                    o.always_allowed_in_sandbox,
                ),
            )
        }
        TwigCallable::Test(t) => {
            let class = match t.node_class() {
                TestNodeClass::Test => "TestExpression",
                TestNodeClass::Constant => "ConstantTest",
                TestNodeClass::Defined => "DefinedTest",
                TestNodeClass::DivisibleBy => "DivisiblebyTest",
                TestNodeClass::Even => "EvenTest",
                TestNodeClass::NonStringCoercingInput => "NonStringCoercingInputTest",
                TestNodeClass::Null => "NullTest",
                TestNodeClass::Odd => "OddTest",
                TestNodeClass::SameAs => "SameasTest",
                TestNodeClass::True => "TrueTest",
                TestNodeClass::Custom(_) => "custom",
            };
            let o = t.options();
            (
                class,
                (
                    o.needs_environment,
                    o.needs_context,
                    o.needs_charset,
                    o.needs_is_sandboxed,
                    o.is_variadic,
                    o.always_allowed_in_sandbox,
                ),
            )
        }
    };
    let mut check = |what: &str, ok: bool| {
        if !ok {
            failures.push(format!(
                "{} {name}: {what}",
                callable.callable_type().as_str()
            ));
        }
    };
    check(
        "name",
        expected.field("name").as_str() == Some(name.as_str()),
    );
    check(
        "node class",
        expected.field("node_class").as_str() == Some(node_class),
    );
    check(
        "needs/variadic/sandbox flags",
        (
            json_bool(expected.field("needs_environment")),
            json_bool(expected.field("needs_context")),
            json_bool(expected.field("needs_charset")),
            json_bool(expected.field("needs_is_sandboxed")),
            json_bool(expected.field("is_variadic")),
            json_bool(expected.field("always_allowed_in_sandbox")),
        ) == options,
    );
    check(
        "minimal number of required arguments",
        expected.field("min_required").as_i64()
            == Some(callable.minimal_number_of_required_arguments() as i64),
    );
    match callable {
        TwigCallable::Filter(f) => {
            let o = f.options();
            let safe = o.is_safe.as_ref().map(|list| {
                list.iter()
                    .map(|v| String::from_utf8_lossy(&v.to_php_string()).into_owned())
                    .collect::<Vec<_>>()
            });
            check("is_safe", json_strings(expected.field("is_safe")) == safe);
            check(
                "is_safe_callback",
                json_bool(expected.field("has_is_safe_callback")) == o.is_safe_callback.is_some(),
            );
            check(
                "pre_escape",
                expected.field("pre_escape").as_str() == o.pre_escape.as_deref(),
            );
            check(
                "preserves_safety",
                expected.field("preserves_safety").is_null() == o.preserves_safety.is_none(),
            );
            let deprecation = o.deprecation_info.as_ref().map(|d| {
                vec![
                    Some(d.package.clone()),
                    Some(d.version.clone()),
                    d.alt_name.clone(),
                    d.alt_package.clone(),
                    d.alt_version.clone(),
                ]
            });
            let expected_deprecation = (!expected.field("deprecation").is_null()).then(|| {
                expected
                    .field("deprecation")
                    .as_array()
                    .iter()
                    .map(|v| v.as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            });
            check("deprecation", deprecation == expected_deprecation);
        }
        TwigCallable::Function(f) => {
            let o = f.options();
            let safe = o.is_safe.as_ref().map(|list| {
                list.iter()
                    .map(|v| String::from_utf8_lossy(&v.to_php_string()).into_owned())
                    .collect::<Vec<_>>()
            });
            check(
                "is_safe",
                json_bool(expected.field("has_is_safe_callback")) == o.is_safe_callback.is_some(),
            );
            // dump is safe for html under the CLI SAPI, like the oracle
            check("is_safe", json_strings(expected.field("is_safe")) == safe);
            check(
                "parser_callable",
                json_bool(expected.field("has_parser_callable")) == o.parser_callable.is_some(),
            );
            check(
                "deprecation",
                expected.field("deprecation").is_null() == o.deprecation_info.is_none(),
            );
        }
        TwigCallable::Test(t) => {
            check(
                "one_mandatory_argument",
                json_bool(expected.field("one_mandatory_argument"))
                    == t.has_one_mandatory_argument(),
            );
            check(
                "deprecation",
                expected.field("deprecation").is_null() == t.deprecation_info().is_none(),
            );
        }
    }
    compare_php_callable(expected.field("callable"), callable.callable(), &mut check);
}

fn compare_php_callable(
    expected: &Json,
    actual: Option<&PhpCallable>,
    check: &mut impl FnMut(&str, bool),
) {
    let Some(actual) = actual else {
        check("null callable", expected.is_null());
        return;
    };
    if expected.is_null() {
        check("callable", false);
        return;
    }
    check(
        "reflection name",
        expected.field("reflection_name").as_str() == Some(actual.reflection_name().as_str()),
    );
    check(
        "required parameters",
        expected.field("required").as_i64() == Some(actual.number_of_required_parameters() as i64),
    );
    let string = match actual.kind() {
        CallableKind::Function(name) => Some(name.as_str()),
        _ => None,
    };
    check(
        "string callable",
        expected.field("string").as_str() == string,
    );
    let params = expected.field("params").as_array();
    check("parameter count", params.len() == actual.parameters().len());
    for (expected, actual) in params.iter().zip(actual.parameters()) {
        check(
            &format!("parameter {} name", actual.name),
            expected.field("name").as_str() == Some(actual.name.as_str()),
        );
        check(
            &format!("parameter {} optional", actual.name),
            json_bool(expected.field("optional")) == actual.is_optional(),
        );
        check(
            &format!("parameter {} default", actual.name),
            expected.field("default").as_str().map(str::to_owned)
                == actual.default_value().map(var_export),
        );
        check(
            &format!("parameter {} variadic", actual.name),
            json_bool(expected.field("variadic")) == actual.variadic,
        );
        check(
            &format!("parameter {} array type", actual.name),
            json_bool(expected.field("array")) == actual.array_type,
        );
    }
}

#[test]
fn callables_match_php_reflection() {
    let vectors = Json::load_vectors("parser/registry.json");
    let set = full_set();
    let mut failures = Vec::new();

    let filters: Vec<TwigCallable> = set
        .filters()
        .map(|(_, f)| TwigCallable::Filter(f.clone()))
        .collect();
    let functions: Vec<TwigCallable> = set
        .functions()
        .map(|(_, f)| TwigCallable::Function(f.clone()))
        .collect();
    let tests: Vec<TwigCallable> = set
        .tests()
        .map(|(_, t)| TwigCallable::Test(t.clone()))
        .collect();
    for (key, actual) in [
        ("filters", &filters),
        ("functions", &functions),
        ("tests", &tests),
    ] {
        let expected = vectors.field(key).as_array();
        let expected_names: Vec<&str> = expected
            .iter()
            .map(|e| e.field("name").as_str().unwrap())
            .collect();
        let actual_names: Vec<String> = actual
            .iter()
            .map(|c| String::from_utf8_lossy(c.name()).into_owned())
            .collect();
        assert_eq!(expected_names, actual_names, "{key} in registration order");
        for (expected, actual) in expected.iter().zip(actual) {
            compare_callable(expected, actual, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn expression_parsers_match_php() {
    let vectors = Json::load_vectors("parser/registry.json");
    let set = full_set();
    let parsers = set.expression_parsers();
    let expected = vectors.field("expression_parsers").as_array();
    let actual: Vec<_> = parsers.iter().collect();
    assert_eq!(expected.len(), actual.len());
    for (expected, (_, entry)) in expected.iter().zip(&actual) {
        let parser = entry.parser();
        let context = entry.display();
        assert_eq!(expected.field("string").as_str(), Some(context.as_str()));
        assert_eq!(
            expected.field("name").as_str(),
            Some(parser.name()),
            "{context}"
        );
        assert_eq!(
            expected.field("precedence").as_i64(),
            Some(parser.precedence()),
            "{context}"
        );
        let associativity = match entry {
            ExpressionParserEntry::Infix(infix) => Some(match infix.associativity() {
                InfixAssociativity::Left => "Left",
                InfixAssociativity::Right => "Right",
            }),
            ExpressionParserEntry::Prefix(_) => None,
        };
        assert_eq!(
            expected.field("associativity").as_str(),
            associativity,
            "{context}"
        );
        let change = parser.precedence_change().map(|c| {
            (
                c.package().to_owned(),
                c.version().to_owned(),
                c.new_precedence(),
            )
        });
        let expected_change = (!expected.field("precedence_change").is_null()).then(|| {
            let c = expected.field("precedence_change").as_array();
            (
                c[0].as_str().unwrap().to_owned(),
                c[1].as_str().unwrap().to_owned(),
                c[2].as_i64().unwrap(),
            )
        });
        assert_eq!(expected_change, change, "{context}");
        assert_eq!(
            json_strings(expected.field("aliases")),
            Some(parser.aliases().to_vec()),
            "{context}"
        );
        assert_eq!(
            json_strings(expected.field("tokens")),
            Some(parser.operator_tokens()),
            "{context}"
        );
        assert_eq!(
            expected.field("description").as_str(),
            Some(parser.description()),
            "{context}"
        );
    }

    let expected_changes: Vec<(String, Vec<String>)> = vectors
        .field("precedence_changes")
        .as_array()
        .iter()
        .map(|c| {
            let c = c.as_array();
            (
                c[0].as_str().unwrap().to_owned(),
                json_strings(&c[1]).unwrap(),
            )
        })
        .collect();
    let changes = parsers.precedence_changes();
    let actual_changes: Vec<(String, Vec<String>)> = actual
        .iter()
        .filter_map(|(id, entry)| {
            let list = changes[*id as usize].as_ref()?;
            Some((
                entry.display(),
                list.iter()
                    .map(|c| parsers.get(*c).unwrap().display())
                    .collect(),
            ))
        })
        .collect();
    assert_eq!(expected_changes, actual_changes);
}

#[test]
fn the_lexer_gets_its_operators_from_the_registry() {
    let vectors = Json::load_vectors("parser/registry.json");
    let set = full_set();
    let expected = json_strings(vectors.field("operator_tokens")).unwrap();
    assert_eq!(expected, set.expression_parsers().operator_tokens());
    // The lexer stage's constant was dumped from the oracle; the registry reproduces it.
    let defaults: Vec<String> = DEFAULT_OPERATORS.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        defaults,
        ExtensionSet::with_core_extensions()
            .expression_parsers()
            .operator_tokens()
    );
}

#[test]
fn registers_the_tags_in_twig_order() {
    let vectors = Json::load_vectors("parser/registry.json");
    let expected = json_strings(vectors.field("tags")).unwrap();
    let set = full_set();
    let actual: Vec<String> = set
        .token_parsers()
        .map(|(tag, _)| String::from_utf8(tag.to_vec()).unwrap())
        .collect();
    assert_eq!(expected, actual);
    for (tag, parser) in set.token_parsers() {
        assert_eq!(tag, parser.tag().as_bytes());
    }
}

/// The node visitors of the oracle's environment (`new Environment()` plus the Debug, StringLoader, Sandbox and
/// SandboxBridge extensions) are registered in the same order with the same priorities, except Twig's
/// `YieldNotReadyNodeVisitor`, which twig-rs does not need (every node it compiles is yield-ready; the extension
/// registers nothing). The parser tests drop the escaper, optimizer and sandbox visitors again, as the oracle's
/// `keep_parser_visitors()` does.
#[test]
fn registers_the_node_visitors_of_twig() {
    let vectors = Json::load_vectors("parser/registry.json");
    let expected: Vec<(String, i64)> = vectors
        .field("visitors")
        .as_array()
        .iter()
        .map(|v| {
            let v = v.as_array();
            (v[0].as_str().unwrap().to_owned(), v[1].as_i64().unwrap())
        })
        .collect();
    assert_eq!(
        expected,
        [
            ("CorrectnessNodeVisitor".to_owned(), -255),
            ("EscaperNodeVisitor".to_owned(), 0),
            ("YieldNotReadyNodeVisitor".to_owned(), 255),
            ("OptimizerNodeVisitor".to_owned(), 255),
            ("SandboxNodeVisitor".to_owned(), 0),
        ]
    );
    let env = Environment::new(ArrayLoader::new(Vec::<(String, String)>::new()));
    env.add_extension(Arc::new(DebugExtension::new())).unwrap();
    env.add_extension(Arc::new(StringLoaderExtension::new()))
        .unwrap();
    env.add_extension(Arc::new(SandboxExtension::default()))
        .unwrap();
    env.add_extension(Arc::new(SandboxBridgeExtension::new()))
        .unwrap();
    let short = |class: &str| class.rsplit('\\').next().unwrap_or(class).to_owned();
    let actual: Vec<(String, i64)> = env.with_extensions(|set| {
        set.node_visitors()
            .iter()
            .map(|visitor| (short(visitor.class_name()), visitor.priority()))
            .collect()
    });
    let twig_rs: Vec<(String, i64)> = expected
        .into_iter()
        .filter(|(class, _)| class != "YieldNotReadyNodeVisitor")
        .collect();
    assert_eq!(actual, twig_rs);
    let parser_only: Vec<String> = keep_parser_visitors(full_set())
        .node_visitors()
        .iter()
        .map(|visitor| short(visitor.class_name()))
        .collect();
    assert_eq!(parser_only, ["CorrectnessNodeVisitor"]);
}
