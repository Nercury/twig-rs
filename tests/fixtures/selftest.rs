//! The harness's own checks, run as trials named `harness::...` before the fixture trials. The known-failures list
//! does not apply to them: they guard the parts that decide whether a fixture trial passes.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use libtest_mimic::{Failed, Trial};
use serde::Deserialize;
use twig_test_support::json::decode_base64;
use twig_test_support::{
    Autoescape, CacheOption, Case, EnvironmentOptions, ErrorInfo, Fixture as CorpusFixture, Phase,
    TypedNode,
};

use super::Suite;
use super::compare::{self, string_matches_format};
use super::condition::{self, Verdict};
use super::environment;
use super::extension_vectors;
use super::parse::{self, ParsedFixture};

/// A self-test.
type Check = fn(&Suite) -> Result<(), String>;

/// The self-test trials.
pub fn trials(suite: &Arc<Suite>) -> Vec<Trial> {
    let checks: [(&str, Check); 9] = [
        (
            "harness::fixture_files_match_the_corpus",
            fixture_files_match_the_corpus,
        ),
        (
            "harness::fixture_parser_matches_upstream",
            fixture_parser_matches_upstream,
        ),
        ("harness::fixture_parser_rules", fixture_parser_rules),
        (
            "harness::conditions_match_the_corpus",
            conditions_match_the_corpus,
        ),
        ("harness::corpus_is_consistent", corpus_is_consistent),
        (
            "harness::format_matching_matches_phpunit",
            format_matching_matches_phpunit,
        ),
        (
            "harness::comparisons_follow_the_harness",
            comparisons_follow_the_harness,
        ),
        (
            "harness::test_extension_matches_the_oracle",
            extension_vectors::check,
        ),
        (
            "harness::known_failures_list_is_valid",
            known_failures_list_is_valid,
        ),
    ];
    checks
        .into_iter()
        .map(|(name, check)| {
            let suite = Arc::clone(suite);
            Trial::test(name, move || check(&suite).map_err(Failed::from))
        })
        .collect()
}

/// Collects failures and reports the first few.
#[derive(Default)]
struct Failures(Vec<String>);

impl Failures {
    fn push(&mut self, failure: String) {
        self.0.push(failure);
    }

    fn check(&mut self, ok: bool, failure: impl FnOnce() -> String) {
        if !ok {
            self.0.push(failure());
        }
    }

