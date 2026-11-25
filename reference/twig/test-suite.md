# Twig 3.31.1 test suite and the Docker PHP oracle — reference for twig-rs

Scope: how Twig 3.31.1 tests itself (the `.test` integration fixtures, the harness that runs them, the PHPUnit
unit tests), what of it can be ported to Rust, and a working Docker oracle that runs PHP 8.5.13 + Twig 3.31.1 so
that any behaviour can be checked against the real implementation.

All paths below are relative to these roots unless absolute:

* `twig/` = `<scratch>\ext\twig-3.31.1`
  (git tag `v3.31.1`, commit `6f6ec372231b53d1d564567a78beb8844cafa132`, 2026-11-24)
* `oracle/` = `<scratch>\survey\php-oracle`
* `work/` = `<scratch>\work`
  (throw-away analysis scripts and probe batches used for the numbers in this document)
* PHP parse of every fixture (output of `oracle/tools/parse-fixtures.php`):
  `<scratch>\survey\fixtures-parsed.json`

Everything marked **verified** was run in the oracle container (PHP 8.5.13, Twig 3.31.1, PHPUnit 9.6.39).
Anything not verified is marked **UNVERIFIED**.

## Table of contents

- [0. Key facts](#0-key-facts)
- [1. The `.test` fixture format](#1-the-test-fixture-format)
  - [1.1 File discovery and naming](#11-file-discovery-and-naming)
  - [1.2 The regexes, verbatim](#12-the-regexes-verbatim)
  - [1.3 Sections](#13-sections)
  - [1.4 Exact whitespace rules for template sources](#14-exact-whitespace-rules-for-template-sources)
  - [1.5 Multiple DATA/CONFIG/EXPECT pairs](#15-multiple-dataconfigexpect-pairs)
  - [1.6 Parser quirks (verified)](#16-parser-quirks-verified)
  - [1.7 Port-ready parsing algorithm](#17-port-ready-parsing-algorithm)
- [2. How a fixture is executed and compared](#2-how-a-fixture-is-executed-and-compared)
  - [2.1 doIntegrationTest() step by step](#21-dointegrationtest-step-by-step)
  - [2.2 Load phase vs render phase](#22-load-phase-vs-render-phase)
  - [2.3 Exception comparison and format placeholders](#23-exception-comparison-and-format-placeholders)
  - [2.4 Output comparison](#24-output-comparison)
  - [2.5 Deprecation capture and comparison](#25-deprecation-capture-and-comparison)
  - [2.6 Conditions, skips, legacy tests](#26-conditions-skips-legacy-tests)
  - [2.7 Global state that leaks between fixtures](#27-global-state-that-leaks-between-fixtures)
  - [2.8 NodeTestCase (compiled-PHP tests)](#28-nodetestcase-compiled-php-tests)
- [3. Harness environment (tests/IntegrationTest.php)](#3-harness-environment-testsintegrationtestphp)
  - [3.1 Environment options](#31-environment-options)
  - [3.2 Globals, runtime loaders, registration order](#32-globals-runtime-loaders-registration-order)
  - [3.3 Core-package extensions: Debug, Sandbox, StringLoader](#33-core-package-extensions-debug-sandbox-stringloader)
  - [3.4 Test-only extensions (full PHP code)](#34-test-only-extensions-full-php-code)
  - [3.5 Undefined-callable callbacks](#35-undefined-callable-callbacks)
  - [3.6 Helper classes used by DATA sections (full PHP code)](#36-helper-classes-used-by-data-sections-full-php-code)
  - [3.7 PHP runtime settings in effect](#37-php-runtime-settings-in-effect)
  - [3.8 Checklist for a Rust fixture harness](#38-checklist-for-a-rust-fixture-harness)
- [4. PHP code in DATA / CONFIG sections](#4-php-code-in-data--config-sections)
  - [4.1 Constructs with counts](#41-constructs-with-counts)
  - [4.2 Classes and the behaviour fixtures rely on](#42-classes-and-the-behaviour-fixtures-rely-on)
  - [4.3 Inline class declarations](#43-inline-class-declarations)
  - [4.4 CONFIG values](#44-config-values)
  - [4.5 Turning DATA into Rust values](#45-turning-data-into-rust-values)
- [5. Fixture catalogue and classification](#5-fixture-catalogue-and-classification)
  - [5.1 Per-directory matrix](#51-per-directory-matrix)
  - [5.2 Category lists](#52-category-lists)
  - [5.3 Exception fixtures by phase and class](#53-exception-fixtures-by-phase-and-class)
  - [5.4 Deprecation messages used by fixtures](#54-deprecation-messages-used-by-fixtures)
  - [5.5 Behaviour facts surfaced by the fixtures](#55-behaviour-facts-surfaced-by-the-fixtures)
  - [5.6 Coverage gaps (features with no fixture)](#56-coverage-gaps-features-with-no-fixture)
- [6. PHPUnit test files: what they test, how to port](#6-phpunit-test-files-what-they-test-how-to-port)
  - [6.1 Portability classes](#61-portability-classes)
  - [6.2 Per-file table](#62-per-file-table)
  - [6.3 Error messages asserted by unit tests](#63-error-messages-asserted-by-unit-tests)
- [7. The Docker PHP oracle](#7-the-docker-php-oracle)
  - [7.1 Files](#71-files)
  - [7.2 Build and pinned versions](#72-build-and-pinned-versions)
  - [7.3 Runner protocol (JSON in, JSON out)](#73-runner-protocol-json-in-json-out)
  - [7.4 Sample session (real output)](#74-sample-session-real-output)
  - [7.5 Running Twig's own PHPUnit suite (real results)](#75-running-twigs-own-phpunit-suite-real-results)
  - [7.6 Fixture corpus export](#76-fixture-corpus-export)
  - [7.7 Caveats](#77-caveats)
- [8. Unverified points and open questions](#8-unverified-points-and-open-questions)

---

## 0. Key facts

* 488 `.test` fixture files under `twig/tests/Fixtures` (455 normal + 33 `*.legacy.test`), 682 templates,
  **502 DATA/EXPECT cases**. No fixture lives directly in `Fixtures/`; 8 non-`.test` files there belong to other tests.
* Fixture format = PCRE regexes in `twig/src/Test/IntegrationTestCase.php:191`, `:198`, `:204`, `:390`
  (verbatim in §1.2). They are not anchored to line starts and rely on lazy/greedy backtracking and lookahead;
  a regex-free port that reproduces PHP's parse for **all 488 files (verified)** is in
  `oracle/tools/fixture_parser_reference.py`.
* Template sources keep the newline that follows `--TEMPLATE--`, so **template line 1 is always empty and the first
  visible line is line 2** (all expected error messages rely on this). Non-last templates keep their trailing
  newline; the last template loses exactly one `\n` (EXPECT fixtures) or all trailing whitespace (EXCEPTION fixtures).
* For DATA/EXPECT pair `i` (0-based) every template gets `i` spaces appended **cumulatively** (total
  `i*(i+1)/2`) — `IntegrationTestCase.php:256-258`.
* Output comparison: `trim($output, "\n ")` vs `trim($expected, "\n ")`, strict string equality (only LF and space are
  trimmed; not `\t`, not `\r`).
* Exceptions: load-phase (compile) errors must match `--EXCEPTION--` exactly (`assertSame`) **and** the PHP message
  must end with `.` or `?`; render-phase errors are compared with PHPUnit `assertStringMatchesFormat` (`%s` etc.).
  Only `expressions/matches_error_runtime.test` actually uses a placeholder.
* Deprecations: every `E_USER_DEPRECATED` raised while loading templates is captured; render-time ones only when the
  fixture has `--DEPRECATION--`. Compared as `implode("\n", $messages)` against the trimmed section, order matters.
* Harness environment: `cache=false`, `strict_variables=true` (overridable by CONFIG), global `global` = `'global'`,
  extensions Debug, Sandbox (policy allows only function `dump`, **not** global), StringLoader, `TwigTestExtension`,
  attribute-based test extensions, plus undefined-callable callbacks (§3).
* On PHP 8.5.13 the integration suite gives **488 tests, 486 passed, 2 skipped** (conditions for PHP < 8.0 / < 8.1);
  the full suite gives **2986 tests, 2981 passed, 0 failed, 5 skipped** (§7.5, verified).
* 103 fixtures expect an exception (56 load-phase, 46 render-phase, 1 skipped); 21 expect deprecations; 68 need PHP
  objects in DATA; 13 touch dates; 7 need the sandbox; 22 need test-only callables; **262 are "core features + plain
  data + expected output"** — the natural first target for twig-rs.
* The oracle (`oracle/`) is built and working: `docker build -t twig-oracle:3.31.1 oracle/`, then
  `oracle/oracle.sh < batch.json`. It renders templates (integration or plain environment), evaluates PHP
  expressions, runs individual fixtures with a faithful copy of the harness, exports the whole fixture corpus with
  actual PHP results and typed DATA (`oracle/corpus/fixtures-3.31.1.json`), and runs Twig's PHPUnit suite.

---

## 1. The `.test` fixture format

### 1.1 File discovery and naming

`IntegrationTestCase::assembleTests(bool $legacyTests, string $fixturesDir)` (`IntegrationTestCase.php:175-218`):

1. `$fixturesDir = realpath($fixturesDir)`; for `IntegrationTest` it is `tests/Fixtures/` (`IntegrationTest.php:125-128`).
2. Iterates `new RecursiveIteratorIterator(new RecursiveDirectoryIterator($fixturesDir), LEAVES_ONLY)` — **filesystem
   order, not sorted** (on Linux this is directory-hash order).
3. Keeps files matching `/\.test$/`.
4. Legacy split: `if ($legacyTests xor str_contains($file->getRealpath(), '.legacy.test')) continue;` — so
   `testIntegration` gets the non-legacy files, `testLegacyIntegration` (`@group legacy`) gets `*.legacy.test`.
5. The test key / data-set name / `$file` argument is `str_replace($fixturesDir.'/', '', $file)`, e.g.
   `filters/abs.test` (PHPUnit shows `testIntegration with data set "filters/abs.test"`).
6. A file that matches neither regex throws `InvalidArgumentException('Test "<name>" is not valid.')` while building
   the data provider (none in 3.31.1).
7. If the legacy run finds no file it returns one dummy case `['not', '-', '', [], '', []]`; `doIntegrationTest()`
   recognises empty `$outputs` and calls `expectNotToPerformAssertions()` (`:235-240`).

Data-provider plumbing: PHPUnit ≥ 10 uses the static `provideTests()` / `provideLegacyTests()` (`:148-156`, attributes
`#[DataProvider]`), PHPUnit < 10 uses the `@dataProvider getTests` annotation; `getTests($name, $legacyTests)`
(`:163-173`) falls back to the deprecated `getFixturesDir()` with a deprecation when `getFixturesDirectory()` is not
overridden.

Each data set is `[$file, $message, $condition, $templates, $exception, $outputs, $deprecation]` and is passed to
`testIntegration($file, $message, $condition, $templates, $exception, $outputs, $deprecation = '')`.

### 1.2 The regexes, verbatim

Exception fixtures (tried first, `IntegrationTestCase.php:191`):

```php
preg_match('/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)\s*(?:--DATA--\s*(.*))?\s*--EXCEPTION--\s*(.*)/sx', $test, $match)
// $message = $match[1]; $condition = $match[2]; $deprecation = $match[3];
// $templates = self::parseTemplates($match[4]); $exception = $match[6];
// $outputs = [[null, $match[5], null, '']];
```

Output fixtures (`:198` and `:204`):

```php
preg_match('/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)--DATA--.*?--EXPECT--.*/s', $test, $match)
// $message = $match[1]; $condition = $match[2]; $deprecation = $match[3];
// $templates = self::parseTemplates($match[4]); $exception = false;
preg_match_all('/--DATA--(.*?)(?:--CONFIG--(.*?))?--EXPECT--(.*?)(?=\-\-DATA\-\-|$)/s', $test, $outputs, \PREG_SET_ORDER);
```

Template splitter (`parseTemplates()`, `:387-396`):

```php
preg_match_all('/--TEMPLATE(?:\((.*?)\))?--(.*?)(?=\-\-TEMPLATE|$)/s', $test, $matches, \PREG_SET_ORDER);
foreach ($matches as $match) {
    $templates[$match[1] ?: 'index.twig'] = $match[2];
}
```

Regex notes that matter for a port:

* `s` = dot matches newline. The `x` flag on the first regex is inert (the pattern has no literal whitespace and no `#`).
* No `m` and no `D` flag: `$` matches at the very end **or before a final `\n`** — this is what removes exactly one
  trailing newline from the last template and the last EXPECT block.
* `\s` (PCRE2, no `/u`) = `[ \t\n\x0B\f\r]`.
* PHP fills unmatched groups that precede a matched group with `""` — so absent CONDITION/DEPRECATION/DATA/CONFIG
  arrive as empty strings, not `null`.
* `$outputs` entries are `[full, data, config, expect]` (exception fixtures: `[null, data, null, '']`).

### 1.3 Sections

| Section | Required | Content | Notes (exact semantics) |
|---|---|---|---|
| `--TEST--` | yes | one-line description | `\s*(.*?)\s*` → trimmed. Used only in the failure message `"$message (in $file)"`. 1 file has two lines (`filters/escape_html_attr_relaxed.test`) — both lines are kept. |
| `--CONDITION--` | no | PHP **expression** | Must come right after TEST. Greedy `(.*)`: keeps trailing newline (harmless). `eval('$ret = '.$condition.';')`; falsy → `markTestSkipped($condition)`. See quirk §1.6 #1. 15 files use it. |
| `--DEPRECATION--` | no | expected deprecation messages, one per line | After CONDITION, before the first TEMPLATE. Trimmed (lazy between `\s*`). Compared with `assertSame($deprecation, implode("\n", $deprecations))`. 21 files. |
| `--TEMPLATE--` | yes | template `index.twig` | Body = everything after the marker up to the next `--TEMPLATE`, see §1.4. |
| `--TEMPLATE(name)--` | no | named template | `(.*?)` lazy name (no `)` allowed). Empty name or `0` → `index.twig` (PHP `?:`). `tags/embed/error_line.test` spells `--TEMPLATE(index.twig)--`. Duplicate names: last body wins, key keeps the first position. |
| `--DATA--` | yes for output fixtures, optional for exception fixtures | PHP **statements** returning the context array | Evaluated as `eval($data.';')` in the harness method scope, so `$twig` (the Environment) is visible; nothing else from the scope is used by any fixture. Empty/absent in exception fixtures → `eval(';')` returns `null` (fine as long as rendering is never reached — all 19 such fixtures fail at compile time). |
| `--CONFIG--` | no | PHP statements returning an options array | Only between DATA and EXPECT. `array_merge(['cache' => false, 'strict_variables' => true], eval($config.';'))`. 17 files / 20 blocks. Not supported in exception fixtures (it would be swallowed into DATA). |
| `--EXPECT--` | yes for output fixtures | expected output | Runs to the next `--DATA--` or end of file (one final `\n` dropped by `$`). Compared after `trim(…, "\n ")`. |
| `--EXCEPTION--` | exception fixtures | `FQCN: message` | Everything to end of file (`(.*)` greedy). Compared after `trim()` (default PHP trim set: `" \t\n\r\0\x0B"`). If the file contains `--EXCEPTION--` after the templates the exception regex wins. |

Order that the regexes accept: `TEST, [CONDITION], [DEPRECATION], TEMPLATE+, ( [DATA], EXCEPTION | (DATA, [CONFIG], EXPECT)+ )`.
There is no section-name validation: unknown text between markers just becomes part of the previous section.

### 1.4 Exact whitespace rules for template sources

Let `raw(t)` be the text between the end of the `--TEMPLATE…--` marker of template `t` and the start of the next marker.

| Template | EXPECT fixtures | EXCEPTION fixtures |
|---|---|---|
| any template except the last | `raw(t)` unchanged (leading `\n` and trailing `\n` kept) | same |
| last template | `raw(t)` minus **one** trailing `\n` if present (from `$` before final newline) | `raw(t)` minus **all** trailing `[ \t\n\x0B\f\r]` (lazy body followed by `\s*`) |

Consequences (verified against PHP):

* The leading newline is kept: `index.twig` of `filters/abs.test` is `"\n{{ (-5.5)|abs }}\n…"`. Errors in the first
  visible line report **line 2** (`expressions/call_argument_defined_twice.test`: `… in "index.twig" at line 2.`).
* `tags/include/basic.test`: index = `"\nFOO\n{% include \"foo.twig\" %}\n\nBAR\n"`, foo.twig (last) = `"\nFOOBAR"`.
  Rendering gives `"\nFOO\n" + "\nFOOBAR" + "\nBAR\n"` (the `%}` eats one newline) → expected `FOO\n\nFOOBAR\nBAR`.
  If foo.twig kept its trailing newline the output would have an extra blank line — the rule is load-bearing.
* An empty template section (`--TEMPLATE(base.twig)--` directly followed by `--DATA--`) is `""` when last, `"\n"` otherwise.

The DATA/CONFIG strings are not trimmed (whitespace is irrelevant to `eval`). EXPECT text is `raw` (minus one final
`\n` for the last pair) and is trimmed with `"\n "` before comparison.

### 1.5 Multiple DATA/CONFIG/EXPECT pairs

* 11 fixtures have 2–3 pairs (25 cases): `autoescape/markup_safe_strategies`, `expressions/array`, `filters/default`,
  `functions/deprecated`, `tags/for/else`, `tags/if/basic`, `tags/if/expression`, `tags/inheritance/parent_change`,
  `tests/defined`, `tests/defined_for_attribute`, `tests/defined_for_attribute.legacy`.
* Each pair gets its own CONFIG (none → `[]`), a **fresh `ArrayLoader` + `Environment`**, fresh deprecation list.
* Before building pair `i` the harness runs (`:255-258`)
  `foreach ($templateSources as $name => $template) { $templateSources[$name] = $template.str_repeat(' ', $i); }`
  on the **already modified** array, so pair 0 adds 0 spaces, pair 1 adds 1, pair 2 adds 2 more (3 total): cumulative
  `i*(i+1)/2`. Purpose (comment in source): force recompilation. No current fixture's expected output depends on the
  spaces (they end up at the end of output and are trimmed), but a faithful harness should replicate it.
* The `--DEPRECATION--` section applies to every pair (e.g. `functions/deprecated.test`: both pairs expect the same two messages).

### 1.6 Parser quirks (verified)

All verified with the oracle (`work/q1.json` style cases, see §7.3).

1. **CONDITION + several templates breaks the fixture**: the greedy `(.*)` swallows every template but the last.
   `--TEST--\nx\n--CONDITION--\ntrue\n--TEMPLATE--\nA\n--TEMPLATE(b.twig)--\nB\n--DATA--…` parses to condition
   `"true\n--TEMPLATE--\nA\n"` and templates `['b.twig']` only. All 15 real CONDITION fixtures have one template.
2. Duplicate template names: `--TEMPLATE(a)--1 … --TEMPLATE(b)--2 … --TEMPLATE(a)--3` → `['a' => "\n3", 'b' => "\n2\n"]`.
3. Template name `0` maps to `index.twig` (PHP falsy string). Not used by any fixture.
4. Exception fixtures: CONFIG is not supported; DEPRECATION is parsed but **never compared** (the harness returns before
   the deprecation assertion). No fixture combines DEPRECATION with EXCEPTION.
5. Markers are found anywhere (not only at line start). A template containing the text `--TEMPLATE` or `--DATA--`
   would be split. None does.
6. `__DIR__` inside DATA is the directory of the PHP file that calls `eval` (in PHPUnit: `twig/src/Test`; in the oracle:
   `/oracle`). Only `tests/in.test` uses it, and its output does not depend on the value.

### 1.7 Port-ready parsing algorithm

String-scanning equivalent of the regexes; validated to give identical results to PHP for all 488 fixtures
(`python -I oracle/tools/fixture_parser_reference.py --check <fixtures-parsed.json> <Fixtures dir>` → `checked 488 fixtures, 0 mismatches`).

```
WS = " \t\n\x0B\x0C\r"
parse(text):
  t0 = find(text, "--TEST--");             if none: invalid
  it = find(text, "--TEMPLATE", t0);       if none: invalid
  head = text[t0+8 .. it]
  ic = find(head, "--CONDITION--"); id = find(head, "--DEPRECATION--")
  message     = strip_ws(head[0 .. min(ic, id, len(head))])
  condition   = ic>=0 ? lstrip_ws(head[ic+13 .. (id>=0 ? id : len(head))]) : ""
  deprecation = id>=0 ? strip_ws(head[id+15 ..]) : ""
  ie = find(text, "--EXCEPTION--", it)
  if ie >= 0:                                       # exception fixture
     idata = find(text, "--DATA--", it)
     end   = (idata>=0 && idata<ie) ? idata : ie
     block = rstrip_ws(text[it .. end])
     data  = (idata>=0 && idata<ie) ? lstrip_ws(text[idata+8 .. ie]) : ""
     exception = lstrip_ws(text[ie+13 ..])
     return {message, condition, deprecation, templates: split_templates(block), exception,
             outputs: [{data, config: null, expect: ""}]}
  idata = find(text, "--DATA--", it); require idata>=0 and find(text, "--EXPECT--", idata)>=0
  block = text[it .. idata]                         # no whitespace stripping here
  outputs = []
  p = find(text, "--DATA--")                        # first DATA in the whole file
  while p >= 0:
     e = find(text, "--EXPECT--", p+8);  if e < 0: break
     seg = text[p+8 .. e];  c = find(seg, "--CONFIG--")
     (data, config) = c>=0 ? (seg[..c], seg[c+10..]) : (seg, "")
     n = find(text, "--DATA--", e+10)
     expect = text[e+10 .. (n>=0 ? n : len)];  if n<0 and expect ends with "\n": drop that one "\n"
     outputs.push({data, config, expect});  p = n
  return {..., templates: split_templates(block), exception: false, outputs}

split_templates(block):
  pieces = substrings of block starting at each occurrence of "--TEMPLATE" up to the next one (or end)
  for each piece (index k):
     rest = piece after "--TEMPLATE"; name = ""
     if rest starts with "(": j = find(rest, ")--"); name = rest[1..j]; rest = rest[j+1..]
     body = rest[2..]                                     # skip "--"
     if k is last and body ends with "\n": drop one "\n"
     key = (name == "" or name == "0") ? "index.twig" : name
     templates[key] = body                                # ordered map, overwrite keeps first position
```

Recommendation: do not re-parse `.test` files in Rust at all; consume `oracle/corpus/fixtures-3.31.1.json` (§7.6),
which already contains the PHP parse (the cumulative-space rule of §1.5 remains the harness's job). If `.test` files
must be read directly (e.g. for new fixtures), port the algorithm above; the Rust `regex` crate has no lookahead, so a
literal regex port would need `fancy-regex`/`pcre2`.

---

## 2. How a fixture is executed and compared

### 2.1 doIntegrationTest() step by step

`IntegrationTestCase::doIntegrationTest($file, $message, $condition, $templateSources, $exception, $outputs, $deprecation = '')`,
`IntegrationTestCase.php:233-382`. Port-ready pseudocode:

```
if outputs is empty: return (no assertion)                         # legacy dummy
if condition != "": if !eval("$ret = " + condition + ";"): SKIP(condition)
for i, (_, data, config, expect) in outputs:
    options = {cache: false, strict_variables: true} merged with (config != "" ? eval(config + ";") : {})
    for each template: source += " " * i                           # cumulative, see 1.5
    twig = new Environment(new ArrayLoader(templateSources), options)
    twig.addGlobal("global", "global")
    add runtime loaders, extensions, filters, tests, functions,
        undefined filter/function/test/token-parser callbacks       # see 3.2
    deprecations = []
    # LOAD PHASE (deprecation handler always installed)
    try:
        for name in templateSources.keys() (file order): templates[name] = twig.load(name)
    catch \Exception e:
        if exception expected:
            assertSame(trim(exception), trim(class(e) + ": " + e.message))
            assertTrue(last char of e.message is "." or "?")
            return                                                 # remaining pairs are NOT run
        throw new Twig\Error\Error(class(e) + ": " + e.message, -1, null, e)   # test errors
    # RENDER PHASE (deprecation handler only if deprecation != "")
    try:
        output = trim(templates["index.twig"].render(eval(data + ";")), "\n ")
    catch \Exception e:
        if exception expected:
            assertStringMatchesFormat(trim(exception), trim(class(e) + ": " + e.message))
            return
        output = "Twig\Error\Error: " + class(e) + ": " + e.message   # (wrapped in Twig\Error\Error, -1, no source)
    if exception expected: FAIL 'Failed asserting that exception of type "<class before first ':'>" is thrown.'
    expected = trim(expect, "\n ")
    (on mismatch the compiled PHP of every template is printed as a debugging aid)
    assertEquals(expected, output, message + " (in " + file + ")")    # strict string equality
    assertSame(deprecation, implode("\n", deprecations))
```

Details:

* Templates are loaded **eagerly, in section order, all of them** — a syntax error in any template (even one never
  rendered) fails the load phase. `load()` also compiles and instantiates the template class: `use` traits are
  resolved at that point (`exceptions/undefined_trait.test` is a *load*-phase `RuntimeError`), while parents and
  includes are resolved at render (a missing parent is a *render*-phase `LoaderError`). The phases are **verified**
  (§5.3); that trait resolution happens in the compiled constructor is inferred (§8).
* Only `index.twig` is rendered. DATA is evaluated after loading, immediately before `render()`, inside the render
  `try` (exceptions thrown by DATA code count as render exceptions).
* `catch (\Exception $e)` does not catch `\Error` (`TypeError`, `ParseError` …): those error the test.
* `render(eval(…))`: DATA must return an array (`TemplateWrapper::render(array $context = [])`).
* PHPUnit's error handler is the "previous handler" for everything except `E_USER_DEPRECATED`; with Twig's
  `phpunit.xml.dist` (`convertErrorsToExceptions`, `convertNoticesToExceptions`, `convertWarningsToExceptions` = true,
  `error_reporting=-1`) any PHP warning/notice during load or render becomes a `PHPUnit\Framework\Error\*`
  exception (an `\Exception`), i.e. a failure or an "expected exception" mismatch.

### 2.2 Load phase vs render phase

| | load phase (`$twig->load()`) | render phase (`render()` incl. DATA eval) |
|---|---|---|
| typical errors | `SyntaxError` (lexer/parser/compile-time checks), errors from constructors of compiled templates (`use` traits) | `RuntimeError`, `LoaderError` (include/extends/embed targets), `SecurityError`s, `SyntaxError` from `template_from_string` |
| exception comparison | `assertSame` after `trim()` + message must end with `.`/`?` | `assertStringMatchesFormat` after `trim()` |
| deprecations captured | always | only if `--DEPRECATION--` present |

In 3.31.1: 56 exception fixtures fail at load (55 `SyntaxError`, 1 `RuntimeError`), 46 at render (33 `RuntimeError`,
11 `LoaderError`, 1 `SyntaxError`, 1 `Twig\Sandbox\SecurityNotAllowedFilterError`), 1 skipped (§5.3).

Rust implication: the split is observable only through the comparison mode. A Rust harness can use
"compile errors → exact match + punctuation rule, runtime errors → format match" if it separates compile from render
like PHP does; otherwise using format matching for everything accepts all 3.31.1 fixtures (the only `%` sequences in
load-phase expectations are `{% block %}` — `%` followed by a space is not a placeholder).

### 2.3 Exception comparison and format placeholders

The compared string is `sprintf('%s: %s', $e::class, $e->getMessage())`, i.e. **fully-qualified PHP class name**
(e.g. `Twig\Error\SyntaxError`, `Twig\Sandbox\SecurityNotAllowedFilterError`), colon, space, and the decorated
message. For `Twig\Error\Error` subclasses `getMessage()` is built by `Error::updateRepr()` (`twig/src/Error/Error.php:131-160`):

```
message = rawMessage
if message ends with "." or "?": remember it and strip it
if source && source.name != "": message .= sprintf(' in "%s"', name)
if lineno > 0: message .= sprintf(' at line %d', lineno); if columnno !== null: message .= sprintf(' column %d', columnno)
re-append the remembered punctuation
```

So `Unknown "foo" function.` at line 5 of `index` → `Unknown "foo" function in "index" at line 5.`; a raw message
without final punctuation stays without it (`… in "index.twig" at line 2` in `matches_error_runtime.test`); a `?`
moves to the end (`… Did you forget to put the content inside a {% block %} tag in "index.twig" at line 3?`).
Column is appended for some syntax errors (`… in "index.twig" at line 2 column 5.`).

PHPUnit 9.6 `assertStringMatchesFormat` (`vendor/phpunit/phpunit/src/Framework/Constraint/String/StringMatchesFormatDescription.php`,
verified): both strings have `\r\n` → `\n`; the format is `preg_quote`d then these replacements are applied with
`strtr`, and the result is matched as `/^…$/s`:

| placeholder | regex | meaning |
|---|---|---|
| `%%` | `%` | literal percent |
| `%e` | `\/` (DIRECTORY_SEPARATOR) | path separator |
| `%s` | `[^\r\n]+` | one or more non-newline chars |
| `%S` | `[^\r\n]*` | zero or more non-newline chars |
| `%a` | `.+` | one or more of anything |
| `%A` | `.*` | zero or more of anything |
| `%w` | `\s*` | optional whitespace |
| `%i` | `[+-]?\d+` | signed integer |
| `%d` | `\d+` | unsigned integer |
| `%x` | `[0-9a-fA-F]+` | hex |
| `%f` | `[+-]?\.?\d+\.?\d*(?:[Ee][+-]?\d+)?` | float |
| `%c` | `.` | one char |

Only use in fixtures: `expressions/matches_error_runtime.test`
`Twig\Error\RuntimeError: Regexp "3" passed to "matches" is not valid: Delimiter must not be alphanumeric%sbackslash%sin "index.twig" at line 2`
(PHP 8.5 actual: `… Delimiter must not be alphanumeric, backslash, or NUL byte in "index.twig" at line 2`).

### 2.4 Output comparison

* Actual: `trim($template->render($context), "\n ")`; expected: `trim($match[3], "\n ")` — both trim only `\n` and
  space, from both ends (leading indentation of the first line is trimmed too, e.g. `regression/missing-unwrap.test`).
* `assertEquals` on two strings is strict `!==` comparison (sebastian/comparator 4 `ScalarComparator`, verified).
  No line-ending normalisation: fixture files are LF (`.gitattributes` export-ignores tests; the git checkout is LF).
* Expected strings contain trailing spaces in the middle of the text in several fixtures (e.g. `filters/trim.test`,
  `tags/embed/nested.test`) — never strip lines individually.

### 2.5 Deprecation capture and comparison

Handler installed with `set_error_handler` around the load loop (always) and around render (only when
`'' !== $deprecation`), `IntegrationTestCase.php:301-309` and `:334-342`:

```php
$prevHandler = set_error_handler(static function ($type, $msg, $file, $line, $context = []) use (&$deprecations, &$prevHandler) {
    if (\E_USER_DEPRECATED === $type) {
        $deprecations[] = $msg;
        return true;
    }
    return $prevHandler ? $prevHandler($type, $msg, $file, $line, $context) : false;
});
```

* It ignores `error_reporting()`, so deprecations raised with `@trigger_error()` (that is how
  `trigger_deprecation()` works, `vendor/symfony/deprecation-contracts/function.php`:
  `@trigger_error(($package || $version ? "Since $package $version: " : '').($args ? vsprintf($message, $args) : $message), \E_USER_DEPRECATED);`)
  are recorded. Note the format `Since <package> <version>: ` — with an empty version this yields a space before
  the colon, e.g. `Since foo/bar : The "index.twig" template is deprecated…` (verified, §5.4).
* Order = emission order. Example `tests/null_coalesce.legacy.test`: `??` messages for lines 4,5,6,7,10 come before
  `~` messages for lines 9,11 — the order follows the parser's emission, not line numbers. A Rust port must emit in the
  same order or the joined string differs.
* Runtime deprecations without a DEPRECATION section are not captured; under PHPUnit they reach the Symfony
  PHPUnit-bridge handler (`@group legacy` tests: "Legacy deprecation notices", not a failure). In 3.31.1 these are the
  `{% deprecated %}` tag fixtures and the `sandboxed` include-argument fixtures (§5.4).

### 2.6 Conditions, skips, legacy tests

The 15 `--CONDITION--` expressions (verbatim):

| fixture(s) | condition | on PHP 8.5.13 |
|---|---|---|
| `functions/dump.test` | `!extension_loaded('xdebug')` | runs |
| `functions/dump_array.test` | `!extension_loaded('xdebug') \|\| (($r = new \ReflectionExtension('xdebug')) && version_compare($r->getVersion(), '2.2-dev', '<'))` | runs |
| 10 × `functions/enum*/…` | `\PHP_VERSION_ID >= 80100` | runs |
| `functions/template_from_string_error.test` | `PHP_VERSION_ID >= 80100` | runs |
| `functions/template_from_string_error_php80.test` | `PHP_VERSION_ID < 80100` | **skipped** |
| `regression/simple_xml_element.test` | `version_compare(phpversion(), '8.0', '<')` | **skipped** |

Legacy fixtures (`*.legacy.test`, 33) run through `testLegacyIntegration` with `@group legacy` /
`#[Group('legacy')]`; semantics are identical, only deprecation reporting by the Symfony bridge differs. They test
deprecated-but-supported 3.x behaviour (e.g. `spaceless` filter, macro calls without parentheses, `sandbox` tag,
`extends`/`use`/`macro` outside the template root, operator-precedence change warnings).

### 2.7 Global state that leaks between fixtures

* `date_default_timezone_set()` in DATA (10 fixtures) is never restored; PHPUnit runs fixtures in directory order,
  so later fixtures run with the leaked default timezone. Harmless in 3.31.1: every fixture that formats dates sets the
  timezone itself (§5.2), and all fixtures pass both under PHPUnit (leaking, directory order) and in the corpus export
  (one fresh process per fixture, default `UTC`). The initial default is `UTC` (§3.7).
* `CoreExtension::setTimezone()/setDateFormat()/setNumberFormat()` act on the per-case Environment (no leak).
* Classes declared in DATA are declared once per PHP process (names are unique across fixtures).
* Compiled template classes are cached per process by name `__TwigTemplate_<xxh128(…)>` (the appended spaces make
  each pair's source unique).

A Rust harness should reset "PHP default timezone" to `UTC` before each case and let the DATA translation set it.

### 2.8 NodeTestCase (compiled-PHP tests)

`twig/src/Test/NodeTestCase.php` (public test helper, used by the 47 `tests/Node/**` files):

* `testCompile($node, $source, $environment = null, $isPattern = false)` → `assertNodeCompilation()` compiles the
  node with `new Compiler($environment ?? $this->getEnvironment())` and compares `trim($compiler->getSource())`
  with `$source` via `assertEquals` (or `assertStringMatchesFormat` when `$isPattern`) (`:55-74`).
* Default environment: `new Environment(new ArrayLoader())` (`createEnvironment()`, `:94-97`), memoised per test instance.
* Helpers: `createVariableGetter($name, $line = false)` → `"// line N\n" . '($context["name"] ?? null)'`;
  `createAttributeGetter()` → `'CoreExtension::getAttribute($this->env, $this->source, '` (`:111-133`).
* Providers: static `provideTests()` (3.13+; base version triggers a deprecation and returns `[]`), legacy non-static
  `getTests()` triggers a deprecation from the `#[BeforeClass] checkDataProvider()` hook.

These assertions compare generated **PHP source** and cannot be ported as-is (§6).

---

## 3. Harness environment (tests/IntegrationTest.php)

`Twig\Tests\IntegrationTest extends Twig\Test\IntegrationTestCase` (`twig/tests/IntegrationTest.php`, 540 lines). It
overrides only `getExtensions()`, `getRuntimeLoaders()`, `getUndefinedFunctionCallbacks()`,
`getUndefinedTestCallbacks()`, `getUndefinedFilterCallbacks()` and `getFixturesDirectory()`; `getTwigFilters()`,
`getTwigFunctions()`, `getTwigTests()`, `getUndefinedTokenParserCallbacks()` return `[]` (base class).

### 3.1 Environment options

Twig defaults (`twig/src/Environment.php:123-130`): `debug=false`, `charset='UTF-8'`, `strict_variables=false`,
`autoescape='html'`, `cache=false`, `auto_reload=null` (→ follows debug), `optimizations=-1`, `use_yield=false`.

Harness: `array_merge(['cache' => false, 'strict_variables' => true], <CONFIG>)` (`IntegrationTestCase.php:251-254`).
So by default fixtures run with **strict_variables=true, autoescape='html', debug=false, optimizations=-1 (all),
use_yield=false**. CONFIG overrides used by fixtures: see §4.4. Twig's CI additionally runs the whole suite on PHP 8.2
after `sed -i -e "s/'use_yield' => false/'use_yield' => true/" src/Environment.php` (`twig/.github/workflows/ci.yml`),
so every fixture must give the same output with output buffering and with generator (`yield`) rendering — an
implementation detail with no Rust equivalent, but it confirms output does not depend on that mode (only
`regression/iterator_to_array.test` sets `use_yield` explicitly).

### 3.2 Globals, runtime loaders, registration order

`IntegrationTestCase.php:259-296`, in this order:

1. `new ArrayLoader($templateSources)`; `new Environment($loader, $config)` — the constructor registers, in order,
   `CoreExtension`, `EscaperExtension($options['autoescape'])`, `YieldNotReadyExtension($useYield)` (PHP ≥ 8.0) and
   `OptimizerExtension($optimizations)`, plus a default `FactoryRuntimeLoader` for `EscaperRuntime`
   (`twig/src/Environment.php:140-152`).
2. `$twig->addGlobal('global', 'global');` — visible in templates as `{{ global }}`; appears last in `_context`
   iteration and `dump()` (context = DATA + globals: `functions/dump_array.test` dumps keys `foo`, `bar`, `global`;
   `functions/include/with_context.test` lists `foo,global,_parent,`).
3. Runtime loaders: one `FactoryRuntimeLoader([TwigTestLazyExtensionWithAttributes::class => static fn () => new TwigTestLazyExtensionWithAttributes('+')])` (`IntegrationTest.php:69-76`).
4. Extensions (`IntegrationTest.php:55-67`), in this order:
   ```php
   $policy = new SecurityPolicy([], [], [], [], ['dump']);
   return [
       new DebugExtension(),
       new SandboxExtension($policy, false),
       new StringLoaderExtension(),
       new TwigTestExtension(),
       new TwigTestExtensionWithAttributes('*'),
       new AttributeExtension(TwigTestLazyExtensionWithAttributes::class),
   ];
   ```
5. `getTwigFilters()`, `getTwigTests()`, `getTwigFunctions()` (all empty here).
6. Undefined filter, function, test, token-parser callbacks (§3.5).

Later registrations override earlier ones with the same name: `ExtensionSet::initExtension()` does
`$this->filters[$name = $filter->getName()] = $filter;` for each extension in registration order (core first)
(`twig/src/ExtensionSet.php:503-526`). Hence the test extension's `nl2br` filter **replaces the core `nl2br`** in every
fixture (different implementation: it only handles `"\n"` and accepts a separator argument, used by
`tags/autoescape/with_filters_arguments.test` as `var|nl2br("<br />")`). Names containing `*` are additionally
registered as dynamic patterns `'#^'.str_replace('\\*', '(.*?)', preg_quote($name, '#')).'$#'` (so `*_foo_*_bar` →
`#^(.*?)_foo_(.*?)_bar$#`, captured groups become leading arguments). Lookup order in `getFilter()` (`:285-310`, same
for functions/tests): exact name → dynamic patterns in registration order → undefined-callable callbacks.

### 3.3 Core-package extensions: Debug, Sandbox, StringLoader

* **DebugExtension** (`twig/src/Extension/DebugExtension.php:21-62`): function `dump` with
  `needs_context`, `needs_environment`, `is_variadic`, and `is_safe => ['html']` **when `PHP_SAPI === 'cli'`** (always
  true under PHPUnit and in the oracle) or when Xdebug develop mode overloads `var_dump`. `dump()` returns `null` (prints
  nothing) unless `debug` is on; without arguments it `var_dump`s the context minus `Template`/`TemplateWrapper`
  values; with arguments `var_dump(...$vars)`. Output is PHP's `var_dump` format (`string(3) "foo"`, `array(3) {` …).
  Used by `functions/dump.test`, `functions/dump_array.test`, `tags/sandbox/array.legacy.test` (all with
  `debug => true, autoescape => false`).
* **SandboxExtension($policy, false)**: `SecurityPolicy($allowedTags = [], $allowedFilters = [], $allowedMethods = [],
  $allowedProperties = [], $allowedFunctions = ['dump'], $allowedTests = [])` (constructor at
  `twig/src/Sandbox/SecurityPolicy.php:32`). Second argument `false` = sandbox **not global**: it only applies inside
  `include(…, sandboxed = true)` and the deprecated `{% sandbox %}` tag. Example: `functions/include/sandbox.legacy.test`
  → `Twig\Sandbox\SecurityNotAllowedFilterError: Filter "e" is not allowed in "foo.twig" at line 4.` (the alias name
  as written is reported).
* **StringLoaderExtension**: function `template_from_string(template, name = null)` (`needs_environment`), which calls
  `Environment::createTemplate()`; the template name is `sprintf('%s (string template %s)', $name, hash('xxh128', $source))`
  or `__string_template__<hash>` without a name (`twig/src/Environment.php:469-488`), e.g.
  `foo.twig (string template 85e7b092afbbcd36f11981c2ef8f1569)` in `functions/template_from_string_error.test` (a Rust
  port needs XXH3-128 to reproduce such names).

### 3.4 Test-only extensions (full PHP code)

`IntegrationTest.php:218-376` verbatim (namespace `Twig\Tests`). The only token parser is `§`; there are no custom
node classes (the parser returns a core `PrintNode`).

```php
class TwigTestTokenParser_§ extends AbstractTokenParser
{
    public function parse(Token $token): Node
    {
        $this->parser->getStream()->expect(Token::BLOCK_END_TYPE);

        return new PrintNode(new ConstantExpression('§', -1), -1);
    }

    public function getTag(): string
    {
        return '§';
    }
}

class TwigTestExtension extends AbstractExtension
{
    public function getTokenParsers(): array
    {
        return [
            new TwigTestTokenParser_§(),
        ];
    }

    public function getFilters(): array
    {
        return [
            new TwigFilter('§', [$this, '§Filter']),
            new TwigFilter('escape_and_nl2br', [$this, 'escape_and_nl2br'], ['needs_environment' => true, 'is_safe' => ['html']]),
            new TwigFilter('nl2br', [$this, 'nl2br'], ['pre_escape' => 'html', 'is_safe' => ['html']]),
            new TwigFilter('escape_something', [$this, 'escape_something'], ['is_safe' => ['something']]),
            new TwigFilter('preserves_safety', [$this, 'preserves_safety'], ['preserves_safety' => ['html']]),
            new TwigFilter('static_call_string', 'Twig\Tests\TwigTestExtension::staticCall'),
            new TwigFilter('static_call_array', ['Twig\Tests\TwigTestExtension', 'staticCall']),
            new TwigFilter('magic_call', [$this, 'magicCall']),
            new TwigFilter('magic_call_closure', \Closure::fromCallable([$this, 'magicCall'])),
            new TwigFilter('magic_call_string', 'Twig\Tests\TwigTestExtension::magicStaticCall'),
            new TwigFilter('magic_call_array', ['Twig\Tests\TwigTestExtension', 'magicStaticCall']),
            new TwigFilter('*_path', [$this, 'dynamic_path']),
            new TwigFilter('*_foo_*_bar', [$this, 'dynamic_foo']),
            new TwigFilter('not', [$this, 'notFilter']),
            new TwigFilter('anon_foo', static function ($name) { return '*'.$name.'*'; }),
        ];
    }

    public function getFunctions(): array
    {
        return [
            new TwigFunction('§', [$this, '§Function']),
            new TwigFunction('safe_br', [$this, 'br'], ['is_safe' => ['html']]),
            new TwigFunction('unsafe_br', [$this, 'br']),
            new TwigFunction('static_call_string', 'Twig\Tests\TwigTestExtension::staticCall'),
            new TwigFunction('static_call_array', ['Twig\Tests\TwigTestExtension', 'staticCall']),
            new TwigFunction('*_path', [$this, 'dynamic_path']),
            new TwigFunction('*_foo_*_bar', [$this, 'dynamic_foo']),
            new TwigFunction('anon_foo', static function ($name) { return '*'.$name.'*'; }),
            new TwigFunction('deprecated_function', static function () { return 'foo'; }, ['deprecation_info' => new DeprecatedCallableInfo('foo/bar', '1.1', 'not_deprecated_function')]),
        ];
    }

    public function getTests(): array
    {
        return [
            new TwigTest('multi word', [$this, 'is_multi_word']),
            new TwigTest('test_*', [$this, 'dynamic_test']),
        ];
    }

    public function notFilter($value)
    {
        return 'not '.$value;
    }

    public function §Filter($value)
    {
        return "§{$value}§";
    }

    public function §Function($value)
    {
        return "§{$value}§";
    }

    /**
     * nl2br which also escapes, for testing escaper filters.
     */
    public function escape_and_nl2br($env, $value, $sep = '<br />')
    {
        return $this->nl2br($env->getRuntime(EscaperRuntime::class)->escape($value, 'html'), $sep);
    }

    /**
     * nl2br only, for testing filters with pre_escape.
     */
    public function nl2br($value, $sep = '<br />')
    {
        // not secure if $value contains html tags (not only entities)
        // don't use
        return str_replace("\n", "$sep\n", $value ?? '');
    }

    public function dynamic_path($element, $item)
    {
        return $element.'/'.$item;
    }

    public function dynamic_foo($foo, $bar, $item)
    {
        return $foo.'/'.$bar.'/'.$item;
    }

    public function dynamic_test($element, $item)
    {
        return $element === $item;
    }

    public function escape_something($value)
    {
        return strtoupper($value);
    }

    public function preserves_safety($value)
    {
        return strtoupper($value);
    }

    public static function staticCall($value)
    {
        return "*$value*";
    }

    public function br()
    {
        return '<br />';
    }

    public function is_multi_word($value)
    {
        return str_contains($value, ' ');
    }

    public function __call($method, $arguments)
    {
        if ('magicCall' !== $method) {
            throw new \BadMethodCallException('Unexpected call to __call.');
        }

        return 'magic_'.$arguments[0];
    }

    public static function __callStatic($method, $arguments)
    {
        if ('magicStaticCall' !== $method) {
            throw new \BadMethodCallException('Unexpected call to __callStatic.');
        }

        return 'static_magic_'.$arguments[0];
    }
}
```

Attribute-declared callables (`IntegrationTest.php:490-540`):

```php
class TwigTestExtensionWithAttributes extends AttributeExtension      // registered as new ...('*')
{
    public function __construct(private string $wrapper) {}
    #[AsTwigFunction('attribute_function')] public function attributeFunction(string $value): string { return $this->wrapper.$value.$this->wrapper; }
    #[AsTwigFilter('attribute_filter')]     public function attributeFilter(string $value): string   { return $this->wrapper.$value.$this->wrapper; }
    #[AsTwigTest('attribute_test')]         public function attributeTest(string $value): bool       { return $this->wrapper === $value; }
}

class TwigTestLazyExtensionWithAttributes extends AbstractExtension  // registered via new AttributeExtension(this::class);
{                                                                    // runtime instance new ...('+') from the FactoryRuntimeLoader
    public function __construct(private string $wrapper) {}
    #[AsTwigFunction('lazy_attribute_function')] public function attributeFunction(string $value): string { return $this->wrapper.$value.$this->wrapper; }
    #[AsTwigFilter('lazy_attribute_filter')]     public function attributeFilter(string $value): string   { return $this->wrapper.$value.$this->wrapper; }
    #[AsTwigTest('lazy_attribute_test')]         public function attributeTest(string $value): bool       { return $this->wrapper === $value; }
}
```

(Condensed formatting; bodies verbatim.) Expected by `extensions/attributes.test`: `*foo*`, `*foo*`, `yes`, `+foo+`, `+foo+`, `yes`.

Behaviour summary of the test callables (for a Rust re-implementation):

| name | kind | options | behaviour | fixtures |
|---|---|---|---|---|
| `§` | tag | — | prints `§` | `tags/special_chars.test` |
| `§` | filter / function | — | `"§{$v}§"` | `filters/special_chars.test`, `functions/special_chars.test` |
| `escape_and_nl2br` | filter | needs_environment, is_safe html | html-escape then replace `"\n"` → `"<br />\n"` | `tags/autoescape/with_filters.test` |
| `nl2br` (overrides core) | filter | pre_escape html, is_safe html | `str_replace("\n", "$sep\n", $v ?? '')`, `$sep='<br />'` | `filters/nl2br.test`, `tags/autoescape/with_filters_arguments.test`, `tags/autoescape/with_pre_escape_filters.test` |
| `escape_something` | filter | is_safe `['something']` | `strtoupper` | `tags/autoescape/with_pre_escape_filters.test`, `…/with_preserves_safety_filters.test` |
| `preserves_safety` | filter | preserves_safety html | `strtoupper` | `tags/autoescape/with_preserves_safety_filters.test` |
| `static_call_string`, `static_call_array` | filter + function | — | `"*$v*"` | `filters/static_calls.test`, `functions/static_calls.test` |
| `magic_call`, `magic_call_closure` | filter | — | `__call` → `'magic_'.$arg` | `functions/magic_call.test` |
| `magic_call_string`, `magic_call_array` | filter | — | `__callStatic` → `'static_magic_'.$arg` | `functions/magic_static_call.test` |
| `*_path` | filter + function (dynamic) | — | `$element.'/'.$item` (`foo_path` → element `foo`) | `filters/dynamic_filter.test`, `functions/dynamic_function.test` |
| `*_foo_*_bar` | filter + function (dynamic) | — | `"$foo/$bar/$item"` | same |
| `not` | filter | — | `'not '.$v` | `tests/in.test` |
| `anon_foo` | filter + function | — | `'*'.$v.'*'` | `extensions/anonymous_functions.test` |
| `safe_br` / `unsafe_br` | function | is_safe html / none | `'<br />'` | `tags/autoescape/functions.test` |
| `deprecated_function` | function | `DeprecatedCallableInfo('foo/bar', '1.1', 'not_deprecated_function')` | `'foo'` + compile-time deprecation `Since foo/bar 1.1: Twig Function "deprecated_function" is deprecated; use "not_deprecated_function" instead in index.twig at line N.` | `functions/deprecated.test` |
| `multi word` | test | — | `str_contains($v, ' ')` | `regression/multi_word_tests.test` |
| `test_*` | test (dynamic) | — | `$element === $item` (`'bar' is test_bar` → true) | `tests/dynamic_test.test` |
| `attribute_*`, `lazy_attribute_*` | function/filter/test | — | wrap with `*` / `+`; test compares with wrapper | `extensions/attributes.test` |

Other declarations in the file: namespace function `Twig\Tests\html()` returning `'foo'` ("check that escaping
strategies like html works even if a function with the same name is defined", `:46-51`) and `Twig\Tests\test_foo($value = 'foo')` (`:131-134`, unused by fixtures).

### 3.5 Undefined-callable callbacks

Verbatim (`IntegrationTest.php:78-123`):

```php
protected function getUndefinedFunctionCallbacks(): array
{
    return [
        static function (string $name) {
            if ('throwing_undefined_function' === $name) {
                throw new SyntaxError('This function is undefined in the tests.');
            }

            return false;
        },
    ];
}

protected function getUndefinedTestCallbacks(): array
{
    return [
        static function (string $name) {
            if ('throwing_undefined_test' === $name) {
                throw new SyntaxError('This test is undefined in the tests.');
            }
            if ('throwing_undefined_two words_test' === $name) {
                throw new SyntaxError('This test is undefined in the tests.');
            }

            // Ensure this does not conflict with `divisible by` and `same as`.
            if (\in_array($name, ['divisible', 'same'], true)) {
                return new TwigTest($name, static fn () => '');
            }

            return false;
        },
    ];
}

protected function getUndefinedFilterCallbacks(): array
{
    return [
        static function (string $name) {
            if ('throwing_undefined_filter' === $name) {
                throw new SyntaxError('This filter is undefined in the tests.');
            }

            return false;
        },
    ];
}
```

Used by `tags/guard/throwing_handler.test` (a `guard` on a name whose callback throws must take the `else` branch,
not fail) and implicitly by every fixture using `divisible by` / `same as` (`expressions/divisibleby.test`,
`expressions/sameas.test`, `tags/guard/basic.test`): the two-word test must win over the single-word test the callback
would provide.

### 3.6 Helper classes used by DATA sections (full PHP code)

Declared in `IntegrationTest.php` (so they exist only once that file is loaded), namespace `Twig\Tests`:

```php
class TwigTestFoo implements \Iterator                      // :136-216
{
    public const BAR_NAME = 'bar';
    public $position = 0;
    public $array = [1, 2];
    public static $foo = 'Foo';
    public function bar($param1 = null, $param2 = null) { return 'bar'.($param1 ? '_'.$param1 : '').($param2 ? '-'.$param2 : ''); }
    public function getFoo() { return 'foo'; }
    public function getEmpty() { return ''; }
    public function getNull() { return null; }
    public function getSelf() { return $this; }
    public function is() { return 'is'; }
    public function in() { return 'in'; }
    public function not() { return 'not'; }
    public function strToLower($value) { return strtolower($value); }
    public function rewind(): void { $this->position = 0; }
    #[\ReturnTypeWillChange] public function current() { return $this->array[$this->position]; }
    #[\ReturnTypeWillChange] public function key() { return 'a'; }
    public function next(): void { ++$this->position; }
    public function valid(): bool { return isset($this->array[$this->position]); }
}

class MagicCallStub                                        // :382-388 — asserts __call is NOT used by length/empty
{
    public function __call($name, $args): void { throw new \Exception('__call shall not be called.'); }
}

class ToStringStub                                         // :390-406
{
    private $string;
    public function __construct($string) { $this->string = $string; }
    public function __toString() { return $this->string; }
}

class CountableStub implements \Countable                  // :413-431 — Countable is preferred over __toString
{
    private $count;
    public function __construct($count) { $this->count = $count; }
    public function count(): int { return $this->count; }
    public function __toString() { throw new \Exception('__toString shall not be called on \Countables.'); }
}

class IteratorAggregateStub implements \IteratorAggregate  // :436-449
{
    private $data;
    public function __construct(array $data) { $this->data = $data; }
    public function getIterator(): \Traversable { return new \ArrayIterator($this->data); }
}

class SimpleIteratorForTesting implements \Iterator         // :451-488
{
    private $data = [1, 2, 3, 4, 5, 6, 7];
    private $key = 0;
    #[\ReturnTypeWillChange] public function current() { return $this->key; }
    public function next(): void { ++$this->key; }
    #[\ReturnTypeWillChange] public function key() { return $this->key; }
    public function valid(): bool { return isset($this->data[$this->key]); }
    public function rewind(): void { $this->key = 0; }
    public function __toString() { return str_repeat('X', iterator_count($this) + 10); } // length != iterator_count
}
```

(Condensed formatting; bodies verbatim.) Enums referenced by name from templates (`tests/DummyBackedEnum.php`,
`tests/DummyUnitEnum.php`): `enum DummyBackedEnum: string { case FOO = 'foo'; case BAR = 'bar'; }` and
`enum DummyUnitEnum { case BAR; case BAZ; }` (namespace `Twig\Tests`).

### 3.7 PHP runtime settings in effect

Verified in the oracle image (official `php:8.5-cli`, which ships **no php.ini**; only `memory_limit=-1` added):

| setting | value | relevance |
|---|---|---|
| PHP | 8.5.13 NTS, SAPI `cli` | `dump` is html-safe under CLI |
| `precision` / `serialize_precision` | `14` / `-1` | `(string)$float` uses `precision` (14 significant digits), `var_export`/`json_encode` use `serialize_precision` (shortest round-trip) |
| `date.timezone` / default timezone | `UTC` / `UTC` | initial default for fixtures that do not set it |
| locale | `LC_CTYPE=C.UTF-8`, all others `C` (env `LANG`/`LC_ALL` empty) | no locale-dependent formatting |
| `mb_internal_encoding()` / `default_charset` | `UTF-8` / `UTF-8` | |
| extensions | Core ctype curl date dom fileinfo filter hash iconv json lexbor libxml mbstring mysqlnd openssl pcre PDO pdo_sqlite Phar posix random readline Reflection session SimpleXML sodium SPL sqlite3 standard tokenizer uri xml xmlreader xmlwriter Zend OPcache zlib | no `intl`, no `xdebug` |
| `zend.assertions` | `1` | |
| PHPUnit config | `convertErrorsToExceptions`, `convertNoticesToExceptions`, `convertWarningsToExceptions` = true, `error_reporting=-1`, listener `Symfony\Bridge\PhpUnit\SymfonyTestsListener` (`twig/phpunit.xml.dist`) | warnings during render fail the fixture |

Example (verified, §7.4): Twig's `{{ 0.1 + 0.2 }}` prints `0.3` (string conversion of floats uses `precision=14`),
while `var_export(0.1 + 0.2)` gives `0.30000000000000004` (uses `serialize_precision=-1`); `{{ 123456789012345678.0 }}`
prints `1.2345678901235E+17`.

### 3.8 Checklist for a Rust fixture harness

1. Load fixtures from `oracle/corpus/fixtures-3.31.1.json` (or parse `.test` files with the §1.7 algorithm).
2. For each DATA/EXPECT pair `i`: append `i*(i+1)/2` spaces to every template; build a fresh environment with
   `strict_variables=true`, `cache` off, autoescape `html`, global `global='global'`, then apply CONFIG.
3. Register Rust equivalents of: Debug `dump` (var_dump formatting, html-safe), StringLoader `template_from_string`
   (xxh128 names), Sandbox (non-global, policy = only function `dump`), the `TwigTestExtension` callables and `§` tag,
   the attribute callables (`attribute_*` with `*`, `lazy_attribute_*` with `+`), and the three undefined-callable
   callbacks; register the test `nl2br` **after** core so it replaces it.
4. Load (compile) every template in section order; compile errors → compare `"<php class>: <message>"` exactly, and
   require the message to end with `.` or `?`; stop the fixture.
5. Build the context from the DATA translation (typed tree in the corpus), applying side effects: default timezone,
   `CoreExtension` timezone / date format / number format (the corpus records the resulting `state`, §7.6).
6. Render `index.twig`; runtime errors → format-match against `--EXCEPTION--`; otherwise compare
   `trim("\n ")`-ed output strictly.
7. Collect deprecations (compile-time always; runtime only if expected) and compare `join("\n")` exactly.
8. Map Rust error types to the PHP class names in messages: `Twig\Error\SyntaxError`, `Twig\Error\RuntimeError`,
   `Twig\Error\LoaderError`, `Twig\Sandbox\SecurityNotAllowedFilterError` (plus the other `SecurityNotAllowed*Error`
   classes for unit tests).

---

## 4. PHP code in DATA / CONFIG sections

### 4.1 Constructs with counts

502 DATA blocks in 488 fixtures (computed by regex over the PHP-parsed sections; "fixtures" = files containing the
construct in at least one DATA block). Script: `work/data_constructs.py` (scratchpad).

| Construct | fixtures | DATA blocks | examples |
|---|---:|---:|---|
| empty DATA section (exception fixtures only) | 19 | 19 | `exceptions/child_contents_outside_blocks.test`, `exceptions/syntax_error_in_reused_template.test`, `macros/varargs_argument.test` … |
| exactly `return []` (with or without `;`) | 289 | 290 | most fixtures |
| `array(...)` long syntax | 2 | 2 | `expressions/attributes.test`, `macros/unknown_macro_different_template.test` |
| double-quoted string with escapes (`\n`, `\xNN`) | 12 | 12 | `filters/nl2br.test`, `filters/characters_trailing_newline.test`, `tags/autoescape/type.test` … |
| invalid UTF-8 literal `"\xC3\x28abc"` | 5 | 5 | `expressions/matches_error_bad_utf8.test`, `filters/reverse_invalid_utf8.test`, `filters/shuffle_invalid_utf8.test`, `filters/split_invalid_utf8.test`, `functions/random_invalid_utf8.test` |
| float literal | 6 | 6 | `expressions/floats.test`, `filters/abs.test` … |
| `null`/`true`/`false` literal | 16 | 19 | `filters/default.test`, `tests/defined.test` … |
| `new \ArrayObject(...)` | 20 | 21 | `expressions/spread_*`, `filters/first.test`, `tags/include/only.test` … |
| `new \ArrayIterator(...)` | 12 | 12 | `expressions/has_every.test`, `filters/map.test` … |
| `(object) [...]` / `new \stdClass()` | 9 | 10 | `expressions/set.test`, `filters/length.test`, `tests/iterable.test` … |
| `new \Twig\Markup(...)` | 7 | 8 | `autoescape/markup_safe_strategies.test`, `regression/markup_test.test` … |
| `Twig\Tests\TwigTestFoo` | 10 | 12 | `expressions/method_call.test`, `filters/default.test`, `tests/defined.test` … |
| `Twig\Tests\ToStringStub` / `CountableStub` / `MagicCallStub` | 2 each | 2 each | `filters/length.test`, `tests/empty.test` |
| `Twig\Tests\IteratorAggregateStub` | 3 | 3 | `filters/filter.test`, `filters/find.test`, `filters/length.test` |
| `Twig\Tests\SimpleIteratorForTesting` | 1 | 1 | `filters/length.test` |
| `$twig->load('x')` (a `TemplateWrapper`) | 6 | 6 | `functions/block_with_template.test`, `tags/include/template_instance.test`, `tags/inheritance/template_instance.test` … |
| `$twig->getExtension(CoreExtension::class)->set*()` | 4 | 4 | `filters/date_default_format.test` (`setDateFormat('Y-m-d', '%d days %h hours')`), `filters/date_time_zone_conversion.test` (`setTimezone('UTC')`), `filters/number_format_default.test` (`setNumberFormat(2, '!', '=')`) |
| `date_default_timezone_set(...)` | 10 | 10 | `filters/date*.test`, `functions/date*.test` |
| `mktime(...)` | 6 | 6 | `filters/date.test` … |
| `new \DateTime(...)` / `\DateTimeImmutable` / `\DateTimeZone` / `\DateInterval` | 4 / 2 / 6 / 2 | same | `filters/date.test`, `filters/date_immutable.test`, `filters/date_interval.test`, `expressions/attributes.test` |
| `new \SimpleXMLElement(...)` | 5 | 5 | `filters/filter.test`, `filters/find.test`, `filters/length.test`, `filters/slice.test`, `regression/simple_xml_element.test` (skipped) |
| `new SplObjectStorage()` | 1 | 1 | `expressions/array.test` |
| `new \SplFileInfo(__DIR__)` + `opendir(__DIR__)` resource | 1 | 1 | `tests/in.test` |
| `new \CallbackFilterIterator($iter, function …)` (closures) | 1 | 1 | `tests/empty.test` |
| inline named class declaration | 6 | 6 | see §4.3 |
| anonymous class (`new class …`) incl. generator (`yield`) | 1 | 1 | `expressions/set.test` |
| PHP constant `DATE_W3C` | 2 | 2 | `functions/constant.test`, `tests/defined_for_constants.test` |
| local variables / several statements | 4 | 4 | `expressions/array.test`, `filters/column.test`, `tests/in_with_iterator.test`, `tests/in_with_objects.test` |

The only harness variable referenced by DATA is `$twig` (10 fixtures); `$this` appears only inside class bodies.

### 4.2 Classes and the behaviour fixtures rely on

Object classes that actually occur in evaluated DATA (from the typed corpus, number of fixtures):

| class | # | behaviour the fixtures depend on |
|---|---:|---|
| `ArrayObject` | 20 | ArrayAccess + IteratorAggregate + Countable; iteration preserves keys; `is sequence`/`is mapping` (`tests/sequence.test`, `tests/mapping.test`: empty `ArrayObject` is a sequence, not a mapping); `merge`, `join`, `sort`, `slice`, `first`/`last`, `reverse`, `shuffle`, spread `...`, `include … with`, `with` tag accept it; `constant('ARRAY_AS_PROPS', object)` = 2, `constant('class', object)` = `ArrayObject` |
| `ArrayIterator` | 12 | Iterator; call-argument unpacking `...it`, `has some/every`, `filter`/`map`/`reduce`, `column`, `is empty` |
| `Twig\Tests\TwigTestFoo` | 10 | method calls with positional/named args (`bar(param2: …)`), `getFoo` via `.foo`, `getSelf` via `.self`, methods named `is`/`in`/`not`, class constant `BAR_NAME` via `.BAR_NAME`, Iterator (`join` gives `1, 2`), `in` operator with circular objects (`tests/in_with_iterator.test`, `tests/in_with_objects.test`) |
| `stdClass` | 9 | dynamic public properties; `is mapping` → true; `length` → 1 (non-countable object); `object in 'stdClass'` false; `replace(stdClass)` error |
| `Twig\Markup` | 7 | safe output, `length` counts characters (`été` → 3), truthiness of empty/whitespace markup (`regression/markup_test.test`), `json_encode`, `matches`, per-strategy safety (`new Markup($s, 'UTF-8', ['html'])`) |
| `Twig\TemplateWrapper` | 6 | from `$twig->load()`; accepted by `include`, `extends`, `block(name, template)` |
| `DateTime` / `DateTimeImmutable` / `DateTimeZone` / `DateInterval` | 4 / 2 / 4 / 2 | `date`/`date_modify` filters, `date()` function; `date.timezone` attribute (Twig special-cases `date`, `timezone`, `timezone_type` on `DateTimeInterface` by reading `((array) $object)[$item]`, `twig/src/Extension/CoreExtension.php:1882-1888`) |
| `SimpleXMLElement` | 4 | iteration keys are element names (`elem/foo`), re-iterable, `length` = child count, `slice`, `find` |
| `Twig\Tests\IteratorAggregateStub` | 3 | IteratorAggregate (`filter`, `find`, `length` → 3) |
| `Twig\Tests\CountableStub` / `MagicCallStub` / `ToStringStub` | 2 each | `length`/`empty` prefer Countable over `__toString`, never use `__call`, use `__toString` length otherwise |
| `Twig\Tests\SimpleIteratorForTesting` | 1 | `length` of a plain Iterator = `iterator_count` (7), not `__toString` length |
| `SplObjectStorage` | 1 | ArrayAccess with object keys (`object_storage[object]`) |
| `SplFileInfo`, stream resource | 1 | string conversion inside `in` tests (`'' ~ resource`, `'' ~ dir_object`) |
| `CallbackFilterIterator` | 1 | `is empty` on filtered iterators |
| inline classes | 7 | see §4.3 |

### 4.3 Inline class declarations

| fixture | declares | behaviour exercised |
|---|---|---|
| `expressions/array_dynamic_keys.test` | `TwigTestStringy implements \Stringable` (ctor `private string $v`), `TwigTestHolder` (public `string $name = 'attr'`, typed property `TwigTestStringy $stringable`, method `getStringable()`) | Stringable objects as mapping keys (`{(obj): 'a'}` → key `ctx_obj`) |
| `expressions/magic_call.test` | `TestClassForMagicCallAttributes` (`getBar()`, `__call` returning `'foo_from_call'` for `foo`, else `false`) | `.foo` falls back to `__call`, `.bar` uses `getBar()` |
| `expressions/set.test` | anonymous class with `getName()`; anonymous `IteratorAggregate` with generator `getIterator()` and public `int $yielded` counter | object destructuring via getters; sequence destructuring from a Traversable stops after the needed items (`yielded` = 2) |
| `regression/block-inheritance-issue.test` | `MyObj` with `__toString()` returning `'foo'` | Stringable object used as array key (`hash[key]`) |
| `tags/autoescape/objects.test` | `UserForAutoEscapeTest` (`getName()`, `__toString()` returning `'Fabien<br />'`) | autoescaping of method results and of object string conversion |
| `tags/for/objects.test` | `ItemsIterator implements \Iterator` over `['foo' => 'bar', 'bar' => 'foo']` | `for` over an Iterator without Countable; the fixture only uses `loop.index`/`index0`/`first` because `length`, `revindex`, `revindex0`, `last` are not defined for non-Countable traversables (verified: with strict_variables `{{ loop.length }}` → `Key "length" for sequence/mapping with keys "parent, index0, index, first" does not exist in "index.twig" at line 1.`) |
| `tags/for/objects_countable.test` | `ItemsIteratorCountable implements \Iterator, \Countable` | with Countable: `loop.length`, `revindex`, `last` available |

### 4.4 CONFIG values

17 fixtures, 20 blocks:

| CONFIG | blocks | fixtures |
|---|---:|---|
| `return ['strict_variables' => false]` | 8 | `expressions/array`, `expressions/const`, `expressions/method_call`, `filters/default`, `tags/for/else`, `tests/defined`, `tests/defined_for_attribute`, `tests/defined_for_attribute.legacy` |
| `return ['strict_variables' => true]` | 2 | `filters/default`, `tags/for/loop_in_nested_sequence` |
| `return ['autoescape' => 'name']` | 2 | `autoescape/block`, `autoescape/name` |
| `return ['autoescape' => 'html']` / `['autoescape' => 'js']` | 1 / 1 | `autoescape/markup_safe_strategies` |
| `return ['debug' => true, 'autoescape' => false]` | 2 | `functions/dump`, `functions/dump_array` |
| `return ['autoescape' => false, 'debug' => true]` | 1 | `tags/sandbox/array.legacy` |
| `return ['use_yield' => true]` | 1 | `regression/iterator_to_array` |
| `return []` | 1 | `filters/invoke` |

`autoescape => 'name'` is the file-extension strategy (`FileExtensionEscapingStrategy::guess`): `.html`→html,
`.js`→js, `.css`→css, `.txt`→false (see `tests/FileExtensionEscapingStrategyTest.php` table, §6.2).

### 4.5 Turning DATA into Rust values

* 290 DATA blocks are literally `return []`; together with simple array/scalar literals, **367 fixtures need no PHP
  object at all** (and of those, 262 also need no exception/deprecation/date/sandbox/custom callable/string-loader/
  debug/enum/constant support).
* Use the typed trees in `oracle/corpus/fixtures-3.31.1.json` (`actual.cases[i].data`): every evaluated DATA context
  serialised as `{"t": "null"|"bool"|"int"|"float"|"string"|"array"|"object"|"enum"|"closure"|"resource"|"ref", …}`
  with ordered `[key, value]` pairs (int keys stay JSON ints, string keys JSON strings), floats as their `var_export`
  text (shortest round-trip, plus `INF`/`-INF`/`NAN`), invalid UTF-8 strings as `v` + `v_b64`, objects with class
  name and class-specific payload (ArrayObject/ArrayIterator `entries`, Markup `content`+`charset`, DateTime
  `datetime`/`timezone`/`timestamp`, DateTimeZone `name`, DateInterval `spec`, TemplateWrapper `template`, SimpleXML
  `xml`, otherwise `interfaces`, `toString` and public `props`). Each case also records `state`: PHP default
  timezone and `CoreExtension` timezone/date format/number format after DATA ran, e.g.
  `filters/date_time_zone_conversion.test` → `php_default_timezone: Europe/Paris`, `core_timezone: UTC`.
* Objects need a Rust "host object" model: public properties, case-insensitive methods with `get`/`is`/`has`
  fallbacks and `__call`, class constants, ArrayAccess, Countable, Traversable (Iterator/IteratorAggregate),
  Stringable. Test helper classes (§3.6) and inline classes (§4.3) must be re-implemented as Rust test types.
* Dates: `mktime(13, 45, 0, 10, 4, 2010)` is evaluated in the DATA's default timezone (Europe/Paris →
  1286192700); the corpus contains the resulting ints and DateTime timestamps, so Rust tests can use literals.

---

## 5. Fixture catalogue and classification

Counts computed from the corpus (`work/corpus_flags.py`, `work/matrix.py`). Flags are heuristic regex scans of
templates/DATA (custom-callable detection has one false positive, `tags/macro/special_chars.test`, which defines a
*macro* named `§`; real count 22). "dates" also flags 2 compile-error fixtures that merely call `date(…)`
(`expressions/call_argument_defined_twice.test`, `expressions/call_positional_arg_after_named_arg.test`).

### 5.1 Per-directory matrix

Columns: exc = expects exception, depr = expects deprecations, objects = PHP objects in DATA, helpers = `Twig\Tests\*`
classes, custom = test-only callables, strload = `template_from_string`, debug = `dump`, config = has CONFIG,
multi = several DATA/EXPECT pairs, pure-out = core-only + plain data + expected output, skip = skipped on PHP 8.5.

| directory | total | exc | depr | legacy | objects | helpers | dates | custom | sandbox | strload | debug | config | multi | pure-out | skip |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `autoescape` | 3 |  |  |  | 1 |  |  |  |  |  |  | 3 | 1 | 2 |  |
| `exceptions` | 14 | 14 |  |  |  |  |  |  |  |  |  |  |  |  |  |
| `expressions` | 55 | 10 |  |  | 14 | 3 | 3 |  |  |  |  | 3 | 1 | 31 |  |
| `extensions` | 2 |  |  |  |  |  |  | 2 |  |  |  |  |  |  |  |
| `filters` | 72 | 5 | 2 | 2 | 25 | 5 | 8 | 4 |  |  |  | 2 | 1 | 34 |  |
| `functions` | 38 | 13 | 1 | 2 | 4 | 1 | 2 | 6 |  | 4 | 2 | 2 | 1 | 10 | 1 |
| `functions/enum` | 5 | 4 |  |  |  |  |  |  |  |  |  |  |  |  |  |
| `functions/enum_cases` | 5 | 4 |  |  |  |  |  |  |  |  |  |  |  |  |  |
| `functions/include` | 17 | 5 |  | 3 | 1 |  |  |  | 3 |  |  |  |  | 9 |  |
| `functions/include_only` | 5 |  |  |  |  |  |  |  |  |  |  |  |  | 5 |  |
| `macros` | 30 | 12 | 4 | 4 |  |  |  |  |  |  |  |  |  | 14 |  |
| `operators` | 5 |  | 3 | 3 |  |  |  |  |  |  |  |  |  | 2 |  |
| `regression` | 10 | 1 |  |  | 2 |  |  | 1 |  |  |  | 1 |  | 5 | 1 |
| `tags` (root) | 2 |  |  |  |  |  |  | 1 |  |  |  |  |  | 1 |  |
| `tags/apply` | 7 |  |  |  |  |  |  |  |  |  |  |  |  | 7 |  |
| `tags/autoescape` | 15 |  |  |  | 1 |  |  | 5 |  |  |  |  |  | 9 |  |
| `tags/block` | 4 | 1 |  |  |  |  |  |  |  |  |  |  |  | 3 |  |
| `tags/deprecated` | 5 |  |  | 5 |  |  |  |  |  |  |  |  |  | 5 |  |
| `tags/embed` | 9 | 1 |  |  |  |  |  |  |  |  |  |  |  | 8 |  |
| `tags/for` | 16 | 1 |  |  | 2 |  |  |  |  |  |  | 2 | 1 | 13 |  |
| `tags/guard` | 6 | 1 | 1 | 1 |  |  |  | 1 |  |  |  |  |  | 3 |  |
| `tags/if` | 3 |  |  |  |  |  |  |  |  |  |  |  | 2 | 3 |  |
| `tags/include` | 11 | 4 |  |  | 3 |  |  |  |  |  |  |  |  | 4 |  |
| `tags/inheritance` | 41 | 9 | 3 | 3 | 2 |  |  |  |  |  |  |  | 1 | 27 |  |
| `tags/macro` | 40 | 12 | 2 | 2 |  |  |  | 1* |  | 1 |  |  |  | 24 |  |
| `tags/sandbox` | 4 | 2 | 2 | 4 |  |  |  |  | 4 |  | 1 | 1 |  |  |  |
| `tags/set` | 9 | 1 |  |  |  |  |  |  |  |  |  |  |  | 8 |  |
| `tags/use` | 13 |  |  |  |  |  |  |  |  |  |  |  |  | 13 |  |
| `tags/verbatim` | 2 |  |  |  |  |  |  |  |  |  |  |  |  | 2 |  |
| `tags/with` | 8 | 2 |  |  | 1 |  |  |  |  |  |  |  |  | 5 |  |
| `tests` | 26 | 1 | 3 | 4 | 12 | 5 |  | 2 |  |  |  | 3 | 3 | 9 |  |
| `whitespace` | 6 |  |  |  |  |  |  |  |  |  |  |  |  | 6 |  |
| **total** | **488** | **103** | **21** | **33** | **68** | **14** | **13** | **23*** | **7** | **5** | **3** | **17** | **11** | **262** | **2** |

`*` includes the `tags/macro/special_chars.test` false positive.

Other totals: templates per fixture 1 → 341, 2 → 108, 3 → 32, 4 → 6, 5 → 1 (682 templates); every fixture has
`index.twig`; 15 have a CONDITION.

### 5.2 Category lists

* **needs dates (13)**: `expressions/attributes`, `expressions/call_argument_defined_twice`*, `expressions/call_positional_arg_after_named_arg`*,
  `filters/date`, `filters/date_default_format`, `filters/date_default_format_interval`, `filters/date_immutable`,
  `filters/date_interval`, `filters/date_modify`, `filters/date_namedargs`, `filters/date_time_zone_conversion`,
  `functions/date`, `functions/date_namedargs` (`*` compile-error only). Every runtime date fixture sets its timezone in
  DATA except `expressions/attributes` (uses an explicit `DateTimeZone('Europe/Paris')` and `now`, output `Europe/Paris`).
* **needs sandbox (7, all legacy)**: `functions/include/sandbox.legacy`, `functions/include/sandbox_disabling.legacy`,
  `functions/include/sandbox_disabling_ignore_missing.legacy`, `tags/sandbox/array.legacy`, `tags/sandbox/not_valid1.legacy`,
  `tags/sandbox/not_valid2.legacy`, `tags/sandbox/simple.legacy`.
* **needs test-only callables (22)**: `extensions/anonymous_functions`, `extensions/attributes`, `filters/dynamic_filter`,
  `filters/nl2br`, `filters/special_chars`, `filters/static_calls`, `functions/deprecated`, `functions/dynamic_function`,
  `functions/magic_call`, `functions/magic_static_call`, `functions/special_chars`, `functions/static_calls`,
  `regression/multi_word_tests`, `tags/autoescape/functions`, `tags/autoescape/with_filters`,
  `tags/autoescape/with_filters_arguments`, `tags/autoescape/with_pre_escape_filters`,
  `tags/autoescape/with_preserves_safety_filters`, `tags/guard/throwing_handler`, `tags/special_chars`,
  `tests/dynamic_test`, `tests/in` (`not` filter).
* **needs StringLoader (5)**: `functions/include_template_from_string`, `functions/template_from_string`,
  `functions/template_from_string_error`, `functions/template_from_string_error_php80` (skipped), `tags/macro/import_from_string_template`.
* **needs Debug `dump` (3)**: `functions/dump`, `functions/dump_array`, `tags/sandbox/array.legacy`.
* **uses harness global / `_context` listing (9)**: `functions/dump_array`, `functions/include/with_context`,
  `tags/include/only`, `tags/macro/global`, `tags/with/globals`, `tags/with/precedence`, plus 3 false positives that only
  pass `with_context=…` (`exceptions/multiline_function_*`, `functions/include/ignore_missing`).
* **PHP constants (3)**: `functions/constant` (`DATE_W3C`, `ARRAY_AS_PROPS` on an object, `class`), `tests/constant`
  (`E_NOTICE` = 8, `Twig\Tests\TwigTestFoo::BAR_NAME`), `tests/defined_for_constants` (`FOOBAR` undefined).
* **enums (10)**: `functions/enum/*`, `functions/enum_cases/*` (refer to `Twig\Tests\DummyBackedEnum` / `DummyUnitEnum`).
* **PHP test helper classes (14)**: `expressions/const`, `expressions/dynamic_attribute`, `expressions/method_call`,
  `filters/default`, `filters/filter`, `filters/find`, `filters/join`, `filters/length`, `functions/attribute.legacy`,
  `tests/defined`, `tests/empty`, `tests/in_with_iterator`, `tests/in_with_objects`, `tests/null_coalesce`.
* **multi-output (11)**: listed in §1.5. **has CONFIG (17)**: listed in §4.4.
* **skipped on PHP 8.5 (2)**: `functions/template_from_string_error_php80`, `regression/simple_xml_element`.

The complete per-fixture flag set is in the corpus (`flags` object per fixture) and in
`<scratch>\work\categories.json`.

### 5.3 Exception fixtures by phase and class

Verified by running every fixture (§7.6):

* **load / `Twig\Error\RuntimeError` (1)**: `exceptions/undefined_trait`.
* **load / `Twig\Error\SyntaxError` (55)**: `exceptions/child_contents_outside_blocks`, `exceptions/child_print_outside_blocks`,
  `exceptions/multiline_function_with_unknown_argument`, `exceptions/syntax_error_in_reused_template`, `exceptions/unclosed_tag`,
  `expressions/call_argument_defined_twice`, `expressions/call_argument_unpacking_before_normal`,
  `expressions/call_positional_arg_after_named_arg`, `expressions/dot_as_concatenation`, `expressions/matches_error_compilation`,
  `expressions/two_word_operators_as_variables`, `expressions/underscored_numbers_error`, `filters/arrow_reserved_names`,
  `functions/attribute_with_wrong_args.legacy`, `functions/block_without_name`, `functions/cycle_without_enough_args`,
  `functions/enum/invalid_enum`, `functions/enum/invalid_enum_escaping`, `functions/enum/invalid_literal_type`,
  `functions/enum_cases/invalid_enum`, `functions/enum_cases/invalid_enum_escaping`, `functions/enum_cases/invalid_literal_type`,
  `functions/max_without_args`, `functions/parent_outside_of_a_block`, `macros/duplicate_argument`,
  `macros/duplicate_reserved_argument`, `macros/varargs_argument`, `macros/variadic_default_value`, `macros/variadic_duplicate`,
  `macros/variadic_invalid_name`, `macros/variadic_not_last`, `tags/block/block_unique_name`, `tags/for/reserved_names`,
  `tags/guard/exception`, `tags/inheritance/conditional_block`, `tags/inheritance/extends_in_block`,
  `tags/inheritance/extends_in_block_after_parent`, `tags/inheritance/extends_in_condition_in_block`,
  `tags/inheritance/extends_in_macro`, `tags/inheritance/extends_in_macro_after_parent`, `tags/inheritance/multiple_extends`,
  `tags/inheritance/parent_without_extends`, `tags/macro/argument_reserved_names`,
  `tags/macro/colon_not_supported_as_default_separator`, `tags/macro/from_embed_with_global_macro`,
  `tags/macro/from_in_block_is_local`, `tags/macro/from_nested_blocks`, `tags/macro/from_reserved_names`,
  `tags/macro/from_syntax_error`, `tags/macro/import_reserved_names`, `tags/macro/import_syntax_error`,
  `tags/sandbox/not_valid1.legacy`, `tags/sandbox/not_valid2.legacy`, `tags/set/reserved_names`, `tests/defined_on_complex_expr`.
* **render / `Twig\Error\LoaderError` (11)**: `exceptions/undefined_parent`, `exceptions/undefined_template_in_child_template`,
  `functions/include/{ignore_missing_exists,include_missing_extends,missing,missing_nested}`,
  `tags/include/{ignore_missing_exists,include_missing_extends,missing,missing_nested}`, `tags/inheritance/dynamic_parent_from_include`.
* **render / `Twig\Error\RuntimeError` (33)**: `exceptions/exception_in_extension_extends`, `exceptions/exception_in_extension_include`,
  `exceptions/multiline_array_with_undefined_variable`, `exceptions/multiline_array_with_undefined_variable_again`,
  `exceptions/multiline_function_with_undefined_variable`, `exceptions/multiline_tag_with_undefined_variable`,
  `expressions/matches_error_bad_utf8`, `expressions/matches_error_execution`, `expressions/matches_error_runtime`,
  `filters/replace_invalid_arg`, `filters/reverse_invalid_utf8`, `filters/shuffle_invalid_utf8`, `filters/split_invalid_utf8`,
  `functions/block_without_parent`, `functions/cycle_empty_mapping`, `functions/cycle_empty_sequence`,
  `functions/enum/invalid_dynamic_enum`, `functions/enum_cases/invalid_dynamic_enum`, `functions/random_invalid_utf8`,
  `functions/undefined_block`, `functions/undefined_block_deep`, `macros/constant_name_non_identifier`,
  `macros/dynamic_name_non_string`, `macros/dynamic_name_unknown`, `macros/unknown_macro`,
  `macros/unknown_macro_different_template`, `regression/combined_debug_info`, `tags/embed/error_line`,
  `tags/macro/import_embed_with_global_macro`, `tags/macro/import_in_block_is_local`, `tags/macro/import_nested_blocks`,
  `tags/with/with_no_mapping`, `tags/with/with_only`.
* **render / `Twig\Error\SyntaxError` (1)**: `functions/template_from_string_error` (compiled at runtime by `template_from_string`).
* **render / `Twig\Sandbox\SecurityNotAllowedFilterError` (1)**: `functions/include/sandbox.legacy`.
* **skipped (1)**: `functions/template_from_string_error_php80`.

### 5.4 Deprecation messages used by fixtures

Exact formats (note the inconsistent quoting of template names — each source formats differently):

| source | format (verbatim example) | fixtures |
|---|---|---|
| deprecated filter (`DeprecatedCallableInfo`) | `Since twig/twig 3.12: Twig Filter "spaceless" is deprecated in index.twig at line 2.` | `filters/spaceless*.legacy` |
| deprecated function with alternative | `Since foo/bar 1.1: Twig Function "deprecated_function" is deprecated; use "not_deprecated_function" instead in index.twig at line 2.` | `functions/deprecated` |
| macro call without parentheses | `Since twig/twig 3.29: Omitting parentheses when calling the macro "hello" is deprecated and will throw a SyntaxError in Twig 4.0; add parentheses after the macro name in "index.twig" at line 4.` (dynamic name: `… when calling a macro is deprecated …`) | `macros/call_without_parentheses.legacy` |
| duplicate macro | `Since twig/twig 3.29: Defining the macro "greet" more than once in "index.twig" is deprecated and will throw a SyntaxError in Twig 4.0 (previous definition at line 3, new definition at line 4). The last definition is used in Twig 3.` | `macros/duplicate_definition.legacy` |
| implicit optional macro args | `Since twig/twig 3.29: Not passing a value for the "a" argument of macro "test" is deprecated and the argument will be required in Twig 4.0; give it a default value in the macro definition or pass a value when calling it (in "index.twig" at line 10).` | `macros/implicit_optional_arguments.legacy` |
| implicit varargs | `Since twig/twig 3.29: Passing more arguments than the macro "test1" accepts is deprecated and will throw in Twig 4.0; declare a variadic argument ("...name") in the macro definition to accept extra arguments (in "index.twig" at line 12).` | `macros/varargs_implicit.legacy` |
| precedence change | `Since twig/twig 3.15: As the "~" infix operator will change its precedence in the next major version, add explicit parentheses to avoid behavior change in "index.twig" at line 2.` (also `"??"` infix 3.15, `"not"` prefix 3.15, `"\|"` infix 3.21) | `operators/*.legacy`, `tests/null_coalesce.legacy` |
| tag outside root | `Since twig/twig 3.28: Using the "extends" tag outside the root of a template is deprecated in index.twig at line 2.` (also `"use"`, `"macro"`) | `tags/guard/discarded_parent.legacy`, `tags/inheritance/*_in_*.legacy`, `tags/macro/macro_in_*.legacy` |
| sandbox tag | `Since twig/twig 3.15: The "sandbox" tag is deprecated in "index.twig" at line 2.` | `tags/sandbox/*.legacy` |
| defined test with parentheses | `Since twig/twig 3.29: Using parentheses when testing a macro with the "defined" test is deprecated and will throw a SyntaxError in Twig 4.0; remove the parentheses after the macro name in "index.twig" at line 5.` | `tests/defined_for_macros*_with_parentheses.legacy` |

Runtime deprecations raised by fixtures but **not compared** (no DEPRECATION section; verified actual texts):

* `functions/include/sandbox*.legacy`: `Since twig/twig 3.29: The "sandboxed" argument of the "include" function is deprecated, use the "render_sandboxed" function to render untrusted templates instead.`
* `tags/deprecated/block.legacy`: `The "welcome" block is deprecated, use "hello" instead. in "greeting.twig" at line 3.` (sic: `. in`)
* `tags/deprecated/macro.legacy`: `The "welcome" macro is deprecated, use "hello" instead. in "greeting.twig" at line 3.`
* `tags/deprecated/template.legacy`: `The "index.twig" template is deprecated, use "greeting.twig" instead. in "index.twig" at line 4.`
* `tags/deprecated/with_package.legacy`: `Since foo/bar : The "index.twig" template is deprecated, use "greeting.twig" instead. in "index.twig" at line 2.`
* `tags/deprecated/with_package_version.legacy`: `Since foo/bar 1.1: The "index.twig" template is deprecated, use "greeting.twig" instead. in "index.twig" at line 2.`

### 5.5 Behaviour facts surfaced by the fixtures

Selected non-obvious facts a port must reproduce (all from fixtures that pass on PHP 8.5.13):

* Line numbers start with the empty first line (§1.4); some syntax errors add `column N`
  (`expressions/underscored_numbers_error.test`: `… ("end of print statement" expected) in "index.twig" at line 2 column 5.`).
* `{% … %}` eats the newline directly after the tag; `{{ … }}` does not (`tags/include/basic` vs `functions/include/basic`).
* Context key order seen by an included template's `_context`: with context → data keys, globals, the `with`
  variables, `_parent` (`foo,global,foo1,_parent,`); without context (`only` / `with_context = false`) → the `with`
  variables, globals, `_parent` (`foo1,global,_parent,`) (`functions/include/with_context.test`, `tags/include/only.test`).
* `dump()` output is PHP `var_dump` text (`string(3) "foo"`), separate args separated by newline.
* Float printing uses PHP `precision=14` (`{{ 1.99E+3 }}` → `1990`, `{{ (-2)**(-3) }}` → `-0.125`).
* PCRE messages leak into Twig errors (`Malformed UTF-8 characters, possibly incorrectly encoded`,
  `Backtrack limit exhausted`, `No ending delimiter '/' found`, `Delimiter must not be alphanumeric, backslash, or NUL byte`).
* `Unable to split the string passed to "reverse" into characters: Malformed UTF-8 characters, possibly incorrectly encoded`
  (same for `shuffle`, `split`, `random`).
* `template_from_string` names use XXH128 of the source (32 hex chars).
* `{% deprecated %}` message quirk `". in"` (§5.4); `Since foo/bar :` with empty version.
* `date.timezone` on a DateTime returns the zone name (special case, §4.2); `date(timezone=false)` keeps the object's zone.
* `autoescape => 'name'` picks the strategy from the template name; `.txt` → no escaping.
* Arrays with integer-like string keys are normalised (`{'1': …} == {1: …}`, `expressions/array.test`).
* `max({2:"two", 1:"one", …})` compares values (`two`), `min` → `five` — PHP string comparison.
* `range(low=0+1, high=10+0, step=2)` named arguments; `'a'..'z'` letter ranges.

### 5.6 Coverage gaps (features with no fixture)

From a scan of all fixture templates (`work/coverage.py`): tests `none`, `null`, `true` and the null-safe operator `?.`
have **no fixture** (`?.` is covered by `ExpressionParserTest::testNullSafeOperator`); tags `flush`, `types`, `line`
and documentation comments `{## … #}` have no fixture (`flush` only appears in `Extension/SandboxTest`; `types` in
`TokenParser/TypesTokenParserTest`; `{% line N %}` in `LexerTest`; doc comments in `ParserTest` and
`TokenParser/BlockTokenParserTest`). Every core filter and function appears in at least one
fixture (`last`, `merge`, `slice`, `striptags`, `url_encode`, `capitalize`, `convert_encoding`, `invoke`, `filter`,
`find`, `min`, `source` in exactly one). Use the oracle to cover the rest.

---

## 6. PHPUnit test files: what they test, how to port

108 `*Test.php` files under `twig/tests` (plus helpers `DummyBackedEnum.php`, `DummyUnitEnum.php`,
`FilesystemHelper.php`, `Extension/Fixtures/*.php`, `Node/Expression/FilterTestExtension.php`,
`Profiler/Dumper/ProfilerTestCase.php`, `Fixtures/environment_options_hash.php`). On PHP 8.5.13 the whole suite is
2986 tests / 7241 assertions, all passing except 5 skips (§7.5). Many suites use
`Symfony\Bridge\PhpUnit\ExpectDeprecationTrait` (13 files) to assert deprecation messages.

### 6.1 Portability classes

* **A — behaviour**: assertions are on rendered output, error class/message/template line, or deprecation text.
  Port as Rust tests (template + context → output/error); expected values can be regenerated with the oracle.
* **B — component API**: lexer tokens, parser AST, loaders, wrappers, registries. Portable if twig-rs keeps an
  equivalent API; assertions must be rewritten against the Rust types.
* **C — generated PHP**: compares PHP source produced by the compiler or relies on PHP class generation. Not portable;
  cover the semantics with fixtures/oracle instead.
* **D — PHP infrastructure**: compiled-class cache files, opcache, phar, PSR-11, PHP attributes/reflection, legacy
  global functions, PHPUnit meta tests. Not applicable.

### 6.2 Per-file table

| file | tests | what it tests | class | porting notes |
|---|---:|---|---|---|
| `BlockChainTest.php` | 44 | `Twig\BlockChain` (3.31): composing blocks of several templates/chains, precedence, `parent()`, default vs render context, globals, sandbox policy re-checks, macro ownership, profiler attribution, error context, circular inheritance | A/B | only if BlockChain is implemented; 69 render calls; messages e.g. `A block chain requires at least one template.`, `Circular template inheritance detected while building a block chain from "one".` |
| `Cache/ChainTest.php` | 10 | chain of compiled-PHP caches | D | — |
| `Cache/FilesystemTest.php` | 12 | filesystem cache keys, write failures (needs non-root), opcache interplay | D | — |
| `Cache/ReadOnlyFilesystemTest.php` | 6 | read-only cache | D | — |
| `CallMacroTest.php` | 17 | macro namespace calls: positional+named args, parent templates, case-mismatched names (deprecation), legacy `macro_` prefix, unknown macro, macros imported in an unrendered body | A/B | messages: `Macro "missing" is not defined in template "index"`, `Macros imported in the body of template "index" are not available because the body was not rendered; move the "import" or "from" tag inside the block or the macro that uses it in "index" at line 1.` |
| `CompilerTest.php` | 3 | PHP literal emission (`'` → `\x27`), float `repr` independent of locale / `serialize_precision` | C | — (1 test skipped: no fr_FR locale) |
| `ContainerRuntimeLoaderTest.php` | 2 | PSR-11 runtime loader | D | — |
| `CustomExtensionTest.php` | 1 | invalid legacy `getOperators()` → `"Twig\Tests\InvalidOperatorExtension::getOperators()" must return an array of 2 elements, got 3.` | B | only with a legacy operator API |
| `DeprecatedCallableInfoTest.php` | 2 | deprecation text for deprecated callables, e.g. `Since foo/bar 1.1: Twig Function "foo" is deprecated; use "alt_foo" from the "all/bar" package (available since version 12.10) instead in foo.twig at line 1.`; without file/line: `… is deprecated.` / `… is deprecated in foo.twig.` | A | pure formatting — port 1:1 |
| `EnvironmentOptionsHashTest.php` | 13 | template class name includes resolved autoescape strategy, sandbox staging, options; determinism across processes (spawns PHP) | C/D | concept: cache keys must include options |
| `EnvironmentTest.php` | 41 | version constants, autoescape option/callback, wrapper from another env, globals lifecycle, extension registry (`Unable to register extension "…" as it is already registered.`), getExtension/getRuntime by class, undefined callbacks, cache/auto_reload, `use_yield` + legacy echoing node error, template class invalidation, source paths | B (some A) | port registry/globals/callbacks/messages; skip cache/class/path tests |
| `ErrorTest.php` | 15 | error enrichment with template name and line for missing variables and thrown exceptions in templates, includes, `parent()` blocks, child blocks; message `An exception has been thrown during the rendering of a template ("Runtime error...") in "index.html" at line 3.`; `filter`/`map`/`reduce` on null → `RuntimeError` at line 2; `Unknown "foo" function in "index" at line 5.`; line guessing without debug info (`Runtime error in "…".` with line 0) | A (D for `getFile()`/`getLine()`) | port name/line/message assertions |
| `ExpressionParserTest.php` | 40 | assignment targets, sequence/mapping AST, misplaced spread, string interpolation AST, destructuring, null-safe `?.` (render), invalid short-circuiting messages, macro default values must be constants, unknown function/filter/test + suggestions (`Unknown "cycl" function. Did you mean "cycle" in "index" at line 1?`), ready/not-ready callables, compiled code of dynamic callables, binding-power table (render-based, 76 cases comparing `{{ expr }}` with an explicitly parenthesised twin), two-word test precedence, custom unary precedence | A + B (+C) | port render & message tests directly; AST tests if AST mirrors Twig nodes; skip compiled-code tests |
| `Extension/AttributeExtensionTest.php` | 12 | `#[AsTwigFilter/Function/Test]` discovery and validation | D | — |
| `Extension/CoreTest.php` | 23 | CoreExtension helpers: `cycle`, `random` (incl. empty and non-UTF-8), `reverse` non-UTF-8, `first`/`last`, `keys`, `in`, `slice`, `compare` (incl. NAN), sandboxed include, include returns Markup / empty string | A/B | plain-data tables → unit tests of the Rust helpers |
| `Extension/EscaperTest.php` | 5 | custom escapers via `setEscaper`, per environment, safe classes | B | if custom-escaper API exists |
| `Extension/LegacyDebugFunctionsTest.php` | 1 | `twig_var_dump()` alias | D | — |
| `Extension/LegacyStringLoaderFunctionsTest.php` | 1 | `twig_template_from_string()` alias | D | — |
| `Extension/SandboxBridgeExtensionTest.php` | 10 | `render_sandboxed(name, vars, output_strategy)` (3.31) | A | only if implemented; `The "output_strategy" argument of the "render_sandboxed" function must be a non-empty literal string other than "all"` |
| `Extension/SandboxDeprecationsTest.php` | 5 | deprecations: `enableSandbox()`, `disableSandbox()`, `isSandboxedGlobally()`, include `sandboxed` argument | A | messages |
| `Extension/SandboxStateChangeTest.php` | 16 | policy/sandbox changes after first render, pre-warmed parents/macros | A | render-based |
| `Extension/SandboxTest.php` | 154 | full sandbox matrix: core tags (incl. `flush`), extends/use, parser-callable functions, tests (`constant`), strict mode, methods/properties/`__toString`, arrow functions (`The callable passed to the "filter" filter must be a Closure in sandbox mode in "index" at line 1.`), messages with `(string template <xxh128>)` names | A | main sandbox spec |
| `Extension/StringLoaderExtensionTest.php` | 1 | include of a string template without sandbox | A | trivial |
| `FactoryRuntimeLoaderTest.php` | 2 | factory runtime loader | D | — |
| `FileExtensionEscapingStrategyTest.php` | 1 (17 cases) | `guess()`: `foo.html`/`foo`/`foo.bar.twig`/`foo.txt/foo` → html; `foo.css`, `foo.twig.css`, `foo.js.css.twig` → css; `foo.js`, `foo.txt.twig/foo.js` → js; `foo.txt`, `foo.txt.twig` → false | A | pure function — port 1:1 |
| `IntegrationTest.php` | 0 (488 data sets) | the fixture runner | — | §1–§3 |
| `LexerTest.php` | 41 | token types/values/lines/offsets/columns, `{% line %}` directive, 100k-char tokens, big int literal kept as string, escaped delimiters (+deprecation), interpolation (nested, escaped `#{`), `#` in strings, operators ending with a letter at end of line, filter/attribute named like operators, inline comments `#`, unclosed/unexpected brackets with column, custom syntax, offsets with normalised line endings | B | port to the Rust lexer; messages exact (`Unclosed "variable" in "index" at line 3`, `Unexpected "}" in "index" at line 1 column N.`) |
| `Loader/ArrayTest.php` | 8 | ArrayLoader: missing template, cache key (duplicate content, edge collisions), setTemplate, isFresh | B | port |
| `Loader/ChainTest.php` | 6 | ChainLoader | B | port |
| `Loader/FilesystemTest.php` | 18 | FilesystemLoader: paths, `@namespace`, normalisation, `..` security, error messages (`Unable to find template "@named/nowhere.html" (looked into: …).`), cache, phar | B (phar D) | port with `tests/Loader/Fixtures` (requires CWD = repo root in PHP) |
| `MarkupTest.php` | 3 | `isSafeFor`: default safe for all; `all`; `html_attr` implies `html` and `html_attr_relaxed`; `withContent` keeps strategies | A | port 1:1 |
| `Node/**` (47 `NodeTestCase` subclasses: AutoEscape, BlockReference, Block, Deprecated, Do, Embed, For, If, Import, Include, Macro, Macros, Module, Print, Sandbox, Set, Text, Types, Expression/{Array, Conditional, Constant, Filter, Function, GetAttr, NullCoalesce, Parent, Test, Binary/*, Filter/{Escape,Raw}, Ternary/ConditionalTernary, Unary/*, Variable/*}) | 0–3 each | constructor attributes + **generated PHP source** | B / C | do not port compile assertions |
| `Node/NodeTest.php` | 10 | node `__toString()` tree dump, attribute/node deprecation helpers | B | optional |
| `Node/TextTest.php` | 2 | `isBlank()` true for ` `, `\t`, `\n`, `"\n\t\n   "` also with a UTF-8 BOM prefix (+ compile) | B | port `isBlank` |
| `Node/MacrosTest.php` | 2 | MacrosNode only accepts MacroNode (messages) | B | optional |
| `Node/ModuleTest.php` | 3 | module compilation, `use` names cannot inject PHP | C | — |
| `Node/Expression/CallTest.php` | 12 | argument resolution against PHP signatures (reflection), messages (`Unknown argument "unknown" for function "date(format, timestamp)".`, `Value for argument "from" is required for filter "replace" at line 1.`) | B/C | messages portable |
| `Node/Expression/FilterTest.php` | 3 | compile + wrong/missing named argument messages | B/C | messages portable |
| `Node/Expression/MacroReferenceTest.php` | 3 | macro reference with expression names, runtime resolution | B | partial |
| `NodeTraverserTest.php` | 3 | visitor priority order (lower first), replace/remove, children after enter | B | if visitor model mirrors |
| `NodeVisitor/CorrectnessTest.php` | 3 | body filtering in child templates (incl. BOM) | B/A | messages portable |
| `NodeVisitor/EscaperTest.php` | 3 | escaper runtime fetched only when needed (compiled), render results | C/A | outputs portable |
| `NodeVisitor/OptimizerTest.php` | 5 | optimizer: render block / parent block, for-loop variable optimisation | B | AST-level |
| `NodeVisitor/SafeAnalysisTest.php` | 1 | AST not retained after compile | D | — |
| `NodeVisitor/SandboxTest.php` | 4 | sandbox visitor wrapping (`CheckToStringNode`) in compiled code | C | — |
| `ParserTest.php` | 30 | unknown tag with suggestion (`Unknown "foo" tag. Did you mean "for" at line 1?`), reentrancy, var names, macro calls without parentheses (one deprecation per call site and line), implicit macro defaults, embedded template indices, child body cleanup, documentation comments, duplicate macro deprecations | A/B | deprecation/messages portable |
| `Profiler/ProfileTest.php` | 11 | Profile structure, durations, serialisation safety | B/D | only with profiler |
| `Profiler/Dumper/{Blackfire,Html,Text}Test.php` | 1/3/1 | dump formats (timings format-matched) | B | only with profiler |
| `Resources/LegacyCoreTest.php` | 13 | legacy `twig_*` functions enforce sandbox on callables | D | — |
| `Runtime/EscaperRuntimeTest.php` | 24 | escaping tables for `html` (`'`→`&#039;`), `html_attr` (`'`→`&#x27;`, `Ā`→`&#x0100;`, `\0`→`&#xFFFD;`, space→`&#x20;`), `html_attr_relaxed`, `js` (`<`→`\u003C`, `/`→`\/`, `😀`→`\uD83D\uDE00`), `css`, `url`; OWASP ranges; custom escapers; object escaping and safe classes; Markup strategies | A | **port 1:1 — pure tables** |
| `Sandbox/SandboxTest.php` | 37 | `Twig\Sandbox\Sandbox` (3.31 dedicated sandbox environment) | A | only if implemented |
| `SourceTest.php` | 3 | `getColumn()` 1-based incl. `\r\n` and `\r`, lazy path closure | B | port `getColumn` |
| `TemplateEscapeStrategyTest.php` | 5 | `getDefaultEscapeStrategy()` (name strategy, autoescape tag does not change it, embedded templates) | B | port |
| `TemplateTest.php` | 32 | `getAttribute` matrix over arrays, ArrayAccess, ArrayObject, stdClass, magic props/methods, getters (`get`/`is`/`has`), strict vs non-strict, `defined`; error table (`Impossible to access a key ("a") on a string variable ("foo") in "%s" at line 1.`, `Key "a" for sequence/mapping with keys "foo" does not exist …`, `Neither the property "a" nor one of the methods "a()", "geta()", "isa()", "hasa()" or "__call()" exist and have public access in class "…"`); sandbox checks; Stringable keys; printing arrays/objects (`Array to string conversion`) | A/B | **high value**; needs Rust host-object fixtures mirroring the PHP test classes |
| `TemplateWrapperTest.php` | 11 | `hasBlock`, `getBlockNames`, `renderBlock`, `displayBlock`, `streamBlock` with globals, dynamic parent errors | B | port |
| `Test/DataProviderCompatibilityTest.php` | 1 | data providers are static (PHPUnit 11) | D | — |
| `TokenParser/BlockTokenParserTest.php` | 7 | documentation comments `{## … #}` attached to blocks, whitespace control | B | if doc comments implemented |
| `TokenParser/GuardTokenParserTest.php` | 1 | `guard` with a throwing undefined-function callback | A | port |
| `TokenParser/TypesTokenParserTest.php` | 2 | `{% types %}` mapping attribute, docs | B | port |
| `TokenStreamTest.php` | 3 | `next()`/`look()`, `Unexpected end of template` | B | port |
| `TwigMacroTest.php` | 4 | `TwigMacro::callLegacy` errors (`Positional arguments cannot be used after named arguments for macro "input" in "index.twig" at line 7.`) and deprecations | B | messages portable |
| `Util/CallableArgumentsExtractorTest.php` | 15 | named/positional/default/variadic argument extraction for PHP callables, conversions, messages | B/C | messages portable |
| `Util/CallableParametersTest.php` | 2 | reflection-based string-coercion safety | D | — |
| `Util/DeprecationCollectorTest.php` | 1 | collect compile-time deprecations of templates | B | if implemented |

### 6.3 Error messages asserted by unit tests

Extracted with `grep -rhoE "expectExceptionMessage(Matches)?\(…"` over `twig/tests` (subset; all are exact unless a
regex is used). Useful as a message catalogue beyond the fixtures:

```
A "Twig\TemplateWrapper" can only be used with the "Twig\Environment" that created it in "theme" at line 1.
A default value for an argument must be a constant (a boolean, a string, a number, a sequence, or a mapping) in "index" at line 1
An argument must be a name. Unexpected token "string" of value "a" ("name" expected) in "index" at line 1 column 14.
Argument "format" is defined twice for function "date" in "test.twig" at line 2.
Block "foo" should not call parent() in "index.twig" as the block does not exist in the parent template "parent.twig"
Block "missing" on template "theme" does not exist in "theme".
Calling "__tostring" method on a "<FooObject class>" object is not allowed
Calling "withcontent" method on a "Twig\Markup" object is not allowed
Cannot destructure to an empty list of variables in "index" at line 1.
Empty array elements are only allowed in destructuring assignments
Failed to load Twig template "testFailLoadTemplate.twig", index "112233": cache might be corrupted in "testFailLoadTemplate.twig".
Filter "json_encode" is not allowed in "1_child" at line 3.
Macro "macro_missing" is not defined in template "index"
Positional arguments cannot be used after named arguments for function "date" in "test.twig" at line 2.
Tag "extends" is not allowed
The callable passed to the "filter" filter must be a Closure in sandbox mode in "index" at line 1.
The sandbox requires a strict security policy, call "setStrict(true)" on a dedicated policy for this sandbox.
The spread operator can only be used on sequence elements, mapping elements, and call arguments in "index" at line 1.
Unable to register extension "Twig\Tests\EnvironmentTest_Extension" as it is already registered.
Unclosed """
Unclosed "block" in "index" at line 3
Unclosed "variable" in "index" at line 3
Unexpected end of template
Unknown "cycl" function. Did you mean "cycle" in "index" at line 1?
Unknown "foo" tag. Did you mean "for" at line 1?
Unknown "foobar" filter in "index" at line 1.
Unknown "foobar" tag at line 1.
Unknown "lowe" filter. Did you mean "lower" in "index" at line 1?
Unknown "nul" test. Did you mean "null" in "index" at line 1
Unknown argument "foobar" for filter "date(format, timezone)" at line 1.
Unknown arguments "unknown1", "unknown2" for function "date(format, timestamp)".
Value for argument "from" is required for filter "replace" at line 1.
Value for argument "some_name" is required for function "custom_static_function" in "test.twig" at line 2.
```

---

## 7. The Docker PHP oracle

### 7.1 Files

All in `oracle/` (`<scratch>\survey\php-oracle\`):

| file | purpose |
|---|---|
| `Dockerfile` | `FROM php:8.5-cli`; composer 2.11.3; `git`, `unzip`; `memory_limit=-1`; `composer install` from the lock; symlink `vendor/twig/twig/vendor → /oracle/vendor`; non-root user `oracle` (uid 1000) |
| `composer.json` | requires `twig/twig: 3.31.1` (installed **from git source** via `preferred-install`, so `tests/` exists), dev: `phpunit/phpunit ^9.6.29`, `symfony/phpunit-bridge ^7.3`, `psr/container ^2.0`; `autoload-dev` maps `Twig\Tests\` → `vendor/twig/twig/tests/`; `config.platform.php = 8.5.13` |
| `composer.lock` | pins twig/twig `v3.31.1` at `6f6ec372231b53d1d564567a78beb8844cafa132` (same commit as the local checkout) and 33 other packages |
| `runner.php` | JSON batch runner (stdin → stdout), §7.3 |
| `lib.php` | helpers: diagnostics capture, error description, typed value export, integration/plain environments, verbatim fixture parser, faithful `run_fixture()`, PHPUnit format matcher |
| `run-suite.sh` | runs Twig's PHPUnit suite inside the image and prints counts and failures |
| `phpunit-twig.xml` | PHPUnit 9.6 config mirroring `twig/phpunit.xml.dist` (copied into the Twig root as `phpunit-oracle.xml`) |
| `oracle.sh` | host wrapper: builds the image if missing, pipes stdin to `docker run -i --rm twig-oracle:3.31.1 php runner.php`; `oracle.sh suite …` runs the suite |
| `tools/parse-fixtures.php` | dumps the PHP parse of all fixtures as JSON (standalone, any PHP ≥ 8.1) |
| `tools/export-corpus.php` | exports the full fixture corpus with actual results and typed DATA (§7.6) |
| `tools/fixture_parser_reference.py` | regex-free reference parser + `--check` against the PHP parse |
| `corpus/fixtures-3.31.1.json` | generated corpus (≈ 0.9 MB) |
| `examples/smoke.json`, `examples/smoke.out.json`, `examples/isolation.json` | sample batches and real output |
| `.dockerignore`, `.gitattributes` | `* text=auto eol=lf` so shell scripts survive a Windows checkout |

### 7.2 Build and pinned versions

From Windows Git Bash (tested):

```bash
cd <scratch>/survey/php-oracle
docker build -t twig-oracle:3.31.1 .          # ~1 min first time (apt + composer + git clone of twig)
./oracle.sh --pretty < examples/smoke.json    # render/eval batch
MSYS_NO_PATHCONV=1 docker run --rm twig-oracle:3.31.1 ./run-suite.sh integration
MSYS_NO_PATHCONV=1 docker run --rm twig-oracle:3.31.1 ./run-suite.sh all
MSYS_NO_PATHCONV=1 docker run --rm twig-oracle:3.31.1 php tools/export-corpus.php > corpus/fixtures-3.31.1.json
```

Regenerating the lock (only when changing composer.json):
`MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W):/app" -w /app composer:2.11.3 composer update --no-install`.

| component | version / digest (observed 2026-11-25) |
|---|---|
| base image | `php:8.5-cli` = `php@sha256:dbf32b33595283b79fd1a37ebc1614bdf1b83d3750daad9a57d67dbb9b96ec16` (Debian 13 trixie, PHP 8.5.13 NTS, built 2026-11-24) — tag is floating; pin the digest for strict reproducibility |
| composer | `composer:2.11.3` (`sha256:0049ce90b432795bf413a738b073963ada1a689e1a8708104684baafe94586ec`) |
| twig/twig | v3.31.1, source ref `6f6ec372231b53d1d564567a78beb8844cafa132` |
| phpunit/phpunit | 9.6.39 (Twig CI runs `vendor/bin/simple-phpunit`, whose default is `SYMFONY_PHPUNIT_VERSION` `'9.6'`, `vendor/symfony/phpunit-bridge/bin/simple-phpunit.php:100`; a second CI job uses 11.3, `twig/.github/workflows/ci.yml`) |
| symfony/phpunit-bridge | v7.4.24 (`SymfonyTestsListener`, `ExpectDeprecationTrait`) |
| symfony/polyfill-mbstring / -ctype / deprecation-contracts | v1.43.3 / v1.37.0 / v3.8.1 (polyfills inactive: real extensions present) |
| psr/container | 2.0.2 |

### 7.3 Runner protocol (JSON in, JSON out)

`php runner.php [--pretty]` reads one JSON document from stdin: an array of cases, or
`{"cases": [...], "isolation": "none"|"process"}`, or a single case object. Output is a JSON array of results in input
order (`JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_PRESERVE_ZERO_FRACTION`). Every case may carry an `id`
that is echoed. Kind = `kind` or inferred: `fixture` key → fixture, `php`/`php_code` → php, otherwise twig.

**(a) twig render**

| input field | meaning |
|---|---|
| `templates` | required object `name → source` (insertion order kept) |
| `main` | template to render; default `index.twig` if present, else the first |
| `data_json` | context as JSON object (JSON → PHP arrays; numeric-string keys become ints as in PHP) |
| `data_php` | PHP statements evaluated exactly like a `--DATA--` section: `eval($data_php.';')` with `$twig` in scope; must `return` an array. Code that contains no `return` keyword is treated as an expression (`['a' => new \ArrayObject([1])]` works) |
| `config` / `config_php` | Environment options (JSON / PHP `return [...]` or expression), merged over the env defaults |
| `env` | `integration` (default): the IntegrationTest setup of §3 (`cache=false`, `strict_variables=true` unless overridden, global `global`, all extensions and callbacks; `Twig\Tests\*` helper classes are loaded). `plain`: `new Environment(new ArrayLoader($templates), ['cache' => false] + config)` with Twig defaults |
| `extensions`, `globals`, `sandbox` | plain env only: `["debug","string_loader","sandbox"]`, `{"name": value}`, `{"global": bool, "policy": {"tags":[], "filters":[], "methods":{}, "properties":{}, "functions":[]}}` |
| `timezone` | `date_default_timezone_set()` before `data_php` runs; reset to UTC after the case |
| `load_all` | default `true`: load every template in order first (like the harness) so syntax errors anywhere surface in phase `load` |
| `errors` | `collect` (default): PHP warnings/notices recorded in `diagnostics` and execution continues; `throw`: converted to `\ErrorException` (≈ PHPUnit behaviour) |

Result: `{"id", "kind": "twig", "ok": true, "output": "…", "deprecations": [...], "diagnostics": [{"type": "E_WARNING", "message": "…", "silenced"?: true}]}`
or `{"ok": false, "phase": "setup"|"load"|"data"|"render", "error": {"class", "message", "formatted", "rawMessage", "templateName", "templateLine", "templateColumn", "previous"?}, …}`.
`formatted` is exactly the string the harness compares (`Class: message`). Non-UTF-8 strings come with `?`
substitution plus a `<field>_b64` copy (e.g. `"output": "?(abc", "output_b64": "wyhhYmM="`).

**(b) php eval**: `{"php": "<expression>"}` (wrapped as `return (EXPR);`) or `{"php_code": "<statements with return>"}`,
optional `timezone`, `errors`, `typed: true`. Result: `echo` (captured output), `type` (`get_debug_type`),
`var_export`, `var_dump`, `json` (+ `json_error`), `string` (PHP string conversion, for scalars/Stringable),
`typed` (optional typed tree, §4.5), `diagnostics`; on throw: `ok: false` + `error`.

**(c) fixture**: `{"fixture": "filters/abs.test", "typed": false}` (path relative to
`vendor/twig/twig/tests/Fixtures`). Runs a faithful copy of `doIntegrationTest()` and returns, per DATA/EXPECT pair,
`phase`, `output` (trimmed like the harness), `expected`, `error`, `pass` (harness verdict), `deprecations` (what the
harness compares) and `render_deprecations`. With `typed: true` also `data`, `config` and `state` (§4.5).
Fixture-mode differences from PHPUnit: warnings become `\ErrorException` instead of `PHPUnit\Framework\Error\Warning`.

**Isolation**: default `none` runs all cases in one PHP process (fast). A fatal error (e.g. redeclared class in
`data_php`, timeout) still yields valid JSON on stdout — the dying case gets `phase: "fatal"`, later ones
`phase: "lost"` — but the process exits 255 and prints the PHP fatal error on stderr. `"isolation": "process"` runs each
case in its own PHP process (≈ 30 ms per case), which also isolates `date_default_timezone_set()` and class declarations.

### 7.4 Sample session (real output)

`./oracle.sh --pretty < examples/smoke.json` (12 cases; full output in `oracle/examples/smoke.out.json`). Excerpts:

```text
render-json-data   templates {"index.twig": "{{ name|title }}: {{ 1.0 }} {{ 0.1 + 0.2 }} {{ 7 / 2 }} {{ 10 // 3 }} {{ -7 % 3 }} {{ [3, 1, 2]|sort|join(',') }}"}, data_json {"name": "hello world"}
  -> "output": "Hello World: 1 0.3 3.5 3 -1 1,2,3"
render-php-data    data_php "return ['obj' => new \Twig\Tests\TwigTestFoo(), 'list' => new \ArrayObject([1, 2, 3]), 'm' => new \Twig\Markup('été', 'UTF-8')]"
                   "{{ obj.foo }} {{ obj.bar('x') }} {{ list|length }} {{ m|length }}"
  -> "output": "foo bar_x 3 3"
render-error-runtime  "\n{% include 'missing.twig' %}"
  -> "phase": "render", "error": {"class": "Twig\\Error\\LoaderError",
     "message": "Template \"missing.twig\" is not defined in \"index.twig\" at line 2.",
     "rawMessage": "Template \"missing.twig\" is not defined.", "templateName": "index.twig", "templateLine": 2}
render-error-syntax   "{% for x in [1] %}{{ x }}"
  -> "phase": "load", "error": {"class": "Twig\\Error\\SyntaxError", "message": "Unexpected end of template in \"index.twig\" at line 1."}
plain-env-defaults    env plain, "[{{ undefined_var }}] {{ '<b>' }} {{ html }}", data_json {"html": "<b>"}
  -> "output": "[] <b> &lt;b&gt;"            (strict_variables off; literals are not escaped)
dates-with-timezone   timezone Europe/Paris, "{{ ts|date('Y-m-d H:i:s T P') }} / {{ ts|date('D, d M Y', 'America/New_York') }}", ts = mktime(13, 45, 0, 10, 4, 2010)
  -> "output": "2010-10-04 13:45:00 CEST +02:00 / Mon, 04 Oct 2010"
deprecation-captured  "{{ '42' ~ 1 + 41 }}"
  -> "output": "462", "deprecations": ["Since twig/twig 3.15: As the \"~\" infix operator will change its precedence in the next major version, add explicit parentheses to avoid behavior change in \"index.twig\" at line 1."]
php-eval-expr         "[1.0 + '1e3', 0.1 + 0.2, 7 <=> '7', '10' == '1e1', intdiv(-7, 2), -7 % 3, fmod(-7, 3)]"
  -> "var_export": "array (\n  0 => 1001.0,\n  1 => 0.30000000000000004,\n  2 => 0,\n  3 => true,\n  4 => -3,\n  5 => -1,\n  6 => -1.0,\n)"
php-eval-code         "$a = ['b' => 1, 'a' => 2, 10 => 3]; asort($a); echo implode(',', array_keys($a)); return sprintf('%05.1f|%x|%b|%e', 3.14159, 255, 5, 12345.678);"
  -> "echo": "b,a,10", "string": "003.1|ff|101|1.234568e+4"
php-eval-warning      "'5 apples' + 1"
  -> "var_export": "6", "diagnostics": [{"type": "E_WARNING", "message": "A non-numeric value encountered"}]
fixture-run           {"fixture": "tags/if/expression.test"}
  -> 3 cases, outputs "A1", "A2", "A3", all "pass": true
```

Other verified one-offs: `'abc' + 1` → `TypeError: Unsupported operand types: string + int`; `[] . ''` → `Array` +
`E_WARNING Array to string conversion`; `(string) 123456789012345678.0` → `1.2345678901235E+17`, Twig prints the same;
`(string) -0.0` → `-0`.

### 7.5 Running Twig's own PHPUnit suite (real results)

`run-suite.sh` runs `php /oracle/vendor/bin/phpunit -c phpunit-oracle.xml --testsuite <integration|all> --log-junit …`
from the Twig root (`/oracle/vendor/twig/twig`) as user `oracle`, then summarises the JUnit log. Results on
2026-11-25:

```text
$ MSYS_NO_PATHCONV=1 docker run --rm twig-oracle:3.31.1 ./run-suite.sh integration
PHPUnit 9.6.39 by Sebastian Bergmann and contributors.
...............................................................  63 / 488 ( 12%)
...
Time: 00:00.597, Memory: 34.00 MB
OK, but incomplete, skipped, or risky tests!
Tests: 488, Assertions: 954, Skipped: 2.
Legacy deprecation notices (8)
== twig/twig 3.31.1 suite "integration" on PHP 8.5.13: 488 tests, 486 passed, 0 failed, 0 errors, 2 skipped, 0 warnings, 954 assertions
   SKIP testIntegration with data set "functions/template_from_string_error_php80.test"
   SKIP testIntegration with data set "regression/simple_xml_element.test"

$ MSYS_NO_PATHCONV=1 docker run --rm twig-oracle:3.31.1 ./run-suite.sh all
Time: 00:01.477, Memory: 66.00 MB
OK, but incomplete, skipped, or risky tests!
Tests: 2986, Assertions: 7241, Skipped: 5.
Legacy deprecation notices (99)
== twig/twig 3.31.1 suite "all" on PHP 8.5.13: 2986 tests, 2981 passed, 0 failed, 0 errors, 5 skipped, 0 warnings, 7241 assertions
   SKIP Twig\Tests\CompilerTest::testReprNumericValueWithLocale            (no fr_FR locale in the image)
   SKIP Twig\Tests\IntegrationTest::testIntegration with data set "functions/template_from_string_error_php80.test"
   SKIP Twig\Tests\IntegrationTest::testIntegration with data set "regression/simple_xml_element.test"
   SKIP Twig\Tests\Node\Expression\CallTest::testResolveArgumentsWithMissingValueForOptionalArgument   (PHP >= 8.0)
   SKIP Twig\Tests\Util\CallableArgumentsExtractorTest::testResolveArgumentsWithMissingValueForOptionalArgument   (PHP >= 8.0)
```

Extra PHPUnit arguments pass through, e.g. `./run-suite.sh integration --filter 'filters/abs'`. The integration
count 488 = 455 `testIntegration` + 33 `testLegacyIntegration` data sets. Lessons from the first run (now fixed in the
image): running as root made `Cache/FilesystemTest::testWriteFailMkdir`/`testWriteFailDirWritable` fail (chmod is
ignored by root) and running outside the Twig root made `Loader/FilesystemTest::testPaths` fail (relative paths).

### 7.6 Fixture corpus export

`php tools/export-corpus.php [--only <substring>] [--pretty]` (inside the image, ≈ 15 s for all fixtures, one PHP
process per fixture) writes:

```json
{"generated_by": "php-oracle/tools/export-corpus.php", "twig_version": "3.31.1", "php_version": "8.5.13",
 "fixtures": [{
   "file": "filters/first.test", "legacy": false, "message": "\"first\" filter", "condition": "",
   "deprecation_expected": "", "exception_expected": false,
   "templates": {"index.twig": "\n{{ [1, 2, 3, 4]|first }}\n…"},
   "outputs": [{"data_php": "\nreturn ['arr' => new \\ArrayObject([1, 2, 3, 4])]\n", "config_php": "", "expect_raw": "\n1\n1\n1\n1\nÄ\n**\n**"}],
   "flags": {"custom_extension": [], "debug_extension": false, "string_loader_extension": false, "sandbox": false,
             "harness_global": false, "php_constants": false, "enum": false, "dates": false, "data_uses_twig": false,
             "data_defines_classes": false, "data_new_classes": ["ArrayObject"], "config_keys": []},
   "actual": {"kind": "fixture", "skipped": null, "cases": [{
       "index": 0, "phase": "render", "error": null, "output": "1\n1\n1\n1\nÄ\n**\n**", "expected": "…", "pass": true,
       "deprecations": [], "render_deprecations": [],
       "config": {"t": "array", "v": [["cache", {"t": "bool", "v": false}], ["strict_variables", {"t": "bool", "v": true}]]},
       "data": {"t": "array", "v": [["arr", {"t": "object", "class": "ArrayObject", "id": 541,
                "entries": {"t": "array", "v": [[0, {"t": "int", "v": 1}], [1, {"t": "int", "v": 2}], …]}}]]},
       "state": {"php_default_timezone": "UTC", "core_timezone": "UTC",
                 "core_date_format": ["F j, Y H:i", "%d days"], "core_number_format": [0, ".", ","]}}]}}]}
```

Verified: the corpus reproduces the PHPUnit verdict exactly — 486 fixtures pass, 0 fail, 2 skipped, 500 cases run.
`templates`/`outputs` are the PHP parse (templates without the cumulative spaces, which the harness adds per case).

### 7.7 Caveats

* The oracle reports **actual** PHP behaviour; for Twig fixtures that equals the expectation (all pass), but for new
  probes treat the oracle as the spec.
* `env: "integration"` loads `tests/IntegrationTest.php` and therefore PHPUnit classes; the namespace function
  `Twig\Tests\html()` and all helper classes exist in that process.
* `diagnostics` in collect mode swallow warnings; the same template under PHPUnit would fail. Use `"errors": "throw"`
  to see where PHPUnit would stop.
* Twig compiles templates to PHP classes named by an xxh128 hash of the source + options and keeps them for the
  process lifetime; identical sources across cases share a class (harmless — each case has its own Environment).
* Timezone is reset to UTC after every case (unlike PHPUnit, where it leaks).
* `oracle.sh` exports `MSYS_NO_PATHCONV=1` so Git Bash does not rewrite container paths; when calling `docker run`
  directly from Git Bash with `/oracle/...` arguments or `-v` mounts, set it yourself and use `$(pwd -W)` for mounts.
* `php:8.5-cli` is a moving tag; rebuilding later may give PHP 8.5.14+. Pin the digest above if exact 8.5.13 matters.
* Using it from Rust tooling: spawn `docker run --rm -i twig-oracle:3.31.1 php runner.php`, write one JSON batch to
  stdin, parse stdout. Each `docker run` costs about 1 s of container start-up, so batch many cases per call (the
  whole 488-fixture suite runs in under 1 s once PHP is up).

---

## 8. Unverified points and open questions

* **UNVERIFIED**: that the cumulative trailing spaces (§1.5) never change any expected output beyond trimming — inferred
  from all multi-output fixtures passing; not tested with a Rust harness that omits them.
* **UNVERIFIED**: the precise compile-time mechanism that makes `use` traits load-phase and parents render-phase is
  inferred from observed phases (§5.3), not traced through `ModuleNode::compile*`.
* **UNVERIFIED**: equivalence of `oracle/tools/fixture_parser_reference.py` with the PCRE regexes for arbitrary input —
  it is verified only on the 488 real fixtures (quirk #1 of §1.6 is deliberately not reproduced).
* Portability classes in §6.2 come from reading test names and skimming bodies, not from porting attempts; expect
  some B/C boundaries to move.
* PHPUnit 11 (Twig's second CI job) was not run; the oracle uses PHPUnit 9.6, Twig's default.
* The `harness_global` and custom-callable flags in the corpus are regex heuristics (known false positives listed in §5).

