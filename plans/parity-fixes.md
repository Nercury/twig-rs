# Parity fixes

The distinct root causes behind the mismatches that differential fuzzing (`crates/twig-fuzz`) and the PHPUnit ports
(`tests/upstream*.rs`) still find. Each item is meant to be worked on alone: it lists what it covers, a minimal
reproduction, the upstream behaviour with citations, where twig-rs goes wrong and how to check the fix. Finished items
are removed from this file, as in `roadmap.md`; item numbers are never reused.

`TWIG` and `PHPSRC` are the source trees named in `reference/target.md`. Templates are written as JSON strings (`\n`
is a newline). "PHP" is the oracle's result (`twig-oracle:3.31.1`), "twig-rs" the current one. PHP's `, called in
<file> on line N` suffixes are left out (accepted deviation).

## Status

- **Round 1** (the eight fuzz families, the round-0 tooling run and the PHPUnit ports) produced P1 to P30, the
  deviation candidates D1 to D5 and the tooling items T1 to T9. The fix round (groups G1 to G6) was verified on
  2026-11-26:
  - P1 to P15, P17 to P23 and P25 to P30 are fixed and removed. P16 and P24 are partly fixed; their open parts are
    below.
  - 194 `known_mismatch` markers were removed because their vectors now match:
    - 190 under `tests/vectors/fuzz/` and in `crates/twig-fuzz/vectors/round0-mismatches.json`;
    - `formats-interval-days-property`, after `twig-fuzz refresh` recorded its oracle result again with `props`;
    - the 3 `opt-env-m16-sbx-*` vectors of `tests/vectors/visitors/optimizer_mode_env.json`.
  - 8 of the 11 ignored PHPUnit ports pass and are no longer ignored. Still ignored: P24's lazy-path test and D1's two
    tests.
  - D1 to D4 are listed in `reference/deviations.md`. D5 was not needed, because P8 reproduces Twig's `$_vN`
    numbering.
- **Round 2** (2026-11-26, the tree after the fix round): 10,000 generated cases per family, through `twig-fuzz drive`
  with minimisation. Seeds 7001 to 7005, 2000 cases each.

  | family | cases | identical | deviations | mismatches | clusters | not comparable |
  |---|---|---|---|---|---|---|
  | expressions | 10000 | 9954 | 45 | 1 | 1 (P31) | 0 |
  | callables | 10000 | 9630 | 367 | 1 | 1 (P32) | 2 |
  | control | 10000 | 9960 | 40 | 0 | 0 | 0 |
  | composition | 10000 | 9574 | 364 | 0 | 0 | 62 |
  | escaping | 10000 | 9787 | 211 | 2 | 1 (D2) | 0 |
  | errors | 10000 | 9842 | 158 | 0 | 0 | 0 |
  | formats | 10000 | 9924 | 75 | 0 | 0 | 1 |
  | sandbox | 10000 | 9504 | 480 | 0 | 0 | 16 |
  | total | 80000 | 78175 | 1740 | 4 | 3 | 81 |

  - The deviations are the accepted ones the diff knows: `called in` and `passed in` suffixes, closure names, object
    handles, and the `not supported by twig-rs` errors of mbstring encodings.
  - Of the three clusters, two are new bugs (P31, P32). The escaping one is accepted deviation D2 in a form round 1 did
    not show: no `debug` is needed.
  - P33 records an open part of P14 that G5 found while fixing it.
  - The not-comparable cases are harness limits (see the G6 notes at the end).
  - Every cluster has a minimised vector plus oracle-recorded variants in `tests/vectors/fuzz/<family>/round2-*.json`:
    27 marked `known_mismatch`, and 20 controls that match and must keep matching.

## Working rules

- Port the upstream code path named in the item and cite it in doc comments (`reference/conventions.md`). Messages
  are verbatim.
- An item is done when its vectors replay identically and its tests pass. Then remove the `known_mismatch` markers of
  exactly those vector ids (`cargo test --test fuzz_regressions` fails while a fixed vector is still marked) and the
  `#[ignore]` of its tests. `twig-fuzz replay --update <files> [--id ID]...` removes the markers of the vectors that
  match now. Never edit an `oracle` field: re-run the case with `twig-fuzz refresh` or `twig-fuzz oracle` instead.
