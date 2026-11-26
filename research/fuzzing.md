# Differential fuzzing

How twig-rs is checked against the PHP oracle with generated templates: how a round runs, how its clusters are
triaged, what the six rounds of roadmap phase 5 found, and what the harness still cannot reach. The tool is documented
in its crate (`crates/twig-fuzz`: `cargo doc -p twig-fuzz`, `twig-fuzz help`), the regression vectors in
`tests/vectors/fuzz/README.md`, and the differences the diff accepts in `reference/deviations.md`.

## Running a round

- A round covers the 12 families of `twig-fuzz generate`: expressions, callables, control, composition, escaping,
  errors, formats, sandbox, whitespace, unicode, dynamic and structure. Since round 4 every family gets 15 seeds of
  2,000 cases (30,000 per family, 360,000 in all): `twig-fuzz generate <family> --seed N --count 2000 --out
  <batch.json>`, then `twig-fuzz drive <batch.json> --out <dir> --timeout-ms 15000`, with minimisation on and without
  `--d2`, on a release build with `--features glibc-iconv` (the unicode family converts glibc's other charsets too).
- Seeds used so far: 7001-7005 (round 2), 9001-9010 (round 3), 9201-9215 (round 4), 11001-14015 (round 5) and
  15001-18015 (round 6). Rounds 5 and 6 gave each group of three families a block of its own: 11001-11015 for
  expressions, callables and control, 12001-12015 for composition, escaping and errors, 13001-13015 for formats,
  sandbox and whitespace, 14001-14015 for unicode, dynamic and structure, and the same layout 4000 higher in round 6.
  A new round takes seeds nobody used.
- The four groups can run in parallel on one machine (round 6: up to 9 oracle containers at once). A drive of 2,000
  cases takes 15 to 150 s.
- Before and after a round: `cargo +stable test --workspace --all-features --test fuzz_regressions` and `twig-fuzz
  replay tests/vectors/fuzz crates/twig-fuzz/vectors` (every unmarked vector replays identically, every marked one
  still differs).
- Count from the 180 drive reports (`report.json`): identical, deviations per rule, mismatches, clusters, not
  comparable, unsupported. Record the tree (the commit, plus `git diff | sha256sum` while the change is not
  committed). Compare the deviations per family and rule with the last round (below): a change beyond about 5% means
  that a generator, the diff or the engine changed what the cases reach, unless the reach of the data explains it.
- To check an engine change in a setting the generators rarely vary, drive the counted batches again with that
  setting changed. Round 6 re-drove the 86,934 expressions, callables and control cases that registered no
  SandboxExtension with `"sandbox"` added to `extensions`: 182 cases had warnings only P43's array walk raises, and
  all of them matched.

## Triage

- Check each cluster's minimised reproduction with the oracle in a fresh PHP process (`"isolation": "process"`).
  Batches without it share one process, where distinct enum cases sort by heap address and a template compiled by an
  earlier case is reused without its compile-time deprecations.
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
  vector descriptions cite its item numbers, so a new list continues them at P44, D6 and T21.
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

Items the table does not name came from the fixers' reports and from the rounds' cases that were not comparable.
Every P item is fixed; D1-D4 are rows of `reference/deviations.md` (D5 was not needed); of the T items, the gaps below
are left.

Round 6, the exit round of phase 5 and the baseline for the next round:

| family | cases | identical | deviations | mismatches | not comparable |
|---|---|---|---|---|---|
| expressions | 30000 | 29829 | 171 | 0 | 0 |
| callables | 30000 | 28931 | 1062 | 1 (T19) | 6 |
| control | 30000 | 29859 | 141 | 0 | 0 |
| composition | 30000 | 28936 | 1064 | 0 | 0 |
| escaping | 30000 | 29434 | 566 | 0 | 0 |
| errors | 30000 | 29454 | 546 | 0 | 0 |
| formats | 30000 | 29719 | 281 | 0 | 0 |
| sandbox | 30000 | 28658 | 1342 | 0 | 0 |
| whitespace | 30000 | 30000 | 0 | 0 | 0 |
| unicode | 30000 | 28121 | 1879 | 0 | 0 |
| dynamic | 30000 | 27936 | 2064 | 0 | 0 |
| structure | 30000 | 27870 | 2130 | 0 | 0 |
| total | 360000 | 348747 | 11246 | 1 | 6 |

- Deviations by rule: `called in` suffixes 7,432, `passed in` locations 631 (527 of them with a closure name),
  closure names 17 more, object handles 1,004 (expressions, callables, control, formats), and the `not supported by
  twig-rs` errors of mbstring encodings 2,162 (unicode 1,879, escaping 243, callables 40; never an iconv charset or
  a static method call). The rules `dump-string-length`, `silenced-compile-warning` and `memory-limit` never fired,
  and no case was unsupported. Every family is within 5% of round 5 except formats (281 against 317, all object
  handles; its data held a `DatePeriod` in 384 cases against 421).
- Not comparable: 5 callables cases of D3 (`Cannot use positional argument after argument unpacking`) and
  `c15010-00326m`, the endless self-include of T19.
- Reach: every kind of sandbox security error (tag 4,080, filter 2,891, property 2,839, method 2,460, function 681,
  test 9), lexer options of the case's own in 4,380 whitespace cases, a `DatePeriod` in 384 formats cases, 9,510
  charset arguments of `convert_encoding` in the unicode family (glibc's included: BIG5-HKSCS 246, CP866 243,
  Windows-1252 232).

## Vectors that stay marked

The only marked vectors are D2's 9 (`reference/deviations.md`, "Lone CR bytes in the generated PHP"), marked by design
because the diff cannot accept a different line. Under `tests/vectors/fuzz/`: `escaping/round2-cr-escape-lines.json`
(4, with 8 controls), `escaping/round3-d2-cr-escape-lines.json` (`\xd`: 2, minimised from `esc-9001-1578` and
`esc-9007-1471`, 2 controls), `errors/round3-d2-debug-cr-lines.json` (`debug` and an embed: 1, from
`errors-9006-1784-emb.div.cr`, 2 controls), `errors/known-mismatches.json` (`cr-debug-embed-line`) and
`whitespace/d2-cr-escape-lines.json` (`ws-d2-line-after-raw-cr-and-cr-escape`).

- The generators leave D2's inputs out unless `twig-fuzz generate --d2` asks for them (CR escapes in the literals of
  the escaping, callables and formats families, lone CRs with `debug` in the errors family). Rounds 4 to 6, run
  without it, had no D2 case.
