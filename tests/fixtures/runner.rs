//! One DATA/EXPECT case through the steps of `IntegrationTestCase::doIntegrationTest()`
//! (TWIG/src/Test/IntegrationTestCase.php, `reference/twig/test-suite.md` §2.1):
//!
//! 1. skip the fixture when its `--CONDITION--` is false on PHP 8.5.13;
//! 2. build the environment of the case ([`environment`]) over its templates (with the cumulative padding);
//! 3. load phase: `$twig->load()` every template in section order; an exception is compared exactly with
//!    `--EXCEPTION--` (plus the final-punctuation rule) or ends the case as a test error;
//! 4. render phase: evaluate DATA (the oracle's typed tree, [`data`]), apply the PHP state, render `index.twig`; an
//!    exception is format-matched with `--EXCEPTION--` or becomes the compared output (`Twig\Error\Error: ...`);
//! 5. compare the output (`trim($output, "\n ")`, strict) and the deprecations (those of the load phase, plus those
//!    of the render phase when the fixture has a `--DEPRECATION--` section).

use std::fmt;

use twig::runtime::{Environment, Notice, TemplateWrapper};
use twig::value::Level;

use super::compare;
use super::condition::{self, Verdict};
use super::data;
use super::environment;
use super::parse::ParsedFixture;
use twig_test_support::Phase;

/// How a case ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The harness's assertions hold.
    Pass,
    /// PHPUnit skips the fixture: its `--CONDITION--` is false on PHP 8.5.13.
    Skipped(String),
    /// An assertion of the harness fails, or the case cannot run.
    Fail(Failure),
}

/// Why a case failed: the step and PHPUnit's message (or the harness's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The step that failed.
    pub kind: FailureKind,
    /// The message.
    pub message: String,
}

/// The step a case failed in, for the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FailureKind {
    /// The harness cannot set the case up (fixture, corpus, options, environment).
    Harness,
    /// Loading raised an exception the fixture does not expect (PHPUnit reports a test error).
    LoadError,
    /// Loading raised another exception than the expected one.
    LoadException,
    /// Loading raised no exception where PHP's load phase throws the expected one.
    LoadMissingException,
    /// The DATA context cannot be rebuilt.
    Data,
    /// Rendering raised another exception than the expected one.
    RenderException,
    /// No exception although the fixture expects one.
    MissingException,
    /// Rendering raised an exception the fixture does not expect (it becomes the compared output).
    RenderError,
    /// The output differs.
    Output,
    /// The deprecations differ.
    Deprecations,
}

impl FailureKind {
    /// A short label for the summary.
    pub fn label(self) -> &'static str {
        match self {
            FailureKind::Harness => "harness setup",
            FailureKind::LoadError => "unexpected exception while loading",
            FailureKind::LoadException => "wrong exception while loading",
            FailureKind::LoadMissingException => "expected exception not thrown while loading",
            FailureKind::Data => "DATA not reproducible",
            FailureKind::RenderException => "wrong exception while rendering",
            FailureKind::MissingException => "expected exception not thrown",
            FailureKind::RenderError => "unexpected exception while rendering",
            FailureKind::Output => "output differs",
            FailureKind::Deprecations => "deprecations differ",
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.kind.label(), self.message)
    }
}

/// How a case ended, with what the harness does not compare but differs from the oracle's run of the case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The verdict of the harness.
    pub outcome: Outcome,
    /// Differences from PHP that `IntegrationTestCase` does not assert (the deprecations raised while rendering a
    /// fixture without `--DEPRECATION--` section, the exact message of an exception matched with a format).
    pub parity: Vec<String>,
}

impl From<Outcome> for Report {
    fn from(outcome: Outcome) -> Self {
        Report {
            outcome,
            parity: Vec::new(),
        }
    }
}

impl Outcome {
    /// Whether the trial counts as failed (before the known-failures list is applied).
    pub fn is_failure(&self) -> bool {
        matches!(self, Outcome::Fail(_))
    }

    /// A short label for the summary.
    pub fn category(&self) -> String {
        match self {
            Outcome::Pass => "passed".to_owned(),
            Outcome::Skipped(_) => "skipped".to_owned(),
            Outcome::Fail(failure) => failure.kind.label().to_owned(),
        }
    }
}

fn fail(kind: FailureKind, message: impl Into<String>) -> Outcome {
    Outcome::Fail(Failure {
        kind,
        message: message.into(),
    })
}

/// One case of a fixture.
pub struct Case<'a> {
    /// The fixture path (`filters/abs.test`).
    pub file: &'a str,
    /// The parsed `.test` file.
    pub fixture: &'a ParsedFixture,
    /// The 0-based index of the DATA/EXPECT section.
    pub index: usize,
    /// PHP's run of this case (from the corpus), if PHP ran it: the evaluated CONFIG and DATA, the PHP state.
    pub php: Option<twig_test_support::Case<'a>>,
}

