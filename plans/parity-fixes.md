# Parity fixes

The distinct root causes behind the mismatches that differential fuzzing (`crates/twig-fuzz`) and the PHPUnit ports
(`tests/upstream*.rs`) still find. Each item is meant to be worked on alone: it lists what it covers, a minimal
reproduction, the upstream behaviour with citations, where twig-rs goes wrong and how to check the fix. Finished items
are removed from this file, as in `roadmap.md`; item numbers are never reused.

`TWIG` and `PHPSRC` are the source trees named in `reference/target.md`. Templates are written as JSON strings (`\n`
is a newline). "PHP" is the oracle's result (`twig-oracle:3.31.1`), "twig-rs" the current one. PHP's `, called in
<file> on line N` suffixes are left out (accepted deviation).

## Status

- **Rounds 1 to 3** (2026-11-26) produced P1 to P39, the deviation candidates D1 to D5 and the tooling items T1 to
  T14. Every P item up to P39 is fixed and removed; the round-3 fixes also turned the accepted deviation
  "`needEscaping()`'s `TypeError`" into PHP's own unwrapped `TypeError` and resolved T10 to T14. D1 to D4 are rows of
  `reference/deviations.md`; D5 was not needed. T8 is partly done. Round 3 found 17 raw mismatches in 10 clusters in
  240,000 cases (20,000 per family, seeds 9001 to 9010).
- **Round 4** (2026-11-26, the tree after the round-3 fixes): 30,000 generated cases per family through `twig-fuzz
  drive` with minimisation, seeds 9201 to 9215 (2,000 cases each), without `--d2`, twig-rs runs bounded at 15 s.
  - Before it, `cargo +stable test --workspace` passed: 1863 tests, none failed. The 13 ignored ones are the 2
    fixture cases their CONDITION skips, the 3 tests that need an external vector file, D1's 2 tests, the 5 Docker
    tests of twig-fuzz and 1 doctest.
  - `twig-fuzz replay --update tests/vectors/fuzz` removed no marker (the round-3 fixers had removed theirs); the
    4690 vectors there and the 11 round-0 vectors replay with no failure.

  | family | cases | identical | deviations | mismatches | clusters | not comparable | causes (raw) |
  |---|---|---|---|---|---|---|---|
  | expressions | 30000 | 29824 | 175 | 1 | 1 | 0 | P40 (1) |
  | callables | 30000 | 28919 | 1071 | 4 | 1 | 6 | T15 (4) |
  | control | 30000 | 29860 | 140 | 0 | 0 | 0 | – |
  | composition | 30000 | 28767 | 1041 | 0 | 0 | 192 | – |
  | escaping | 30000 | 29440 | 560 | 0 | 0 | 0 | – |
  | errors | 30000 | 29469 | 531 | 0 | 0 | 0 | – |
  | formats | 30000 | 29778 | 220 | 0 | 0 | 2 | – |
  | sandbox | 30000 | 28590 | 1379 | 0 | 0 | 31 | – |
  | whitespace | 30000 | 30000 | 0 | 0 | 0 | 0 | – |
  | unicode | 30000 | 28085 | 1915 | 0 | 0 | 0 | – |
  | dynamic | 30000 | 27923 | 2077 | 0 | 0 | 0 | – |
  | structure | 30000 | 27912 | 2088 | 0 | 0 | 0 | – |
  | total | 360000 | 348567 | 11197 | 5 | 2 | 231 | |

  - The deviations are the accepted ones the diff knows: `called in` suffixes (7436), `passed in` locations (614, 479
    of them with a closure name), closure names (13 more), object handles (961), and the `not supported by twig-rs`
    errors of mbstring encodings (2173: unicode 1915, escaping 233, callables 25). The rules `dump-string-length` and
    `silenced-compile-warning` never fired, and no mismatch came from D2, whose inputs the generators leave out
    without `--d2`.
  - One cluster was a parity bug (P40, fixed since): the line of a sequence destructuring after an array literal whose last element
    ends with a constant PHP folds (G1, the generated-PHP line model). The other is harness noise: T15, template names
    that are not valid UTF-8 in the oracle's `templateName`. 5 raw cases in 360,000.
  - The not-comparable cases are harness limits: 192 composition cases whose `data_php` loads a template that fails
    (T17), 31 sandbox cases that access `SimpleXMLElement` properties (T8), 2 formats cases whose data throws (`new
    \DateTimeZone("Etc/Unknown")`), and 6 callables cases where PHP stops at a compile-time fatal error (D3: `Cannot use
    positional argument after argument unpacking`).
  - The harness features of the round-3 fixes ran without a mismatch: custom lexer syntax in 4543 whitespace cases
    (T13), source policies in 1875 sandbox cases and wall-clock intervals in 235 formats cases (T8). No generator puts
    a `DatePeriod` into the data yet; `callables/round4-date-period.json` covers it instead.
  - Vectors: three `tests/vectors/fuzz/<family>/round4-*.json` files with 30 oracle-recorded vectors (process
    isolation): 10 marked `known_mismatch` (P40: 7, T15: 3) and 20 controls that match and must keep matching, 8 of
    them in `callables/round4-date-period.json` (the `DatePeriod` data of T8 and the method parameters of P34, which
    the fuzz data could not hold when P34 was fixed).
  - Items reported by the round-3 fixers: P41 (G2's report on P38), P42 and T16 (G4's reports); G1's remark that the
    loop and `with` bookkeeping of P36 has no vector for a host object whose `count()` or iteration throws stays a
    known coverage gap (the typed data cannot hold such an object).
  - A whole 30,000-case `drive` took 77 s (formats) to 520 s (dynamic, process isolation), with three drives running
    at once.

