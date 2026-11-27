# Differential fuzzing

How twig-rs is checked against the PHP oracle with generated templates: how a round runs, how its clusters are
triaged, what the six rounds of roadmap phase 5 and the confirmation round of the second performance pass (round 7)
found, what the harness fixed after them, and what it still cannot reach. The tool is documented in its crate
(`crates/twig-fuzz`: `cargo doc -p twig-fuzz`, `twig-fuzz help`), the regression vectors in
`tests/vectors/fuzz/README.md`, and the differences the diff accepts in `reference/deviations.md`.

## Running a round

- A round covers the 12 families of `twig-fuzz generate`: expressions, callables, control, composition, escaping,
  errors, formats, sandbox, whitespace, unicode, dynamic and structure. Since round 4 every family gets 15 seeds of
  2,000 cases (30,000 per family, 360,000 in all): `twig-fuzz generate <family> --seed N --count 2000 --out
  <batch.json>`, then `twig-fuzz drive <batch.json> --out <dir> --timeout-ms 15000`, with minimisation on and without
  `--d2`, on a release build with `--features glibc-iconv` (the unicode family converts glibc's other charsets too).
- Seeds used so far: 7001-7005 (round 2), 9001-9010 (round 3), 9201-9215 (round 4), 11001-14015 (round 5),
  15001-18015 (round 6), 19001-22015 (round 7) and 25001-25013 (the smoke round of the harness fixes after round 6,
  below). Rounds 5 to 7 gave each group of three families a block of its own: 11001-11015 for expressions, callables
  and control, 12001-12015 for composition, escaping and errors, 13001-13015 for formats, sandbox and whitespace,
  14001-14015 for unicode, dynamic and structure, and the same layout 4000 higher in round 6 and 8000 higher in round
  7. A new round takes seeds nobody used (the next block in this layout starts at 23001).
- Drives run in parallel: round 7 ran 8 at a time with `--jobs 4` on a 24-core machine, the 180 drives in 16 minutes.
  A drive of 2,000 cases took 13 to 115 s; the families whose batches run a PHP process per case (composition,
  escaping, sandbox, whitespace, unicode, dynamic, structure) take the longest.
- Docker Desktop can fail a `docker run` by itself (round 7: `502 Bad Gateway` from its engine, Docker's status 125):
  the drive then stops with status 2 and "the oracle could not be started", and is simply run again. A container that
  dies under a batch that runs a PHP process per case is replaced by two that run its halves, which the drive's log
  reports as an oracle incident.
- A twig-rs run that exceeds `--timeout-ms` where PHP finished the case runs once more with the case's PHP time
  limit (30 s unless the case sets one), so that a case that is only slow on a loaded machine is compared; the
  drive's log, `report.md` and `report.json` (`second_runs`) name such cases. Round 6's probe `ctl-15014-925-sbx`
  (`{% set str = [[false, loop], ...] %}` inside nested loops, printed through `_context|keys`: the walked
  `_context` grows exponentially) took twig-rs about 14 s with the SandboxExtension registered (0.7 s without)
  against the drive's 15 s, and PHP about 2 minutes, with identical output.
- Before and after a round: `cargo +stable test --workspace --all-features --test fuzz_regressions`, `twig-fuzz
  replay tests/vectors/fuzz crates/twig-fuzz/vectors` (every unmarked vector replays identically, every marked one
  still differs) and `docker ps -a --filter name=twig-fuzz-` (no container left: the oracle client removes the
  containers it gives up, `--rm` the others; see `crates/twig-fuzz/src/oracle.rs` for what a killed client leaves).
- Count from the 180 drive reports (`report.json`): identical, deviations per rule, mismatches, clusters, not
  comparable, unsupported. Record the tree (the commit, plus `git diff | sha256sum` while the change is not
  committed). Compare the deviations per family and rule with the last round (below): a change beyond about 5% means
  that a generator, the diff or the engine changed what the cases reach, unless the reach of the data explains it.
  Fresh seeds alone can move a family by up to about 8% (round 7: composition 979 against 1,064); a batch of the last
  round driven with the new tree tells the two apart, since it gives the last round's counts again unless something
  changed.
- To check an engine change in a setting the generators rarely vary, drive the counted batches again with that
  setting changed. Round 6 re-drove the 86,934 expressions, callables and control cases that registered no
  SandboxExtension with `"sandbox"` added to `extensions`: 182 cases had warnings only P43's array walk raises, and
  all of them matched.

## Triage

- Check each cluster's minimised reproduction with the oracle in a fresh PHP process (`"isolation": "process"`).
  Batches without it share one process, where the order `asort()` gives distinct enum cases (by heap address)
  depends on the cases before, and a template compiled by an earlier case is reused without its compile-time
  deprecations.
- Classify it: a parity bug (P), an accepted deviation the diff cannot accept or a candidate for
  `reference/deviations.md` (D), or a harness artefact (T).
- Record a parity bug as `tests/vectors/fuzz/<family>/round<N>-<topic>.json`: the minimised vector and
  oracle-recorded variants, marked `known_mismatch` with the diagnosis as `reason`, and controls that match.
- Drive a batch full of variants of one cause with `--no-minimize` first, then minimise one case per cause.
- Probe variants of a cause with `twig-fuzz oracle <batch> --out <o.json>` and `twig-fuzz diff <batch> --oracle
  <o.json>`, and keep the ones a fix must get right as vectors.
- Long output diffs share the coarse signature `output:changed`, so the minimiser can drift to an unrelated
  difference (round 5: `c11007-00260m` ended at a `dump()` of a `TemplateWrapper`). `data_php` minimisation only
  drops top-level keys.
- Phase 5 kept its findings in a working list, `plans/parity-fixes.md`, removed when the phase ended (`git log --
  plans/parity-fixes.md` has every version). It held one entry per root cause: what it covers, a minimal
  reproduction, the upstream behaviour with citations, where twig-rs went wrong and how to check the fix, with an
  owner group that leads: G1 codegen and VM (`src/lltl`, `src/vm`), G2 runtime (`src/runtime`, `src/escaper`), G3
  callables (`src/extension`), G4 syntax (`src/lexer`, `src/parser`, `src/token_parser`, `src/visitor`, `src/ast`),
  G5 library crates, G6 fuzz tooling (`crates/twig-fuzz`, `research/php-oracle`). Commit messages, test headers and
  vector descriptions cite its item numbers; round 7 continued them with P44, T21 and T22 (below), so the next item
  numbers are P45, D6 and T23.
- A fix ports the upstream code path and cites it (`reference/conventions.md`). It is done when its vectors replay
  identically; `twig-fuzz replay --update <files> --id ID...` then removes the markers of exactly those vectors.

## Rounds

| round | tree | cases | raw mismatches | found |
|---|---|---|---|---|
| 1 | the engine at the end of phase 4 | 8 families, the round-0 tooling run, the PHPUnit ports | about 1% of cases | P1-P30, D1-D5, T1-T9 |
| 2 | after the round-1 fixes | 80,000 (8 × 10,000) | 4 in 3 clusters | P31, P32, D2 without `debug` |
| 3 | after the round-2 fixes, with the families whitespace, unicode, dynamic and structure | 240,000 (12 × 20,000) | 17 in 10 clusters | P35-P37 (6 raw), D2 (7), the compile-time warning a cache silences (1), noise (T10, T11) |
| 4 | after the round-3 fixes | 360,000 | 5 | P40, T15 |
| 5 | after the performance pass (`fdc54d8`) | 360,000 | 4 | P43, T18, T19 (2) |
| 6 | P43 and T18 fixed (`4ad572c` and the fix) | 360,000 | 1 | T19 again: nothing new |
| 7 | the second performance pass (`dc83b8b`) merged with the harness fixes after round 6 (`080d932`): `1cd373e` | 360,000 | 3 in 3 clusters | P44 (1, older than the pass), T21 (1), T22 (1): nothing from the performance pass |

Items the table does not name came from the fixers' reports and from the rounds' cases that were not comparable.
Every P item up to P43 is fixed and P44 is open (below); D1-D4 are rows of `reference/deviations.md` (D5 was not
needed); of the T items, the gaps below are left.

Round 7, the confirmation round of the second performance pass and the baseline for the next round, with the
escaping and formats families as driven again after T21 and T22 (round 6, the exit round of phase 5: 348,747
identical, 11,246 deviations, 1 mismatch of T19, 6 not comparable):

| family | cases | identical | deviations | round 6 | mismatches | not comparable |
|---|---|---|---|---|---|---|
| expressions | 30000 | 29815 | 184 | 171 | 1 (P44) | 0 |
| callables | 30000 | 28966 | 1027 | 1062 | 0 | 7 |
| control | 30000 | 29866 | 134 | 141 | 0 | 0 |
| composition | 30000 | 29021 | 979 | 1064 | 0 | 0 |
| escaping | 30000 | 29454 | 546 | 566 | 0 | 0 |
| errors | 30000 | 29462 | 538 | 546 | 0 | 0 |
| formats | 30000 | 29729 | 271 | 281 | 0 | 0 |
| sandbox | 30000 | 28686 | 1314 | 1342 | 0 | 0 |
| whitespace | 30000 | 30000 | 0 | 0 | 0 | 0 |
| unicode | 30000 | 28114 | 1886 | 1879 | 0 | 0 |
| dynamic | 30000 | 27817 | 2183 | 2064 | 0 | 0 |
| structure | 30000 | 27858 | 2142 | 2130 | 0 | 0 |
| total | 360000 | 348788 | 11204 | 11246 | 1 | 7 |

- Deviations by rule (round 6 in brackets): `called in` suffixes 7,411 (7,432), `passed in` locations 668 (631), 535
  of them with a closure name (527), closure names 12 more (17), object handles 979 (1,004; expressions 144, callables
  477, control 87, formats 271), and the `not supported by twig-rs` errors of mbstring encodings 2,134 (2,162;
  unicode 1,886, escaping 218, callables 30). The rules `dump-string-length`, `silenced-compile-warning` and
  `memory-limit` never fired, no case was unsupported, and no case ran a second time.
- Beyond 5% of round 6: expressions (184 against 171), composition (979 against 1,064), dynamic (2,183 against
  2,064), control at 5% (134 against 141). Their generators did not change, and three of round 6's batches, driven
  with the round-7 tree, give round 6's counts again (composition 16001: 88 deviations; expressions 15001: 2 `called
  in` and 14 object handles; control 15001: 3 and 8): the seeds explain the differences (round 6's composition seeds
  gave 61 to 88 deviations a batch, round 7's 56 to 80). The sandbox family's generation changed (`NAN` elements,
  below) and stays within 5%.
- Not comparable: 7 callables cases of D3 (`Cannot use positional argument after argument unpacking`).
- Reach: every kind of sandbox security error (tag 4,097, filter 2,990, property 2,688, method 2,506, function 621,
  test 10); a `NAN` in the data of 2,896 sandbox cases, of which 290 raised the walk's `unexpected NAN value was
  coerced to bool` (624 warnings, 192 of the cases under a global sandbox), all identical (P43); `enum_cases()` in 534
  callables cases and 2,140 calls of `include()`/`include_only()` (T19); lexer options of the case's own in 4,413
  whitespace cases; a `DatePeriod` in 381 formats cases. Of what the performance pass changed: `@` timestamps in 556
  formats cases (its `@<int>` parse without the scanner) and `PHP_INT_MAX` in 3,149 expressions cases (the overflow
  of its fused int arithmetic).

## Harness fixes after round 6

Before round 7 the harness closed T19, T20, the gap P43 left in the generators and the false timeouts
(`git log -- crates/twig-fuzz`):

- T19. The callables generator sorts what may hold enum cases (a call of `enum_cases()`, an enum's `.cases`) with an
  arrow: `asort()` orders distinct cases by heap address (row "Enum ordering in `SORT_REGULAR`"), and also in a PHP
  process of its own the oracle gives other orders than twig-rs's stand-in (`[en, ue]|sort|join` and
  `en.cases|sort|json_encode` both differed when checked). A string template that prints `_self` gets no name or a
  name that is valid UTF-8, and `_self` (from `special()`) never reaches the arguments of `include()` or
  `include_only()`. The sandbox family's sort without an arrow (`coerce()`) takes one when the data holds an enum
  variable, which is what lets an expression hold two distinct cases. `min()`/`max()`, `<=>` in arrows, `in` and `==`
  compare enum cases alike on both sides (checked with the oracle in a shared process): `asort()` is the only order
  by address the generators reach.
- T20. Oracle containers are named `twig-fuzz-<pid>-<random>-<n>`, so a process that gets an earlier one's id does
  not meet the containers it left. A name that is taken all the same is given up once for the next one, after
  `docker rm` (not `-f`) of the container of that name. The client removes the containers it gives up (no progress,
  an error, a panic that unwinds, the client dropped) with `docker rm -f`, which also removes a container that was
  created but never started (`--rm` does not); a `docker run` that announces nothing for 5 minutes is an error
  instead of a hang. A client killed outright still cannot remove a container that was created and never started.
- P43. The sandbox family puts `NAN`, `INF` and `-INF` into its arrays (12% of the elements that are not objects;
  also the elements of its `Traversable` objects, a `stdClass` property, the leaf of `nest` and `iv`), so the walk of
  wrapped arrays and PHP 8.5's `unexpected NAN value was coerced to bool` are generated, sandboxed or not.
- Timeouts near the limit: the second run of a case that timed out where PHP finished it (see "Running a round").
  That the probe takes twig-rs 20 times longer with the SandboxExtension than without is the walk of the growing
  `_context`, which PHP does too and needed about 2 minutes for: neither a parity question nor a harness one.

How the changes affect generation: the expressions, control, composition, escaping, errors, formats, whitespace,
unicode, dynamic and structure families generate what they did. A callables case changes only where it reaches a
changed construct (5 of the 4,000 cases of seeds 15003 and 16002: a `{{ _self }}` template's name, a `sort` of
`enum_cases()`, and the draws after them). A sandbox case changes whenever its data has an element that is not an
object, which takes one more draw (1,550 of the 2,000 cases of seed 17008), so the sandbox family's counts move
against round 6's for that reason; about 9% of its cases hold a NAN, and about 1% raise the walk's warning (seed
25013: 186 and 16 of 2,000 cases, 26 warnings, 13 of the cases under a global sandbox).

The smoke round checked the fixes on their tree: one fresh seed of 500 cases per family (seeds 25001-25012 in the
order of the table) plus 2,000 sandbox cases (seed 25013), each through `generate`, `drive` (minimisation on,
`--timeout-ms 15000`, release build with `glibc-iconv`) and `diff` (twig-rs run again). No harness error, no second
run, no container left, and `twig-fuzz replay` of the 4,795 vectors passed:

| family | seed | cases | identical | deviations | mismatches | not comparable |
|---|---|---|---|---|---|---|
| expressions | 25001 | 500 | 496 | 4 | 0 | 0 |
| callables | 25002 | 500 | 475 | 25 | 0 | 0 |
| control | 25003 | 500 | 497 | 3 | 0 | 0 |
| composition | 25004 | 500 | 484 | 16 | 0 | 0 |
| escaping | 25005 | 500 | 493 | 7 | 0 | 0 |
| errors | 25006 | 500 | 491 | 9 | 0 | 0 |
| formats | 25007 | 500 | 496 | 4 | 0 | 0 |
| sandbox | 25008 | 500 | 478 | 22 | 0 | 0 |
| whitespace | 25009 | 500 | 500 | 0 | 0 | 0 |
| unicode | 25010 | 500 | 471 | 29 | 0 | 0 |
| dynamic | 25011 | 500 | 467 | 33 | 0 | 0 |
| structure | 25012 | 500 | 469 | 31 | 0 | 0 |
| sandbox | 25013 | 2000 | 1911 | 89 | 0 | 0 |
| total | | 8000 | 7728 | 272 | 0 | 0 |

- Deviations by rule: `called in` suffixes 201, `passed-in` locations 18 (12 of them with a closure name), object
  handles 20 (expressions, callables, formats), mbstring encodings `not supported by twig-rs` 33 (unicode 29,
  escaping 4).
- The sandbox seed 25008 had 58 cases with a NAN and 9 with the walk's warning (17 warnings); the callables seed
  25002 reached `enum_cases()` in 9 cases and `include()`/`include_only()` 32 times.

## Round 7

The confirmation round of the second performance pass ran on `1cd373e`: the pass (`dc83b8b`) merged with the harness
fixes above (`080d932`). Seeds 19001-22015, 360,000 cases, 3 raw mismatches in 3 clusters. Every minimised case also
ran against the tree before the pass (`a258816`, release build with `glibc-iconv`): none of them comes from the pass.

- **P44. The `odd` and `even` tests inside array literals report an earlier line than PHP** (parity, G1; older than
  the pass, whose base reports the same lines). Minimised from `x19005-00668-logic`: `{{dump(([]??\ns)is\nodd)}}`
  makes PHP report `Unsupported operand types: array % int` at line 2, twig-rs at line 1.
  - Twig compiles the tests inline (`OddTest`, `EvenTest`: `(node % 2 != 0)`, `(node % 2 == 0)`). PHP's `ZEND_MOD`
    carries `CG(zend_lineno)` as its last compiled operand left it: the literal `2`, which Twig writes right after the
    node, after the last debug marker inside it (the `// line 2` of the name `s`).
  - twig-rs has the line right outside literals. Inside an array or hash literal (also the variadic arguments of
    `dump()`, which Twig passes as `...[...]`) it reports the line of the literal or of the node's first token. The
    `%` operator written out, `divisible by`, `same as` and `is null` in the same places match.
  - Cause: `Codegen::test()` (`src/lltl/codegen.rs`, `TestKind::Even | TestKind::Odd`) emits the `2` without
    `Codegen::leaf()`, so the `Mod` takes the `php_marker` the node's compilation left, which inside a literal need not
    be the marker in effect. An uncommitted experiment with `self.leaf()` after `emit_const(Value::Int(2))` made every
    P44 vector match and kept the other 4,795 vectors passing.
  - Vectors: `tests/vectors/fuzz/expressions/round7-odd-even-literal-lines.json`: 12 marked (the minimised case,
    `dump()` arguments, list and hash literals, `even`, a name two lines down, a constant conditional, a name or a
    concatenation left of the name, a later element, `not`) and 9 controls.
- **T21. A template that comes back in a batch sharing one PHP process loses its compile-time deprecations**
  (harness). `esc-20003-1154`, `{{ v|spaceless }}`, came after `esc-20003-270` with the same template. Twig names
  the template class after the template's name and source, the extensions, `debug`, `strict_variables`, `use_yield`,
  the optimizations, the default escaping strategy and the sandbox policy, not after the cache directory, so PHP
  reused the class of the earlier case and did not raise the deprecation of `spaceless` again; twig-rs raised it.
  - The escaping family writes short templates that come back (`{{ v }}` 107 times within round 7's escaping
    batches), so its batches now run a PHP process per case (`escaping::batch_with()`). Its 15 batches were driven
    again: no mismatch, and of the 30,000 oracle results only `esc-20003-1154`'s changed (it has the deprecation now).
  - The other families whose batches share a process (expressions, callables with its id comment, control, errors,
    formats) repeated no template that raises a deprecation of its own when it is compiled.
- **T22. Milliseconds and microseconds of "now" in the formats family** (harness). `formats-21006-512` printed
  `{{ v1|first|date('yu\\PT-mw') }}` of a `DatePeriod` whose end is its start: the period is empty, so `first` and
  `last` are `null`, and `null|date` is "now", whose microseconds cannot match (PHP reads the clock at each call,
  twig-rs uses the time the oracle started rendering).
  - `first` and `last` of a period now get a format without `u` and `v`. The formats of dates of "now" also no longer
    name the `RFC3339_EXTENDED` constants (`v`, milliseconds), which become `RFC3339`: of 300 probe cases that printed
    "now" with `DATE_RFC3339_EXTENDED`, 5 differed.
  - 19 of the round's 30,000 formats cases changed; their 15 batches were driven again without a mismatch.
- The run: the 180 drives in 16 minutes, the 30 batches of T21 and T22 again in 7. Docker Desktop's engine faltered
  during the second run: one drive stopped with `502 Bad Gateway` and was run again, and five escaping drives needed
  three containers each, which the drive's log now reports as oracle incidents. No container was left, and `twig-fuzz
  replay` passed before and after the round (4,795 vectors, then 4,816).

## Vectors that stay marked

P44's 12 vectors (`expressions/round7-odd-even-literal-lines.json`, above) stay marked until it is fixed. Besides them
the only marked vectors are D2's 9 (`reference/deviations.md`, "Lone CR bytes in the generated PHP"), marked by design
because the diff cannot accept a different line. Under `tests/vectors/fuzz/`: `escaping/round2-cr-escape-lines.json`
(4, with 8 controls), `escaping/round3-d2-cr-escape-lines.json` (`\xd`: 2, minimised from `esc-9001-1578` and
`esc-9007-1471`, 2 controls), `errors/round3-d2-debug-cr-lines.json` (`debug` and an embed: 1, from
`errors-9006-1784-emb.div.cr`, 2 controls), `errors/known-mismatches.json` (`cr-debug-embed-line`) and
`whitespace/d2-cr-escape-lines.json` (`ws-d2-line-after-raw-cr-and-cr-escape`).

- The generators leave D2's inputs out unless `twig-fuzz generate --d2` asks for them (CR escapes in the literals of
  the escaping, callables and formats families, lone CRs with `debug` in the errors family). Rounds 4 to 7, run
  without it, had no D2 case.
- Raw CR bytes of a template are normalized before compiling and shift nothing (controls `round2-cr-control-raw-*`).
- Round 2's raw case `esc-7001-346` gets its CR from `trim(" \t\n\r\0\x0B")` (PHP's default trim list), and its
  error line moves from 8 to 9. Emulating that needs the line layout of the generated PHP (how many LFs Twig writes
  between a call and the next `// line N`), which twig-rs does not model. Revisit if real templates report it.

## Open harness gaps

- **Faults inside macros (errors family).** A macro body does not see the caller's context, so the faults that name
  a data key literally (`min(empty)`, ...) test `null` there: deterministic, but not the error they were written for.
  `random()`, which returns an unseeded number for `null`, writes the literal `[]` in such scopes since T18
  (`Scope::surely()`, `Scope::maybe_null` in `crates/twig-fuzz/src/generators/errors.rs`).
- **Fatal errors of the generated PHP (D3)** cannot be compared: PHP stops, the oracle reports a `FatalError`, and
  the case is not comparable (rounds 4 to 7: 6, 8, 5 and 7 callables cases). `tests/engine.rs` (topic
  `compile_errors`) checks the message of each one.
- **T8. Typed data and environments the harness cannot express.** What the generators leave out because the harness
  cannot rebuild or set it up. Checked again after round 6 (with the host API of `e55f314`): none of these is a gap
  twig-fuzz can close alone; each needs PHP code to run, a class port in twig-test-support, support in the oracle's
  runner (`research/php-oracle`) or an engine API, as noted:
  - closures and resources (PHP code, process state), and TemplateWrappers of another environment (the typed tree
    of `lib.php` records only the template name, and twig-rs now checks a wrapper against its environment, so the
    tree would have to record which environment made it);
  - `SimpleXMLElement` writes, `xpath()`, `addChild()`/`addAttribute()` and namespaces (twig-test-support throws
    `... is not supported by twig-test-support`, which the diff reports as not comparable), and objects the data
    derives from one document (the typed tree records each object's `asXML()`, and the factory rebuilds it as the
    root of a document of its own, so `$sxe->child == $sxe` would differ);
  - an `ArrayAccess` class outside `ARRAY_LIKE_CLASSES`, which needs a class port in twig-test-support first (so the
    sandbox's property check of array keys is out of reach);
  - a host object whose `count()` or iteration throws (a class port in twig-test-support), so the loop and `with`
    bookkeeping of P36 has no vector for it;
  - `render_sandboxed()` and `Twig\Sandbox\Sandbox`: the second environment is easy to build on both sides, but PHP's
    error handler records the diagnostics of both environments in one sequence, and twig-rs collects them per
    environment (`Environment::take_notices()`; twig-rs's `Sandbox` has an environment of its own), so their order
    across the two cannot be compared without an API that shares one diagnostics sink between environments; the
    oracle's plain environment does not register the `SandboxBridgeExtension` either;
  - custom policy classes (PHP code);
  - `config_php` and a callable `autoescape` (PHP code), a `FilesystemLoader` (lazy source paths; the runner takes
    templates only as an `ArrayLoader`'s), and the integration environment (the twig-rs runner implements the plain
    one).
