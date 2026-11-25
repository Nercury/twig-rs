//! The oracle corpus (`tests/twig/oracle-corpus.json`) and this crate's supplement
//! (`tests/vectors/fixture_data.json`).
//!
//! The corpus is written by `research/php-oracle/tools/export-corpus.php`: for every `.test` file
//! of Twig 3.31.1's `tests/Fixtures` (sorted by path) PHP's parse of the file (sections and
//! classification flags) and the **actual** PHP results of every DATA/EXPECT case, produced by a
//! faithful copy of `IntegrationTestCase::doIntegrationTest()` (`TwigOracle\run_fixture()` in
//! `research/php-oracle/lib.php`, one PHP process per fixture). Cases that reach the render phase
//! carry the typed tree of the evaluated `--DATA--` context and the PHP state right before
//! rendering.
//!
//! The supplement holds richer trees of the same DATA contexts (see [`crate::typed`]); a case uses
//! its supplement tree when it agrees with the corpus tree ([`SupplementStatus`]).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use twig_value::PhpString;

use crate::compare::{CompareOptions, compare_trees};
use crate::json::{ErrorInfo, false_or_string, field_bytes, ordered_map};
use crate::typed::TypedNode;

/// The whole corpus file.
#[derive(Clone, Debug, Deserialize)]
pub struct CorpusFile {
    /// `php-oracle/tools/export-corpus.php`.
    pub generated_by: String,
    /// `3.31.1`.
    pub twig_version: String,
    /// `8.5.13`.
    pub php_version: String,
    /// One entry per `.test` file, sorted by path.
    pub fixtures: Vec<Fixture>,
}

/// One `.test` file: PHP's parse (`IntegrationTestCase::assembleTests()` regexes) and the oracle run.
#[derive(Clone, Debug, Deserialize)]
pub struct Fixture {
    /// Path relative to `tests/Fixtures` (`filters/abs.test`).
    pub file: String,
    /// Whether the file name contains `.legacy.test`.
    pub legacy: bool,
    /// The `--TEST--` description.
    pub message: String,
    /// The `--CONDITION--` PHP expression (`''` when absent).
    pub condition: String,
    /// The `--DEPRECATION--` section (`''` when absent).
    pub deprecation_expected: String,
    /// The `--EXCEPTION--` section; `None` for EXPECT fixtures.
    #[serde(deserialize_with = "false_or_string")]
    pub exception_expected: Option<String>,
    /// Template name → source, in section order (`index.twig` for an unnamed `--TEMPLATE--`).
    #[serde(deserialize_with = "ordered_map")]
    pub templates: Vec<(String, String)>,
    /// The DATA/CONFIG/EXPECT sections (one entry with an empty expectation for EXCEPTION
    /// fixtures).
    pub outputs: Vec<OutputSection>,
    /// Heuristic classification (regexes over the sources, see `export-corpus.php`).
    pub flags: Flags,
    /// The oracle run.
    pub actual: FixtureRun,
}

/// One DATA/CONFIG/EXPECT section of a fixture.
#[derive(Clone, Debug, Deserialize)]
pub struct OutputSection {
    /// The `--DATA--` PHP code.
    #[serde(default)]
    pub data_php: Option<String>,
    /// The `--CONFIG--` PHP code.
    #[serde(default)]
    pub config_php: Option<String>,
    /// The raw `--EXPECT--` text (untrimmed).
    pub expect_raw: String,
}

/// Classification flags of `export-corpus.php` (regex heuristics, not PHP facts).
#[derive(Clone, Debug, Deserialize)]
pub struct Flags {
    /// Callables of the test harness the templates use (`filter:nl2br`, `test:multi word`, ...).
    pub custom_extension: Vec<String>,
    /// The templates call `dump()`.
    pub debug_extension: bool,
    /// The templates call `template_from_string()`.
    pub string_loader_extension: bool,
    /// The templates use the sandbox.
    pub sandbox: bool,
    /// The templates use the `global` global, `_context` or `dump()` without arguments.
    pub harness_global: bool,
    /// The templates use `constant()`.
    pub php_constants: bool,
    /// The templates use `enum()` / `enum_cases()`.
    #[serde(rename = "enum")]
    pub enum_functions: bool,
    /// Dates in DATA or templates.
    pub dates: bool,
    /// The DATA code uses `$twig`.
    pub data_uses_twig: bool,
    /// The DATA code declares classes.
    pub data_defines_classes: bool,
    /// Classes instantiated with `new` in DATA.
    pub data_new_classes: Vec<String>,
    /// Option names set by CONFIG.
    pub config_keys: Vec<String>,
}

