# Parity fixes

The distinct root causes behind the mismatches that differential fuzzing (`crates/twig-fuzz`) and the PHPUnit ports
(`tests/upstream*.rs`) still find. Each item is meant to be worked on alone: it lists what it covers, a minimal
reproduction, the upstream behaviour with citations, where twig-rs goes wrong and how to check the fix. Finished items
are removed from this file, as in `roadmap.md`; item numbers are never reused.

`TWIG` and `PHPSRC` are the source trees named in `reference/target.md`. Templates are written as JSON strings (`\n`
is a newline). "PHP" is the oracle's result (`twig-oracle:3.31.1`), "twig-rs" the current one. PHP's `, called in
<file> on line N` suffixes are left out (accepted deviation).

## Status

- **Rounds 1 and 2** (2026-11-26) produced P1 to P33, the deviation candidates D1 to D5 and the tooling items T1 to
  T9. Every P item up to P33 is fixed and removed (P16, P24, P31, P32 and P33 in the round-2 fixes, which removed 25
  `known_mismatch` markers). D1 to D4 are rows of `reference/deviations.md`; D5 was not needed. T8 is partly done.
- **Round 3** (2026-11-26, the tree after the round-2 fixes, with the four new families whitespace, unicode, dynamic
  and structure): 20,000 generated cases per family through `twig-fuzz drive` with minimisation, seeds 9001 to 9010
  (2,000 cases each).
  - Before it, `cargo +stable test --workspace` passed: 1806 tests, none failed. The 13 ignored ones are the 2
    fixture cases their CONDITION skips, the 3 tests that need an external vector file, D1's 2 tests, the 5 Docker
    tests of twig-fuzz and 1 doctest.
  - `twig-fuzz replay --update tests/vectors/fuzz` removed no marker (the round-2 fixers had removed theirs); the
    4475 vectors there and the 11 round-0 vectors replay with no failure.

  | family | cases | identical | deviations | mismatches | clusters | not comparable | causes (raw) |
  |---|---|---|---|---|---|---|---|
  | expressions | 20000 | 19869 | 125 | 6 | 3 | 0 | P35 (4), P37 (1), compiled-code warning (1) |
  | callables | 20000 | 19266 | 730 | 2 | 2 | 2 | T11 (2) |
  | control | 20000 | 19893 | 105 | 2 | 2 | 0 | P36 (1), T10 (1) |
  | composition | 20000 | 19146 | 711 | 0 | 0 | 143 | – |
  | escaping | 20000 | 19603 | 394 | 3 | 2 | 0 | D2 (3) |
  | errors | 20000 | 19654 | 342 | 4 | 1 | 0 | D2 (4) |
  | formats | 20000 | 19826 | 169 | 0 | 0 | 5 | – |
  | sandbox | 20000 | 19048 | 915 | 0 | 0 | 37 | – |
  | whitespace | 20000 | 20000 | 0 | 0 | 0 | 0 | – |
  | unicode | 20000 | 18772 | 1228 | 0 | 0 | 0 | – |
  | dynamic | 20000 | 18580 | 1420 | 0 | 0 | 0 | – |
  | structure | 20000 | 18558 | 1442 | 0 | 0 | 0 | – |
  | total | 240000 | 232215 | 7581 | 17 | 10 | 187 | |

  - The deviations are the accepted ones the diff knows: `called in` and `passed in` suffixes, closure names, object
    handles, and the `not supported by twig-rs` errors (all 1228 of unicode: mbstring encodings).
  - Three clusters are parity bugs, all wrong error lines of the generated-PHP line model (G1): P35 part b and P37
    are new; P36 is the loop-bookkeeping bug the round-2 fix of P31/P32 reported. Six raw cases in 240,000.
  - The other seven clusters are not bugs: D2 three times (CR escapes, and lone CR bytes with `debug`), the accepted
    deviation "Warnings of compiled PHP code" once (now in `reference/deviations.md` with the filesystem cache's
    silencing), and harness noise three times (T10, T11).
  - Items reported by the round-2 fixers and reproduced in round 3: P34 (G5), P35 part a and P36's decided-`or` form
    (G1), P38 and P39 (G2).
  - Vectors: `tests/vectors/fuzz/<family>/round3-*.json`, 8 files with 81 oracle-recorded vectors (process isolation):
    43 marked `known_mismatch`, 38 controls that match and must keep matching. The noise clusters of T10 and T11 have
    no vectors: their results depend on object ids and unseeded randomness.
  - The not-comparable cases are harness limits (see the round-3 notes of G6).

