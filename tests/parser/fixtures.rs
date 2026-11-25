//! The templates of Twig 3.31.1's integration fixtures (`tests/twig/Fixtures`) through the parser, in the
//! environment of Twig's integration tests (`Twig\Tests\IntegrationTest`):
//!
//! - `fixture_templates_match_the_oracle`: every template against `tests/vectors/parser/fixtures.json` (the node tree,
//!   or the error, and the deprecations of `$env->parse($env->tokenize($source))` with the parser's own visitors);
//! - `load_errors_of_the_corpus`: the `.test` files split on their `--TEMPLATE--` markers, every template parsed in
//!   load order with `compileSource()`'s error handling; a fixture whose oracle result (`tests/twig/oracle-corpus.json`)
//!   is a load-phase error raised by the parser fails with the identical message, every other template parses. The
//!   load errors raised later (compilation, template construction, PCRE) are listed with their reason.
//!
//! Regenerate the vectors (from the repository root; Git Bash on Windows needs MSYS_NO_PATHCONV=1):
//!
//!     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/parser/fixtures.gen.php > tests/vectors/parser/fixtures.json

use std::any::Any;
use std::sync::Arc;

use twig::ast::dump::dump_node_bytes;
use twig::ast::{ConstantValue, Expr, Node};
use twig::deprecation::Deprecation;
use twig::extension::{
    CallableParameter, DebugExtension, DeprecatedCallableInfo, Extension, ExtensionSet,
    FilterOptions, FunctionOptions, PhpCallable, SandboxExtension, StringLoaderExtension,
    TestOptions, TokenParser, TwigFilter, TwigFunction, TwigTest,
};
use twig::lexer::{Token, TokenType};
use twig::parser::{ParseContext, Parser};
use twig::{Error, Result, Source};

use crate::common::Json;

/// A callable with any arguments (the PHP callables of the test extensions; only their names and options matter
/// to the parser).
fn callable() -> Option<PhpCallable> {
    Some(PhpCallable::closure(vec![CallableParameter::variadic(
        "args",
    )]))
}

fn filter(name: &str, options: FilterOptions) -> TwigFilter {
    TwigFilter::new(name, callable(), options)
}

fn function(name: &str, options: FunctionOptions) -> TwigFunction {
    TwigFunction::new(name, callable(), options)
}

fn test(name: &str) -> TwigTest {
    TwigTest::new(name, callable(), TestOptions::default())
}

fn safe(strategies: &[&str]) -> Option<Vec<ConstantValue>> {
    Some(strategies.iter().map(|s| (*s).into()).collect())
}

/// `TwigTestTokenParser_§`: `{% § %}` prints `§`.
struct SectionTokenParser;

impl TokenParser for SectionTokenParser {
    fn tag(&self) -> &str {
        "§"
    }

    fn parse(&self, parser: &mut Parser<'_>, _token: &Token) -> Result<Option<Node>> {
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        Ok(Some(Node::print(Expr::constant("§", -1), -1)))
    }

    fn class_name(&self) -> &str {
        "Twig\\Tests\\TwigTestTokenParser_§"
    }
}

/// `Twig\Tests\TwigTestExtension` (TWIG/tests/IntegrationTest.php).
struct TwigTestExtension;