/// The oracle run of one fixture (`TwigOracle\run_fixture()`).
#[derive(Clone, Debug, Deserialize)]
pub struct FixtureRun {
    /// Batch id (always `null` in the corpus).
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    /// `fixture`.
    pub kind: String,
    /// Same as [`Fixture::file`].
    pub file: String,
    /// Same as [`Fixture::message`].
    pub message: String,
    /// Same as [`Fixture::legacy`].
    pub legacy: bool,
    /// Same as [`Fixture::exception_expected`].
    #[serde(deserialize_with = "false_or_string")]
    pub exception_expected: Option<String>,
    /// Same as [`Fixture::deprecation_expected`].
    pub deprecation_expected: String,
    /// `condition: <expr>` when the `--CONDITION--` was false (no cases then).
    pub skipped: Option<String>,
    /// The cases that ran; a load-phase error ends the fixture, like the harness.
    pub cases: Vec<CaseResult>,
}

/// Where a case ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Loading (compiling) the templates; DATA was not evaluated.
    Load,
    /// DATA evaluation and rendering of `index.twig`.
    Render,
}

/// PHP's state right before rendering (after the DATA code ran).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PhpState {
    /// `date_default_timezone_get()` (DATA may call `date_default_timezone_set()`).
    pub php_default_timezone: String,
    /// `CoreExtension::getTimezone()->getName()`.
    pub core_timezone: String,
    /// `CoreExtension::getDateFormat()`: `[date format, interval format]`.
    pub core_date_format: (String, String),
    /// `CoreExtension::getNumberFormat()`: `[decimals, decimal point, thousands separator]`.
    pub core_number_format: (i64, String, String),
}

/// The actual PHP result of one DATA/EXPECT case.
#[derive(Clone, Debug, Deserialize)]
pub struct CaseResult {
    /// 0-based case index (the harness appends `index` spaces to every template, cumulatively).
    pub index: usize,
    /// Typed tree of `array_merge(['cache' => false, 'strict_variables' => true], CONFIG)`.
    pub config: TypedNode,
    /// Where the case ended.
    pub phase: Phase,
    /// Typed tree of the evaluated DATA (render-phase cases).
    #[serde(default)]
    pub data: Option<TypedNode>,
    /// PHP state before rendering (render-phase cases).
    #[serde(default)]
    pub state: Option<PhpState>,
    /// The exception PHP threw, if any.
    pub error: Option<ErrorInfo>,
    /// The output: rendered and trimmed with `"\n "`, or `"Twig\Error\Error: <class>: <message>"`
    /// for an unexpected render exception (what the harness compares); `None` for exception
    /// fixtures and load errors.
    pub output: Option<String>,
    /// Base64 bytes of the output when it is not valid UTF-8.
    #[serde(default)]
    pub output_b64: Option<String>,
    /// The trimmed `--EXPECT--` text (EXPECT fixtures that reached the comparison).
    #[serde(default)]
    pub expected: Option<String>,
    /// Base64 bytes of the expected text when it is not valid UTF-8.
    #[serde(default)]
    pub expected_b64: Option<String>,
    /// The harness verdict.
    pub pass: bool,
    /// The deprecations the harness compared with `--DEPRECATION--` (load-phase ones, plus the
    /// render-phase ones when the fixture declares deprecations).
    pub deprecations: Vec<String>,
    /// Every deprecation raised while evaluating DATA and rendering.
    #[serde(default)]
    pub render_deprecations: Option<Vec<String>>,
}

impl CaseResult {
    /// The exact output bytes.
    pub fn output_bytes(&self) -> Option<PhpString> {
        self.output
            .as_deref()
            .map(|o| field_bytes(o, self.output_b64.as_deref()))
    }

    /// The exact expected bytes.
    pub fn expected_bytes(&self) -> Option<PhpString> {
        self.expected
            .as_deref()
            .map(|e| field_bytes(e, self.expected_b64.as_deref()))
    }
}