## Working rules

- Port the upstream code path named in the item and cite it in doc comments (`reference/conventions.md`). Messages
  are verbatim.
- An item is done when its vectors replay identically and its tests pass. Then remove the `known_mismatch` markers of
  exactly those vector ids (`cargo test --test fuzz_regressions` fails while a fixed vector is still marked) and the
  `#[ignore]` of its tests. `twig-fuzz replay --update <files> [--id ID]...` removes the markers of the vectors that
  match now. Never edit an `oracle` field: re-run the case with `twig-fuzz refresh` or `twig-fuzz oracle` instead.
- Vector files hold items of several groups. Edit only the markers of your item's ids. The controls in the `round2-*`,
  `round3-*` and `round4-*` files are unmarked on purpose: a fix must not break them.
- Check fixes with `twig-fuzz replay <file>`, and probe variants with `twig-fuzz oracle <batch> --out <o.json>`
  followed by `twig-fuzz diff <batch> --oracle <o.json>`. Add oracle-backed vectors for the "also check" cases.
- `tests/fuzz_regressions.rs` also replays `crates/twig-fuzz/vectors/round0-mismatches.json`.

Groups: **G1** codegen + VM (`src/lltl`, `src/vm`), **G2** runtime (`src/runtime`, `src/escaper`), **G3** callables
(`src/extension`, including the runtime groups), **G4** syntax (`src/lexer`, `src/parser`, `src/token_parser`,
`src/visitor`, `src/ast`), **G5** library crates (`crates/twig-value`, `twig-date`, `twig-pcre`, `twig-unicode`,
`twig-timelib`, `twig-test-support`), **G6** fuzz tooling (`crates/twig-fuzz`, `research/php-oracle`). When an item
needs changes in several groups, the first group listed leads.

## Summary

"Raw" counts the generated cases attributed to the cause; "report" marks an item a round-3 fixer reported. Priority:
**high** = wrong output, or more than 100 raw cases; **medium** = a wrong error or diagnostic that real templates can
hit; **low** = obscure error paths, API-only behaviour, or harness work that hides little.

| id | root cause | groups | raw | vectors | tests | priority |
|---|---|---|---|---|---|---|
| P41 | A template closure compares equal to the lazy path of a template's `Twig\Source` | G5 | report | – | – | low |
| P42 | PHP `\Error`s of API misuse are LogicException-kind errors that `compile_source()` wraps | G4 | report | – | – | low |
| T8 | Typed data and environments the fuzz harness cannot express yet (partly done) | G6, G5 | 31 not comparable | – | – | low |
| T15 | The oracle's `templateName` substitutes invalid UTF-8 differently from twig-rs's lossy name | G6 | 4 | 3 +3 controls | – | low |
| T16 | The escaping generator never writes `{% autoescape null %}` | G6 | – | – | – | low |
| T17 | Composition cases whose `data_php` loads a failing template are not compared | G6 | 192 not comparable | – | – | low |

---

## G4: syntax

### P42. PHP `\Error`s of API misuse are LogicException-kind errors that `compile_source()` wraps