impl Extension for TwigTestExtension {
    fn class_name(&self) -> &str {
        "Twig\\Tests\\TwigTestExtension"
    }

    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        vec![Arc::new(SectionTokenParser)]
    }

    fn filters(&self) -> Vec<TwigFilter> {
        let d = FilterOptions::default;
        vec![
            filter("§", d()),
            filter(
                "escape_and_nl2br",
                FilterOptions {
                    needs_environment: true,
                    is_safe: safe(&["html"]),
                    ..d()
                },
            ),
            filter(
                "nl2br",
                FilterOptions {
                    pre_escape: Some("html".to_owned()),
                    is_safe: safe(&["html"]),
                    ..d()
                },
            ),
            filter(
                "escape_something",
                FilterOptions {
                    is_safe: safe(&["something"]),
                    ..d()
                },
            ),
            filter(
                "preserves_safety",
                FilterOptions {
                    preserves_safety: safe(&["html"]),
                    ..d()
                },
            ),
            filter("static_call_string", d()),
            filter("static_call_array", d()),
            filter("magic_call", d()),
            filter("magic_call_closure", d()),
            filter("magic_call_string", d()),
            filter("magic_call_array", d()),
            filter("*_path", d()),
            filter("*_foo_*_bar", d()),
            filter("not", d()),
            filter("anon_foo", d()),
        ]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        let d = FunctionOptions::default;
        vec![
            function("§", d()),
            function(
                "safe_br",
                FunctionOptions {
                    is_safe: safe(&["html"]),
                    ..d()
                },
            ),
            function("unsafe_br", d()),
            function("static_call_string", d()),
            function("static_call_array", d()),
            function("*_path", d()),
            function("*_foo_*_bar", d()),
            function("anon_foo", d()),
            function(
                "deprecated_function",
                FunctionOptions {
                    deprecation_info: Some(DeprecatedCallableInfo {
                        alt_name: Some("not_deprecated_function".to_owned()),
                        ..DeprecatedCallableInfo::new("foo/bar", "1.1")
                    }),
                    ..d()
                },
            ),
        ]
    }

    fn tests(&self) -> Vec<TwigTest> {
        vec![test("multi word"), test("test_*")]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `TwigTestExtensionWithAttributes('*')` and `AttributeExtension(TwigTestLazyExtensionWithAttributes::class)`: the
/// attribute-declared callables.
struct AttributeCallables {
    class: &'static str,
    prefix: &'static str,
}

impl Extension for AttributeCallables {
    fn class_name(&self) -> &str {
        self.class
    }

    fn filters(&self) -> Vec<TwigFilter> {
        vec![filter(
            &format!("{}attribute_filter", self.prefix),
            FilterOptions::default(),
        )]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        vec![function(
            &format!("{}attribute_function", self.prefix),
            FunctionOptions::default(),
        )]
    }

    fn tests(&self) -> Vec<TwigTest> {
        vec![test(&format!("{}attribute_test", self.prefix))]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The extensions and undefined-callable callbacks of `IntegrationTest`, registered in the harness order (Twig's
/// core extensions first, then Debug, Sandbox, StringLoader and the test extensions).
pub fn harness_extensions() -> ExtensionSet {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_extension(Arc::new(DebugExtension::new())).unwrap();
    set.add_extension(Arc::new(SandboxExtension::new()))
        .unwrap();
    set.add_extension(Arc::new(StringLoaderExtension::new()))
        .unwrap();
    set.add_extension(Arc::new(TwigTestExtension)).unwrap();
    set.add_extension(Arc::new(AttributeCallables {
        class: "Twig\\Tests\\TwigTestExtensionWithAttributes",
        prefix: "",
    }))
    .unwrap();
    set.add_extension(Arc::new(AttributeCallables {
        class: "Twig\\Extension\\AttributeExtension",
        prefix: "lazy_",
    }))
    .unwrap();
    set.register_undefined_filter_callback(Arc::new(|name: &[u8]| {
        if name == b"throwing_undefined_filter" {
            return Err(Error::syntax("This filter is undefined in the tests."));
        }
        Ok(None)
    }));
    set.register_undefined_function_callback(Arc::new(|name: &[u8]| {
        if name == b"throwing_undefined_function" {
            return Err(Error::syntax("This function is undefined in the tests."));
        }
        Ok(None)
    }));
    set.register_undefined_test_callback(Arc::new(|name: &[u8]| {
        if name == b"throwing_undefined_test" || name == b"throwing_undefined_two words_test" {
            return Err(Error::syntax("This test is undefined in the tests."));
        }
        // Ensure this does not conflict with `divisible by` and `same as`.
        if name == b"divisible" || name == b"same" {
            return Ok(Some(TwigTest::new(
                name.to_vec(),
                Some(PhpCallable::closure(Vec::new())),
                TestOptions::default(),
            )));
        }
        Ok(None)
    }));
    set
}

#[test]
fn fixture_templates_match_the_oracle() {
    let vectors = Json::load_vectors("parser/fixtures.json");
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in vectors.as_array() {
        let file = case.field("file").as_str().unwrap();
        let template = case.field("template").as_str().unwrap();
        let code = match case.get("source") {
            Some(source) => source.as_bytes(),
            None => crate::common::base64_decode(case.field("source_b64").as_str().unwrap()),
        };
        // `MatchesBinary` validates constant patterns with PHP's PCRE at parse time; the validator comes with the
        // PCRE port.
        if case.get("error").is_some_and(|e| {
            e.field("message")
                .as_lossy_string()
                .starts_with("Regexp \"")
        }) {
            continue;
        }
        checked += 1;
        let context = ParseContext::new(harness_extensions());
        let mut deprecations: Vec<Deprecation> = Vec::new();
        let result = context.parse_source(&Source::new(code, template), &mut deprecations);
        let id = format!("{file} {template}");

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
                "{id}: deprecations\n  expected {expected_deprecations:?}\n  actual   {actual_deprecations:?}"
            ));
        }
        match (case.get("dump"), case.get("error"), result) {
            (Some(dump), None, Ok(module)) => {
                let expected = String::from_utf8_lossy(&dump.as_bytes()).into_owned();
                let actual = String::from_utf8_lossy(&dump_node_bytes(&module)).into_owned();
                if expected != actual {
                    let line = expected
                        .lines()
                        .zip(actual.lines())
                        .position(|(e, a)| e != a)
                        .unwrap_or(0);
                    failures.push(format!(
                        "{id}: dump differs at line {}\n  expected {:?}\n  actual   {:?}",
                        line + 1,
                        expected.lines().nth(line),
                        actual.lines().nth(line)
                    ));
                }
            }
            (None, Some(error), Err(actual)) => {
                let expected = error.field("message").as_lossy_string();
                let class = error.field("class").as_str().unwrap();
                if expected != actual.to_string() || class != actual.kind().class_name() {
                    failures.push(format!(
                        "{id}: error\n  expected {class}: {expected}\n  actual   {}: {actual}",
                        actual.kind().class_name()
                    ));
                }
            }
            (_, _, result) => failures.push(format!("{id}: unexpected result {result:?}")),
        }
    }
    assert!(checked > 650, "only {checked} templates");
    assert!(
        failures.is_empty(),
        "{} of {checked} templates differ:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// The templates of a `.test` file: the text after each `--TEMPLATE--` / `--TEMPLATE(name)--` marker up to the next
/// marker, the `--DATA--`/`--EXCEPTION--` section, or the end; the last template loses one trailing newline (all
/// trailing whitespace in exception fixtures), a repeated name replaces the earlier template in place. A simple
/// port of `IntegrationTestCase::parseTemplates()` for the shapes the fixtures use
/// (`reference/twig/test-suite.md` §1.4, §1.7).
fn split_templates(test: &str) -> Vec<(String, String)> {
    const MARKER: &str = "--TEMPLATE";
    let start = test.find(MARKER).expect("a fixture has a template");
    let exception = test[start..].find("--EXCEPTION--").map(|i| start + i);
    let data = test[start..].find("--DATA--").map(|i| start + i);
    let end = match (exception, data) {
        (Some(e), Some(d)) => e.min(d),
        (Some(e), None) => e,
        (None, Some(d)) => d,
        (None, None) => test.len(),
    };
    let mut block = &test[start..end];
    if exception.is_some() {
        block = block.trim_end_matches([' ', '\t', '\n', '\x0B', '\x0C', '\r']);
    }
    let mut templates: Vec<(String, String)> = Vec::new();
    let pieces: Vec<&str> = block.split(MARKER).skip(1).collect();
    let count = pieces.len();
    for (index, piece) in pieces.into_iter().enumerate() {
        let (name, rest) = match piece.strip_prefix('(') {
            Some(rest) => {
                let close = rest.find(")--").expect("a closed template name");
                (&rest[..close], &rest[close + 1..])
            }
            None => ("", piece),
        };
        let mut body = rest
            .strip_prefix("--")
            .expect("a template marker")
            .to_owned();
        if index + 1 == count && exception.is_none() && body.ends_with('\n') {
            body.pop();
        }
        let name = if name.is_empty() || name == "0" {
            "index.twig".to_owned()
        } else {
            name.to_owned()
        };
        match templates.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = body,
            None => templates.push((name, body)),
        }
    }
    templates
}

/// Load-phase errors of the corpus that are not raised by the parser, with the reason.
const NOT_PARSER_ERRORS: &[(&str, &str)] = &[
    (
        "exceptions/multiline_function_with_unknown_argument.test",
        "compilation: argument binding",
    ),
    (
        "exceptions/undefined_trait.test",
        "template construction: trait check (RuntimeError)",
    ),
    (
        "expressions/call_argument_defined_twice.test",
        "compilation: argument binding",
    ),
    (
        "expressions/call_positional_arg_after_named_arg.test",
        "compilation: argument binding",
    ),
    (
        "expressions/matches_error_compilation.test",
        "parser: needs the PCRE validator",
    ),
    (
        "functions/cycle_without_enough_args.test",
        "compilation: argument binding",
    ),
    (
        "functions/enum/invalid_enum.test",
        "compilation: EnumFunction",
    ),
    (
        "functions/enum/invalid_enum_escaping.test",
        "compilation: EnumFunction",
    ),
    (
        "functions/enum/invalid_literal_type.test",
        "compilation: EnumFunction",
    ),
    (
        "functions/enum_cases/invalid_enum.test",
        "compilation: EnumCasesFunction",
    ),
    (
        "functions/enum_cases/invalid_enum_escaping.test",
        "compilation: EnumCasesFunction",
    ),
    (
        "functions/enum_cases/invalid_literal_type.test",
        "compilation: EnumCasesFunction",
    ),
    (
        "functions/max_without_args.test",
        "compilation: argument binding",
    ),
];

#[test]
fn load_errors_of_the_corpus() {
    let corpus = Json::load_path(&format!(
        "{}/tests/twig/oracle-corpus.json",
        env!("CARGO_MANIFEST_DIR")
    ));
    let context = ParseContext::new(harness_extensions());
    let mut failures = Vec::new();
    let mut templates_parsed = 0;
    let mut parser_errors = 0;
    let mut later_errors: Vec<(String, &str)> = Vec::new();
    for fixture in corpus.field("fixtures").as_array() {
        let file = fixture.field("file").as_str().unwrap();
        let path = format!("{}/tests/twig/Fixtures/{file}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap();
        let templates = split_templates(&text);

        // the simple split agrees with PHP's parse of the fixture
        let expected: Vec<(String, String)> = fixture
            .field("templates")
            .as_object()
            .iter()
            .map(|(name, code)| (name.clone(), code.as_str().unwrap().to_owned()))
            .collect();
        if templates != expected {
            failures.push(format!("{file}: the templates differ from PHP's parse"));
            continue;
        }

        // the first template that fails, in load order
        let mut error: Option<(String, Error)> = None;
        for (name, code) in &templates {
            let mut deprecations: Vec<Deprecation> = Vec::new();
            match context.parse_template(&Source::new(code.as_bytes(), name), &mut deprecations) {
                Ok(_) => templates_parsed += 1,
                Err(e) => {
                    error = Some((name.clone(), e));
                    break;
                }
            }
        }

        // fixtures skipped by their condition have no case
        let expected_error = fixture
            .field("actual")
            .field("cases")
            .as_array()
            .first()
            .filter(|case| case.get("phase").and_then(Json::as_str) == Some("load"))
            .map(|case| case.field("error"));
        match (expected_error, error) {
            (None, None) => {}
            (None, Some((name, error))) => {
                failures.push(format!("{file}: {name} fails to parse: {error}"));
            }
            (Some(expected), Some((name, actual))) => {
                let class = expected.field("class").as_str().unwrap();
                let message = expected.field("message").as_lossy_string();
                if class != actual.kind().class_name() || message != actual.to_string() {
                    failures.push(format!(
                        "{file}: {name}\n  expected {class}: {message}\n  actual   {}: {actual}",
                        actual.kind().class_name()
                    ));
                } else {
                    parser_errors += 1;
                }
            }
            (Some(expected), None) => match NOT_PARSER_ERRORS.iter().find(|(f, _)| *f == file) {
                Some((_, reason)) => later_errors.push((file.to_owned(), reason)),
                None => failures.push(format!(
                    "{file}: parses, but PHP fails to load it: {}",
                    expected.field("message").as_lossy_string()
                )),
            },
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert_eq!(
        later_errors.len(),
        NOT_PARSER_ERRORS.len(),
        "listed fixtures that are no longer load errors: {later_errors:?}"
    );
    let count = |prefix: &str| {
        later_errors
            .iter()
            .filter(|(_, reason)| reason.starts_with(prefix))
            .count()
    };
    // 56 load-phase errors: 43 from the parser, 11 from the compiler, 1 from the template constructor, 1 needs PCRE
    assert_eq!(parser_errors, 43);
    assert_eq!(count("compilation"), 11);
    assert_eq!(count("template construction"), 1);
    assert_eq!(count("parser: needs the PCRE validator"), 1);
    assert!(
        templates_parsed > 600,
        "only {templates_parsed} templates parsed"
    );
}
