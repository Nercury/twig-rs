//! Differential testing of twig-rs against the PHP oracle (Twig 3.31.1 on PHP 8.5.13,
//! `research/php-oracle`): the same case runs in PHP and in twig-rs, the results are compared field by
//! field, mismatches are clustered by a stable signature, minimised by delta debugging and written
//! as regression vectors that `tests/fuzz_regressions.rs` replays without Docker.
//!
//! # Cases
//!
//! A case is an oracle "twig" case (the `(a)` kind at the top of `research/php-oracle/runner.php`)
//! with `"env": "plain"`:
//!
//! ```json
//! {"id": "f-1", "env": "plain",
//!  "templates": {"index.twig": "{{ x|round(1, 'floor') }}"},
//!  "data_json": {"x": 2.25},
//!  "config": {"autoescape": false, "strict_variables": true},
//!  "extensions": ["debug", "string_loader", "sandbox"],
//!  "sandbox": {"global": true, "policy": {"tags": ["if"], "filters": ["round"], "methods": {},
//!              "properties": {}, "functions": [], "tests": [], "strict": true}},
//!  "globals": {"g": 1}, "timezone": "Europe/Paris", "load_all": true, "errors": "collect"}
//! ```
//!
//! Data comes as `data_json` (decoded like PHP's `json_decode($json, true)`) or as `data_php` (PHP
//! code, the way a fixture's `--DATA--` is written). twig-rs cannot run PHP: for `data_php` the oracle
//! returns the typed tree of the evaluated data (`"typed_data": true`, which the tools add), and
//! `twig-test-support` rebuilds the values, objects included (`stdClass`, `ArrayObject`,
//! `ArrayIterator`, `SplObjectStorage`, `Twig\Markup`, dates, the helper classes of Twig's
//! `tests/IntegrationTest.php` with `"test_classes": true`, ...). Templates that are not valid UTF-8
//! go in `"templates_b64"`. `"time_limit"` and `"memory_limit"` bound the PHP side of a case (the
//! tools set 30 s and 512M unless the case sets them). The tools also set `"clock": true`: the oracle
//! returns the time it started rendering and twig-rs renders with that "now", so `date()` and
//! `null|date` agree unless the render crosses a second or prints microseconds.
//!
//! Generators must avoid what cannot match by design: unseeded randomness (`random()`, `shuffle`),
//! microsecond-resolution "now", `var_dump()` object handles (`dump()` of objects), closures in data.
//!
//! Not supported (reported as "unsupported", never as mismatches): the `integration` environment
//! (the oracle's default: always set `"env": "plain"`), `config_php`, callable `autoescape`
//! options, data with closures, resources or classes `twig-test-support` does not port.
//!
//! Keep cases self-contained: in a batch without `"isolation": "process"` the cases share one PHP
//! process, so classes or functions declared by one case's `data_php` exist in the next.
//!
//! # Command line
//!
//! Build with `CARGO_TARGET_DIR` of your choice; the binary is `twig-fuzz`.
//!
//! ```text
//! twig-fuzz drive batch.json --out fuzz-out          # the whole loop (needs Docker)
//! twig-fuzz drive batch.json --out fuzz-out --oracle-results oracle.json   # reuse oracle results
//! twig-fuzz oracle batch.json --out oracle.json      # oracle only
//! twig-fuzz run batch.json --oracle oracle.json      # twig-rs only, in the oracle's schema
//! twig-fuzz diff batch.json --oracle oracle.json     # compare (twig-rs runs unless --twig-rs)
//! twig-fuzz minimize batch.json --id f-1             # minimise one case
//! twig-fuzz replay tests/vectors/fuzz                # replay regression vectors
//! twig-fuzz record batch.json --out vectors.json     # every case as a vector (mismatches marked)
//! twig-fuzz selftest-fixtures --out dir              # fixture corpus as plain-env batches
//! twig-fuzz selftest-engine --out dir                # engine vector batches + recorded results
//! ```
//!
//! twig-rs runs in a child process (`twig-fuzz worker`) restarted after a timeout (`--timeout-ms`,
//! default 5 s) or a crash, which become results in phase `timeout` / `crash`; a panic becomes a
//! result in phase `panic`. Build with `--release` for long runs.
//!
//! `drive` writes `report.md` (one section per mismatch cluster with its minimised reproduction),
//! `report.json`, the raw `oracle.json` / `twig-rs.json` and `vectors.json` (one vector per cluster,
//! marked `known_mismatch`). Copy vectors into `tests/vectors/fuzz/` to make them regression tests
//! (format: `tests/vectors/fuzz/README.md`, [`kit::vectors`]). `vectors/round0-mismatches.json` in
//! this crate holds the diagnosed mismatches found while the toolkit was built.
//!
//! The oracle runs as `docker run --rm -i twig-oracle:3.31.1 php runner.php`, what
//! `research/php-oracle/oracle.sh` does (`TWIG_ORACLE_IMAGE` picks the image, `TWIG_FUZZ_ORACLE`
//! replaces the command, e.g. `bash research/php-oracle/oracle.sh`). Batches go in chunks of 2000
//! cases (`--chunk`); the minimiser keeps one container running (`runner.php --serve`).
//!
//! # Comparison
//!
//! [`kit::diff::compare`] compares `ok`, the output bytes, the failing phase, the error (class,
//! message, raw message, template name, line, column, previous errors), the deprecations and the
//! diagnostics. Differences that `reference/deviations.md` accepts (the `, called in <file>` suffix
//! of userland `TypeError`s, closure names with file and line, `var_dump()` object handles, lossy
//! non-UTF-8 messages) make a [`Verdict::Deviation`](kit::diff::Verdict::Deviation), not a mismatch.
//! Each mismatch has a signature ([`kit::diff::Mismatch::signature`]) built from the differing fields
//! with quoted strings and numbers blanked out: the minimiser keeps a candidate only while its
//! signature stays the same, and the driver clusters by it.
//!
//! # Library
//!
//! | module | contents |
//! |---|---|
//! | [`kit`] | cases, results, the twig-rs runner, the diff, vector files (also compiled into `tests/fuzz_regressions.rs`) |
//! | [`oracle`] | the Docker oracle: batch mode and server mode |
//! | [`minimize`] | ddmin over templates (lines, tokens, bytes), data and settings |
//! | [`drive`] | the one-shot driver and its report |
//! | [`selftest`] | batches built from the fixture corpus and the engine vectors |
//! | [`worker`] | twig-rs runs in a child process (hangs, memory blow-ups and crashes stay contained) |

pub mod drive;
pub mod kit;
pub mod minimize;
pub mod oracle;
pub mod selftest;
pub mod worker;