- **Groups**: G4 (`src/visitor/escaper.rs`, `src/parser/mod.rs`, `src/token_parser/apply.rs`).
- **Covers**: reported by the round-3 fix of `needEscaping()`'s `TypeError`. No vector: none of them is reachable from
  templates in an environment with the standard extensions, so the oracle's plain environment cannot show them.
- **What**: three method calls on `null` that PHP reports as an `Error` (`Call to a member function %s() on %s`,
  `zend_invalid_method_call()`, PHPSRC/Zend/zend_execute.c:2560) are `Error::logic(..)` in twig-rs:
  - `EscaperNodeVisitor::getEscaperFilter()`, `$env->getFilter('escape')->getNodeClass()` without an `escape`
    filter: `Call to a member function getNodeClass() on null` (`src/visitor/escaper.rs:244`);
  - `ImportedSymbol::macro_variable()` of a symbol without an import node: `Call to a member function getNode() on
    null` (`src/parser/mod.rs:71`);
  - `ApplyTokenParser::parse()` without a `FilterExpressionParser`: `Call to a member function parse() on null (no
    FilterExpressionParser is registered)` (`src/token_parser/apply.rs:35`), which also adds a twig-rs suffix to PHP's
    text.
- **Upstream**: `Environment::compileSource()` (TWIG/src/Environment.php:614) wraps only `\Exception`s in a
  `SyntaxError`; a PHP `Error` leaves it unwrapped.
- **Fix notes**: return `Error::php(PhpError::error(..))` with PHP's verbatim message, as the round-3 fix did for
  `needEscaping()`'s `TypeError`, and check each case with an API test against a PHP script.

## G5: library crates

### P41. A template closure compares equal to the lazy path of a template's `Twig\Source`

- **Groups**: G5 (`crates/twig-value/src/object/closure.rs`).
- **Covers**: reported by the round-3 fix of P38. No vector: only a template loaded from a file has a lazy path
  (`ModuleNode::compileGetSourceContext()` without `debug` and with a non-empty path), and the oracle's plain
  environment uses an `ArrayLoader`. Check it with a `tests/engine_api.rs` scenario run against PHP, like P38's
  lazy-path cases.
- **Repro**: `s.createWithLazyPath(s.getCode(), s.getName(), (x) => x) == s`, with `s` the `getSourceContext()` of a
  template from a `FilesystemLoader` compiled without `debug`, before its path is resolved. PHP: false; twig-rs: true.
  The other order is false in both.
- **Upstream**: `zend_closure_compare()` (PHPSRC/Zend/zend_closures.c:446): two closures are equal only when both are
  first-class callables (`ZEND_ACC_FAKE_CLOSURE`) of the same function, scope and `$this`; any other pair is
  `ZEND_UNCOMPARABLE`.
- **twig-rs**: twig-value's `Closure::compare()` answers only for another twig-value `Closure`. For the
  `SourcePathClosure` of `src/runtime/template.rs` (class `Closure` too) it returns `None`, and
  `std_compare_objects()` finds two objects of one class without properties equal. `SourcePathClosure::compare()`
  already treats every other closure as uncomparable.
- **Fix notes**: in `Closure::compare()`, treat any other object of class `Closure` (`instance_of("Closure")`) as
  uncomparable.

## G6: fuzz tooling

