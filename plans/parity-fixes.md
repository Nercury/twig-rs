# Parity fixes

The distinct root causes behind the mismatches that differential fuzzing (`crates/twig-fuzz`) and the PHPUnit ports
(`tests/upstream*.rs`) still find. Each item is meant to be worked on alone: it lists what it covers, a minimal
reproduction, the upstream behaviour with citations, where twig-rs goes wrong and how to check the fix. Finished items
are removed from this file, as in `roadmap.md`; item numbers are never reused.

`TWIG` and `PHPSRC` are the source trees named in `reference/target.md`. Templates are written as JSON strings (`\n`
is a newline). "PHP" is the oracle's result (`twig-oracle:3.31.1`), "twig-rs" the current one. PHP's `, called in
<file> on line N` suffixes are left out (accepted deviation).

## Status

- **Rounds 1 to 4** (2026-11-26) produced P1 to P42, the deviation candidates D1 to D5 and the tooling items T1 to
  T17. Every P item up to P42 is fixed and removed, and so is every T item up to T17 except the remaining gaps of T8;
  D1 to D4 are rows of `reference/deviations.md`, D5 was not needed. Round 4 (the tree after the round-3 fixes, seeds
  9201 to 9215) found 5 raw mismatches in 360,000 cases (P40, T15) and 231 cases not comparable (T17, T8, 2 formats
  cases whose data throws, D3); all of them but D3's are fixed.