    fn finish(self, checked: &str) -> Result<(), String> {
        if self.0.is_empty() {
            return Ok(());
        }
        Err(format!(
            "{} failure(s) in {checked}:\n\n{}",
            self.0.len(),
            self.0[..self.0.len().min(30)].join("\n\n")
        ))
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The `.test` files on disk are the corpus's files, and the Rust parse of each equals PHP's parse.
fn fixture_files_match_the_corpus(suite: &Suite) -> Result<(), String> {
    let corpus = suite.corpus()?;
    let discovered: Vec<&str> = suite
        .fixtures
        .iter()
        .map(|f| f.file.name.as_str())
        .collect();
    let exported: Vec<&str> = corpus.fixtures().iter().map(|f| f.file.as_str()).collect();
    if discovered != exported {
        let on_disk: BTreeSet<&str> = discovered.iter().copied().collect();
        let in_corpus: BTreeSet<&str> = exported.iter().copied().collect();
        return Err(format!(
            "the fixture files differ from the corpus\nonly on disk: {:?}\nonly in the corpus: {:?}",
            on_disk.difference(&in_corpus).collect::<Vec<_>>(),
            in_corpus.difference(&on_disk).collect::<Vec<_>>()
        ));
    }
    // reference/twig/test-suite.md §0
    let legacy = suite.fixtures.iter().filter(|f| f.file.legacy).count();
    if (discovered.len(), legacy) != (488, 33) {
        return Err(format!(
            "expected 488 fixtures, 33 of them legacy; found {} and {legacy}",
            discovered.len()
        ));
    }
    let mut failures = Failures::default();
    for (entry, php) in suite.fixtures.iter().zip(corpus.fixtures()) {
        match &entry.parsed {
            Ok(parsed) => {
                if let Err(difference) = same_parse(parsed, php) {
                    failures.push(format!("{}: {difference}", php.file));
                }
            }
            Err(error) => failures.push(format!("{}: {error}", php.file)),
        }
        failures.check(entry.file.legacy == php.legacy, || {
            format!("{}: legacy flag differs", php.file)
        });
    }
    failures.finish("the fixture parse")
}

/// Compares a Rust parse with PHP's parse from the corpus.
fn same_parse(parsed: &ParsedFixture, php: &CorpusFixture) -> Result<(), String> {
    let field = |name: &str, rust: &[u8], php: &str| {
        if rust == php.as_bytes() {
            Ok(())
        } else {
            Err(format!("{name}: Rust {:?}, PHP {php:?}", lossy(rust)))
        }
    };
    field("message", &parsed.message, &php.message)?;
    field("condition", &parsed.condition, &php.condition)?;
    field(
        "deprecation",
        &parsed.deprecation,
        &php.deprecation_expected,
    )?;
    let templates: Vec<(String, String)> = parsed
        .templates
        .iter()
        .map(|t| (lossy(&t.name), lossy(&t.source)))
        .collect();
    if templates != php.templates {
        return Err(format!(
            "templates: Rust {templates:?}, PHP {:?}",
            php.templates
        ));
    }
    if parsed.exception.as_deref().map(lossy) != php.exception_expected {
        return Err(format!(
            "exception: Rust {:?}, PHP {:?}",
            parsed.exception.as_deref().map(lossy),
            php.exception_expected
        ));
    }
    let outputs: Vec<(String, Option<String>, String)> = parsed
        .outputs
        .iter()
        .map(|o| {
            (
                lossy(&o.data),
                o.config.as_deref().map(lossy),
                lossy(&o.expect),
            )
        })
        .collect();
    let php_outputs: Vec<(String, Option<String>, String)> = php
        .outputs
        .iter()
        .map(|o| {
            (
                o.data_php.clone().unwrap_or_default(),
                o.config_php.clone(),
                o.expect_raw.clone(),
            )
        })
        .collect();
    if outputs != php_outputs {
        return Err(format!("outputs: Rust {outputs:?}, PHP {php_outputs:?}"));
    }
    Ok(())
}

/// `tests/fixtures/vectors/fixture_parser.json`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserVectors {
    generator: String,
    twig: String,
    php: String,
    cases: Vec<ParserVector>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserVector {
    id: String,
    text: String,
    result: Option<VectorParse>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorParse {
    message: String,
    condition: String,
    deprecation: String,
    templates: Vec<(String, String)>,
    #[serde(deserialize_with = "false_or_string")]
    exception: Option<String>,
    outputs: Vec<(String, Option<String>, String)>,
}

fn false_or_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FalseOrString {
        Text(String),
        Flag(bool),
    }
    match FalseOrString::deserialize(d)? {
        FalseOrString::Text(text) => Ok(Some(text)),
        FalseOrString::Flag(false) => Ok(None),
        FalseOrString::Flag(true) => Err(serde::de::Error::custom("expected false or a string")),
    }
}

/// The parser against the real `assembleTests()` on edge cases and random inputs.
fn fixture_parser_matches_upstream(suite: &Suite) -> Result<(), String> {
    let path = suite
        .root
        .join("tests/fixtures/vectors/fixture_parser.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let vectors: ParserVectors =
        serde_json::from_str(&text).map_err(|e| format!("invalid {}: {e}", path.display()))?;
    if (vectors.twig.as_str(), vectors.php.as_str()) != ("3.31.1", "8.5.13")
        || !vectors.generator.ends_with("fixture_parser.gen.php")
        || vectors.cases.len() < 1900
    {
        return Err(format!(
            "unexpected vector file: {} {} {} with {} cases",
            vectors.generator,
            vectors.twig,
            vectors.php,
            vectors.cases.len()
        ));
    }
    let mut failures = Failures::default();
    let mut ids = HashSet::new();
    for case in &vectors.cases {
        failures.check(ids.insert(case.id.as_str()), || {
            format!("duplicate id {}", case.id)
        });
        let rust = parse::parse_fixture(case.text.as_bytes()).map(|p| VectorParse {
            message: lossy(&p.message),
            condition: lossy(&p.condition),
            deprecation: lossy(&p.deprecation),
            templates: p
                .templates
                .iter()
                .map(|t| (lossy(&t.name), lossy(&t.source)))
                .collect(),
            exception: p.exception.as_deref().map(lossy),
            outputs: p
                .outputs
                .iter()
                .map(|o| {
                    (
                        lossy(&o.data),
                        o.config.as_deref().map(lossy),
                        lossy(&o.expect),
                    )
                })
                .collect(),
        });
        failures.check(rust == case.result, || {
            format!(
                "{} {:?}\n  PHP  {:?}\n  Rust {rust:?}",
                case.id, case.text, case.result
            )
        });
    }
    failures.finish("the fixture-parser vectors")
}

/// The template line numbering, the cumulative trailing spaces of later DATA/EXPECT pairs and the "not valid" case.
fn fixture_parser_rules(suite: &Suite) -> Result<(), String> {
    let mut failures = Failures::default();

    // The newline after `--TEMPLATE--` belongs to the template, so its first visible line is line 2.
    let text = b"--TEST--\nline numbers\n--TEMPLATE--\n{{ 1 }}\n{% endfor %}\n--DATA--\nreturn []\n--EXPECT--\n";
    let parsed = parse::parse_fixture(text).ok_or("the line-number fixture is not valid")?;
    failures.check(
        parsed.templates.len() == 1 && parsed.templates[0].source == b"\n{{ 1 }}\n{% endfor %}",
        || format!("templates {:?}", parsed.templates),
    );
    let options = environment::options(&EnvironmentOptions::harness_default())?;
    let env = environment::build(&parsed.templates, options)?;
    match env.load("index.twig") {
        Err(error) => failures.check(error.template_line() == 3, || {
            format!("{{% endfor %}} on the second visible line reported as {error}")
        }),
        Ok(_) => failures.push("{% endfor %} without {% for %} loaded".to_owned()),
    }

    // Case i gets i spaces appended to every template of the already padded list: 0, 1, 3, 6, ... in total.
    let text = b"--TEST--\npairs\n--TEMPLATE--\nA\n--TEMPLATE(b)--\nB\n--DATA--\n1\n--EXPECT--\n--DATA--\n2\n--EXPECT--\n--DATA--\n3\n--EXPECT--\n--DATA--\n4\n--EXPECT--\n";
    let parsed = parse::parse_fixture(text).ok_or("the multi-pair fixture is not valid")?;
    for (case, spaces) in [(0, 0), (1, 1), (2, 3), (3, 6)] {
        let sources: Vec<Vec<u8>> = parsed
            .case_templates(case)
            .into_iter()
            .map(|t| t.source)
            .collect();
        // the last template loses one final newline (`$` matches before it)
        let pad = " ".repeat(spaces);
        let expected = vec![
            format!("\nA\n{pad}").into_bytes(),
            format!("\nB{pad}").into_bytes(),
        ];
        failures.check(sources == expected, || {
            format!(
                "case {case}: {:?}",
                sources.iter().map(|s| lossy(s)).collect::<Vec<_>>()
            )
        });
    }

    // upstream throws 'Test "<name>" is not valid.' for these
    for text in [
        &b""[..],
        b"--TEST--\nx\n",
        b"--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\n",
    ] {
        failures.check(parse::parse_fixture(text).is_none(), || {
            format!("{:?} parsed", lossy(text))
        });
    }

    // every fixture file is valid
    for entry in &suite.fixtures {
        if let Err(error) = &entry.parsed {
            failures.push(format!("{}: {error}", entry.file.name));
        }
    }
    failures.finish("the fixture-parser rules")
}

/// The static verdict of every `--CONDITION--` agrees with the oracle run.
fn conditions_match_the_corpus(suite: &Suite) -> Result<(), String> {
    let corpus = suite.corpus()?;
    let mut failures = Failures::default();
    let mut with_condition = 0;
    let mut skipped = 0;
    for fixture in corpus.fixtures() {
        if !fixture.condition.is_empty() {
            with_condition += 1;
        }
        match condition::evaluate(fixture.condition.as_bytes()) {
            Ok(Verdict::Run) => failures.check(fixture.actual.skipped.is_none(), || {
                format!("{}: runs here, skipped by PHP", fixture.file)
            }),
            Ok(Verdict::Skip(reason)) => {
                skipped += 1;
                // the oracle records `'condition: '.trim($condition)`; PHPUnit skips with the raw condition
                let expected = format!(
                    "condition: {}",
                    lossy(compare::php_trim(fixture.condition.as_bytes()))
                );
                failures.check(
                    fixture.actual.skipped.as_deref() == Some(expected.as_str()),
                    || {
                        format!(
                            "{}: skipped here, PHP: {:?}",
                            fixture.file, fixture.actual.skipped
                        )
                    },
                );
                failures.check(reason == fixture.condition, || {
                    format!("{}: skip message {reason:?}", fixture.file)
                });
            }
            Err(error) => failures.push(format!("{}: {error}", fixture.file)),
        }
    }
    failures.check((with_condition, skipped) == (15, 2), || {
        format!("{with_condition} fixtures with a condition, {skipped} skipped; expected 15 and 2")
    });
    failures.check(condition::evaluate(b"0") == Ok(Verdict::Run), || {
        "the condition \"0\" is falsy, so no condition".to_owned()
    });
    failures.check(condition::evaluate(b"PHP_OS === 'Linux'").is_err(), || {
        "an unknown condition is accepted".to_owned()
    });
    failures.finish("the conditions")
}

/// The corpus has the shape the harness relies on: versions, per-fixture metadata that agrees with the parse, one
/// result per case that ran, results that fit their phase, well-formed typed trees and understood options.
fn corpus_is_consistent(suite: &Suite) -> Result<(), String> {
    let corpus = suite.corpus()?;
    let mut failures = Failures::default();
    let file = corpus.file();
    failures.check(
        file.generated_by == "php-oracle/tools/export-corpus.php"
            && file.twig_version == "3.31.1"
            && file.php_version == "8.5.13",
        || {
            format!(
                "corpus of {} for Twig {} on PHP {}",
                file.generated_by, file.twig_version, file.php_version
            )
        },
    );
    failures.check(
        corpus.fixtures().windows(2).all(|w| w[0].file < w[1].file),
        || "the fixtures are not sorted by path".to_owned(),
    );
    let mut phases: BTreeMap<&str, usize> = BTreeMap::new();
    let mut options = Vec::new();
    for fixture in corpus.fixtures() {
        let file = fixture.file.as_str();
        let run = &fixture.actual;
        failures.check(
            run.id.as_ref().is_none_or(serde_json::Value::is_null)
                && run.kind == "fixture"
                && run.file == fixture.file
                && run.message == fixture.message
                && run.legacy == fixture.legacy
                && fixture.legacy == file.contains(".legacy.test")
                && run.exception_expected == fixture.exception_expected
                && run.deprecation_expected == fixture.deprecation_expected,
            || format!("{file}: the run's metadata differs from the fixture's"),
        );
        let known_options = [
            "debug",
            "charset",
            "strict_variables",
            "autoescape",
            "cache",
            "auto_reload",
            "optimizations",
            "use_yield",
        ];
        failures.check(
            fixture
                .flags
                .config_keys
                .iter()
                .all(|k| known_options.contains(&k.as_str())),
            || {
                format!(
                    "{file}: unknown CONFIG keys {:?}",
                    fixture.flags.config_keys
                )
            },
        );
        failures.check(
            fixture
                .outputs
                .iter()
                .all(|o| o.config_php.is_none() == fixture.exception_expected.is_some()),
            || format!("{file}: CONFIG is null exactly in exception fixtures"),
        );
        if run.skipped.is_some() {
            failures.check(run.cases.is_empty(), || {
                format!("{file}: a skipped fixture has cases")
            });
            continue;
        }
        let count = run.cases.len();
        failures.check(
            count > 0
                && count <= fixture.outputs.len()
                && run.cases.iter().enumerate().all(|(i, c)| c.index == i)
                && (count == fixture.outputs.len()
                    || run.cases.last().is_some_and(|c| c.phase == Phase::Load)),
            || {
                format!(
                    "{file}: {count} cases for {} sections",
                    fixture.outputs.len()
                )
            },
        );
        for case in &run.cases {
            *phases
                .entry(match case.phase {
                    Phase::Load => "load",
                    Phase::Render => "render",
                })
                .or_default() += 1;
            let checked = corpus
                .case(file, case.index)
                .ok_or_else(|| "the corpus does not find the case".to_owned())
                .and_then(check_case);
            match checked {
                Ok(case_options) => options.push(case_options),
                Err(problem) => failures.push(format!("{file}#{}: {problem}", case.index)),
            }
        }
    }
    let cases: usize = phases.values().sum();
    failures.check(
        cases == 500 && phases.get("load") == Some(&56) && phases.get("render") == Some(&444),
        || format!("cases by phase {phases:?}; expected 56 load and 444 render"),
    );
    let census = option_census(&options);
    let expected: BTreeMap<String, usize> = [
        ("autoescape=false", 3),
        ("autoescape=html", 494),
        ("autoescape=js", 1),
        ("autoescape=name (file extension)", 2),
        ("debug=false auto_reload=false", 497),
        ("debug=true auto_reload=true", 3),
        ("strict_variables=false", 8),
        ("strict_variables=true", 492),
        ("use_yield=false", 499),
        ("use_yield=true", 1),
        ("cache=false charset=UTF-8 optimizations=-1 unknown=0", 500),
    ]
    .into_iter()
    .map(|(key, count)| (key.to_owned(), count))
    .collect();
    failures.check(census == expected, || {
        format!("options of the cases {census:#?}, expected {expected:#?}")
    });
    failures.finish("the corpus")
}

/// Checks one case and returns its options.
fn check_case(corpus_case: Case<'_>) -> Result<EnvironmentOptions, String> {
    let fixture = corpus_case.fixture;
    let case = corpus_case.result;
    if !case.pass {
        return Err("PHP fails the case".to_owned());
    }
    let options = corpus_case.options().map_err(|e| e.to_string())?;
    match case.phase {
        Phase::Load => {
            if case.error.is_none()
                || case.output.is_some()
                || case.data.is_some()
                || case.state.is_some()
                || case.expected.is_some()
                || case.render_deprecations.is_some()
            {
                return Err("a load-phase case with render-phase fields".to_owned());
            }
        }
        Phase::Render => {
            let (Some(data), Some(state), Some(_)) =
                (&case.data, &case.state, &case.render_deprecations)
            else {
                return Err(
                    "a render-phase case without data, state or render deprecations".to_owned(),
                );
            };
            if !matches!(data, TypedNode::Array(_)) {
                return Err(format!("the context is not an array: {}", data.tag()));
            }
            if state.php_default_timezone.is_empty()
                || state.core_timezone.is_empty()
                || state.core_date_format.0.is_empty()
                || state.core_number_format.1.is_empty()
                || state.core_number_format.0 < 0
            {
                return Err(format!("unexpected state {state:?}"));
            }
        }
    }
    match (&fixture.exception_expected, &case.error) {
        (Some(_), Some(error)) => {
            check_error(error)?;
            if case.output.is_some() || case.expected.is_some() {
                return Err("an expected exception with an output".to_owned());
            }
        }
        (Some(_), None) => return Err("no exception".to_owned()),
        (None, error) => {
            if let Some(error) = error {
                check_error(error)?;
            }
            let (Some(output), Some(expected)) = (&case.output, &case.expected) else {
                return Err("an output fixture without output".to_owned());
            };
            let output = bytes(output, case.output_b64.as_deref())?;
            let expected = bytes(expected, case.expected_b64.as_deref())?;
            if output != expected
                || expected
                    != compare::trim_output(fixture.outputs[case.index].expect_raw.as_bytes())
            {
                return Err("the output, the expectation and --EXPECT-- disagree".to_owned());
            }
            let wanted = &fixture.deprecation_expected;
            if case.deprecations.join("\n") != *wanted {
                return Err(format!("deprecations {:?}", case.deprecations));
            }
        }
    }
    Ok(options)
}

/// The option values of the cases that ran, by count, as `reference/twig/test-suite.md` §4.4 lists the CONFIG
/// sections (merged over the harness's `cache => false, strict_variables => true` and Twig's defaults).
fn option_census(options: &[EnvironmentOptions]) -> BTreeMap<String, usize> {
    let mut census = BTreeMap::new();
    for o in options {
        let mut count = |key: String| *census.entry(key).or_default() += 1;
        count(match &o.autoescape {
            Autoescape::Disabled => "autoescape=false".to_owned(),
            Autoescape::Strategy(name) => format!("autoescape={name}"),
            Autoescape::FileExtension => "autoescape=name (file extension)".to_owned(),
            Autoescape::Callable(_) => "autoescape=callable".to_owned(),
        });
        count(format!("debug={} auto_reload={}", o.debug, o.auto_reload));
        count(format!("strict_variables={}", o.strict_variables));
        count(format!("use_yield={}", o.use_yield));
        let cache = match &o.cache {
            CacheOption::Disabled => "false".to_owned(),
            other => format!("{other:?}"),
        };
        count(format!(
            "cache={cache} charset={} optimizations={} unknown={}",
            o.charset,
            o.optimizations,
            o.unknown.len()
        ));
    }
    census
}

/// The bytes of a string exported with `put_string()`.
fn bytes(text: &str, b64: Option<&str>) -> Result<Vec<u8>, String> {
    match b64 {
        None => Ok(text.as_bytes().to_vec()),
        Some(b64) => {
            let raw = decode_base64(b64)?;
            if std::str::from_utf8(&raw).is_ok() {
                return Err(format!("{text:?} has a base64 copy but is valid UTF-8"));
            }
            Ok(raw)
        }
    }
}

fn check_error(error: &ErrorInfo) -> Result<(), String> {
    let message = bytes(&error.message, error.message_b64.as_deref())?;
    let formatted = bytes(&error.formatted, error.formatted_b64.as_deref())?;
    if formatted != [error.class.as_bytes(), b": ", &message].concat() {
        return Err(format!(
            "formatted {:?} is not class: message",
            error.formatted
        ));
    }
    let twig = error.class.starts_with("Twig\\");
    if twig != error.raw_message.is_some() || twig == error.code.is_some() {
        return Err(format!("{}: unexpected fields", error.class));
    }
    if let Some(raw) = &error.raw_message {
        bytes(raw, error.raw_message_b64.as_deref())?;
    }
    if error.template_line.is_some() != twig
        || error.template_name.as_ref().is_some_and(|n| n.is_empty())
        || error.template_column.is_some_and(|c| c < 1)
    {
        return Err(format!("{}: unexpected template info", error.class));
    }
    match &error.previous {
        Some(previous) => check_error(previous),
        None => Ok(()),
    }
}

/// `tests/fixtures/vectors/format_matching.json`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormatVectors {
    generator: String,
    phpunit: String,
    cases: Vec<FormatVector>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormatVector {
    format: String,
    #[serde(default)]
    format_b64: Option<String>,
    string: String,
    #[serde(default)]
    string_b64: Option<String>,
    matches: bool,
}

/// [`string_matches_format`] against PHPUnit's verdicts.
fn format_matching_matches_phpunit(suite: &Suite) -> Result<(), String> {
    let path = suite
        .root
        .join("tests/fixtures/vectors/format_matching.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let vectors: FormatVectors =
        serde_json::from_str(&text).map_err(|e| format!("invalid {}: {e}", path.display()))?;
    if !vectors.generator.ends_with("format_matching.gen.php")
        || !vectors.phpunit.starts_with("9.6.")
        || vectors.cases.len() < 80
    {
        return Err(format!(
            "unexpected vector file: {} for PHPUnit {} with {} cases",
            vectors.generator,
            vectors.phpunit,
            vectors.cases.len()
        ));
    }
    let mut failures = Failures::default();
    for case in &vectors.cases {
        let format = bytes(&case.format, case.format_b64.as_deref())?;
        let string = bytes(&case.string, case.string_b64.as_deref())?;
        let rust = string_matches_format(&format, &string);
        failures.check(rust == case.matches, || {
            format!(
                "format {:?}, string {:?}: PHPUnit {}, Rust {rust}",
                lossy(&format),
                lossy(&string),
                case.matches
            )
        });
    }
    failures.finish("the format-matching vectors")
}

/// The assertions of `doIntegrationTest()`.
fn comparisons_follow_the_harness(_suite: &Suite) -> Result<(), String> {
    let mut failures = Failures::default();
    let syntax = "Twig\\Error\\SyntaxError";
    // load phase: trim() on both sides (" \t\n\r\0\x0B", not "\x0C"), exact comparison, final "." or "?"
    let cases: [(&[u8], &str, bool); 7] = [
        (b"Twig\\Error\\SyntaxError: Oops.\n", "Oops.", true),
        (
            b" \t\x0B\0Twig\\Error\\SyntaxError: Oops.\r\n",
            "Oops.",
            true,
        ),
        (b"Twig\\Error\\SyntaxError: Oops.\x0C", "Oops.", false),
        (b"Twig\\Error\\SyntaxError: Oops.", "Oops. ", false),
        (b"Twig\\Error\\SyntaxError: Really?", "Really?", true),
        (b"Twig\\Error\\SyntaxError: No dot", "No dot", false),
        (b"Twig\\Error\\SyntaxError: Oops%s", "Oops.", false),
    ];
    for (expected, message, ok) in cases {
        failures.check(
            compare::check_load_exception(expected, syntax, message).is_ok() == ok,
            || format!("load exception {:?} vs {message:?}", lossy(expected)),
        );
    }
    // render phase: the format comparison
    failures.check(
        compare::check_render_exception(
            b"Twig\\Error\\RuntimeError: Oops in %s at line %d\n",
            "Twig\\Error\\RuntimeError",
            "Oops in \"index.twig\" at line 2",
        )
        .is_ok(),
        || "render exception with placeholders".to_owned(),
    );
    failures.check(
        compare::missing_exception(b"Twig\\Error\\RuntimeError: Oops: really.\n")
            == "Failed asserting that exception of type \"Twig\\Error\\RuntimeError\" is thrown.",
        || "missing exception message".to_owned(),
    );
    // output: trim($s, "\n ") on the expectation, strict comparison
    failures.check(
        compare::check_output(b"\n  a b \n\n", b"a b", "m", "f.test").is_ok(),
        || "output trimming".to_owned(),
    );
    failures.check(
        compare::check_output(b"\na\t\n", b"a", "m", "f.test").is_err(),
        || "a tab is not trimmed".to_owned(),
    );
    failures.check(
        compare::check_output(b"\na\r\n", b"a", "m", "f.test").is_err(),
        || "a carriage return is not trimmed".to_owned(),
    );
    failures.check(compare::trim_output(b" \n a \n ") == b"a", || {
        "trim_output".to_owned()
    });
    // deprecations: joined with "\n", compared exactly
    failures.check(
        compare::check_deprecations(b"one\ntwo", &["one".to_owned(), "two".to_owned()]).is_ok()
            && compare::check_deprecations(b"", &[]).is_ok()
            && compare::check_deprecations(b"one", &["one".to_owned(), "two".to_owned()]).is_err(),
        || "deprecation comparison".to_owned(),
    );
    failures.finish("the comparisons")
}

/// `tests/twig/known-failures.txt` is sorted and lists only fixture trials that run.
fn known_failures_list_is_valid(suite: &Suite) -> Result<(), String> {
    if suite.update {
        // the run rewrites the list
        return Ok(());
    }
    let known = suite.known_failures()?;
    known.validate(&suite.runnable)
}
