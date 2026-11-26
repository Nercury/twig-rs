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
//!  "globals": {"g": 1}, "timezone": "Europe/Paris", "load_all": true, "errors": "collect",
//!  "lexer": {"tag_block": ["<%", "%>"], "whitespace_trim": "="}}
//! ```
//!
//! `lexer` holds Lexer options (`$twig->setLexer(new \Twig\Lexer($twig, $lexer))` before any
//! template is loaded; twig-rs's `EnvironmentOptions::lexer`).
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
//! Generators must avoid what cannot match by design: unseeded randomness (`random()`, `shuffle`,
//! also a multi-byte character under another charset), microsecond-resolution "now", `var_dump()`
//! object handles (`dump()` of objects) where the template measures or rewrites the dumped text
//! (`length`, `replace()`, `reverse` of a captured dump: the diff accepts handles where they stand,
//! also escaped, case-changed or inside the `string(N)` dump of a captured dump, nothing more),
//! closures in data. They also leave out the inputs of accepted deviations unless asked
//! ([`generators::Options`]: `twig-fuzz generate --d2` for D2's CR escapes and lone CRs with
//! `debug`).
//!
//! Not supported (reported as "unsupported", never as mismatches): the `integration` environment
//! (the oracle's default: always set `"env": "plain"`), `config_php`, callable `autoescape`
//! options, data with closures, resources or classes `twig-test-support` does not port.
//!
//! Keep cases self-contained: in a batch without `"isolation": "process"` the cases share one PHP
//! process, so classes or functions declared by one case's `data_php` exist in the next, and a
//! template compiled by one case (same source and options) is reused by the next without its
//! compile-time deprecations. The generators give every case a filesystem cache directory of its
//! own (`config.cache`, see [`generators`]): with `cache => false` PHP guesses error lines across
//! templates from the wrong frames (a listed deviation), and the runner empties a case's directory
//! before the case runs.
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
//! twig-fuzz replay --update tests/vectors/fuzz       # ... and drop the markers of fixed ones
//! twig-fuzz refresh vectors.json --id v-1            # record a vector's oracle result again
//! twig-fuzz record batch.json --out vectors.json     # every case as a vector (mismatches marked)
//! twig-fuzz selftest-fixtures --out dir              # fixture corpus as plain-env batches
//! twig-fuzz selftest-engine --out dir                # engine vector batches + recorded results
//! twig-fuzz generate escaping --seed 7 --count 2000 --out esc.json   # a random batch of a family
//! twig-fuzz generate escaping --seed 7 --count 2000 --d2 --out esc.json   # ... with D2 inputs
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
//! this crate holds the diagnosed mismatches found while the toolkit was built
//! (`tests/fuzz_regressions.rs` replays it too). `replay --update` removes the `known_mismatch`
//! marker of every marked vector that now matches the oracle and edits nothing else
//! ([`kit::vectors::update_file`]): a vector that still mismatches keeps its marker, also when its
//! signature changed.
//!
//! The oracle runs as `docker run --rm -i twig-oracle:3.31.1 php runner.php`, what
//! `research/php-oracle/oracle.sh` does (`TWIG_ORACLE_IMAGE` picks the image, `TWIG_FUZZ_ORACLE`
//! replaces the command, e.g. `bash research/php-oracle/oracle.sh`). Batches go in chunks of 2000
//! cases (`--chunk`); isolated batches run 8 cases at a time (`--jobs`); the minimiser keeps one
//! container running (`runner.php --serve`) and runs every candidate in a PHP process of its own
//! (`--shared-process` turns that off). A container that dies without its results (PHP's hard
//! memory limit, a crash) or stops reporting progress is killed and its chunk runs again with
//! process isolation ([`oracle`]).
//!
//! # Comparison
//!
//! [`kit::diff::compare`] compares `ok`, the output bytes, the failing phase, the error (class,
//! message, raw message, template name, line, column, previous errors), the deprecations and the
//! diagnostics. Differences that `reference/deviations.md` accepts (the `, called in <file>` suffix
//! of userland `TypeError`s, closure names with file and line or template class scope,
//! `var_dump()` object handles also through escaping strategies and case filters and in the
//! `string(N)` lengths of dumped dumps, lossy non-UTF-8 messages, the compile-time warnings a
//! filesystem cache silences in PHP and twig-rs raises when the expression runs) make a
//! [`Verdict::Deviation`](kit::diff::Verdict::Deviation), not a mismatch.
//! Each mismatch has a signature ([`kit::diff::Mismatch::signature`]) built from the differing fields
//! with quoted strings and numbers blanked out: the minimiser keeps a candidate only while its
//! signature stays the same, and the driver clusters by it.
//!
//! # Library
//!
//! | module | contents |
//! |---|---|
//! | [`kit`] | cases, results, the twig-rs runner, the diff, vector files (also compiled into `tests/fuzz_regressions.rs`) |
//! | [`oracle`] | the Docker oracle: batch mode and server mode, recovery from containers that die or hang |
//! | [`minimize`] | ddmin over templates (lines, tokens, bytes), data and settings; candidates isolated from each other |
//! | [`generators`] | seeded random batches per construct family (`twig-fuzz generate`) |
//! | [`drive`] | the one-shot driver and its report |
//! | [`selftest`] | batches built from the fixture corpus and the engine vectors |
//! | [`worker`] | twig-rs runs in a child process (hangs, memory blow-ups and crashes stay contained) |

pub mod drive;
pub mod generators;
pub mod kit;
pub mod minimize;
pub mod oracle;
pub mod selftest;
pub mod worker;