- **Round 5** (2026-11-26, the tree after the performance pass, `fdc54d8`) was to close phase 5 (`roadmap.md`: exit
  with a round that finds nothing new): 30,000 generated cases per family through `twig-fuzz drive` with
  minimisation, 2,000 per seed (11001 to 11015 for expressions, callables and control, 12001 to 12015 for
  composition, escaping and errors, 13001 to 13015 for formats, sandbox and whitespace, 14001 to 14015 for unicode,
  dynamic and structure), without `--d2`, twig-rs runs bounded at 15 s, release builds with `--features glibc-iconv`.
  The counts are recounted from the 180 drive reports.

  | family | cases | identical | deviations | mismatches | clusters | not comparable | causes (raw) |
  |---|---|---|---|---|---|---|---|
  | expressions | 30000 | 29831 | 168 | 1 | 1 | 0 | P43 (1) |
  | callables | 30000 | 28892 | 1098 | 2 | 2 | 8 | T19 (2) |
  | control | 30000 | 29857 | 142 | 0 | 0 | 1 | – |
  | composition | 30000 | 28944 | 1056 | 0 | 0 | 0 | – |
  | escaping | 30000 | 29412 | 588 | 0 | 0 | 0 | – |
  | errors | 30000 | 29458 | 541 | 1 | 1 | 0 | T18 (1) |
  | formats | 30000 | 29683 | 317 | 0 | 0 | 0 | – |
  | sandbox | 30000 | 28591 | 1409 | 0 | 0 | 0 | – |
  | whitespace | 30000 | 30000 | 0 | 0 | 0 | 0 | – |
  | unicode | 30000 | 28160 | 1840 | 0 | 0 | 0 | – |
  | dynamic | 30000 | 27935 | 2065 | 0 | 0 | 0 | – |
  | structure | 30000 | 27859 | 2141 | 0 | 0 | 0 | – |
  | total | 360000 | 348622 | 11365 | 4 | 4 | 9 | |

  - No mismatch comes from the performance pass, but the round is not the one that finds nothing new: P43 is a parity
    bug as old as the engine (`dd61177`) that no earlier round reached. With the SandboxExtension registered and
    nothing sandboxed, twig-rs skips the walk of the arrays that reach `ensureToStringAllowed()` and
    `ensureSpreadAllowed()`, so PHP 8.5's `unexpected NAN value was coerced to bool` is missing. The other three raw
    mismatches are harness artefacts, each checked with the oracle: unseeded `random()` inside a macro (T18), and two
    accepted deviations the callables generator reaches where the diff cannot accept them (T19).
  - The deviations are the accepted ones the diff knows: `called in` suffixes (7504), `passed in` locations (665, 546
    of them with a closure name), closure names (21 more), object handles (1048), and the `not supported by twig-rs`
    errors of mbstring encodings (2127: unicode 1840, escaping 255, callables 32; no iconv charset and no static method
    call). The rules `dump-string-length` and `silenced-compile-warning` never fired, no case was unsupported, and no
    mismatch came from D2. Formats has 317 deviations against 220 in round 4 (object handles of the `DatePeriod`s its
    data holds since then); the other families are within 5% of round 4.
  - The harness features were reached: lexer options of the case's own in 4,457 whitespace cases, a `DatePeriod` in
    421 formats cases, every kind of sandbox security error (tag 4,021, filter 2,965, property 2,848, method 2,435,
    function 696, test 11), and the unicode family's charsets beyond the clean-room ones through `glibc-iconv` (150 to
    200 `convert_encoding` uses each, e.g. BIG5-HKSCS 166, MacRoman 183, CP866 162, EUC-JP 181, Windows-1252 197).
  - Not comparable: 8 callables cases of D3 (`Cannot use positional argument after argument unpacking`), and the
    control case `ctl-11008-1643`, where `{% for _context in _context %}{% set acc = acc|merge([_context]) %}` inside
    an outer loop grows a value exponentially: PHP stops at its 512M memory limit, twig-rs with its stand-in `Allowed
    memory size of 134217728 bytes exhausted` (row "Memory exhaustion"). The fixes of T17 and of T8's
    `SimpleXMLElement` access, and the formats data without throwing timezones, leave none of round 4's 192
    composition, 31 sandbox and 2 formats cases.
  - Vectors: `expressions/round5-sandbox-nan-bool.json`, 22 oracle-recorded vectors (process isolation): 14 marked
    `known_mismatch` (P43) and 8 controls that match and must keep matching. T18 and T19 have none (a random oracle
    value; accepted deviations).
  - After the round, `cargo +stable test --workspace --all-features --test fuzz_regressions` passes and `twig-fuzz
    replay tests/vectors/fuzz crates/twig-fuzz/vectors` replays 4773 vectors without a failure. 23 of them are marked
    (D2's 9, P43's 14), and all 23 still differ with their recorded signatures.
  - Probes of paths the performance pass changed, not counted above, all identical: `htmlspecialchars()` on invalid
    UTF-8 through every strategy and a concatenation (13 cases); every escaping strategy over ASCII and 22 edge code
    points, with no charset and with 9 explicit ones (372); macro argument binding (47: positional, named, `varargs`,
    `...rest`, spreads, defaults, recursion, frames reused in nested loops); the date-format grid of `formats::grid()`
    (8,320: 41 format characters × 396 instants × 20 zones, and 400 interval formats); fractional seconds (291, 3 of
    which differ only in the microseconds of "now", the documented limit of `"clock"`).
  - The first drive of control seed 11008 stopped after 1 s on a Docker container name held by a stale container
    (T20); the counted result is its second run. Drives of 2,000 cases took 15 to 150 s.

## Working rules

- Port the upstream code path named in the item and cite it in doc comments (`reference/conventions.md`). Messages
  are verbatim.
- An item is done when its vectors replay identically and its tests pass. Then remove the `known_mismatch` markers of
  exactly those vector ids (`cargo test --test fuzz_regressions` fails while a fixed vector is still marked) and the
  `#[ignore]` of its tests. `twig-fuzz replay --update <files> [--id ID]...` removes the markers of the vectors that
  match now. Never edit an `oracle` field: re-run the case with `twig-fuzz refresh` or `twig-fuzz oracle` instead.
- Vector files hold items of several groups. Edit only the markers of your item's ids. The controls in the `round2-*`,
  `round3-*`, `round4-*` and `round5-*` files are unmarked on purpose: a fix must not break them.
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
| P43 | Outside the sandbox, twig-rs skips SecurityChecker's array walk and its NAN-to-bool warnings | G2 | 1 | 14 +8 controls | – | medium |
| T8 | Typed data and environments the fuzz harness cannot express yet | G6, G5 | – | – | – | low |
| T18 | The errors generator's `random(empty)` fault draws an unseeded number inside a macro | G6 | 1 | – | – | low |
| T19 | The callables generator reaches two accepted deviations the diff cannot accept | G6 | 2 | – | – | low |
| T20 | Oracle container names (`twig-fuzz-<pid>-<n>`) collide with stale containers | G6 | – | – | – | low |