- **T8. Typed data and environments the harness cannot express** (partly fixed). Done:
  - the factory rebuilds exact `DateInterval` microseconds and `createFromDateString()` intervals;
  - it places dynamic properties of date objects where PHP keeps them, from the `props` of the typed trees;
  - dates at the second of two equal local times, and dates with five-digit years, are rebuilt from the timestamp;
  - `SplObjectStorage::seek()` is ported (G5);
  - `DatePeriod`s are rebuilt with `__set_state()` from their `props` (`get_object_vars()`;
    `tests/vectors/fuzz/callables/round4-date-period.json`);
  - the civil/wall mode of intervals: the oracle probes it (`interval_is_wall()` in `research/php-oracle/lib.php`:
    `add()`/`sub()` across the end of DST in New York against an `unserialize()`d copy) and marks `new DateInterval()`
    intervals `"wall": true`; the factory passes `civil_or_wall` to `__set_state()`
    (`crates/twig-fuzz/tests/vectors/typed-data.json`);
  - source policies: the case's `"sandbox": {"source_policy": [names]}` (an anonymous `SourcePolicyInterface` in the
    oracle, `SandboxExtension::with_source_policy()` in the runner); the sandbox family sets one in a quarter of the
    cases that are not sandboxed globally.

  Remaining:
  - closures and resources (not reproducible: PHP code, process state), and TemplateWrappers of another environment
    (the typed tree records only the template name);
  - no generator writes a `DatePeriod` into its data yet (the formats or callables family could);
  - the full `SimpleXMLElement` port (G5, `crates/twig-test-support/src/classes/simplexml.rs`): property and
    dimension access (`$sxe->child` lists, `$sxe['attr']`), `children()`, `attributes()`. 31 sandbox cases of round 4
    (37 in round 3) are not comparable because of it (`SimpleXMLElement property access is not supported by
    twig-test-support`): `sx.c`, `sx['b']`, and every `.name` access, which looks for a property first. They become
    comparable without a generator change once the port exists;
  - coverage the plain oracle environment cannot express yet:
    - an ArrayAccess class outside `ARRAY_LIKE_CLASSES`, which needs a G5 class port first;
    - `render_sandboxed()` and `Twig\Sandbox\Sandbox`: the second environment is easy to build on both sides, but
      PHP's error handler records the diagnostics of both environments in one sequence, and twig-rs collects them
      per environment (`Environment::take_notices()`), so their order across the two cannot be compared without an
      API that shares one diagnostics sink between environments;
    - custom policy classes (PHP code);
    - `config_php`, a callable `autoescape`, a `FilesystemLoader` (lazy source paths, P41), and the integration
      environment.
- **T15. Template names that are not valid UTF-8 in the oracle's `templateName`.** Round 4: 4 raw of 30,000 callables
  cases (`c9205-00691m`, `c9206-01172m`, `c9209-00884v`, `c9211-01749v`), all in
  `tests/vectors/fuzz/callables/round4-template-name-bytes.json`: `round4-template-name-bytes-truncated-sequence`
  (minimised from `c9205-00691m`), `-named-argument`, `-overlong-prefix`; controls `-ctl-valid-prefix`,
  `-ctl-lone-byte`, `-ctl-utf8`.
  - Repro: `"{{ block('x', template_from_string('', \"\\xed\\xa0\")) }}"` (extension `string_loader`). The error's
    `templateName` is `"\u{fffd} (string template ...)"` in the oracle's result and `"\u{fffd}\u{fffd} (string template
    ...)"` in twig-rs's; the messages, which embed the name, are identical.
  - Cause (harness, not twig-rs): `describe_throwable()` (`research/php-oracle/lib.php`) writes `templateName` as it
    is instead of with `put_string()`, so the runner's `json_encode(..., JSON_INVALID_UTF8_SUBSTITUTE)` puts one U+FFFD
    for each sequence `php_next_utf8_char()` rejects (PHPSRC/ext/standard/html.c:139: `\xed\xa0` and `\xe0\x80` before
    a space are one two-byte sequence). twig-rs's name is `to_string_lossy()` of the bytes (accepted deviation
    "Non-UTF-8 template names given to `template_from_string()`"), which replaces maximal subparts: two U+FFFD.
  - Fix: write `templateName` with `put_string()` (adding `templateName_b64`), read it with `get_bytes()` in
    `ErrorDesc::from_json()` (`kit/result.rs`) and compare its lossy conversion with twig-rs's name, as `kit/diff.rs`
    does for messages; rebuild the oracle image, then `twig-fuzz refresh` the three marked vectors and `replay
    --update` flips them.
- **T16. The escaping generator never writes `{% autoescape null %}`.** `autoescape_arg()` in
  `crates/twig-fuzz/src/generators/escaping.rs` leaves `null` out, and the module docs ("What the cases avoid") still
  call it "a listed deviation". Since the round-3 fixes `needEscaping()` raises PHP's own unwrapped `TypeError`
  (`tests/vectors/visitors/need_escaping_env.json`, 53 oracle cases), so `null` can be one of the arguments again, in
  a few percent of the cases.
