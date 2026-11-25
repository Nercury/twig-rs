# Roadmap

Working plan for finishing twig-rs. Finished items are removed from this file: what they produced lives in code,
tests and `reference/`. Facts live in `reference/` (pinned versions in `reference/target.md`), investigations in
`research/`.

## Goal

A Rust implementation of Twig that renders any template written for **Twig 3.31.1 on PHP 8.5.13** with identical
output and identical error messages. Templates must work copy-pasted from PHP projects. Host integration (passing
data, registering extensions) is Rust-native; running inside PHP is out of scope.

## Principles

- **The oracle decides.** When code, docs and memory disagree, the Docker oracle (`research/php-oracle`) wins.
  Every PHP-level behaviour gets oracle-generated test vectors.
- **Port, don't reinvent.** Twig logic is ported from Twig 3.31.1 sources and PHP behaviour from php-src 8.5.13,
  keeping the same algorithms, evaluation order and error texts. A ported function says which upstream function
  it mirrors in its doc comment.
- **Same structure as Twig where behaviour depends on it.** Same AST node kinds, same node visitors in the same
  order, codegen in the same order as `Node::compile()`, so flags (`always_defined`, `with_loop`,
  `ignore_strict_check`, escaper insertion) and error-line attribution come out identical.
- **Deviations are explicit.** Behaviour that cannot match (PHP-internal TypeError texts with file paths, object
  handle numbers, unseeded randomness, recursion limits) is listed in `reference/deviations.md` with the chosen
  stand-in.

## Architecture decisions

- **Pipeline** (the original twig-rs vision, kept): lexer → parser (static AST mirroring Twig node classes) →
  node visitors (Correctness −255, Escaper 0, Sandbox 0, Optimizer 255; YieldNotReady is a no-op here) → codegen to
  LLTL (low-level template language: per-template instruction streams for body, blocks and macros, constant pool,
  run-length line table) → in-tree VM. The `little` crate is retired; its role is taken by `src/lltl` + `src/vm`.
- **Line attribution** follows Twig's `Compiler::addDebugInfo` points: the line table gets an entry exactly where
  Twig emits a debug marker, in Twig's compile order, so "last marker at or before the failing instruction"
  reproduces `Error::guess()`.
- **Values are PHP zvals**: null, bool, int (i64, overflow to float), float, byte string, ordered copy-on-write
  array with PHP key normalisation and next-free-element tracking, and objects. Values are `Send + Sync`
  (Arc-based) so one `Environment` can serve many threads; host objects implement an `Object` trait that models the
  PHP object protocol (properties, case-insensitive methods, `__get`/`__isset`/`__call`, ArrayAccess, Countable,
  Traversable, Stringable, class constants, enums).
- **Strings are bytes.** Templates, values and output are byte strings, as in PHP. The lexer is byte based like
  Twig's PCRE patterns (no `u` flag). Rendering writes `&[u8]` to a sink; a `String` convenience API checks UTF-8.
- **Context is one PHP array per frame**, copied by value exactly where Twig copies it (blocks, includes, macros,
  parents), shared where Twig shares it (capture bodies), snapshotted for arrow functions, and restored after `for`
  with Twig's `array_intersect_key` + `+=` sequence.
- **Template instances are per-environment singletons** with the same observable mutable state as Twig's (imported
  macro registry, resolved parents), behind interior mutability.
- **PHP diagnostics** (warnings, notices, deprecations, Twig deprecations) go to an environment-level handler with
  policies ignore / collect / error. The fixture harness uses "error", mirroring PHPUnit's conversion of warnings to
  exceptions.
- **Regex** for `matches` uses PCRE2 **10.44** built from PHP's bundled sources (C, via `cc`), plus a port of PHP's
  delimiter/modifier parsing and warning texts. The lexer uses the `regex` crate on bytes plus hand-written helpers
  for PCRE-only constructs.
- **Dates**: pure-Rust port of timelib 2022.17 (MIT) with PHP's embedded timezonedb 2026.6, plus a port of PHP's
  `DateTime`/`DateTimeZone`/`DateInterval` behaviour. A feasibility check of generating the scanner with re2c's
  Rust backend comes first (`research/timelib-port/`); binding the C library is the fallback.
- **Unicode** case mapping and properties follow mbstring's tables (Unicode 17.0, generated with the same rules as
  php-src's `ucgendat.php`, without porting LGPL libmbfl code). UTF-8 is fully supported first; other charsets
  (iconv, `convert_encoding`, `charset` option) come later.
- **Floats** format with the recipes in `reference/php/engine-semantics.md` §5 (verified on 314k doubles). `**` and
  other libm-dependent results use a port of the musl/ARM optimized-routines `pow` (MIT), the same algorithm as
  the glibc the oracle runs on.
- **Template-name hashes** (`template_from_string`) use XXH3-128 like PHP's `hash('xxh128')`.

## Workspace layout

```
Cargo.toml          workspace + package `twig` (the engine)
src/                lexer, parser, ast, visitors, lltl (instructions + codegen), vm, runtime, extensions,
                    environment, loaders, errors
crates/twig-value   PHP value model and engine semantics (casts, operators, comparison, float formatting,
                    var_dump/var_export) and the PHP builtins Twig calls (strings, math, arrays, sorting, sprintf,
                    number_format, round, json_encode, url, htmlspecialchars, strip_tags, xxh128, ...)
crates/twig-unicode generated Unicode 17.0 tables + mbstring-compatible case mapping and UTF-8 decoders
crates/twig-pcre    vendored PCRE2 10.44 (C) + PHP preg pattern parsing and error texts
crates/twig-timelib timelib port + timezonedb
tests/twig/         vendored Twig 3.31.1 fixtures, oracle corpus, harness inputs
```