---

## G2: runtime

### P43. Outside the sandbox, twig-rs skips SecurityChecker's array walk and its NAN-to-bool warnings

- **Groups**: G2 (`src/runtime/sandbox.rs`).
- **Covers** (round 5: 1 raw of 30,000 expressions cases), all in
  `tests/vectors/fuzz/expressions/round5-sandbox-nan-bool.json` (extension `sandbox`, nothing sandboxed unless noted):
  - `round5-sandbox-nan-bool`, minimised from `x11004-01267-arrays`
    (`"{{([u>(((_context)))..[{'':''}]])}}{%for v in[]%}{%endfor%}"`, data `['nan' => NAN]`: the warning comes before
    `range()`'s `TypeError`); `round5-sandbox-nan-bool-range-ctx`, `-print`, `-length-filter`, `-equal`, `-concat`,
    `-in`, `-filter-arg`, `-function-arg`, `-test`, `-nested`, `-two`, `-throw`, `-spread`;
  - controls, which must keep matching: `round5-sandbox-nan-ctl-no-extension`, `-inf`, `-scalar`, `-sandboxed`,
    `-sandboxed-throw`, `-spread-sandboxed`, `-falsy`, `-literal-element`.
- **Repro**: `"{{ x|length }}"` with `x = [NAN]` (`data_php`), the SandboxExtension registered and no sandbox
  enabled. PHP: `1` and `E_WARNING: unexpected NAN value was coerced to bool`; twig-rs: `1` without the warning. With
  `"errors": "throw"` PHP fails with `An exception has been thrown during the rendering of a template ("unexpected NAN
  value was coerced to bool")` and twig-rs prints `1`.
  - One warning per NAN element at any depth of nested arrays; falsy elements are skipped. The same through every
    wrapped child: a print (before `Array to string conversion`), `==`, `~`, `..`, `in`, a filter's input, untyped
    filter and function arguments, a test's operand and a spread argument (`max(...x)`).
  - Both agree without the extension, when sandboxed (also with `"errors": "throw"` and for a spread), for `INF`, for
    a scalar NAN, for falsy elements only and for the elements of an array literal (never wrapped).
- **Upstream**:
  - `SecurityChecker::doEnsureToStringAllowed()` (TWIG/src/Sandbox/SecurityChecker.php:167-177) hands an array to
    `ensureToStringAllowedForArray()` before its `isSandboxed()` check. `ensureToStringAllowedForArray()` (:221-243)
    evaluates `!$v` for every element and recurses into nested arrays; other truthy elements go back to
    `doEnsureToStringAllowed()`, which returns them at once outside the sandbox. `ensureSpreadAllowed()` (:154-165)
    walks the materialised spread operand the same way.
  - `SandboxNodeVisitor::enterNode()` (TWIG/src/NodeVisitor/SandboxNodeVisitor.php:102-121) and `wrapNode()`
    (:212-233) wrap the string-coerced children of every `CoercesChildrenToStringInterface` node in a
    `CheckToStringNode` (with `spread` for a spread, which calls `ensureSpreadAllowed()`) whenever the extension is
    registered, sandboxed or not.
  - In PHP 8.5, `!` on NAN warns `unexpected NAN value was coerced to bool` (`i_zend_is_true()`,
    PHPSRC/Zend/zend_operators.h). Outside the sandbox these warnings are all the walk shows.
- **twig-rs**: `SecurityChecker::ensure_to_string_allowed()` (`src/runtime/sandbox.rs:813`) returns before any walk
  when `!is_sandboxed()` (its comment says the walk "does nothing observable outside the sandbox"), and
  `ensure_spread_allowed()` (:830) materialises the operand but walks it only when sandboxed (:854). Sandboxed,
  `ensure_array()` (:909) converts every element with `convert::to_bool()` and warns like PHP (controls `-sandboxed`,
  `-spread-sandboxed`). The early return dates from the first engine commit (`dd61177`), not from the performance pass.
- **Fix notes**: keep upstream's order: an array goes to `ensure_array()` before the `is_sandboxed()` check, which
  moves into `do_ensure()` after its array branch (so the non-array elements `ensure_array()` hands it come back
  unchanged outside the sandbox), and `ensure_spread_allowed()` walks its array whether sandboxed or not. Then
  `twig-fuzz replay --update tests/vectors/fuzz/expressions/round5-sandbox-nan-bool.json` removes the 14 markers, and
  the 8 controls must stay identical. The walk costs time on every wrapped array once the extension is registered, as
  in PHP; the render benchmarks register no SandboxExtension. The sandbox family puts no NAN into its data
  (`crates/twig-fuzz/src/generators/sandbox.rs`), which is why it never reached this; NAN elements in its arrays would
  cover the walk.

## G6: fuzz tooling

- **T8. Typed data and environments the harness cannot express.** What the generators leave out because the harness
  cannot rebuild or set it up:
  - closures and resources (not reproducible: PHP code, process state), and TemplateWrappers of another environment
    (the typed tree records only the template name);
  - `SimpleXMLElement` writes, `xpath()`, `addChild()`/`addAttribute()` and namespaces (twig-test-support throws
    `... is not supported by twig-test-support`, which the diff reports as not comparable), and objects the data
    derives from one document (the typed tree records each object's `asXML()`, and the factory rebuilds it as the root
    of a document of its own, so `$sxe->child == $sxe` would differ);
  - an ArrayAccess class outside `ARRAY_LIKE_CLASSES`, which needs a G5 class port first;
  - a host object whose `count()` or iteration throws, so the loop and `with` bookkeeping of P36 has no vector for it
    (G1's remark from the round-3 fixes);
  - `render_sandboxed()` and `Twig\Sandbox\Sandbox`: the second environment is easy to build on both sides, but PHP's
    error handler records the diagnostics of both environments in one sequence, and twig-rs collects them per
    environment (`Environment::take_notices()`), so their order across the two cannot be compared without an API that
    shares one diagnostics sink between environments;
  - custom policy classes (PHP code);
  - `config_php`, a callable `autoescape`, a `FilesystemLoader` (lazy source paths), and the integration environment.
- **T18. The errors generator's `random(empty)` fault draws an unseeded number inside a macro.** Round 5: 1 raw of
  30,000 errors cases (`errors-12002-1715-func.mlib`, signature `output:changed ["N" > "N"]`); no vector, the oracle's
  value is random.
  - The `func` fault `random(empty)` (`crates/twig-fuzz/src/generators/errors.rs:2305`) is meant to throw `The
    "random" function cannot pick from an empty sequence or mapping` (the data's `empty` is `[]`). A macro body does
    not see the caller's context: there `empty` is undefined, the call is `random(null)`, and
    `CoreExtension::random()` (TWIG/src/Extension/CoreExtension.php:462-463) returns an unseeded `mt_rand()`.
  - Minimised: `index.twig` `{%import'macros2.twig'as lib5%}{{lib5.m1}}`, `macros2.twig`
    `{%macro m1()%}{%with{w3:random()}%}{{w3}}{%endwith%}{%endmacro%}` (PHP `643481345`, twig-rs `792595915`; every
    candidate prints another number, so the signature never changes and the minimiser went down to `random()`). In
    the oracle (process isolation) the macro case printed two different numbers in two runs, `{{ random(empty) }}` in
    the root template throws as intended, and `empty is defined` inside the macro is false.
  - Fix: leave `random(empty)` out where `empty` is not among the scope's variables (`Scope::vars`, which a macro body
    gets only for the names passed to it), or write the literal `[]` there. The other faults that name a data key
    literally (`min(empty)`, ...) test `null` in such a macro too: deterministic, but not the error they were written
    for.
- **T19. The callables generator reaches two accepted deviations the diff cannot accept.** Round 5: 2 raw of 30,000
  callables cases, each a cluster of its own (`output:changed`), both checked with the oracle; no vectors (the
  deviations are accepted).
  - `c11002-01066v`, `{{ dump(enum_cases('Twig\\Tests\\DummyBackedEnum')|sort) }}`: `sort()` orders distinct enum
    cases by heap address (row "Enum ordering in `SORT_REGULAR`"). Run alone in a fresh PHP process the oracle prints
    BAR before FOO, as twig-rs does; in the shared process of the 2,000-case batch it printed FOO before BAR. Fix: keep
    enum cases away from `sort` without an arrow (and from other `SORT_REGULAR` orderings of distinct cases).
  - `c11007-00260m`, `{{ dump(include(template_from_string(template='{{ _self }}', name=sover))) }}` with
    `sover = "x\xc0\xafy\xed\xa0\x80z\xf4\x90\x80\x80"`: twig-rs's template name is lossy (row "Non-UTF-8 template
    names given to `template_from_string()`") and `_self` prints it. PHP prints the raw bytes, which
    `htmlspecialchars()` turns into one U+FFFD per sequence `php_next_utf8_char()` rejects (`ED A0 80` gives one where
    the lossy name has three); with `autoescape` off PHP's output keeps the raw bytes. The diff accepts lossy names in
    messages and `templateName`, not in output. Fix: give names that are not valid UTF-8 only to string templates
    that do not print `_self`.
- **T20. Oracle container names collide with stale containers.** `Oracle::spawn()`
  (`crates/twig-fuzz/src/oracle.rs:454`) names every container `twig-fuzz-<pid>-<n>`, `n` counting from 1 in each
  process. A stale container (here one in state `Created`) keeps its name, so a later twig-fuzz process that gets the
  same PID (Windows reuses PIDs quickly) cannot start its first container: Docker exits with status 125 (the name is
  in use), which the oracle module treats as an error, and the drive stops. Round 5: the first drive of control seed
  11008 (`twig-fuzz-20628-1`, still there after the round). Fix: add a random or time-based part to the name, or on
  that conflict remove the container of that name if it is not running and try once more.
- **Smaller notes**:
  - long output diffs share the coarse signature `output:changed` (round 5: the minimiser of `c11007-00260m`, T19,
    drifted to an unrelated `dump()` of a `TemplateWrapper`);
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
    rounds 4 and 5, run without `--d2`, had no D2 case.
  - The raw case `esc-7001-346` of round 2 gets its CR from `trim(" \t\n\r\0\x0B")` (PHP's default trim list), and
    its error line moves from 8 to 9. Emulating it would need the line layout of the generated PHP (how many LFs
    Twig writes between a call and the next `// line N`), which twig-rs does not model. Revisit if real reports come
    in.
  - Raw CR bytes of the template are normalized before compiling and shift nothing (controls
    `round2-cr-control-raw-*`).
- **D3**, compile-time fatal errors of generated PHP (Engine table). The `CompileError` comes in PHP's order (after
  Twig's own errors and a `ParseError`) and leaves `compile_source()` unwrapped. The fuzz diff cannot compare a fatal
  error (round 4: 6 callables cases not comparable, round 5: 8, `Cannot use positional argument after argument
  unpacking`), so `tests/engine.rs`'s `compile_errors` topic checks the message of each one.
- **D4**, the cache of macro defaults is per environment, not per process (Engine table).
- **Warnings of compiled PHP code** (Engine table): with a filesystem cache PHP's compile-time warnings never appear,
  because `FilesystemCache::load()` includes the class with `@include_once`. `expressions/round3-compile-warning.json`
  holds the case (`unexpected NAN value was coerced to string` of a concatenation PHP folds, minimised from
  `x9003-00992-arith`) and a control without a cache; the diff accepts it with the rule `silenced-compile-warning`
  (twig-fuzz `kit::diff::SILENCED_COMPILE_WARNING`), so neither vector is marked. Rounds 4 and 5 did not hit the
  rule.
- Documented rows from earlier fixes (no action): "Named arguments to methods of host objects" (Value model, P33's
  `method_params()`, which the date objects implement since P34) and "Lazily resolved template paths" (Engine, P24).

## Already accepted (no action)

The diff accepts these deviations of `reference/deviations.md`, so they show up as "deviations", not mismatches:
- `, called in` / `passed in` suffixes;
- closure names with file and line;
- `var_dump()` object handles, also escaped or case-changed, and the `string(N)` length of a dumped string that holds
  a dump (rule `dump-string-length`, which checks the length against the bytes);
- lossy non-UTF-8 bytes in messages and template names;
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
  `:1215`, `:1217`, `:1227`, `:1229`; fixed with a non-generic `Rng::pick_bytes` (the MSRV check passes again).

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