## Working rules

- Port the upstream code path named in the item and cite it in doc comments (`reference/conventions.md`). Messages
  are verbatim.
- An item is done when its vectors replay identically and its tests pass. Then remove the `known_mismatch` markers of
  exactly those vector ids (`cargo test --test fuzz_regressions` fails while a fixed vector is still marked) and the
  `#[ignore]` of its tests. `twig-fuzz replay --update <files> [--id ID]...` removes the markers of the vectors that
  match now. Never edit an `oracle` field: re-run the case with `twig-fuzz refresh` or `twig-fuzz oracle` instead.
- Vector files hold items of several groups. Edit only the markers of your item's ids. The controls in the `round2-*`
  and `round3-*` files are unmarked on purpose: a fix must not break them.
- Check fixes with `twig-fuzz replay <file>`, and probe variants with `twig-fuzz oracle <batch> --out <o.json>`
  followed by `twig-fuzz diff <batch> --oracle <o.json>`. Add oracle-backed vectors for the "also check" cases.
- `tests/fuzz_regressions.rs` also replays `crates/twig-fuzz/vectors/round0-mismatches.json`.

Groups: **G1** codegen + VM (`src/lltl`, `src/vm`), **G2** runtime (`src/runtime`, `src/escaper`), **G3** callables
(`src/extension`, including the runtime groups), **G4** syntax (`src/lexer`, `src/parser`, `src/token_parser`,
`src/visitor`, `src/ast`), **G5** library crates (`crates/twig-value`, `twig-date`, `twig-pcre`, `twig-unicode`,
`twig-timelib`, `twig-test-support`), **G6** fuzz tooling (`crates/twig-fuzz`, `research/php-oracle`). When an item
needs changes in several groups, the first group listed leads.

## Summary

"Raw" counts the generated cases attributed to the cause; "report" marks an item a round-2 fixer reported and round 3
reproduced with probes. Priority: **high** = wrong output, or more than 100 raw cases; **medium** = a wrong error or
diagnostic that real templates can hit; **low** = obscure error paths.

| id | root cause | groups | raw | vectors | tests | priority |
|---|---|---|---|---|---|---|
| P34 | Date objects report no method parameters: an unknown named argument before a spread fails late | G5 | report | 3 +9 controls | – | low |
| P35 | Lines of the constants PHP folds inside array literals: chosen branches, parse-time concatenations, dynamic keys | G1 | 4 + report | 15 +9 controls | – | low |
| P36 | The for-loop bookkeeping takes a stale PHP marker after a folded literal or skipped code | G1 | 1 + report | 5 +5 controls | – | low |
| P37 | TrueTest converts its operand to bool at the operand's last marker instead of `$tmp`'s line | G1 | 1 | 12 +5 controls | – | low |
| P38 | `Twig\Source` objects in templates lack `getColumn()` and `__serialize()` | G2 | report | 4 +1 control | – | low |
| P39 | `Twig\Markup::__construct()` assigns `$charset` without the `?string` property type check | G2 | report | 1 +4 controls | – | low |

---

## G1: codegen and VM

### P35. Lines of the constants PHP folds inside array literals

- **Groups**: G1 (`src/lltl/php.rs`, `src/lltl/codegen.rs`).
- **Covers** (round 3: 4 raw of 20,000 expressions cases for part b; part a reported by the round-2 fix of P31/P32,
  fuzz seed 8101), all in `tests/vectors/fuzz/expressions/round3-fold-lines.json`:
  - part b: `round3-fold-branch-line`, minimised from `x9004-00465-arrays` (raw: `x9004-00465-arrays`,
    `x9005-00433-arrays`, `x9007-00933-arrays`, `x9010-01217-arrays`); `round3-fold-branch-line-literal-then`,
    `-elvis`, `-coalesce`, `-packed`, `-names`; `round3-fold-concat-line-else`, `-then`, `-number`, `-operand`,
    `-operand-number`, `-operand-three`;
  - part a: `round3-fold-key-line`, `-int`, `-name-key`;
  - controls, which must keep matching: `round3-fold-branch-ctl-literal`, `-not-in-literal`, `-negative-number`,
    `-true`, `round3-fold-concat-ctl-negative-operand`, `round3-fold-key-ctl-second-element`, `-value-marker`,
    `-one-line`, `-constant-key`.