- Vector files hold items of several groups. Edit only the markers of your item's ids. The controls in the `round2-*`
  files are unmarked on purpose: a fix must not break them.
- Check fixes with `twig-fuzz replay <file>`, and probe variants with `twig-fuzz oracle <batch> --out <o.json>`
  followed by `twig-fuzz diff <batch> --oracle <o.json>`. Add oracle-backed vectors for the "also check" cases.
- `tests/fuzz_regressions.rs` also replays `crates/twig-fuzz/vectors/round0-mismatches.json`.

Groups: **G1** codegen + VM (`src/lltl`, `src/vm`), **G2** runtime (`src/runtime`, `src/escaper`), **G3** callables
(`src/extension`, including the runtime groups), **G4** syntax (`src/lexer`, `src/parser`, `src/token_parser`,
`src/visitor`, `src/ast`), **G5** library crates (`crates/twig-value`, `twig-date`, `twig-pcre`, `twig-unicode`,
`twig-timelib`, `twig-test-support`), **G6** fuzz tooling (`crates/twig-fuzz`, `research/php-oracle`). When an item
needs changes in several groups, the first group listed leads.

## Summary

"Raw" counts the generated cases attributed to the cause; "+n probes" counts hand-written probe cases. Priority:
**high** = wrong output, or more than 100 raw cases; **medium** = a wrong error or diagnostic that real templates can
hit; **low** = obscure error paths.

| id | root cause | groups | raw | vectors | tests | priority |
|---|---|---|---|---|---|---|
| P16 | **PARTLY FIXED** Errors of generated PHP that fails to compile are wrapped by `compile_source_error()` | G4 | 14 +22 n/c | 2 | – | low |
| P24 | **PARTLY FIXED** Source context of compiled templates: the path is not resolved lazily | G2 | 3 | – | 1 | medium |
| P31 | Constant operands inside array literals: conditionals that `zend_try_ct_eval_array()` folds | G1 | 1 +18 probes | 14 | – | low |
| P32 | PHP compile errors of the generated class are raised before Twig's own compile errors | G1, G4 | 1 +14 probes | 7 | – | low |
| P33 | Method calls through `getAttribute()` unpack without the method's parameters | G2, G5 | 5 probes | 2 | – | low |

---

## G1: codegen and VM

### P31. Constant operands inside array literals: conditionals that `zend_try_ct_eval_array()` folds

- **Groups**: G1.
- **Covers** (round 2: 1 raw of 10,000 expressions cases; 14 vectors), all in
  `tests/vectors/fuzz/expressions/round2-array-literal-folding.json`:
  - `round2-fold-dump-coalesce-stringy`, minimised from `x7002-01328-arith`;
  - `round2-fold-sequence-coalesce`, `-sequence-ternary`, `-sequence-elvis`, `-sequence-null-coalesce`,
    `-sequence-band`, `-sequence-bor-float`, `-array-argument`, `-variadic-argument`, `-mapping-value`,
    `-method-argument`, `-macro-argument`, `-concat-cast-right`, `-concat-cast-nan-order`;
  - controls, which must keep matching: `round2-fold-control-print-coalesce`, `-print-ternary`, `-defined-test`,
    `-concat-cast-left`, `-concat-print`.
- **Repro**: `"{{ [(false ?? '') * a]|length }}"`, data `{"a": [1]}`. PHP: `Unsupported operand types: array * bool`.
  twig-rs: `bool * array`. Without the array literal, `"{{ (false ?? '') * a }}"` gives `bool * array` in both.
  Variants:
  - `"{{ [(true ? 1.5 : 0) b-or a]|length }}"`: PHP throws `array | float`; twig-rs first deprecates `Implicit
    conversion from float 1.5 to int loses precision`.
  - `"{{ [o ~ (true ? [] : 1)]|length }}"` (o = stdClass): PHP warns `Array to string conversion` before the object
    error; twig-rs does not.
  - `"{{ [nan ~ ([1] ?? 2)]|length }}"`: PHP warns `Array to string conversion`, then `unexpected NAN value was coerced
    to string`; twig-rs reverses them.
  - The minimised fuzz case is `"{{dump((false??'')*str7)}}"`, because `dump()`'s arguments compile to `...[...]`.
