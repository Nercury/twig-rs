# PHP oracle

PHP 8.5.13 + Twig 3.31.1 in Docker, used to answer "what does PHP Twig do?" and to generate test vectors.
The protocol is documented at the top of `runner.php` and in `reference/twig/test-suite.md` §7.

```bash
cd research/php-oracle
docker build -t twig-oracle:3.31.1 .                       # once (oracle.sh also builds on demand)

echo '[{"templates":{"index.twig":"{{ 0.1 + 0.2 }}"}}]' | ./oracle.sh --pretty   # render
echo '[{"php":"1.0 + \"1e3\""}]' | ./oracle.sh --pretty                           # evaluate PHP
echo '[{"fixture":"filters/abs.test","typed":true}]' | ./oracle.sh --pretty       # run one upstream fixture

./oracle.sh suite integration      # Twig's integration suite: 488 tests, 486 pass, 2 skipped
./oracle.sh suite all              # full PHPUnit suite: 2986 tests, 2981 pass, 5 skipped
```

From Git Bash on Windows keep `MSYS_NO_PATHCONV=1` (oracle.sh sets it) and write JSON batches to files instead of
inline heredocs, which mangle backslashes.

Optional "twig" case fields used by the differential-testing toolkit (`crates/twig-fuzz`; cases without them behave as
before): `templates_b64` (base64 template sources that are not valid UTF-8), `test_classes` (declare the helper
classes of Twig's `tests/IntegrationTest.php` first), `typed_data` (return the rich typed tree of the evaluated data
as `data`, the PHP/`CoreExtension` state after the data phase as `state`, and what the data code raised as
`data_diagnostics`/`data_deprecations`), `clock` (return `gettimeofday()` taken right before rendering, so that
twig-rs can render with the same "now"), `time_limit`, `memory_limit`, `wall_limit` (wall-clock seconds after which an
isolated child is killed), `lexer` (Lexer options: `$twig->setLexer(new \Twig\Lexer($twig, $lexer))` right after the
environment is built, before any template is loaded), and `tests`/`strict` in the sandbox `policy`. Batch option
`"jobs": N` runs up to N isolated cases at a time. Isolated children get their case's own text from the batch (so
`1e400` stays `INF`), and a case whose `config.cache` is a directory under `/tmp/` gets it emptied before it runs.
`php runner.php --serve` reads one batch per line and answers with one line per batch (twig-fuzz's minimiser keeps one
such container running); `--heartbeat` reports each finished case on stderr. The typed data trees also carry `props`
(`get_object_vars()`) for date objects (and, like for any other object, for `DatePeriod`), and `"wall": true` for a
`DateInterval` whose `add()`/`sub()` use wall-clock time (what `new DateInterval()` gives; PHP shows the mode nowhere
else, so `interval_is_wall()` probes it across a DST end). The protocol is at the top of `runner.php`.

| File | Role |
|---|---|
| `Dockerfile`, `composer.json`, `composer.lock` | Pinned image (base image digest, Twig commit, PHPUnit 9.6) |
| `runner.php`, `lib.php` | JSON batch runner: `twig`, `php` and `fixture` case kinds, optional process isolation |
| `run-suite.sh`, `phpunit-twig.xml` | Runs Twig's PHPUnit suite inside the image |
| `tools/parse-fixtures.php` | Dumps how `IntegrationTestCase` parses every fixture |
| `tools/export-corpus.php` | Regenerates `tests/twig/oracle-corpus.json` (parsed fixtures, actual results, typed DATA trees, PHP state) |
| `tools/fixture_parser_reference.py` | Regex-free reference port of the fixture parser (matches PHP on all 488 fixtures) |
| `examples/` | Sample batches and outputs |
