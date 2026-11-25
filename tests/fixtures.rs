//! The acceptance suite: Twig 3.31.1's integration fixtures (`tests/twig/Fixtures`, 488 `.test` files, 502
//! DATA/EXPECT cases) run the way `Twig\Test\IntegrationTestCase` runs them (`reference/twig/test-suite.md` §1-§3).
//!
//! # Running
//!
//! ```text
//! cargo +stable test -p twig --test fixtures                      # everything
//! cargo +stable test -p twig --test fixtures -- tags/if/          # the cases whose name contains "tags/if/"
//! cargo +stable test -p twig --test fixtures -- --exact filters/abs.test#0
//! cargo +stable test -p twig --test fixtures -- --list            # the trial names
//! TWIG_FIXTURES_UPDATE=1 cargo +stable test -p twig --test fixtures   # rewrite tests/twig/known-failures.txt
//! ```
//!
//! The binary uses `libtest-mimic` (`harness = false` in `Cargo.toml`), so the usual libtest arguments work
//! (`--exact`, `--skip`, `--nocapture`, `--test-threads`, `--list`), except that it takes a single filter. Cases
//! skipped by their `--CONDITION--` are decided while running, so `--ignored` selects nothing.
//!
//! # Trials
//!
//! - `harness::*`: the harness's own checks. The fixture parser against PHP's parse of every fixture
//!   (`tests/twig/oracle-corpus.json`) and against the real `assembleTests()` on edge cases and random inputs
//!   (`tests/fixtures/vectors/fixture_parser.json`), the `--CONDITION--` verdicts, the corpus's shape, PHPUnit's
//!   format matching (`tests/fixtures/vectors/format_matching.json`), the comparison rules and the known-failures
//!   list. They always have to pass.
//! - `<fixture path>#<case index>`, e.g. `tags/if/basic.test#2`: one trial per DATA/EXPECT case (an exception
//!   fixture has one case), run through the steps of `doIntegrationTest()` in `tests/fixtures/runner.rs`. Case `i`
//!   gets `i * (i + 1) / 2` spaces appended to every template, as upstream. Today twig-rs has the load phase up to
//!   the parser: a fixture whose expected exception the parser raises is compared like the harness does
//!   (`FQCN: message` after `trim()`, identical, and the message must end with `.` or `?`); every later step
//!   (compilation, rendering) ends the case as "not yet supported", which counts as a failure.
//! - A fixture whose `--CONDITION--` is false on PHP 8.5.13 is reported as ignored (PHPUnit skips it), see
//!   `tests/fixtures/condition.rs`.
//!
//! # The known-failures ratchet
//!
//! `tests/twig/known-failures.txt` lists the fixture trials that are expected to fail, one trial name per line, sorted,
//! nothing else. A listed trial that fails is reported as passing and counted as a known failure; a listed trial that
//! passes fails the run ("passes now: remove it"); an unlisted trial that fails fails the run with the harness's
//! message. `TWIG_FIXTURES_UPDATE=1` makes every fixture trial pass and rewrites the list from the trials that ran
//! (entries of trials that did not run, because of a filter, are kept). After the trials a summary prints how many
//! cases passed, how many are known failures (by reason) and how many were skipped by their condition.
//!
//! # Regenerating the inputs
//!
//! - `tests/twig/oracle-corpus.json`: `research/php-oracle/tools/export-corpus.php` (see `tests/twig/README.md`).
//! - `tests/fixtures/vectors/*.json`: the `.gen.php` next to each file says how (Docker oracle
//!   `twig-oracle:3.31.1`, see `research/php-oracle/README.md`).

#[path = "fixtures/mod.rs"]
mod harness;

fn main() -> std::process::ExitCode {
    harness::main()
}