- **Repro (b)**: `"{{([[...{},\nr]*(''?'')])}}"`. PHP: `Unsupported operand types: array * string` at line 2;
  twig-rs: line 1.
  - The same with the chosen branches `1?'x':''`, `null??''`, `''?:''`, and with a concatenation of literals, as the
    chosen branch (`''?'':'a'~'b'`, `1?'a'~1:''`) or as the operand itself (`'a'~'b'`).
  - With `-1` or `true` as the chosen branch, or `-1~'b'` as the operand, both give line 1.
- **Repro (a)**: `"{{({(('')\n~s):([])+e})}}"`. PHP: `Unsupported operand types: array + null` at line 2; twig-rs:
  line 1. When the marker comes from the value, or the dynamic key is not the first element, both agree.
- **Upstream**:
  - (b) Twig writes `[([ ...[], // line 2\n($context["r"] ?? null)] * (("") ? ("") : ("")))]`.
    `zend_compile_array()` first runs `zend_try_ct_eval_array()` on the outer literal (PHPSRC/Zend/zend_compile.c:10095),
    whose `zend_eval_const_expr()` replaces a conditional with a constant test by its chosen child itself
    (`ZEND_AST_CONDITIONAL` :12198; `??`: `ZEND_AST_COALESCE` :12174). A child that is already a `ZEND_AST_ZVAL`
    keeps the line it was parsed on, here the line after `// line 2`. A value that `zend_eval_const_expr()` creates
    (`zend_ast_create_zval()`: a folded `-1`, `true`, an operator) gets `CG(zend_lineno)`, the line of the outer
    literal. `zend_compile_expr_inner()` then sets `CG(zend_lineno)` to the operand's line (:11817), and the `*`
    emitted after it reports that line.
  - PHP's parser folds the concatenation of two values while it builds the AST (`zend_ast_create_concat_op()`,
    PHPSRC/Zend/zend_ast.c:484), so `("a" . "b")` and `("a" . 1)` are values with the line they are written on, as
    operands and as chosen branches. `(-1 . "b")` is not folded by the parser (`-1` is a unary minus).
  - (a) Twig writes `[ (string)("" . // line 2\n($context["s"] ?? null)) => ([] + ($context["e"] ?? null))]`:
    `ArrayExpression::compile()` writes a dynamic key's `(string) key =>` before the value
    (TWIG/src/Node/Expression/ArrayExpression.php:146-160). The literal's line is its first element's
    (`zend_ast_create_list()`), and the element's line is its value's (`ZEND_AST_ARRAY_ELEM` is created as
    `(value, key)`), so the key's marker comes first. The `null` of `?? null` is a constant the literal's folding
    stamps with that line, and the `+` after it reports it.
- **twig-rs**: `Codegen::expr()` treats every expression that `eval_const()` folds and `is_php_literal()` rejects as
  stamped with the fold marker, the outermost literal's line from `php::first_debug_line()`. Chosen branches and
  parse-time concatenations are such expressions, and `first_debug_line()` looks only at the first element's value.