- **Upstream**:
  - `zend_compile_array()` first calls `zend_try_ct_eval_array()` (PHPSRC/Zend/zend_compile.c:10095). That runs
    `zend_eval_const_expr()` on every element before the element is compiled (P1's rule 4).
  - `zend_eval_const_expr()` folds a `ZEND_AST_CONDITIONAL` whose condition is constant into the chosen branch
    (:12198).
  - Twig compiles `a ?? b` (`NullCoalesceBinary`), `a ?: b` (`ElvisBinary`) and `a ? b : c` (`ConditionalTernary`) to
    `((test) ? (left) : (right))`. With a constant left operand the test is constant (`(true && !(null === false))`;
    the defined test of a constant compiles to `true`). So inside an array literal the conditional becomes the
    constant `false`, an `IS_CONST` operand.
  - That changes what follows:
    - P2's swap of `ZEND_MUL`/`BW_AND`/`BW_OR`/`BW_XOR` applies (`zend_vm_set_opcode_handler()`, `SPEC(COMMUTATIVE)`);
    - P3's `ZEND_CAST` of a constant array operand of `~` applies (`zend_compile_binary_op()`).
  - Outside an array literal, `zend_compile_conditional()` (:10515) does not fold: the result is a TMP (`JMPZ` +
    `QM_ASSIGN`).
  - Every PHP array literal folds its elements: sequence and mapping literals, method-call and macro-call arguments,
    variadic argument arrays, arrays passed to a call. `zend_eval_const_expr()` does not descend into calls.
- **twig-rs**: `PhpModel::operand_kind()` (`src/lltl/php.rs` ~408) and the P3 cast in `binary()`
  (`src/lltl/codegen.rs`) decide `IS_CONST` with `compile_const()` (`Mode::Compile`). `fold()` already folds
  conditionals in `Mode::Eval`.
- **Fix notes**: inside the outermost array literal of the current call-free subtree (the context P1 already tracks
  for `php_marker`), decide `IS_CONST` with `eval_const()`. Check the other users of `compile_const()` in codegen for
  the same context.

### P32. PHP compile errors of the generated class are raised before Twig's own compile errors

- **Groups**: G1 (`src/lltl/codegen.rs`, `src/lltl/php.rs`), G4 (together with P16's unwrapping in
  `src/parser/context.rs`).
- **Covers** (round 2: 1 raw of 10,000 callables cases; 7 vectors), all in
  `tests/vectors/fuzz/callables/round2-php-compile-error-order.json`:
  - `round2-order-attribute-spread-range`, minimised from `c7002-01380m`;
  - `round2-order-attribute-then-range`, `-parse-error-then-range`, `-constant-spread-then-range`,
    `-attribute-then-embedded-range`, `-attribute-then-missing-argument`, `-block-spread-then-range`;
  - controls, which must keep matching: `round2-order-control-range-first`, `-parse-phase-filter`,
    `-parse-phase-macro`, `-parse-phase-block`.
- **Repro**: `"{{ attribute(n, ...[]) }}{{ range() }}"`.
  - PHP: load `SyntaxError: Value for argument "start" is required for function "range" in "index.twig" at line 1.`
  - twig-rs: `SyntaxError: An exception has been thrown during the compilation of a template ("Cannot use positional
    argument after argument unpacking") in "index.twig".`
  - The same happens with `x|default(...[])` (the `ParseError`), `[...true]` and `block(...['b'])` (`CompileError`s),
    and with the `range()` in an embedded template of the same file.
  - An unknown filter, a duplicate block or a duplicate macro argument fails while parsing, so it comes first in both.
- **Upstream**:
  - `Environment::compileSource()` compiles the whole file: the main class and its embedded templates. It raises
    Twig's compile-time `SyntaxError`s as it goes, for example the argument checks of `CallableArgumentsExtractor` in
    `compileArguments()`.
  - PHP sees the generated code only when `Environment::loadTemplate()` evaluates it (`eval()`, or the cache file).
    PHP parses the whole file first, so a `ParseError` anywhere wins over every `E_COMPILE_ERROR` (oracle: `"{{
    attribute(n, ...[]) }}{{ x|default(...[]) }}"` gives the `ParseError`). Then it compiles in file order: the main
    class, then the embedded templates' classes.
- **twig-rs**:
  - `php::generated_parse_error()` runs before codegen.
  - The D3 `CompileError` is raised when codegen reaches the node (`get_attr()`, block references, `array()` in
    `src/lltl/codegen.rs`).
- **Fix notes**:
  - Record the PHP-level error and let codegen finish the whole file. Raise it only when no Twig error came: the
    `ParseError` if there is one, otherwise the first `CompileError` in file order.
  - P16 makes these errors leave `compile_source()` unwrapped; fix both together.

## G2: runtime

### P24. Source context of compiled templates: the path is not resolved lazily

- **Status**: G2 fixed the code part: outside `debug`, a compiled program's source has no code. This covers runtime
  errors, `TemplateWrapper::source_context()` and source policies. `runtime_errors_carry_the_template_code_only_in_debug_mode`
  passes and is no longer ignored, and so does `tests/engine_api.rs::runtime_errors_carry_the_compiled_source`.
- **Groups**: G2 (`src/runtime/environment.rs`), plus the owner of `src/source.rs` (a lazily resolved path in
  `Source`).
- **Covers** (1 test): `tests/upstream/runtime/environment_test.rs::
  test_compiled_templates_keep_their_error_when_the_loader_fails_to_resolve_their_path` (still ignored).
- **Repro**: a loader returns `Source('{{ foo }}', name, '/path/to/'.name)` and throws after the load. PHP's runtime
  error has path `''`; twig-rs keeps `/path/to/failing_loader.twig`.
- **Upstream**:
  - Outside debug mode and for a non-empty path, `ModuleNode::compileGetSourceContext()`
    (TWIG/src/Node/ModuleNode.php:521) emits `Source::createWithLazyPath(..., $this->getSourcePath(...))`.
  - `Template::getSourcePath()` (TWIG/src/Template.php:77) asks the loader the template was loaded with, and caches
    `''` when that loader throws.
- **twig-rs**: the VM attaches `Program::source()` to errors, and that path was resolved when the template compiled.
- **Fix notes**: give `Source` a lazily resolved path. `without_code()` in `src/runtime/environment.rs` then passes a
  resolver over the loader used for the load.
- **Broken test (a wrong expectation, not a twig-rs bug)**: `tests/upstream/visitors/sandbox_sandbox_test.rs::
  security_checker_adds_the_source_to_policy_errors` (line 432) has failed since the code part landed.
  - It asserts the template code on the error of a render without `debug`. PHP gives `''` there: the oracle's
    `Sandbox::render()` gives code `''` and path `''` without debug, and the code with `debug`.
  - Upstream's `testSecurityCheckerAddsTheSourceToPolicyErrors` (TWIG/tests/Sandbox/SandboxTest.php:122) instead
    passes its own `Source` to `SecurityChecker::checkSecurity()`.
  - The port's render variant should assert `b""`, or render with `debug`.

### P33. Method calls through `getAttribute()` unpack without the method's parameters

- **Groups**: G2 (`src/runtime/attribute.rs`), G5 (an optional `Object` hook in `crates/twig-value` that reports a
  method's parameters).
- **Covers** (found by G5 while fixing P14, oracle-checked; 2 vectors), all in
  `tests/vectors/fuzz/callables/round2-method-call-unpack.json`:
  - `round2-unpack-method-unknown-named-then-spread`, `round2-unpack-method-spread-unknown-then-positional`;
  - controls, which must keep matching: `round2-unpack-control-known-named-then-spread`, `-spread-unknown-name`,
    `-spread-known-names`.
- **Repro**: `"{{ foo.bar(zz: 1, ...[2]) }}"`, with `foo = new \Twig\Tests\TwigTestFoo()` (`test_classes`).
  - PHP: `Unknown named parameter $zz`.
  - twig-rs: `Cannot use positional argument after named argument during unpacking`.
  - `foo.bar(...{zz: 1, 0: 2})` differs the same way.
  - `foo.bar(param1: 1, ...[2])` and `foo.bar(...{zz: 1})` already match.
- **Upstream**:
  - `CoreExtension::getAttribute()` calls `$object->$method(...$arguments)`.
  - `ZEND_SEND_UNPACK` (PHPSRC/Zend/zend_vm_def.h:5267) checks each string key with `zend_handle_named_arg()`
    (PHPSRC/Zend/zend_execute.c:5538) as it unpacks. So the unknown `zz` fails before the positional element is
    reached.
- **twig-rs**: `get_attribute()` unpacks with `CallArgs::from_spread()`, which cannot know the method's parameters.
- **Fix notes**:
  - With the method's parameter names and variadic flag, call `CallArgs::unpack(value, Some(&CalleeParams { .. }),
    diag)`. That is G5's port of `ZEND_SEND_UNPACK`, which function calls use since P14.
  - Host objects that give no parameter information keep `from_spread()`.

## G4: syntax

### P16. Errors of generated PHP that fails to compile are wrapped by `compile_source_error()`

- **Status**: G1's part is done:
  - a spread that lands outside an argument list or array literal gives PHP's `ParseError` (`php::generated_parse_error()`);
  - spread children of `getAttribute()`/`block()`, and constant literals that unpack a non-array constant, give D3's
    `CompileError`;
  - `x is constant(...y)` unpacks into `constant()`.

  The vectors are `calls-*` in `tests/engine_parity.rs`. Only the wrapping is left.
- **Groups**: G4 (`src/parser/context.rs`).
- **Covers** (2 vectors): `callables/mismatches.json`: `callables-E1`, `-E2`. Their signature changed: they now fail
  with the wrapped error instead of rendering.
- **Repro**: `"{{ x|default(...[]) }}"`.
  - PHP: load `ParseError: syntax error, unexpected token "..."`.
  - twig-rs: `Twig\Error\SyntaxError: An exception has been thrown during the compilation of a template ("syntax
    error, unexpected token "..."") in "index.twig".`, with the `ParseError` as previous.
  - The D3 cases are wrapped the same way: `"{{ attribute(n, ...[]) }}"` gives a wrapped `CompileError`, where PHP
    stops with a fatal error (not comparable).
- **Upstream**:
  - PHP raises these errors when `Environment::loadTemplate()` evaluates the code that `compileSource()` returned.
    `compileSource()` wraps only the `\Exception`s that compiling throws (TWIG/src/Environment.php).
  - So Twig never wraps them.
- **twig-rs**: `compile_source_error()` (`src/parser/context.rs` ~212) wraps every error that is not a Twig error.
- **Fix notes**:
  - Return unwrapped any `ErrorKind::Php` error whose class is `ParseError` or `CompileError`.
  - Do it together with P32, which moves these errors after the whole compilation.

---

## Accepted deviations: follow-ups

D1 to D4 are now rows of `reference/deviations.md`. D5 was not needed.

- **D1**, unknown-argument errors of AST nodes built without a source (Engine table). The two tests
  `tests/upstream/extensions/util/callable_arguments_extractor_test.rs::test_get_arguments_with_wrong_named_argument_name`
  and `::test_get_arguments_with_wrong_named_argument_names` stay ignored; their `#[ignore]` now cites the deviation.
  - Left: turn them into assertions of twig-rs's documented text (`... in "test.twig".`).
- **D2**, lone CR bytes in the generated PHP (Engine table). Round 2 found a second trigger: a CR byte that a string
  escape (`\r`, `\x0d`, `\15`) puts into a compiled string literal, without `debug`.
  - Covered by `tests/vectors/fuzz/escaping/round2-cr-escape-lines.json`. Marked: `round2-cr-escape-macro-strategy`
    (minimised from `esc-7003-959`, 2 raw), `round2-cr-escape-x0d-strategy`, `-r-strategy` and `-x0d-round-method`.
    Its 8 controls match. Also covered by `errors/known-mismatches.json`: `cr-debug-embed-line`.
  - These vectors stay marked by design, with reasons pointing at the entry.
  - Real templates can hit this form. The other raw case, `esc-7001-346`, gets its CR from `trim(" \t\n\r\0\x0B")`
    (PHP's default trim list), and its error line moves from 8 to 9. Emulating it would need the line layout of the
    generated PHP (how many LFs Twig writes between a call and the next `// line N`), which twig-rs does not model.
    Revisit if real reports come in.
  - Raw CR bytes of the template are normalized before compiling and shift nothing (controls `round2-cr-control-raw-*`).
- **D3**, compile-time fatal errors of generated PHP (Engine table). That twig-rs still wraps them is P16.
- **D4**, the cache of macro defaults is per environment, not per process (Engine table).

## G6: fuzz tooling

- **T8. Typed-data gaps** (partly fixed). Done:
  - the factory rebuilds exact `DateInterval` microseconds and `createFromDateString()` intervals;
  - it places dynamic properties of date objects where PHP keeps them, from the `props` of the typed trees;
  - dates at the second of two equal local times, and dates with five-digit years, are rebuilt from the timestamp;
  - `SplObjectStorage::seek()` is ported (G5).

  Remaining:
  - `DatePeriod`;
  - the civil/wall mode of rebuilt intervals, which PHP's API does not show;
  - closures, resources, and TemplateWrappers of another environment;
  - the full `SimpleXMLElement` port (G5). Round 2 had 16 sandbox cases that are not comparable because of it
    (`SimpleXMLElement property access is not supported by twig-test-support`);
  - coverage the plain oracle environment cannot express yet:
    - an ArrayAccess class outside `ARRAY_LIKE_CLASSES`, which needs a G5 class port first;
    - `render_sandboxed()` and `Twig\Sandbox\Sandbox`;
    - source policies and custom policy classes;
    - `config_php`, a callable `autoescape`, and the integration environment.
- **Round-2 notes**: the other not-comparable cases are harness limits.
  - 62 composition cases have `data_php` that calls `$twig->load()` on a template that fails to compile. The oracle
    stops in phase `data` and returns no typed data, so twig-rs cannot rebuild the data. The template's error could
    still be compared.
  - 2 callables cases exhausted the oracle's 512M memory limit.
  - 1 formats case throws in its data (`new \DateTimeZone("Etc/Unknown")`).
- **Smaller notes** (unchanged):
  - long output diffs share the coarse signature `output:changed`;
  - `data_php` minimisation only drops top-level keys;
  - triage batches full of variants of one cause with `--no-minimize` first, then minimise one case per cause. In
    round 2 a whole 10,000-case `drive` (oracle, twig-rs, minimisation of its at most one cluster) took 29 to 67 s.

## Already accepted (no action)

The diff accepts these deviations of `reference/deviations.md`, so they show up as "deviations", not mismatches:
- `, called in` / `passed in` suffixes;
- closure names with file and line;
- `var_dump()` object handles, also escaped or case-changed;
- lossy non-UTF-8 bytes in messages;
- the `not supported by twig-rs` errors of mbstring encodings and static method calls.

The generators avoid the line guessing of `cache => false` with a cache directory per case. The ported tests also
cover template recursion limits and the wrapped `needEscaping()` `TypeError` of `{% autoescape null %}`.

## Other follow-ups (documentation)

- `plans/engine.md`:
  - `Environment::runtime()` falls back to a registered extension (P26);
  - outside `debug`, a compiled program's source has no code (P24);
  - `extension::runtime::callables` and the generated class table `misc/twig_classes.rs` exist (P15, P27).

## Not parity bugs (recorded for roadmap phase 6)

Missing public APIs reported by the ports:
- `CoreExtension` static helpers and `getAttribute()` as an entry point;
- `TemplateWrapper::unwrap()`, and `Environment::load()` taking a wrapper;
- macro namespaces as values (`getMacroNamespace()`, `MacroNamespace::call`/`has`, `TwigMacro`);
- `resetGlobals()`, `isTemplateFresh()`, `Source::createWithLazyPath()`, `Twig\BlockChain`;
- the `Sandbox` stream methods, lazy runtime factories, `SecurityChecker::new`/`check_security`;
- a codegen hook for extension nodes (already listed);
- `RecursiveArrayIterator`, and ArrayObject/ArrayIterator subclasses in twig-value.