- **T17. Composition cases whose `data_php` loads a failing template.** 192 of 30,000 composition cases in round 4
  (143 of 20,000 in round 3) call `$twig->load()` in their data on a template that fails to compile or load
  (`A template that extends another one cannot include content outside Twig blocks`, `Calling the "parent" function on
  a template that does not call "extends" or "use" is forbidden`, `Template "nope.twig" is not defined`, trait errors,
  ...). The oracle stops in phase `data` without typed data, so twig-rs cannot rebuild the data and the template's
  error is never compared. Fix in `crates/twig-fuzz/src/generators/composition.rs`: name the templates the data loads
  in the case so the runner loads them first and reports their error in phase `data` (an oracle protocol field), or
  load them from the templates instead (`include`, `template_from_string()`).
- **Smaller notes** (unchanged):
  - long output diffs share the coarse signature `output:changed`;
  - `data_php` minimisation only drops top-level keys;
  - triage batches full of variants of one cause with `--no-minimize` first, then minimise one case per cause.

## Accepted deviations: follow-ups

D1 to D4 are rows of `reference/deviations.md`. D5 was not needed.

- **D1**, unknown-argument errors of AST nodes built without a source (Engine table). The two tests
  `tests/upstream/extensions/util/callable_arguments_extractor_test.rs::test_get_arguments_with_wrong_named_argument_name`
  and `::test_get_arguments_with_wrong_named_argument_names` stay ignored; their `#[ignore]` cites the deviation.
  - Left: turn them into assertions of twig-rs's documented text (`... in "test.twig".`).