- Raw CR bytes of a template are normalized before compiling and shift nothing (controls `round2-cr-control-raw-*`).
- Round 2's raw case `esc-7001-346` gets its CR from `trim(" \t\n\r\0\x0B")` (PHP's default trim list), and its
  error line moves from 8 to 9. Emulating that needs the line layout of the generated PHP (how many LFs Twig writes
  between a call and the next `// line N`), which twig-rs does not model. Revisit if real templates report it.

## Open harness gaps

- **T19. The callables generator reaches accepted deviations the diff cannot accept.** Each was checked with the
  oracle; none has a vector, since the deviations are accepted.
  - `sort` of enum cases without an arrow (round 5, `c11002-01066v`:
    `{{ dump(enum_cases('Twig\\Tests\\DummyBackedEnum')|sort) }}`): PHP orders distinct cases by heap address (row
    "Enum ordering in `SORT_REGULAR`"). Alone in a fresh process the oracle prints BAR before FOO, as twig-rs does;
    in the shared process of the batch it printed FOO before BAR. Fix: keep enum cases away from `sort` without an
    arrow and from the other `SORT_REGULAR` orderings of distinct cases.
  - `_self` in a string template whose name is not valid UTF-8 (round 5, `c11007-00260m`; round 6,
    `c15013-00123v`, minimised to `{{include(template_from_string('{{_self}}',"\xe9"))}}` with `autoescape` off):
    twig-rs prints its lossy name (row "Non-UTF-8 template names given to `template_from_string()`"). PHP prints
    the raw bytes, which `htmlspecialchars()` turns into one U+FFFD per sequence `php_next_utf8_char()` rejects
    (`ED A0 80` gives one where the lossy name has three). The diff accepts lossy names in messages and
    `templateName`, not in output. Fix: give names that are not valid UTF-8 only to string templates that do not
    print `_self`.
  - `_self` as the template of `include()` or `include_only()` (round 6, `c15010-00326m`:
    `{{ include_only(_self, {'v': '<b>'})|json_encode }}`): the template includes itself until PHP dies at its 512M
    memory limit, while twig-rs stops at its depth limit (row "Recursion depth"), so the case is not comparable.
    `special()` returns `"_self"` (`crates/twig-fuzz/src/generators/callables.rs:3516`), and `any()` (`:3483`)
    offers it for any argument. Fix: keep `special()`'s `_self` out of the template argument of `include` and
    `include_only`.