/// The supplement file (`tests/vectors/fixture_data.json`). Only the fields the library uses are
/// modelled; the dumps and probes are test expectations.
#[derive(Clone, Debug, Deserialize)]
struct SupplementFile {
    #[allow(dead_code)]
    generator: String,
    php: String,
    twig: String,
    cases: Vec<SupplementCase>,
}

#[derive(Clone, Debug, Deserialize)]
struct SupplementCase {
    file: String,
    index: usize,
    #[serde(default)]
    data: Option<TypedNode>,
}

/// Whether a case uses its supplement tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SupplementStatus {
    /// No supplement was loaded.
    NotLoaded,
    /// The case has no DATA tree (load-phase case).
    NoData,
    /// The supplement has no tree for this case.
    Missing,
    /// The supplement tree agrees with the corpus tree and is used.
    Used,
    /// The supplement tree disagrees with the corpus tree (stale supplement); the corpus tree is
    /// used.
    Mismatch(String),
}

/// Errors while loading the corpus.
#[derive(Debug)]
pub enum LoadError {
    /// Reading a file failed.
    Io(PathBuf, std::io::Error),
    /// A file is not valid corpus JSON.
    Json(&'static str, serde_json::Error),
    /// The supplement belongs to another PHP/Twig version.
    VersionMismatch(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(p, e) => write!(f, "cannot read {}: {e}", p.display()),
            LoadError::Json(what, e) => write!(f, "invalid {what} JSON: {e}"),
            LoadError::VersionMismatch(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for LoadError {}

/// The loaded corpus with the supplement trees merged in.
pub struct Corpus {
    file: CorpusFile,
    /// Per fixture, per case: the supplement tree and its status.
    supplement: Vec<Vec<(Option<TypedNode>, SupplementStatus)>>,
}

/// Path of `tests/twig/oracle-corpus.json` in this repository.
pub fn default_corpus_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/twig/oracle-corpus.json")
}

/// Path of this crate's `tests/vectors/fixture_data.json`.
pub fn default_supplement_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/fixture_data.json")
}

impl Corpus {
    /// Loads the repository's corpus and this crate's supplement.
    pub fn load_default() -> Result<Corpus, LoadError> {
        Corpus::load(default_corpus_path(), Some(default_supplement_path()))
    }

    /// Loads a corpus file and optionally a supplement file.
    pub fn load(
        corpus: impl AsRef<Path>,
        supplement: Option<impl AsRef<Path>>,
    ) -> Result<Corpus, LoadError> {
        let read =
            |p: &Path| std::fs::read_to_string(p).map_err(|e| LoadError::Io(p.to_path_buf(), e));
        let corpus_json = read(corpus.as_ref())?;
        let supplement_json = match &supplement {
            Some(p) => Some(read(p.as_ref())?),
            None => None,
        };
        Corpus::from_json(&corpus_json, supplement_json.as_deref())
    }

    /// Loads a corpus file without a supplement: objects whose private state only the supplement
    /// records are then reported as [`Unsupported::MissingHiddenState`](crate::Unsupported).
    pub fn load_corpus_only(corpus: impl AsRef<Path>) -> Result<Corpus, LoadError> {
        Corpus::load(corpus, None::<&Path>)
    }

    /// Parses a corpus and optionally a supplement from JSON text.
    pub fn from_json(corpus: &str, supplement: Option<&str>) -> Result<Corpus, LoadError> {
        let file: CorpusFile =
            serde_json::from_str(corpus).map_err(|e| LoadError::Json("corpus", e))?;
        let mut sup: Option<SupplementFile> = match supplement {
            Some(text) => {
                Some(serde_json::from_str(text).map_err(|e| LoadError::Json("supplement", e))?)
            }
            None => None,
        };
        if let Some(s) = &sup
            && (s.php != file.php_version || s.twig != file.twig_version)
        {
            return Err(LoadError::VersionMismatch(format!(
                "supplement for PHP {} / Twig {}, corpus for PHP {} / Twig {}",
                s.php, s.twig, file.php_version, file.twig_version
            )));
        }
        let mut by_case: std::collections::HashMap<(String, usize), Option<TypedNode>> = sup
            .as_mut()
            .map(|s| {
                std::mem::take(&mut s.cases)
                    .into_iter()
                    .map(|c| ((c.file, c.index), c.data))
                    .collect()
            })
            .unwrap_or_default();
        let options = CompareOptions {
            extras: false,
            ignore_volatile_dates: true,
            unordered_interfaces_for: &[],
        };
        let supplement = file
            .fixtures
            .iter()
            .map(|fx| {
                fx.actual
                    .cases
                    .iter()
                    .map(|case| {
                        let Some(corpus_tree) = &case.data else {
                            return (None, SupplementStatus::NoData);
                        };
                        if sup.is_none() {
                            return (None, SupplementStatus::NotLoaded);
                        }
                        match by_case.remove(&(fx.file.clone(), case.index)).flatten() {
                            None => (None, SupplementStatus::Missing),
                            Some(tree) => match compare_trees(corpus_tree, &tree, options) {
                                Ok(()) => (Some(tree), SupplementStatus::Used),
                                Err(e) => (None, SupplementStatus::Mismatch(e)),
                            },
                        }
                    })
                    .collect()
            })
            .collect();
        Ok(Corpus { file, supplement })
    }

    /// The corpus file as parsed.
    pub fn file(&self) -> &CorpusFile {
        &self.file
    }

    /// All fixtures, sorted by path.
    pub fn fixtures(&self) -> &[Fixture] {
        &self.file.fixtures
    }

    /// The fixture with the given path (`filters/abs.test`).
    pub fn fixture(&self, file: &str) -> Option<&Fixture> {
        self.file.fixtures.iter().find(|f| f.file == file)
    }

    /// Every case that ran, in corpus order.
    pub fn cases(&self) -> impl Iterator<Item = Case<'_>> {
        self.file
            .fixtures
            .iter()
            .enumerate()
            .flat_map(move |(fi, fx)| {
                (0..fx.actual.cases.len()).map(move |ci| self.case_at(fi, ci))
            })
    }

    /// The case `index` of fixture `file`.
    pub fn case(&self, file: &str, index: usize) -> Option<Case<'_>> {
        let fi = self.file.fixtures.iter().position(|f| f.file == file)?;
        let ci = self.file.fixtures[fi]
            .actual
            .cases
            .iter()
            .position(|c| c.index == index)?;
        Some(self.case_at(fi, ci))
    }