- **Fix notes**:
  - Give a folded conditional (and `??`, `?:`) the line of its chosen branch when that branch is a PHP literal,
    recursively through nested conditionals.
  - Count a concatenation of PHP literals (`zend_ast_create_concat_op()`'s rule: both operands values, no array) as
    a literal written where it stands.
  - In `first_debug_line()`, look at the first element's dynamic key before its value.
  - Keep `round2-array-literal-folding.json` and `tests/vectors/engine/parity/folding.json` passing (P31's operand
    kinds use the same chosen-branch logic).

### P36. The for-loop bookkeeping takes a stale PHP marker after a folded literal or skipped code

- **Groups**: G1 (`src/lltl/codegen.rs`).
- **Covers** (round 3: 1 raw of 20,000 control cases; the decided-`or` form was reported by the round-2 fix of
  P31/P32), all in `tests/vectors/fuzz/control/round3-loop-bookkeeping-line.json`:
  - `round3-loop-bookkeeping-line`, minimised from `ctl-9008-1397`; `round3-loop-bookkeeping-line-set-multi`,
    `-set-literal`, `-decided-or`, `-decided-or-simple`;
  - controls, which must keep matching: `round3-loop-bookkeeping-ctl-set-single`, `-print-name`,
    `-print-name-loop`, `-one-line`, `-name-next-line`.
- **Repro**: `"{%for _parent in map%}{%set\nr,v=(5),y%}{%endfor%}"`, data `{"map": {"x": 1}}`. PHP:
  `array_intersect_key(): Argument #2 must be of type array, int given` at line 2; twig-rs: line 1.
  - `"{%for loop in items%}{%if {'':''} or\nc%}{%endif%}{%endfor%}"`, data `{"items": [1]}`: `Cannot use a scalar
    value as an array` at line 2 in PHP, line 1 in twig-rs.
  - A single-target `{% set r = \ny %}` or a print as the last statement of the body agree.
- **Upstream**:
  - `ForLoopNode` and `ForNode::compile()` write the bookkeeping as statements of their own
    (`++$context['loop']['index0'];` inside the loop, TWIG/src/Node/ForLoopNode.php:38; `$_parent =
    $context['_parent']; ... $context = array_intersect_key($context, $_parent);` after it,
    TWIG/src/Node/ForNode.php:105-119).
  - `zend_compile_stmt()` (PHPSRC/Zend/zend_compile.c:11695) compiles each statement at the line it is written on:
    after the last `// line N` of the body. A marker in code PHP skips (the right operand of `or` that a constant left
    operand decides, `zend_compile_short_circuiting()`) still counts: it is in the PHP source and in
    `getDebugInfo()`.
- **twig-rs**: `for_loop_end()` emits the bookkeeping at `php_marker`, which a folded literal (`Codegen::folded()`) or
  skipped code left behind, instead of the current marker.
- **Fix notes**: start the bookkeeping at the current marker (the round-2 fixer suggested `self.leaf()` first). Also
  check the other statement-level writes that follow an expression without a marker of their own: the ends of
  `with` (`$context = $context['_parent']`), of captures (`set`, `apply`), of macros and of blocks.

### P37. TrueTest converts its operand to bool at the operand's last marker instead of `$tmp`'s line

- **Groups**: G1 (`src/lltl/codegen.rs`).
- **Covers** (round 3: 1 raw of 20,000 expressions cases), all in
  `tests/vectors/fuzz/expressions/round3-true-test-lines.json` (data `['nan' => NAN]`, warnings thrown as exceptions
  with `"errors": "throw"`):
  - `round3-true-test-line`, minimised from `x9001-01440-logic`; `round3-true-test-line-or-right`, `-or-left`,
    `-and-right`, `-and-left`, `-xor`, `-print`, `-if`, `-elseif`, `-not`, `-ternary`, `-elvis`;
  - controls, which must keep matching: `round3-true-test-ctl-one-line`, `-if-one-line`, `-comparison`,
    `-name-first`, `-collected-warnings`.
- **Repro**: `"{%if\nnan%}{%endif%}"`. PHP: `unexpected NAN value was coerced to bool` at line 1; twig-rs: line 2.
  The same for every operand TrueTest wraps: `and`, `or`, `xor`, `not`, the tests of `if`, `elseif` and `? :`, the
  left operand of `?:`.
- **Upstream**:
  - `TrueTest::compile()` (TWIG/src/Node/Expression/Test/TrueTest.php:38) writes `(($tmp = // line 2\n
    ($context["nan"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp)`. `AndBinary`, `OrBinary`,
    `XorBinary`, `NotUnary`, `ConditionalTernary`, `ElvisBinary` and `IfNode` wrap their operands in it.
  - `zend_compile_assign()` sets `CG(zend_lineno)` back to the line of `$tmp` after compiling the value
    (PHPSRC/Zend/zend_compile.c:3483). So the `JMPZ_EX` of `&&`, which converts the value to bool and raises PHP 8.5's
    NAN warning (`i_zend_is_true()`, PHPSRC/Zend/zend_operators.h:425), reports the line where `(($tmp = ` is
    written: the marker in effect before the operand's own markers.
- **twig-rs**: `TestKind::True` emits `Instr::TrueTest` with `self.emit()` after the operand, at the marker the
  operand left.
- **Fix notes**: emit the conversion at the marker in effect where the TrueTest starts (`emit_at(start)`, as calls do).
  The `(string)` cast of a `Markup` comes later in PHP but cannot fail. Only a conversion that raises (NAN, PHP 8.5)
  shows the line, and only when warnings become exceptions.

## G2: runtime

### P38. `Twig\Source` objects in templates lack `getColumn()` and `__serialize()`

- **Groups**: G2 (`src/runtime/template.rs`, the `Twig\Source` class entry).
- **Covers** (reported by the round-2 fix of P24/P33), all in
  `tests/vectors/fuzz/dynamic/round3-twig-object-methods.json`: `round3-source-get-column`,
  `-get-column-past-end`, `-get-column-strict`, `-serialize`; control `round3-source-ctl-get-name`.
- **Repro**: `"{{ template_from_string('a\nbc').getSourceContext().getColumn(3) }}"` (extension `string_loader`).
  PHP: `1` (without `debug` the code is `''`); twig-rs: nothing (null). With `strict_variables` twig-rs raises
  `Neither the property "getColumn" nor one of the methods ... exist`. `getSourceContext().__serialize()|json_encode`
  gives the three private properties in PHP, `null` in twig-rs.
- **Upstream**: TWIG/src/Source.php: `getColumn(int $offset): ?int` (null for a negative offset; the 1-based column of
  the byte offset, with `\r\n` and `\r` read as `\n`) and `__serialize(): array` (resolves the lazy path, then
  `(array) $this`: the keys `"\0Twig\Source\0code"`, `...name`, `...path`).
- **twig-rs**: the class entry implements `getCode()`, `getName()` and `getPath()` only.
- **Fix notes**: port both methods; `__serialize()` resolves the lazy path as `getPath()` does (P24).

### P39. `Twig\Markup::__construct()` assigns `$charset` without the `?string` property type check

- **Groups**: G2 (`src/runtime/markup.rs`).
- **Covers** (reported by the round-2 fix of P24/P33), in `tests/vectors/fuzz/dynamic/round3-twig-object-methods.json`:
  `round3-markup-construct-array-charset`; controls `round3-markup-construct-ctl-int-charset`, `-null-charset`,
  `-float-charset`, `-bool-charset`.
- **Repro**: `"{% set m %}x{% endset %}{{ m.__construct('y', [1]) }}{{ m }}"`. PHP: `Cannot assign array to property
  Twig\Markup::$charset of type ?string` (a `TypeError`, wrapped in a `RuntimeError`); twig-rs: the warning `Array to
  string conversion` and the output `y`.
- **Upstream**: `Markup::__construct($content, $charset, private array $safeStrategies = ['all'])` assigns
  `$this->charset = $charset` to `private ?string $charset` (TWIG/src/Markup.php). Markup.php does not declare strict
  types, so scalars are coerced and an array fails the property type check (`zend_verify_property_type_error()`,
  PHPSRC/Zend/zend_execute.c:849).
- **twig-rs**: the constructor converts `$charset` with `to_string`.
- **Fix notes**: apply the coercion of a `?string` typed property in non-strict mode (null kept; int, float, bool and
  Stringable objects converted; arrays and other objects rejected with PHP's message). Check `$safeStrategies`, the
  promoted `array` parameter, the same way. The "Property model" row of `reference/deviations.md` leaves typed
  properties to each class's implementation, so the `Markup` port has to do this itself.

## G5: library crates

### P34. Date objects report no method parameters: an unknown named argument before a spread fails late

- **Groups**: G5 (`crates/twig-date`: the `Object` implementations of the date classes), using P33's
  `Object::method_params()` of twig-value.
- **Covers** (found while fixing P33): `tests/vectors/fuzz/callables/round2-method-call-params.json`:
  `method-params-date-format-unknown-then-spread`; `tests/vectors/fuzz/callables/round3-date-method-params.json`:
  `round3-date-settime-unknown-then-spread`, `round3-date-interval-format-unknown-then-spread`, and 9 controls
  `round3-date-ctl-*` (named arguments without a spread, which twig-date already binds like PHP).
- **Repro**: `"{{ d.format(zz: 1, ...[2]) }}"`, data `d = new \DateTime('2020-01-02 03:04:05')`. PHP: `Unknown named
  parameter $zz`; twig-rs: `Cannot use positional argument after named argument during unpacking`.
- **Upstream**: `CoreExtension::getAttribute()` calls `$object->$method(...$arguments)`. `ZEND_SEND_UNPACK` sends the
  elements in order and checks each string key against the method's arginfo as it sends it
  (`zend_handle_named_arg()`, PHPSRC/Zend/zend_execute.c:5538), so the unknown name fails before the positional
  element after it is reached.
- **twig-rs**: `method_params()` returns `None` for `DateTime`, `DateTimeImmutable`, `DateTimeZone`, `DateInterval`
  and `DatePeriod`, so `get_attribute()` unpacks with `CallArgs::from_spread()`, which rejects the positional element
  after a named one first.
- **Fix notes**: implement `method_params()` from the stubs (PHPSRC/ext/date/php_date.stub.php), as P33 did for the
  SPL classes, and bind the methods from the same tables.

---

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
  - These vectors stay marked by design, with reasons pointing at the entry.
  - Real templates can hit the escape form. Round 3 reached D2 in 3 of its 10 clusters (7 of 17 raw mismatches):
    the escaping and errors generators still write CR escapes and lone CRs with `debug` (see T14).
  - The raw case `esc-7001-346` of round 2 gets its CR from `trim(" \t\n\r\0\x0B")` (PHP's default trim list), and
    its error line moves from 8 to 9. Emulating it would need the line layout of the generated PHP (how many LFs
    Twig writes between a call and the next `// line N`), which twig-rs does not model. Revisit if real reports come
    in.
  - Raw CR bytes of the template are normalized before compiling and shift nothing (controls
    `round2-cr-control-raw-*`).
- **D3**, compile-time fatal errors of generated PHP (Engine table). The `CompileError` comes in PHP's order (after
  Twig's own errors and a `ParseError`) and leaves `compile_source()` unwrapped. The fuzz diff cannot compare a fatal
  error (round 3: 1 callables case not comparable), so `tests/engine.rs`'s `compile_errors` topic checks the message
  of each one.
- **D4**, the cache of macro defaults is per environment, not per process (Engine table).
- **Warnings of compiled PHP code** (Engine table). Round 3 showed that with a filesystem cache PHP's compile-time
  warnings never appear: `FilesystemCache::load()` includes the class with `@include_once`. The row now says so, and
  `expressions/round3-compile-warning.json` holds the case (`unexpected NAN value was coerced to string` of a
  concatenation PHP folds; marked by design, minimised from `x9003-00992-arith`) and a control without a cache. T12
  would let the diff accept it.
- **`needEscaping()`'s `TypeError`** (Engine table) can go: since P16, `compile_source()` lets PHP `\Error`s through,
  so `src/visitor/escaper.rs` (G4) can raise a real `TypeError` as PHP does. Update
  `tests/upstream/visitors/escaper_test.rs` (it asserts the wrapped form) and remove the row.
- New rows from the round-2 fixes (documented, no action): "Named arguments to methods of host objects" (Value
  model, P33's `method_params()`) and "Lazily resolved template paths" (Engine, P24).

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
  - the full `SimpleXMLElement` port (G5). Round 3 had 37 sandbox cases that are not comparable because of it
    (`SimpleXMLElement property access is not supported by twig-test-support`);
  - coverage the plain oracle environment cannot express yet:
    - an ArrayAccess class outside `ARRAY_LIKE_CLASSES`, which needs a G5 class port first;
    - `render_sandboxed()` and `Twig\Sandbox\Sandbox`;
    - source policies and custom policy classes;
    - `config_php`, a callable `autoescape`, and the integration environment.
- **T10. Object handles inside derived values.** The `object-handle` rule rewrites `#N` where it stands, but a
  `dump()` of objects whose text a template measures or rewrites differs in other places: `length` of a captured
  dump (`ctl-9003-794`: `9360` against `9372`, then `9348` in a fresh worker) and digits rewritten by
  `replace({'1': 'one'})` (`#810` becomes `#8one0`; the round-2 fix check saw the `string(N)` length of a dump
  captured into a `Markup`, `ctl-8101-1000`). twig-rs's ids are a process-wide counter (accepted deviation), so no
  vector can hold these. Fix in the control generator: keep `dump()` of objects away from `length`, `replace` and
  other filters that measure or rewrite text.
- **T11. Unseeded randomness under a non-UTF-8 charset.** `SHUFFLE_INPUTS` and `RANDOM_ARGS` of
  `crates/twig-fuzz/src/generators/callables.rs` treat `'é'` as one character, which it is only in UTF-8. Under a
  non-UTF-8 `charset` (Windows-1252, ...) its two bytes are two characters, so `shuffle` and `random()` depend on
  the unseeded generator (accepted deviation "Randomness"): `c9008-01741m` (`'é'|shuffle`) and `c9001-00769m`
  (`random('é')`), both with `charset: Windows-1252`. Keep multi-byte inputs out when the case sets another charset.
- **T12. Silenced compile-time warnings.** The oracle marks a warning PHP raises while including a cached class
  `"silenced": true` and the diff skips it, while twig-rs raises it at runtime (accepted deviation "Warnings of
  compiled PHP code"). A rule that accepts a twig-rs diagnostic equal to a silenced one of the oracle (type and
  message, in order) would make `x9003-00992-arith` a deviation; flip `round3-compile-warning-nan-concat` then.
- **T13. Lexer options cannot be fuzzed.** Custom delimiters, `whitespace_trim` and `whitespace_line_chars` need a
  lexer-options field in `research/php-oracle/runner.php` and `crates/twig-fuzz/src/kit/run.rs` (`data_php` runs
  after the load phase). It would let the whitespace family reach the lexer-option deviations of
  `reference/deviations.md`.
- **T14. Generators still produce D2.** The escaping generator writes `\xd`/`\r`/`\x0d` escapes and the errors
  generator lone CR bytes with `debug`; round 3 spent 3 of its 10 clusters on them. The whitespace generator already
  moved its CR bytes into the data. Optional: do the same here, now that D2's vectors pin the deviation.
- **Round-3 notes**: the not-comparable cases are harness limits.
  - 143 composition cases have `data_php` that calls `$twig->load()` on a template that fails to compile. The oracle
    stops in phase `data` and returns no typed data, so twig-rs cannot rebuild the data. The template's error could
    still be compared.
  - 37 sandbox cases: `SimpleXMLElement` (T8).
  - 5 formats cases throw in their data (`new \DateTimeZone("Etc/Unknown")`).
  - 2 callables cases: one exhausted the oracle's 512M memory limit, one is a D3 compile fatal.
- **Smaller notes** (unchanged):
  - long output diffs share the coarse signature `output:changed`;
  - `data_php` minimisation only drops top-level keys;
  - triage batches full of variants of one cause with `--no-minimize` first, then minimise one case per cause. In
    round 3 a whole 20,000-case `drive` took 72 s (callables) to 262 s (dynamic, process isolation), with three
    drives running at once.

## Already accepted (no action)

The diff accepts these deviations of `reference/deviations.md`, so they show up as "deviations", not mismatches:
- `, called in` / `passed in` suffixes;
- closure names with file and line;
- `var_dump()` object handles, also escaped or case-changed;
- lossy non-UTF-8 bytes in messages;
- the `not supported by twig-rs` errors of mbstring encodings and static method calls.

The generators avoid the line guessing of `cache => false` with a cache directory per case. The ported tests also
cover template recursion limits and the wrapped `needEscaping()` `TypeError` of `{% autoescape null %}`.

## Other follow-ups (documentation and checks)

- `plans/engine.md`:
  - `Environment::runtime()` falls back to a registered extension (P26);
  - outside `debug`, a compiled program's source has no code, and its path is resolved lazily through the loader of
    the template instance (P24);
  - `Object::method_params()` and the method-call binding of `getAttribute()` (P33);
  - `extension::runtime::callables` and the generated class table `misc/twig_classes.rs` exist (P15, P27).
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` fails (G1): the public documentation of `compile` links
  to the private `php::generated_parse_error` (`src/lltl/codegen.rs:85`). `cargo clippy --workspace --all-targets --
  -D warnings` and `cargo fmt --check` pass (round 3).

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