Crate names avoid "php"/"zend" (PHP License 3.01 clause 4, Zend Engine License). Third-party code and ports are
listed in a `NOTICE` file before the first publish; the licence review happens then.

## Testing

- `tests/twig/Fixtures`: all 488 upstream `.test` fixtures run through a libtest-mimic harness that mirrors
  `IntegrationTestCase` (fixture parsing rules, harness environment, test extension, comparison rules). `--DATA--`
  values come from the oracle-exported typed trees in `tests/twig/oracle-corpus.json`; PHP test classes get Rust
  equivalents. A known-failures list acts as a ratchet: a listed fixture that starts passing fails the run until
  it is removed from the list.
- Ports of the behaviour-level PHPUnit tests (lexer, parser, expression parser, getAttribute matrix, escaper
  tables, sandbox matrix, loaders, environment, errors). PHPUnit tests that compare generated PHP are not ported.
- Oracle vector files for PHP semantics (casts, comparison matrix, float formatting, sprintf, number_format,
  round, range, sort, json, mbstring, dates) generated by `research/php-oracle` and committed, so CI never needs
  Docker.

## Phases

### 1. Foundation
- Workspace with the crate skeletons above; `NOTICE` stub; CI updated (current action versions, workspace-wide).
- Fixture harness skeleton: Rust port of the fixture parser (`reference/twig/test-suite.md` §1; the Python
  reference in `research/php-oracle/tools` matches PHP on all 488 files), corpus loading, ratchet list, reporting.
- Exit: `cargo test` runs all fixtures (all listed as known failures) and the oracle corpus loads.

### 2. Parallel tracks (independent crates/modules)
- **2A Lexer**: full Twig 3.31.1 lexer (`reference/twig/syntax.md` §2), byte based, offsets/columns, CRLF
  normalisation, Twig string escapes, number grammar, operator regex built from the registered expression parsers.
  Port LexerTest + the syntax probes appendix as tests.
- **2B Parser + AST**: Twig 3.31.1 expression parsers (precedence table, prefix/infix/postfix), all core tags,
  `subparse`, imports scopes, embedded templates, CorrectnessNodeVisitor, error texts and suggestions. Port
  ParserTest, ExpressionParserTest, TokenStreamTest and the token-parser tests. Starts once 2A's token types land.
- **2C twig-value**: value model, casts, numeric strings, float formatting, arithmetic, comparison, array key
  rules, string conversion, var_dump/var_export, diagnostics, `Object` trait; oracle vectors.
- **2D twig-pcre**: vendored PCRE2 10.44 build (MSVC + gcc/clang), safe wrapper, PHP pattern/modifier parser,
  `preg_match` semantics Twig needs, warning texts.
- **2E twig-timelib**: feasibility research then port (scanner, `tm2unixtime`, `unixtime2tm`, intervals, tz).
- **2F twig-unicode**: table generator + generated tables, mb_strtoupper/lower/title (final sigma), mb_strlen,
  mb_substr, mbstring's UTF-8 decoder, htmlspecialchars' maximal-subpart decoder.
- **2G builtins in twig-value** (after the 2C core API): sprintf, number_format, round (8.4 algorithm), pow port,
  strtr, trim ranges, nl2br, strip_tags, implode/explode, rawurlencode, http_build_query, json_encode, xxh128,
  levenshtein, zend_sort, range (8.3 rules), min/max.

### 3. Engine core
- LLTL instruction set, codegen from the visited AST in Twig compile order, line table.
- VM: frames with COW context, output sink with capture stack, for-loop opcodes, name/attribute opcodes,
  call binding (named args, variadics, needs_environment/context/charset), macros, blocks and inheritance
  (`yield_block`/`yield_parent_block` ported), traits, embed, include, `with`, `apply`, `set`, `guard`,
  `deprecated`, `types`, `do`, `flush`.
- Runtime: `Environment` (options, extensions, globals, template cache), loaders (Array, Filesystem with
  normalisation and namespaces, Chain), `Template`/`TemplateWrapper`, `getAttribute`, escaper visitor + runtime
  `Markup`, optimizer visitor, error model (`updateRepr`, wrapping of non-Twig errors).
- Exit: the "pure data" fixtures (≈262) pass.

### 4. Extensions
- Core filters/functions/tests/operators (`reference/twig/core-extension.md` §4-§7), escaper strategies, sandbox
  (policy, checker, visitor), debug `dump`, string loader, attribute extension, test-harness extension.
- Exit: every fixture without dates or non-UTF-8 charsets passes.

### 5. Full parity
- Dates wired in (`date`, `date_modify`, DateTime objects in data), PHP test classes, remaining fixtures, error
  message/line parity, deprecations, remaining unit-test ports, `reference/deviations.md` complete.
- Exit: 486/486 non-skipped fixtures pass; oracle vector suites green.

### 6. Product
- Public API review and docs, examples, README, benchmarks, compiled-template caching (optional), non-UTF-8
  charsets, licence review and publishing.

## Open questions

- Exact policy for messages that embed PHP internals (generated class names, file paths): pick stand-ins per case
  while porting, record them in `reference/deviations.md`.
- Whether re2c's Rust backend can process timelib's `parse_date.re` (decides port vs bind for dates).
- PHP `**` parity needs a pow port; confirm musl/ARM pow matches the oracle's glibc on the vector set.