- **D2**, lone CR bytes in the generated PHP (Engine table). Triggers: a CR byte that a string escape (`\r`, `\x0d`,
  `\xd`, `\15`) puts into a compiled string literal; with `debug`, a lone CR of the source that `getSourceContext()`
  embeds; and (whitespace family) `{% line %}` after a raw CR inside a tag, together with `\r` escapes.
  - Vectors: `escaping/round2-cr-escape-lines.json` (4 marked, 8 controls), `escaping/round3-d2-cr-escape-lines.json`
    (`\xd`: 2 marked, minimised from `esc-9001-1578` and `esc-9007-1471`, 2 controls),
    `errors/round3-d2-debug-cr-lines.json` (`debug` and an embed: 1 marked, minimised from
    `errors-9006-1784-emb.div.cr`, 2 controls), `errors/known-mismatches.json` (`cr-debug-embed-line`),
    `whitespace/d2-cr-escape-lines.json` (`ws-d2-line-after-raw-cr-and-cr-escape`).
  - These 9 vectors stay marked by design, with reasons pointing at the entry; they are the only marked vectors
    besides those of the items above.
  - Real templates can hit the escape form. Round 3 reached D2 in 3 of its 10 clusters (7 of 17 raw mismatches);
    since then the generators leave these inputs out unless `twig-fuzz generate --d2` asks for them (CR escapes in
    the literals of the escaping, callables and formats families, lone CRs with `debug` in the errors family), and
    round 4, run without `--d2`, had no D2 case.
  - The raw case `esc-7001-346` of round 2 gets its CR from `trim(" \t\n\r\0\x0B")` (PHP's default trim list), and
    its error line moves from 8 to 9. Emulating it would need the line layout of the generated PHP (how many LFs
    Twig writes between a call and the next `// line N`), which twig-rs does not model. Revisit if real reports come
    in.
  - Raw CR bytes of the template are normalized before compiling and shift nothing (controls
    `round2-cr-control-raw-*`).
- **D3**, compile-time fatal errors of generated PHP (Engine table). The `CompileError` comes in PHP's order (after
  Twig's own errors and a `ParseError`) and leaves `compile_source()` unwrapped. The fuzz diff cannot compare a fatal
  error (round 4: 6 callables cases not comparable, `Cannot use positional argument after argument unpacking`), so
  `tests/engine.rs`'s `compile_errors` topic checks the message of each one.
- **D4**, the cache of macro defaults is per environment, not per process (Engine table).
- **Warnings of compiled PHP code** (Engine table): with a filesystem cache PHP's compile-time warnings never appear,
  because `FilesystemCache::load()` includes the class with `@include_once`. `expressions/round3-compile-warning.json`
  holds the case (`unexpected NAN value was coerced to string` of a concatenation PHP folds, minimised from
  `x9003-00992-arith`) and a control without a cache; the diff accepts it with the rule `silenced-compile-warning`
  (twig-fuzz `kit::diff::SILENCED_COMPILE_WARNING`), so neither vector is marked. Round 4 did not hit the rule.
- Documented rows from earlier fixes (no action): "Named arguments to methods of host objects" (Value model, P33's
  `method_params()`, which the date objects implement since P34) and "Lazily resolved template paths" (Engine, P24).

## Already accepted (no action)

The diff accepts these deviations of `reference/deviations.md`, so they show up as "deviations", not mismatches:
- `, called in` / `passed in` suffixes;
- closure names with file and line;
- `var_dump()` object handles, also escaped or case-changed, and the `string(N)` length of a dumped string that holds
  a dump (rule `dump-string-length`, which checks the length against the bytes);
- lossy non-UTF-8 bytes in messages;
- the `not supported by twig-rs` errors of mbstring encodings and static method calls;
- compile-time warnings a filesystem cache silences in PHP ("Warnings of compiled PHP code", rule
  `silenced-compile-warning`).

The generators keep `dump()` of objects away from what measures or rewrites the dumped text (the control family has
no context dump inside capture bodies), give `shuffle` and `random()` inputs of one character under the case's charset
(a multi-byte one keeps the case in UTF-8), and leave out D2's inputs unless `twig-fuzz generate --d2`. The whitespace
family gives one case in seven lexer options of its own (`"lexer"` in the oracle's case format,
`EnvironmentOptions::lexer` in the runner), without the values the lexer rows of `reference/deviations.md` list.

The generators avoid the line guessing of `cache => false` with a cache directory per case. The ported tests also
cover template recursion limits.

## Other follow-ups (documentation and checks)

- `plans/engine.md`:
  - `Environment::runtime()` falls back to a registered extension (P26);
  - outside `debug`, a compiled program's source has no code, and its path is resolved lazily through the loader of
    the template instance (P24);
  - `Object::method_params()` and the method-call binding of `getAttribute()` (P33), and the date objects' parameter
    tables (`twig-date`'s `args::method_params()`, P34);
  - `extension::runtime::callables` and the generated class table `misc/twig_classes.rs` exist (P15, P27);
  - the line attribution rules of the round-3 fixes in the LineTable section: `TrueTest` at the marker where `$tmp`
    is written (P37), statement-level bookkeeping at the marker in effect (`leaf()`, P36), the line of an array literal
    that comes after a dynamic key (`DeferredLine`, `LineTable::replace_from()`, P35);
  - the `Twig\Source` objects (typed private properties in a property store, `__construct()`, `createWithLazyPath()`,
    `getColumn()`, `__serialize()`, the `SourcePathClosure` lazy path, P38) and `runtime::typed_property` (the typed
    property check of `Markup::__construct()`, P39).
- Checks on the tree of round 4: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`, `cargo clippy
  --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` pass.
- twig-fuzz does not build on the MSRV (G6): `cargo +1.88 check --workspace --all-targets` fails with 24 errors where
  `self.rng.pick(X)` (`crates/twig-fuzz/src/generators/composition/rng.rs:48`, `X: &[&[u8]]`) stands where a `&[u8]`
  is expected (`extend_from_slice()`, a `&[u8]` binding): 1.88 infers `T = [u8]` from it (E0308 ``expected
  `&[[u8]]`, found `&[&[u8]]` ``, E0277 unsized `[u8]`), where newer compilers dereference the `&&[u8]`. Sites:
  `generators/unicode.rs:352`; `generators/whitespace.rs:479`, `:484`, `:498`, `:500`, `:981`, `:987`, `:1021`,
  `:1215`, `:1217`, `:1227`, `:1229`; `*self.rng.pick(X)` fixes each. CI's MSRV job runs `cargo test --workspace` on
  1.88, so it fails until then.

## Not parity bugs (recorded for roadmap phase 6)

Missing public APIs reported by the ports:
- `CoreExtension` static helpers and `getAttribute()` as an entry point;
- `TemplateWrapper::unwrap()`, and `Environment::load()` taking a wrapper;
- macro namespaces as values (`getMacroNamespace()`, `MacroNamespace::call`/`has`, `TwigMacro`);
- `resetGlobals()`, `isTemplateFresh()`, `Twig\BlockChain` (`Source::createWithLazyPath()` is now
  `Source::with_lazy_path()`);
- the `Sandbox` stream methods, lazy runtime factories, `SecurityChecker::new`/`check_security`;
- a codegen hook for extension nodes (already listed);
- `RecursiveArrayIterator`, and ArrayObject/ArrayIterator subclasses in twig-value.
