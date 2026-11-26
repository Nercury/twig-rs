//! The acceptance suite: Twig 3.31.1's integration fixtures (`tests/twig/Fixtures`, 488 `.test` files, 502
//! DATA/EXPECT cases) run end to end the way `Twig\Test\IntegrationTestCase` runs them (`reference/twig/test-suite.md`
//! §1-§3).
//!
//! # Running
//!
//! ```text
//! cargo +stable test -p twig --test fixtures                      # everything
//! cargo +stable test -p twig --test fixtures -- tags/if/          # the cases whose name contains "tags/if/"
//! cargo +stable test -p twig --test fixtures -- --exact filters/abs.test#0
//! cargo +stable test -p twig --test fixtures -- --list            # the trial names
//! TWIG_FIXTURES_UPDATE=1 cargo +stable test -p twig --test fixtures   # rewrite tests/twig/known-failures.txt
//! TWIG_FIXTURES_VERBOSE=1 cargo +stable test -p twig --test fixtures  # also print why every failing case fails
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
//!   format matching (`tests/fixtures/vectors/format_matching.json`), the comparison rules, the harness environment
//!   and its port of the test extensions against the oracle (`tests/fixtures/vectors/test_extension.json`) and the
//!   known-failures list. They always have to pass.
//! - `<fixture path>#<case index>`, e.g. `tags/if/basic.test#2`: one trial per DATA/EXPECT case (an exception
//!   fixture has one case), run through the steps of `doIntegrationTest()` in `tests/fixtures/runner.rs`:
//!   - a fresh [`twig::runtime::Environment`] per case, built like the harness's (`tests/fixtures/environment.rs`):
//!     the options of the CONFIG section (as the oracle evaluated it, resolved by `twig-test-support`), the global
//!     `global`, the runtime loader of `TwigTestLazyExtensionWithAttributes`, the Debug, Sandbox (non-global, policy
//!     allowing `dump`), StringLoader and test extensions (`tests/fixtures/extension.rs`, a port of
//!     TWIG/tests/IntegrationTest.php) and the undefined-callable callbacks, with PHP warnings and notices turned
//!     into exceptions like PHPUnit does;
//!   - case `i` gets `i * (i + 1) / 2` spaces appended to every template, as upstream;
//!   - the load phase loads every template in section order and compares an exception exactly (`FQCN: message`
//!     after `trim()`, plus the final `.`/`?` rule);
//!   - the render phase rebuilds the DATA context from the oracle's typed tree (`tests/fixtures/data.rs` with
//!     `twig-test-support`), applies the PHP state recorded before rendering, renders `index.twig`, format-matches an
//!     exception (PHPUnit's `assertStringMatchesFormat()`) or compares the output (`trim($output, "\n ")`, strict)
//!     and the deprecations (load phase always, render phase when the fixture has `--DEPRECATION--`).
//!
//!   The summary also lists cases whose render deprecations or exact exception message differ from the oracle's
//!   run although the harness does not compare them.
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
//! cases passed, how many are known failures (by the step that failed) and how many were skipped by their condition.
//!
//! Trials that fail only in some builds are listed in `BUILD_KNOWN_FAILURES` (`tests/fixtures/ratchet.rs`) instead:
//! without the `glibc-iconv` feature, `filters/convert_encoding.test#0`, whose ISO-2022-JP round trip needs glibc's
//! iconv (reference/deviations.md). Run the suite with and without the feature (`--features glibc-iconv`, or
//! `--all-features`).
//!
//! # Regenerating the inputs
//!
//! - `tests/twig/oracle-corpus.json`: `research/php-oracle/tools/export-corpus.php` (see `tests/twig/README.md`);
//!   its supplement `crates/twig-test-support/tests/vectors/fixture_data.json`: see that crate.
//! - `tests/fixtures/vectors/*.json`: the `.gen.php` / `.gen.py` next to each file says how (Docker oracle
//!   `twig-oracle:3.31.1`, see `research/php-oracle/README.md`).

#[path = "fixtures/mod.rs"]
mod harness;

fn main() -> std::process::ExitCode {
    harness::main()
}