/// The PHP class of an error (sandbox violations have their own classes, PHP throwables that are not Twig errors
/// theirs).
pub fn error_class(error: &twig::Error) -> &str {
    error.class_name()
}

/// The `E_USER_DEPRECATED` messages among the recorded notices, in order: Twig's `trigger_deprecation()` and
/// user-level deprecations of PHP functions (the harness's handler records nothing else).
fn deprecations(notices: Vec<Notice>) -> Vec<String> {
    notices
        .into_iter()
        .filter_map(|notice| match notice {
            Notice::Deprecation(deprecation) => Some(deprecation.to_string()),
            Notice::Php(diagnostic) if diagnostic.level == Level::UserDeprecated => {
                Some(diagnostic.message.to_string_lossy().into_owned())
            }
            Notice::Php(_) => None,
        })
        .collect()
}

/// Runs a case.
pub fn run(case: &Case<'_>) -> Report {
    let mut parity = Vec::new();
    let outcome = run_steps(case, &mut parity);
    Report { outcome, parity }
}

/// The steps of `doIntegrationTest()` for one case; `parity` receives the differences from the oracle's run that the
/// harness does not assert.
fn run_steps(case: &Case<'_>, parity: &mut Vec<String>) -> Outcome {
    // `if ($condition) { eval('$ret = '.$condition.';'); if (!$ret) $this->markTestSkipped($condition); }`
    match condition::evaluate(&case.fixture.condition) {
        Ok(Verdict::Run) => {}
        Ok(Verdict::Skip(condition)) => return Outcome::Skipped(condition),
        Err(unknown) => return fail(FailureKind::Harness, unknown),
    }
    let Some(section) = case.fixture.outputs.get(case.index) else {
        return fail(
            FailureKind::Harness,
            format!("{} has no DATA/EXPECT section {}", case.file, case.index),
        );
    };
    let Some(php) = &case.php else {
        return fail(
            FailureKind::Harness,
            format!(
                "the oracle corpus has no run of {}#{}",
                case.file, case.index
            ),
        );
    };

    // `$config = array_merge(['cache' => false, 'strict_variables' => true], $match[2] ? eval($match[2].';') :
    // [])`, `$templateSources[$name] = $template.str_repeat(' ', $i)`, `new Environment(new
    // ArrayLoader($templateSources), $config)` and the harness's registrations
    let options = match php.options() {
        Ok(config) => match environment::options(&config) {
            Ok(options) => options,
            Err(error) => return fail(FailureKind::Harness, error),
        },
        Err(error) => return fail(FailureKind::Harness, format!("CONFIG: {error}")),
    };
    let templates = case.fixture.case_templates(case.index);
    let env = match environment::build(&templates, options) {
        Ok(env) => env,
        Err(error) => return fail(FailureKind::Harness, error),
    };

    // Load phase: `foreach (array_keys($templateSources) as $templateName) $templates[$templateName] =
    // $twig->load($templateName);`, deprecations collected
    let mut loaded: Vec<(String, TemplateWrapper)> = Vec::new();
    for template in &templates {
        let name = template.name_lossy();
        match env.load(&name) {
            Ok(wrapper) => loaded.push((name, wrapper)),
            Err(error) => {
                let class = error_class(&error);
                let message = error.to_string();
                return match &case.fixture.exception {
                    Some(expected) => {
                        match compare::check_load_exception(expected, class, &message) {
                            Ok(()) => Outcome::Pass,
                            Err(failure) => fail(FailureKind::LoadException, failure),
                        }
                    }
                    // `throw new Error(sprintf('%s: %s', $e::class, $e->getMessage()), -1, null, $e)`: the test errors
                    None => fail(
                        FailureKind::LoadError,
                        format!(
                            "Twig\\Error\\Error: {}",
                            compare::exception_string(class, &message)
                        ),
                    ),
                };
            }
        }
    }
    let load_deprecations = deprecations(env.take_notices());
    if php.result.phase == Phase::Load {
        let thrown = php
            .result
            .error
            .as_ref()
            .map_or_else(|| "an exception".to_owned(), |e| e.formatted.clone());
        return fail(
            FailureKind::LoadMissingException,
            format!("PHP's load phase throws {thrown}; twig-rs loads every template"),
        );
    }

    // Render phase: `$template->render(eval($match[1].';'))`; DATA runs inside the render `try`
    let rendered = render(case, php, &env, &loaded);
    let render_deprecations = deprecations(env.take_notices());
    if rendered.is_ok()
        && let Some(php_deprecations) = &php.result.render_deprecations
        && *php_deprecations != render_deprecations
    {
        parity.push(format!(
            "render deprecations {render_deprecations:?}, PHP {php_deprecations:?}"
        ));
    }
    let (output, notes) = match rendered {
        Err(failure) => return Outcome::Fail(failure),
        Ok((Ok(output), notes)) => (compare::trim_output(&output).to_vec(), notes),
        Ok((Err(error), notes)) => {
            let class = error_class(&error);
            let message = error.to_string();
            let actual = compare::exception_string(class, &message);
            if let Some(php_error) = &php.result.error
                && php_error.formatted_bytes().as_bytes() != actual.as_bytes()
            {
                parity.push(format!(
                    "render exception {actual:?}, PHP {:?}",
                    php_error.formatted
                ));
            }
            if let Some(expected) = &case.fixture.exception {
                return match compare::check_render_exception(expected, class, &message) {
                    Ok(()) => Outcome::Pass,
                    Err(failure) => fail(FailureKind::RenderException, annotate(failure, &notes)),
                };
            }
            // `$e = new Error(sprintf('%s: %s', $e::class, $e->getMessage()), -1, null, $e); $output =
            // trim(sprintf('%s: %s', $e::class, $e->getMessage()));`
            let output = format!(
                "Twig\\Error\\Error: {}",
                compare::exception_string(class, &message)
            );
            return match compare::check_output(
                &section.expect,
                compare::php_trim(output.as_bytes()),
                &String::from_utf8_lossy(&case.fixture.message),
                case.file,
            ) {
                Ok(()) => Outcome::Pass,
                Err(failure) => fail(
                    FailureKind::RenderError,
                    annotate(failure, &notes) + &compiled(case.index, &loaded),
                ),
            };
        }
    };
    if let Some(expected) = &case.fixture.exception {
        return fail(
            FailureKind::MissingException,
            annotate(compare::missing_exception(expected), &notes),
        );
    }
    if let Err(failure) = compare::check_output(
        &section.expect,
        &output,
        &String::from_utf8_lossy(&case.fixture.message),
        case.file,
    ) {
        return fail(
            FailureKind::Output,
            annotate(failure, &notes) + &compiled(case.index, &loaded),
        );
    }
    // `assertSame($deprecation, implode("\n", $deprecations))`; render deprecations are collected only when the
    // fixture has a `--DEPRECATION--` section
    let mut compared = load_deprecations;
    if !case.fixture.deprecation.is_empty() {
        compared.extend(render_deprecations);
    }
    if let Err(failure) = compare::check_deprecations(&case.fixture.deprecation, &compared) {
        return fail(FailureKind::Deprecations, failure);
    }
    Outcome::Pass
}

