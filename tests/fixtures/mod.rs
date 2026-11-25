//! Support code of the fixture harness (`tests/fixtures.rs`): the suite (fixture files, their parse, the oracle
//! corpus, the known-failures list), the trials and the summary.

pub mod compare;
pub mod condition;
pub mod data;
pub mod environment;
pub mod extension;
pub mod extension_vectors;
pub mod parse;
pub mod ratchet;
pub mod runner;
pub mod selftest;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};

use libtest_mimic::{Arguments, Completion, Failed, Trial};

use parse::{FixtureFile, ParsedFixture};
use ratchet::{KnownFailures, Recorder};
use runner::{Outcome, Report};
use twig_test_support::Corpus;

/// Everything the trials share.
pub struct Suite {
    /// The repository root.
    pub root: PathBuf,
    /// The `.test` files, sorted by name, with their parse.
    pub fixtures: Vec<FixtureEntry>,
    /// The oracle corpus (`tests/twig/oracle-corpus.json`) with `twig-test-support`'s supplement of richer DATA
    /// trees (`crates/twig-test-support/tests/vectors/fixture_data.json`).
    pub corpus: Result<Corpus, String>,
    /// The known-failures list.
    pub known: Result<KnownFailures, String>,
    /// The names of the fixture trials that run (all but those skipped by their `--CONDITION--`).
    pub runnable: BTreeSet<String>,
    /// `TWIG_FIXTURES_UPDATE=1`.
    pub update: bool,
}

/// A `.test` file and its parse.
pub struct FixtureEntry {
    /// The file.
    pub file: FixtureFile,
    /// The parse, or upstream's `Test "<name>" is not valid.` (or a read error).
    pub parsed: Result<ParsedFixture, String>,
}

impl Suite {
    /// Reads the fixtures below `tests/twig/Fixtures`, the corpus and the known-failures list.
    pub fn load(root: &Path) -> Result<Suite, String> {
        let dir = root.join("tests/twig/Fixtures");
        let files =
            parse::discover(&dir).map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
        let fixtures: Vec<FixtureEntry> = files
            .into_iter()
            .map(|file| {
                let parsed = std::fs::read(&file.path)
                    .map_err(|e| format!("cannot read {}: {e}", file.path.display()))
                    .and_then(|text| {
                        parse::parse_fixture(&text)
                            .ok_or_else(|| format!("Test \"{}\" is not valid.", file.name))
                    });
                FixtureEntry { file, parsed }
            })
            .collect();
        let runnable = fixtures
            .iter()
            .filter_map(|entry| entry.parsed.as_ref().ok().map(|p| (&entry.file.name, p)))
            .filter(|(_, parsed)| {
                !matches!(
                    condition::evaluate(&parsed.condition),
                    Ok(condition::Verdict::Skip(_))
                )
            })
            .flat_map(|(name, parsed)| (0..parsed.outputs.len()).map(move |i| trial_name(name, i)))
            .collect();
        Ok(Suite {
            root: root.to_owned(),
            corpus: Corpus::load(root.join(CORPUS), Some(root.join(SUPPLEMENT)))
                .map_err(|e| e.to_string()),
            known: KnownFailures::load(&root.join(KNOWN_FAILURES), KNOWN_FAILURES),
            fixtures,
            runnable,
            update: ratchet::update_mode(),
        })
    }

    /// The corpus, or why it did not load.
    pub fn corpus(&self) -> Result<&Corpus, String> {
        self.corpus.as_ref().map_err(Clone::clone)
    }

    /// The known-failures list, or why it did not load.
    pub fn known_failures(&self) -> Result<&KnownFailures, String> {
        self.known.as_ref().map_err(Clone::clone)
    }

    /// Runs case `index` of fixture number `fixture`.
    fn run_case(&self, fixture: usize, index: usize) -> Report {
        let entry = &self.fixtures[fixture];
        let parsed = match &entry.parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                return Outcome::Fail(runner::Failure {
                    kind: runner::FailureKind::Harness,
                    message: error.clone(),
                })
                .into();
            }
        };
        let php = self
            .corpus
            .as_ref()
            .ok()
            .and_then(|c| c.case(&entry.file.name, index));
        runner::run(&runner::Case {
            file: &entry.file.name,
            fixture: parsed,
            index,
            php,
        })
    }
}

/// The known-failures list, relative to the repository root.
const KNOWN_FAILURES: &str = "tests/twig/known-failures.txt";

/// The oracle corpus, relative to the repository root.
const CORPUS: &str = "tests/twig/oracle-corpus.json";

/// The supplement of the corpus, relative to the repository root.
const SUPPLEMENT: &str = "crates/twig-test-support/tests/vectors/fixture_data.json";

/// The trial name of DATA/EXPECT case `index` of a fixture: `filters/abs.test#0`.
pub fn trial_name(file: &str, index: usize) -> String {
    format!("{file}#{index}")
}