- **T20. Oracle container names collide with stale containers.** `Oracle::spawn()`
  (`crates/twig-fuzz/src/oracle.rs:454`) names every container `twig-fuzz-<pid>-<n>`, `n` counting from 1 in each
  process. A stale container (one in state `Created`) keeps its name, so a later twig-fuzz process that gets the same
  PID (Windows reuses PIDs quickly) cannot start its first container: Docker exits with status 125 (the name is in
  use), which the oracle module treats as an error, and the drive stops. Round 5: the first drive of control seed
  11008 (`twig-fuzz-20628-1`, still there after round 6). Fix: add a random or time-based part to the name, or on that
  conflict remove the container of that name if it is not running and try once more.
- **The SecurityChecker walk (P43).** With the SandboxExtension registered, twig-rs walks every wrapped array as PHP
  does, sandboxed or not, and PHP 8.5 warns `unexpected NAN value was coerced to bool` for each NAN element. The
  families reach that rarely or not at all: the sandbox family puts no float into its data
  (`crates/twig-fuzz/src/generators/sandbox.rs`), unicode and dynamic never register the extension, structure
  registers it only sandboxed globally, and in round 6 none of the expressions, composition and errors cases that
  register it (1,830 of them in expressions with nothing sandboxed) had a NAN in a walked array. The walk is covered
  by the vectors of `tests/vectors/fuzz/expressions/round5-sandbox-nan-bool.json` and
  `tests/vectors/fuzz/sandbox/round6-p43-sandbox-walk.json` (22 hand-picked probes in the sandbox family's settings),
  by `tests/sandbox_walk.rs` and by the re-driven batches of round 6. NAN elements in the sandbox family's arrays
  would close the gap.
- **Timeouts near the limit.** Round 6's probe `ctl-15014-925-sbx` (`{% set str = [[false, loop], ...] %}` inside
  nested loops, printed through `_context|keys`: the walked `_context` grows exponentially) takes twig-rs about 14 s
  with the SandboxExtension registered (0.7 s without) against the drive's 15 s, and PHP about 2 minutes
  (`"time_limit": 600`); the output is identical. The counted control batches never register the extension, but a
  probe like this on a slower machine would report a false timeout.
- **Faults inside macros (errors family).** A macro body does not see the caller's context, so the faults that name
  a data key literally (`min(empty)`, ...) test `null` there: deterministic, but not the error they were written for.
  `random()`, which returns an unseeded number for `null`, writes the literal `[]` in such scopes since T18
  (`Scope::surely()`, `Scope::maybe_null` in `crates/twig-fuzz/src/generators/errors.rs`).
- **Fatal errors of the generated PHP (D3)** cannot be compared: PHP stops, the oracle reports a `FatalError`, and
  the case is not comparable (rounds 4 to 6: 6, 8 and 5 callables cases). `tests/engine.rs` (topic
  `compile_errors`) checks the message of each one.
- **T8. Typed data and environments the harness cannot express.** What the generators leave out because the harness
  cannot rebuild or set it up:
  - closures and resources (PHP code, process state), and TemplateWrappers of another environment (the typed tree
    records only the template name);
  - `SimpleXMLElement` writes, `xpath()`, `addChild()`/`addAttribute()` and namespaces (twig-test-support throws
    `... is not supported by twig-test-support`, which the diff reports as not comparable), and objects the data
    derives from one document (the typed tree records each object's `asXML()`, and the factory rebuilds it as the
    root of a document of its own, so `$sxe->child == $sxe` would differ);
  - an `ArrayAccess` class outside `ARRAY_LIKE_CLASSES`, which needs a class port in twig-test-support first (so the
    sandbox's property check of array keys is out of reach);
  - a host object whose `count()` or iteration throws, so the loop and `with` bookkeeping of P36 has no vector for it;
  - `render_sandboxed()` and `Twig\Sandbox\Sandbox`: the second environment is easy to build on both sides, but PHP's
    error handler records the diagnostics of both environments in one sequence, and twig-rs collects them per
    environment (`Environment::take_notices()`), so their order across the two cannot be compared without an API
    that shares one diagnostics sink between environments;
  - custom policy classes (PHP code);
  - `config_php`, a callable `autoescape`, a `FilesystemLoader` (lazy source paths), and the integration environment.
