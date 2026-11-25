//! One DATA/EXPECT case through the steps of `IntegrationTestCase::doIntegrationTest()`
//! (TWIG/src/Test/IntegrationTestCase.php, `reference/twig/test-suite.md` §2.1), as far as twig-rs goes today: the
//! load phase through the parser ([`twig::parser::ParseContext::parse_template`]). Every step twig-rs does not have
//! yet ends the case with [`Outcome::NotYetSupported`].

use twig::Source;
use twig::deprecation::Deprecation;

use super::compare;
use super::condition::{self, Verdict};
use super::corpus::{CaseResult, Phase};
use super::environment;
use super::parse::ParsedFixture;

/// How a case ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The harness's assertions hold.
    Pass,
    /// PHPUnit skips the fixture: its `--CONDITION--` is false on PHP 8.5.13.
    Skipped(String),
    /// The case needs a part of twig-rs that does not exist yet.
    NotYetSupported(&'static str),
    /// An assertion of the harness fails (PHPUnit's message).
    Fail(String),
}

impl Outcome {
    /// Whether the trial counts as failed (before the known-failures list is applied).
    pub fn is_failure(&self) -> bool {
        matches!(self, Outcome::NotYetSupported(_) | Outcome::Fail(_))
    }

    /// A short label for the summary.
    pub fn category(&self) -> String {
        match self {
            Outcome::Pass => "passed".to_owned(),
            Outcome::Skipped(_) => "skipped".to_owned(),
            Outcome::NotYetSupported(step) => format!("not yet supported: {step}"),
            Outcome::Fail(_) => "assertion failures".to_owned(),
        }
    }
}

/// One case of a fixture.
pub struct Case<'a> {
    /// The fixture path (`filters/abs.test`).
    pub file: &'a str,
    /// The parsed `.test` file.
    pub fixture: &'a ParsedFixture,
    /// The 0-based index of the DATA/EXPECT section.
    pub index: usize,
    /// PHP's result for this case (from the corpus), if PHP ran it.
    pub php: Option<&'a CaseResult>,
}

/// Runs a case.
pub fn run(case: &Case<'_>) -> Outcome {
    // `if ($condition) { eval('$ret = '.$condition.';'); if (!$ret) $this->markTestSkipped($condition); }`
    match condition::evaluate(&case.fixture.condition) {
        Ok(Verdict::Run) => {}
        Ok(Verdict::Skip(condition)) => return Outcome::Skipped(condition),
        Err(unknown) => return Outcome::Fail(unknown),
    }
    if case.index >= case.fixture.outputs.len() {
        return Outcome::Fail(format!(
            "{} has no DATA/EXPECT section {}",
            case.file, case.index
        ));
    }
    // `$config = array_merge(['cache' => false, 'strict_variables' => true], $match[2] ? eval($match[2].';') : [])`:
    // the evaluated options are in the corpus (`CaseResult::config`); the parser does not use any of them.

    // `$templateSources[$name] = $template.str_repeat(' ', $i)` (cumulative), `new ArrayLoader($templateSources)`,
    // `new Environment($loader, $config)` and the harness extensions
    let templates = case.fixture.case_templates(case.index);
    let context = match environment::parse_context() {
        Ok(context) => context,
        Err(error) => return Outcome::Fail(format!("the harness environment: {error}")),
    };

    // Load phase: `foreach (array_keys($templateSources) as $templateName) $templates[$templateName] =
    // $twig->load($templateName);` with deprecations collected. Twig's load() also loads the templates a template
    // `use`s while constructing it, so an error in such a template surfaces while its user loads; the first error is
    // the same as long as one template fails, which holds for every fixture.
    let mut deprecations: Vec<Deprecation> = Vec::new();
    for template in &templates {
        let source = Source::new(template.source.clone(), template.name_lossy());
        if let Err(error) = context.parse_template(&source, &mut deprecations) {
            let class = error.kind().class_name();
            let message = error.to_string();
            return match &case.fixture.exception {
                Some(expected) => match compare::check_load_exception(expected, class, &message) {
                    Ok(()) => Outcome::Pass,
                    Err(failure) => Outcome::Fail(failure),
                },
                // `throw new Error(sprintf('%s: %s', $e::class, $e->getMessage()), -1, null, $e)`: the test errors
                None => Outcome::Fail(format!(
                    "Twig\\Error\\Error: {}",
                    compare::exception_string(class, &message)
                )),
            };
        }
    }
    // The parser covers lexing, parsing and the parser's own node visitors (not yet the PCRE check of constant
    // `matches` patterns). Twig's load() also runs the escaper, sandbox and optimizer visitors, compiles the module
    // and constructs the template (resolving `use` traits).
    if case.php.is_some_and(|php| php.phase == Phase::Load) {
        return Outcome::NotYetSupported("load-phase exception twig-rs does not raise yet");
    }

    // Render phase: evaluate DATA (the typed tree `CaseResult::data` and the PHP state `CaseResult::state`), render
    // `index.twig`, then compare like the harness: an expected exception with `compare::check_render_exception`, a
    // missing one with `compare::missing_exception`, the output trimmed with `compare::trim_output` against
    // `compare::check_output`, and the deprecations (those collected above, plus the render ones when the fixture
    // has a `--DEPRECATION--` section) with `compare::check_deprecations`.
    Outcome::NotYetSupported("render phase")
}