    fn case_at(&self, fi: usize, ci: usize) -> Case<'_> {
        let fixture = &self.file.fixtures[fi];
        let result = &fixture.actual.cases[ci];
        let (tree, status) = &self.supplement[fi][ci];
        Case {
            fixture,
            result,
            supplement_tree: tree.as_ref(),
            supplement_status: status,
        }
    }
}

/// One DATA/EXPECT case of a fixture, as the oracle ran it.
#[derive(Clone, Copy)]
pub struct Case<'a> {
    /// The fixture.
    pub fixture: &'a Fixture,
    /// PHP's result.
    pub result: &'a CaseResult,
    supplement_tree: Option<&'a TypedNode>,
    supplement_status: &'a SupplementStatus,
}

impl<'a> Case<'a> {
    /// The fixture path.
    pub fn file(&self) -> &'a str {
        &self.fixture.file
    }

    /// The case index.
    pub fn index(&self) -> usize {
        self.result.index
    }

    /// The DATA/CONFIG/EXPECT section of this case.
    pub fn section(&self) -> Option<&'a OutputSection> {
        self.fixture.outputs.get(self.result.index)
    }

    /// The tree the context is built from: the supplement tree when it is used, else the corpus
    /// tree; `None` for load-phase cases.
    pub fn data_tree(&self) -> Option<&'a TypedNode> {
        self.supplement_tree.or(self.result.data.as_ref())
    }

    /// The corpus tree of the DATA context.
    pub fn corpus_data_tree(&self) -> Option<&'a TypedNode> {
        self.result.data.as_ref()
    }

    /// Whether the supplement tree is used.
    pub fn supplement_status(&self) -> &'a SupplementStatus {
        self.supplement_status
    }

    /// The template sources the harness used for this case: the fixture's templates with
    /// `index * (index + 1) / 2` spaces appended (the harness appends `index` spaces per case,
    /// cumulatively; `IntegrationTestCase.php:256-258`).
    pub fn templates(&self) -> Vec<(String, String)> {
        let i = self.result.index;
        let pad = " ".repeat(i * (i + 1) / 2);
        self.fixture
            .templates
            .iter()
            .map(|(name, src)| (name.clone(), format!("{src}{pad}")))
            .collect()
    }
}