/// What [`render`] produced: the output or the exception of `render()`, and the parts of the PHP state twig-rs could
/// not apply.
type Rendered = (Result<Vec<u8>, twig::Error>, Vec<String>);

/// The render phase: the DATA context, the PHP state, `render()` of `index.twig`. The error is a case that cannot
/// run.
fn render(
    case: &Case<'_>,
    php: &twig_test_support::Case<'_>,
    env: &Environment,
    loaded: &[(String, TemplateWrapper)],
) -> Result<Rendered, Failure> {
    let failure = |kind: FailureKind, message: String| Failure { kind, message };
    let context = data::context(php, env).map_err(|e| failure(FailureKind::Data, e))?;
    let state = php.result.state.as_ref().ok_or_else(|| {
        failure(
            FailureKind::Data,
            format!(
                "the oracle recorded no PHP state for {}#{}",
                case.file, case.index
            ),
        )
    })?;
    let notes = data::apply_state(env, state).map_err(|e| failure(FailureKind::Data, e))?;
    // `$templates['index.twig']`
    let Some((_, index)) = loaded.iter().find(|(name, _)| name == "index.twig") else {
        return Err(failure(
            FailureKind::Harness,
            "the fixture has no index.twig template".to_owned(),
        ));
    };
    Ok((index.render_bytes(context), notes))
}

/// With `TWIG_FIXTURES_DISASSEMBLE=1`, the compiled programs of the templates, as `doIntegrationTest()` prints the
/// compiled PHP of every template when the output differs.
fn compiled(index: usize, loaded: &[(String, TemplateWrapper)]) -> String {
    if !std::env::var_os("TWIG_FIXTURES_DISASSEMBLE").is_some_and(|v| v == "1") {
        return String::new();
    }
    let mut out = format!("\nCompiled templates that failed on case {}:", index + 1);
    for (name, wrapper) in loaded {
        out.push_str(&format!(
            "\nTemplate: {name}\n{}",
            wrapper.template().program().disassemble()
        ));
    }
    out
}

/// A failure message with the PHP state the case needs and twig-rs could not apply.
fn annotate(message: String, notes: &[String]) -> String {
    if notes.is_empty() {
        message
    } else {
        format!(
            "{message}\n(the DATA code calls {}, which twig-rs cannot apply yet)",
            notes.join(", ")
        )
    }
}