/// Runs the harness: the self-test trials, then one trial per fixture case.
pub fn main() -> ExitCode {
    let args = Arguments::from_args();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let suite = match Suite::load(&root) {
        Ok(suite) => Arc::new(suite),
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let recorder = Arc::new(Recorder::default());
    let parity: Arc<Mutex<BTreeMap<String, Vec<String>>>> = Arc::default();
    let mut trials = selftest::trials(&suite);
    let cases = fixture_trials(&suite, &recorder, &parity);
    let total = cases.len();
    trials.extend(cases);

    let conclusion = libtest_mimic::run(&args, trials);
    if args.list {
        return conclusion.exit_code();
    }
    let recorded = recorder.take();
    let mut failed = conclusion.has_failed();
    if suite.update {
        match suite
            .known_failures()
            .and_then(|known| known.rewrite(&recorded, &suite.runnable))
        {
            Ok((added, removed)) => {
                println!("\n{KNOWN_FAILURES} rewritten: {added} added, {removed} removed");
            }
            Err(error) => {
                eprintln!("error: {error}");
                failed = true;
            }
        }
    }
    if !recorded.is_empty() {
        let parity = std::mem::take(&mut *parity.lock().unwrap_or_else(PoisonError::into_inner));
        print_summary(&suite, &recorded, &parity, total);
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// One trial per DATA/EXPECT case (`filters/abs.test#0`); a file upstream rejects gets one failing trial named after
/// the file.
fn fixture_trials(
    suite: &Arc<Suite>,
    recorder: &Arc<Recorder>,
    parity: &Arc<Mutex<BTreeMap<String, Vec<String>>>>,
) -> Vec<Trial> {
    let mut trials = Vec::new();
    for (fixture, entry) in suite.fixtures.iter().enumerate() {
        let cases = match &entry.parsed {
            Ok(parsed) => parsed.outputs.len(),
            Err(error) => {
                let error = error.clone();
                trials.push(Trial::test(entry.file.name.clone(), move || {
                    Err(Failed::from(error))
                }));
                continue;
            }
        };
        for index in 0..cases {
            let name = trial_name(&entry.file.name, index);
            let suite = Arc::clone(suite);
            let recorder = Arc::clone(recorder);
            let parity = Arc::clone(parity);
            trials.push(Trial::ignorable_test(name.clone(), move || {
                let Report {
                    outcome,
                    parity: notes,
                } = suite.run_case(fixture, index);
                recorder.record(&name, outcome.clone());
                if !notes.is_empty() {
                    parity
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(name.clone(), notes);
                }
                let known = suite.known_failures().map_err(Failed::from)?;
                known
                    .judge(&name, &outcome, suite.update)
                    .map_err(Failed::from)?;
                Ok(match outcome {
                    Outcome::Skipped(condition) => Completion::ignored_with(format!(
                        "skipped by --CONDITION-- on PHP 8.5.13: {}",
                        condition.trim()
                    )),
                    _ => Completion::Completed,
                })
            }));
        }
    }
    trials
}

/// Prints how the fixture cases that ran ended, and the differences from the oracle that the harness does not assert.
fn print_summary(
    suite: &Suite,
    recorded: &BTreeMap<String, Outcome>,
    parity: &BTreeMap<String, Vec<String>>,
    total: usize,
) {
    let listed = |name: &str| suite.known.as_ref().is_ok_and(|known| known.contains(name));
    let mut passed = 0;
    let mut skipped = 0;
    let mut known_failures: BTreeMap<String, usize> = BTreeMap::new();
    let mut unexpected = Vec::new();
    let mut passing_listed = Vec::new();
    for (name, outcome) in recorded {
        match outcome {
            Outcome::Pass if listed(name) => {
                passing_listed.push(name.as_str());
                // in update mode the list is rewritten without it
                if suite.update {
                    passed += 1;
                }
            }
            Outcome::Pass => passed += 1,
            Outcome::Skipped(_) => skipped += 1,
            failure if listed(name) || suite.update => {
                *known_failures.entry(failure.category()).or_default() += 1;
            }
            _ => unexpected.push(name.as_str()),
        }
    }
    let files = suite.fixtures.len();
    let legacy = suite.fixtures.iter().filter(|f| f.file.legacy).count();
    println!(
        "\nTwig 3.31.1 fixtures: {} of {total} cases ran ({files} files, {legacy} of them legacy)",
        recorded.len()
    );
    println!("  passed:               {passed}");
    let label = if suite.update {
        "failing (now listed):"
    } else {
        "known failures:      "
    };
    println!("  {label} {}", known_failures.values().sum::<usize>());
    for (category, count) in &known_failures {
        println!("    {category}: {count}");
    }
    println!("  skipped by CONDITION: {skipped}");
    if !unexpected.is_empty() {
        println!(
            "  unexpected failures:  {} ({})",
            unexpected.len(),
            abbreviate(&unexpected)
        );
    }
    if !passing_listed.is_empty() {
        let label = if suite.update {
            "pass now (unlisted): "
        } else {
            "listed but passing:  "
        };
        println!(
            "  {label} {} ({})",
            passing_listed.len(),
            abbreviate(&passing_listed)
        );
    }
    if !parity.is_empty() {
        let names: Vec<&str> = parity.keys().map(String::as_str).collect();
        println!(
            "  differ from PHP where the harness does not compare: {} ({})",
            names.len(),
            abbreviate(&names)
        );
    }
    if std::env::var_os(VERBOSE_VARIABLE).is_some_and(|v| v == "1") {
        println!("\nFailing cases ({VERBOSE_VARIABLE}=1):");
        for (name, outcome) in recorded {
            if let Outcome::Fail(failure) = outcome {
                println!("\n{name}: {failure}");
            }
        }
        if !parity.is_empty() {
            println!("\nDifferences from PHP that the harness does not compare:");
            for (name, notes) in parity {
                println!("\n{name}: {}", notes.join("; "));
            }
        }
    }
}

/// The environment variable that prints the message of every failing case after the summary, known failures
/// included.
const VERBOSE_VARIABLE: &str = "TWIG_FIXTURES_VERBOSE";

/// The first few names of a list.
fn abbreviate(names: &[&str]) -> String {
    const SHOWN: usize = 10;
    let mut text = names[..names.len().min(SHOWN)].join(", ");
    if names.len() > SHOWN {
        text.push_str(&format!(", ... {} more", names.len() - SHOWN));
    }
    text
}
