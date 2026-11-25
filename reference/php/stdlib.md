# PHP builtins used by Twig 3.31.1 at runtime, mapped to php-src 8.5.13

Reference for porting Twig 3.31.1 to Rust (twig-rs) with byte-identical output and error messages.

- Twig source: `ext/twig-3.31.1/src` (paths below are relative to `src/` unless prefixed).
- php-src: `ext/php-src-8.5.13` (paths relative to the php-src root).
- Oracle: Docker `php:8.5-cli` = PHP 8.5.13, Debian 13, glibc 2.41 (`ICONV_IMPL='glibc'`, `ICONV_VERSION='2.41'`), bundled PCRE2 10.44 (Unicode 15.0.0, JIT on), bundled timezonedb 2026.6, `date.timezone=UTC` (built-in default).
- Evidence tags: **[V]** verified by running PHP 8.5.13 in Docker (scripts kept in `scratchpad/scripts/`, test files in `scratchpad/scripts/t/`); **[S]** read from source (file:line cited); **UNVERIFIED** = neither.

## Resolved since this survey

The implementation settled several open items and recommendations below; where the body disagrees, this list wins.

- Float digit generation (§2 item 14a), `hash('xxh128')` (14b) and `pow` for `round()` (14c) are verified by
  `twig-value` oracle vectors. `pow` is a port of glibc 2.41's FMA variant (2.05M oracle cases, 0 mismatches).
- iconv (§7, items 12/P3) is implemented in `twig-value` (`builtins::string`) from tables measured on the oracle's
  glibc for the common single-byte charsets and UTF variants, including TRANSLIT and the IGNORE byte-loss quirk;
  other charsets raise an explicit error (`reference/deviations.md`).
- Unicode case mapping (14e) uses tables converted from php-src's `unicode_data.h` (`twig-unicode`), checked per code
  point against the oracle.
- Dates (item 10, §13.6, 14f, 14g): re2c's Rust backend handles timelib's scanners; timelib is ported to pure Rust with
  PHP's embedded timezonedb 2026.6 (`twig-timelib`, `research/timelib-port/README.md`). Binding is not needed.
- Enum ordering in sorts (14h): stand-in documented in `reference/deviations.md`.
- Random (§12): seeded `mt_rand`/MT19937 sequences match PHP exactly.
- PCRE (§8): PHP flags strings converted from ints/floats as valid UTF-8 (matters for `/u` with `(*NOTEMPTY)`);
  `/(*UTF)\C/` switches the JIT off with "Allocation of JIT memory failed"; `matches null` raises PHP's null-argument
  deprecation; MARK names are cut at the first NUL byte.
- `ord('')` is deprecated in PHP 8.5 (§5.10). Variadic arguments have no name in coercion messages (§4.1).

## Table of contents

1. [Method and scope](#1-method-and-scope)
2. [Key findings (executive summary)](#2-key-findings-executive-summary)
3. [Inventory: which builtins Twig calls, and from where](#3-inventory-which-builtins-twig-calls-and-from-where)
4. [Cross-cutting PHP mechanics every port depends on](#4-cross-cutting-php-mechanics-every-port-depends-on)
5. [Strings](#5-strings)
6. [mbstring](#6-mbstring)
7. [iconv and charsets](#7-iconv-and-charsets)
8. [PCRE (preg_*)](#8-pcre-preg_)
9. [Math](#9-math)
10. [Arrays, sorting, iteration](#10-arrays-sorting-iteration)
11. [JSON (json_encode)](#11-json-json_encode)
12. [Random (random(), shuffle)](#12-random-random-shuffle)
13. [Date/time and timelib](#13-datetime-and-timelib)
14. [var_dump (dump function)](#14-var_dump-dump-function)
15. [Introspection, constants, enums](#15-introspection-constants-enums)
16. [SPL classes and interfaces used](#16-spl-classes-and-interfaces-used)
17. [Output buffering, flush, error handlers, misc](#17-output-buffering-flush-error-handlers-misc)
18. [PHP constants templates commonly reference](#18-php-constants-templates-commonly-reference)
19. [Reference: zend_sort port (validated)](#19-reference-zend_sort-port-validated)
20. [Reference: complete call-site table (generated)](#20-reference-complete-call-site-table-generated)
21. [Prioritised porting table](#21-prioritised-porting-table)

---

## 1. Method and scope

1. **Tokenizer scan** (`scratchpad/scripts/scan_builtins.php`, run in Docker on `src/`, never executing Twig code): every `T_STRING`/`T_NAME_FULLY_QUALIFIED` followed by `(` whose lowercase name is in `get_defined_functions()['internal']` and is not preceded by `->`, `?->`, `::`, `function`, `new`, `const`. Also scanned: string literals that are exactly an internal function name (string callables like `'abs'`), function names followed by `(` inside string literals (PHP code emitted by `Node::compile()`), and internal class references (`new`, `instanceof`, `::`, `extends/implements`). Raw result: `scratchpad/scripts/scan_raw.tsv` (1288 calls, 87 string-callable hits, 37 emitted-code hits, 245 class refs); 175 distinct internal functions.
2. Manual review of `Node/**` compile methods (operators and emitted constructs) and all runtime helpers (`Extension/CoreExtension.php` read completely, `Runtime/EscaperRuntime.php`, `Extension/DebugExtension.php`, `Markup.php`, `MacroNamespace.php`, `TwigMacro.php`, `Template.php`, `Environment.php`, `Sandbox/*`, `Error/*`).
3. Each runtime builtin was mapped to its php-src definition (`PHP_FUNCTION`/`ZEND_FUNCTION`/`PHP_METHOD`) and helpers; behaviours were verified with PHP 8.5.13 (plain PHP and by rendering real templates with Twig 3.31.1 inside Docker via `scratchpad/scripts/twig_run.php`).

Call-site classes used throughout (see section 20):

| class | meaning | twig-rs relevance |
|---|---|---|
| **R\*** | builtin registered directly as a Twig callable (string callable) or passed as a callback | must port exactly; arguments come straight from templates |
| **R** | called by runtime helpers while rendering | must port |
| **E** | emitted into compiled template PHP code by `Node::compile()` | must reproduce semantics of the emitted expression |
| **C** | compile time (lexer/parser/compiler/node visitors) | still runs inside twig-rs; only observable effects matter (literals, error messages) |
| **I** | infrastructure (filesystem cache, loaders, profiler, reflection, test helpers) | not ported as PHP builtins |

`extra/*` packages (intl-extra, string-extra, html-extra, markdown-extra, cssinliner, inky, cache-extra) are separate Composer packages and are **out of scope** (not under `src/`). intl-extra would require ICU (XL).

---

## 2. Key findings (executive summary)

1. **Directly exposed PHP builtins (R\*)**: filters `abs` and `json_encode`, functions `max`, `min`, `range`, test `iterable` are bare PHP functions (`Extension/CoreExtension.php:248,253,299-301,326`). Their exact PHP semantics *and* PHP error messages leak into templates. `round` filter calls `ceil`/`floor` via `$method(...)` (`:668-672`).
2. **Non-Twig exceptions are wrapped**: any `Error`/`TypeError`/`ValueError`/`DivisionByZeroError` thrown by a builtin becomes `RuntimeError('An exception has been thrown during the rendering of a template ("%s").')` + ` in "<name>" at line N.` (`Template.php:627`) [V]. Therefore PHP's internal messages (`abs(): Argument #1 ($num) must be of type int|float, string given`, `Modulo by zero`, `3 arguments are required, 2 given`, ...) are part of the observable surface.
3. **Warnings/notices/deprecations** (E_WARNING/E_NOTICE/E_DEPRECATED) do not stop rendering; with CLI defaults they are printed to stdout *inside* the output (`\nWarning: ... in <file> on line N\n`) [V]. twig-rs needs a diagnostics channel; verbatim texts are listed per builtin.
4. **PHP 8.5 specifics that change behaviour**: float→int casts of non-representable values now warn (`The float 1.0E+30 is not representable as an int, cast occurred`) and still use modular conversion; NaN→string/bool/... warns (`unexpected NAN value was coerced to string`); null array offsets / `array_key_exists(null, ...)` are deprecated; `ord()` with multi-byte strings and `chr()` outside 0..255 are deprecated; mbstring tables are **Unicode 17.0**; PCRE2 built without `PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK`; `DATE_RFC7231` deprecated.
5. **Two float→string conversions** exist: `precision=14` (`zend_gcvt` mode 2: echo, `~`, `(string)`, `implode`, `sprintf('%s')`, `strval`, `http_build_query`, `str_contains((string)$v)`) and `serialize_precision=-1` (shortest round-trip, `zend_gcvt` mode 0: `json_encode` with lowercase `e`, `var_dump`/`var_export` with `E`). Thresholds for exponent notation are identical code (`Zend/zend_strtod.c:4509`).
6. **`min()` tie-breaking depends on call shape** (frameless 2-arg variant vs variadic): `{{ min('1.0', 1) }}` → `1`, `{{ min(...['1.0', 1]) }}` → `1.0`, `{{ min(['1.0', 1]) }}` → `1.0` [V]. `max()` returns the left/first operand on ties in all shapes.
7. **Sorting** (`sort` filter = `asort`/`uasort`) is stable since PHP 8.0, but PHP comparison is not a total order across types, so results for mixed arrays depend on the exact algorithm. `zend_sort` (hybrid insertion sort ≤16 elements + libc++-style quicksort) was ported to PHP userland and validated bit-exact against native `usort` with an inconsistent comparator on 126 arrays (sizes 1..2000) plus a mixed-type `asort` [V] — section 19.
8. **Escaping** (`escape` filter, autoescape) needs `htmlspecialchars(ENT_QUOTES|ENT_SUBSTITUTE)` (UTF-8 decoder with exact U+FFFD substitution rules), and for js/css/html_attr only a UTF-8 validity check (`preg_match('//u')`, equivalent to RFC 3629 validation = Rust `std::str::from_utf8`), per-code-point mapping and `sprintf('%04X')`. No regex engine is needed to port escaping.
9. **mbstring** (`upper`, `lower`, `title`, `capitalize`, `length`, `slice`, `split`, `first`, `last`): full (multi-code-point) case mapping incl. SpecialCasing unconditional rules, final-sigma rule, title-case with Cased/Case_Ignorable properties, invalid bytes → `?` [V]. Needs Unicode 17.0 tables (`ext/mbstring/unicode_data.h`, 5457 lines, generated by `ucgendat.php`).
10. **Date/time** is the largest item: `DateTime`, `DateTimeImmutable`, `DateTimeZone`, `DateInterval::format`, `modify()`, lenient string parsing via **timelib** (`ext/date/lib`, MIT, version 2022.17; `parse_date.re` 2791 lines → re2c output `parse_date.c` 25641 lines; ~96 named sub-patterns and 51 scanner rule/action blocks; bundled tzdb `timezonedb.h` 6.48 MB, 598 zones, version 2026.6). Pure-Rust parity of the parser is XL; binding timelib (standalone C library) is the pragmatic route.
11. **`matches` operator** runs user PCRE regexes: exact parity requires PCRE2 10.44 (bind the C library) plus a port of PHP's delimiter/modifier parser and warning texts (Twig surfaces `substr($warning, 12)`).
12. **Non-UTF-8 charsets** rely on glibc `iconv` (convert_encoding filter, escaper, reverse/shuffle/random) and on libmbfl encodings; exact parity outside UTF-8 is XL. Recommend UTF-8 first.
13. **Not used by Twig 3.31.1** (verified by scan): `ucwords`, `str_split`, `strrev`, `vsprintf`, `printf` (only in test helper), `mb_str_split`, `mb_convert_encoding`, `mb_check_encoding`, `mb_ucfirst`, `random_int`, `array_reduce`, `usort`, `gettype`, `fmod`, `intdiv`, `pow()` (operator `**` is used), `str_pad`, `wordwrap`, `html_entity_decode`, `urlencode` (only the Twig method named `urlencode`), `serialize`/`unserialize` (infrastructure only).
14. **UNVERIFIED / open items** (all flagged inline too): (a) Rust shortest/fixed float digit generation (Ryu, `format!`) equals `zend_dtoa` modes 0/2/3 for every double — needs fuzzing against PHP; (b) `xxhash-rust` `xxh3_128` hex equals `hash('xxh128')` (3 vectors in 5.11); (c) Rust/libm `pow` vs glibc `pow` for `round()` with |precision| > 22; (d) `encoding_rs` vs glibc iconv for non-UTF-8 charsets, `//TRANSLIT`, `//IGNORE`; (e) third-party Unicode case-mapping crates vs PHP's Unicode 17.0 unconditional-only tables; (f) Rust tz crates vs bundled timezonedb 2026.6; (g) re2rust feasibility for `parse_date.re`; (h) PHP orders distinct enum cases by object address when sorting — not reproducible.

---

## 3. Inventory: which builtins Twig calls, and from where

### 3.1 Builtins registered directly as Twig callables (R\*)

| Twig name | kind | PHP callable | Twig site | php-src |
|---|---|---|---|---|
| `abs` | filter | `abs` | `Extension/CoreExtension.php:248` | `ext/standard/math.c:252` |
| `json_encode` | filter | `json_encode` | `:253` | `ext/json/json.c:220` |
| `max` | function | `max` | `:299` | `ext/standard/array.c:1325` (+ frameless `:1410`) |
| `min` | function | `min` | `:300` | `ext/standard/array.c:1197` (+ frameless `:1282`) |
| `range` | function | `range` | `:301` | `ext/standard/array.c:2999` |
| `iterable` | test | `is_iterable` | `:326` | `ext/standard/type.c:444` |
| (round 'ceil'/'floor') | callback | `ceil`/`floor` via `$method(...)` | `:668,672` | `math.c:275/294` |
| (getAttribute) | callback | `array_map('strtolower', ...)` | `:1915` | `string.c:1263` |
| (getAttribute) | callback | `Closure::fromCallable('property_exists')` | `:2287` | `Zend/zend_builtin_functions.c:1067` |
| (sandbox policy) | callback | `array_map('strtolower', ...)` | `Sandbox/SecurityPolicy.php:56` | |

Compiled templates are emitted **without a namespace**, so `max(...)`, `min(...)`, `range(...)` resolve to the internal functions at PHP compile time and the 2-argument forms of `min`/`max` compile to frameless calls (see 4.7). Twig compiles a string callable verbatim: `Node/Expression/CallExpression.php:36-37`.

### 3.2 Builtins used by runtime helpers (R), grouped

| group | builtins (Twig call sites) |
|---|---|
| formatting | `sprintf` (format filter `CoreExtension.php:562`; ~290 error-message sites), `number_format` (`:702`), `round` (`:665`), `ceil` (`:1721` batch), `implode` (`:851,858,998,1024,1814`, `EscaperRuntime.php:348`, `Environment.php:544`) |
| strings | `strtr` (`:646` replace), `trim`/`ltrim`/`rtrim` (`:1216-1218`, `:1127,1138,1154,1165` compare, `:1247` spaceless), `nl2br` (`:1235`), `strip_tags` (`:1348`), `explode` (`:887` split), `str_contains` (`:1082` in), `str_starts_with` (`:1921-1927`), `substr` (`:619,1185,1382,1696,1923-1940`), `strtolower` (`:1696,1957`, `SecurityPolicy.php:149`), `strtoupper` (`EscaperRuntime.php:187`, `Environment.php:644`), `strlen`/`ord` (`EscaperRuntime.php:265,297,311`), `strcasecmp` (`MacroNamespace.php:82,102`), `levenshtein` (`Error/SyntaxError.php:32`), `ucfirst` (deprecation messages) |
| escaping/URL | `htmlspecialchars` (`EscaperRuntime.php:160,184,191,195`), `rawurlencode` (`CoreExtension.php:718`, `EscaperRuntime.php:341`), `http_build_query` (`CoreExtension.php:715`) |
| mbstring | `mb_strlen` (`:894,1296,1308`, `Markup.php:87`), `mb_substr` (`:783,901,1372`), `mb_strtoupper` (`:1323,1372`), `mb_strtolower` (`:1335,1372`), `mb_convert_case` (`:1360`, `MB_CASE_TITLE`), `mb_ord` (`EscaperRuntime.php:233,265,331`) |
| iconv | `iconv` (`CoreExtension.php:1263`, `EscaperRuntime.php:197,248,269,335,360`), `function_exists('iconv')` (`:1259`, `EscaperRuntime.php:356`) |
| ctype | `ctype_digit` (`CoreExtension.php:619` date conversion) |
| PCRE | `preg_match` (`:1188` matches; `EscaperRuntime.php:206,258,280` `//u`), `preg_match_all` (`:1275` `/./us`), `preg_replace` (`:1247` `/>\s+</`), `preg_replace_callback` (`EscaperRuntime.php:210,262,289`), `preg_last_error_msg` (`:1189,1276`) |
| math | `mt_rand` (`:463,479`), `is_nan` (`:1151,1162`), operators `%` (`:442` cycle), `**` (`:672`) |
| arrays | `array_merge` (`:743,1554`, `EscaperRuntime.php:89,132,348`, `Template.php`), `array_chunk` (`:1723`), `array_column` (`:2056`), `array_slice` (`:780,858`), `array_reverse` (`:985,989,998`), `array_keys` (`:959,1814`), `array_values` (`:1416`), `array_filter` (`:2073`, `ARRAY_FILTER_USE_BOTH`), `array_rand` (`:510`), `shuffle` (`:1023,1035`), `asort`/`uasort` (`:1062`/`:1060`), `sort` (`:1914` method names), `array_map` (`:1915`), `in_array` (`:1103,1769,1882,1930`), `array_key_exists` (`:1782`), `array_is_list` (`:1501,1524`), `count` (`:438,506,846,854,858,1300,1426,1467,1726`), `current` (`:799,815`), `array_unique` (`EscaperRuntime.php:132`, `Template.php:325`), `array_key_first` (`EscaperExtension.php:130`), `array_diff_key` (`TwigMacro.php:104`) |
| iteration | `iterator_to_array` (`:770,776,985,1052,1409,1498,1521,2038`), `iterator_count` (`:1304,1471`), `is_iterable`, `is_countable` (`:428`) |
| types | `is_array`, `is_string`, `is_int`, `is_float`, `is_bool`, `is_object`, `is_resource`, `is_scalar` (`:1295`), `is_numeric` (`:1128,1139,1155,1166`), `get_debug_type` (25 sites, error messages), `get_class_methods` (`:1910`), `class_parents`/`class_implements` (`EscaperRuntime.php:130`) |
| constants/enums | `defined` (`:1691,1892`), `constant` (`:1703,1897`), `enum_exists` (`:1637,1657`), `$enum::cases()` (`:1641,1661`) |
| date | `date_default_timezone_get` (`:187`), `new \DateTimeZone` (`:176,187,592`), `new \DateTime` (`:615,620,622`), `->setTimezone()`, `->format()`, `->modify()`, `DateInterval::format()` (`:530`) |
| debug | `var_dump` (`DebugExtension.php:55,57`), `ob_start`/`ob_get_clean` (`:45,60`) |
| output | `ob_start`, `ob_get_clean`, `ob_get_level`, `ob_end_clean` (`Template.php:207-266,441-457`, `CoreExtension.php:2209-2224`) |
| hashing | `hash('xxh128', $template)` (`Environment.php:471` → name of `template_from_string` templates appears in error messages) |
| error plumbing | `set_error_handler`/`restore_error_handler` (`CoreExtension.php:1184,1194`), `debug_backtrace` (`Error/Error.php:168`), `func_num_args`/`func_get_arg` (BC shims) |

### 3.3 PHP emitted by `Node::compile()` (E)

| node (file:line) | emitted PHP | builtins / constructs |
|---|---|---|
| `Node/Expression/Binary/FloorDivBinary.php:21` | `(int) floor(a / b)` | `floor`, `/`, `(int)` cast |
| `Node/Expression/Binary/RangeBinary.php:23` | `range(a, b)` | `range` (int/float/string inputs, see 10.1) |
| `Node/Expression/Binary/StartsWithBinary.php:24-28` | `(is_string($a = L) && is_string($b = R) && str_starts_with($a, $b))` | `is_string`, `str_starts_with` |
| `Node/Expression/Binary/EndsWithBinary.php:24-28` | same with `str_ends_with` | |
| `Node/Expression/Binary/SequenceDestructuringSetBinary.php:53-68` | `[(($v = R) instanceof \Traversable ? CoreExtension::destructureSequence(...) : ([...] = array_pad($v, N, null))), $v = null][0]` | `instanceof \Traversable`, `array_pad` |
| `Node/Expression/GetAttrExpression.php:64-96,196-198` | `(($v = node) && is_array($v) \|\| $v instanceof ArrayAccess ? ($v[key] ?? null) : null)`; sandbox variant uses `in_array($v::class, CoreExtension::ARRAY_LIKE_CLASSES, true)`; non-constant keys: `(($k = key) instanceof \Stringable && (is_array($v) \|\| $v instanceof \ArrayObject \|\| $v instanceof \ArrayIterator) ? (string) $k : $k)` | `is_array`, `in_array`, `instanceof`, `??` |
| `Node/Expression/NameExpression.php:49-89` | `array_key_exists('x', $context)`, `(isset($context['x']) \|\| array_key_exists('x', $context) ? $context['x'] : ...)` | `array_key_exists`, `isset` |
| `Node/Expression/Test/ConstantTest.php:31-46` | `(node === constant(get_class(obj)."::".NAME))` / `(node === constant(NAME))` | `constant`, `get_class` |
| `Node/Expression/Test/EvenTest.php`, `OddTest.php`, `DivisiblebyTest.php` | `(x % 2 == 0)`, `(x % 2 != 0)`, `(0 == x % y)` | `%` (`mod_function`), `==` |
| `Node/Expression/Test/NullTest.php`, `SameasTest.php`, `TrueTest.php` | `(null === x)`, `(a === b)`, `(($tmp = x) && $tmp instanceof Markup ? (string) $tmp : $tmp)` | identity, truthiness |
| `Node/ForNode.php:74-120` | `if (is_array($context['_seq']) \|\| (is_object(...) && ... instanceof \Countable)) { $length = count(...); }`, `foreach`, `$context = array_intersect_key($context, $_parent); $context += $_parent;` | `is_array`, `is_object`, `count`, `array_intersect_key`, array union |
| `Node/CaptureNode.php:35-53` | `('' === $tmp = implode('', iterator_to_array((function () {...})(), false))) ? '' : new Markup($tmp, charset)` (yield mode) | `implode`, `iterator_to_array` |
| `Node/WithNode.php:50` | `if (!is_iterable($x)) { throw ... }` | `is_iterable` |
| `Node/FlushNode.php:38` | `flush();` | `flush` |
| `Node/ModuleNode.php:274,301,322,395` | `sprintf('Block "%s" is not defined in trait "%s".', ...)`, `array_merge(...)` for traits/blocks | `sprintf`, `array_merge` |
| `Node/DeprecatedNode.php:35-48` | `trigger_deprecation(pkg, ver, sprintf(...))` | `sprintf` (+ symfony `trigger_deprecation`, not a builtin) |
| `Node/Expression/Binary/*` via `operator()` | `+ - * / % ** . & \| ^ && \|\| xor <=> === !== ?: ??` as PHP operators; `== != < > <= >=` compile to `(0 === CoreExtension::compare(a, b))`, `(-1 === ...)`, `(1 === ...)`, `(0 <= ...)`, `(0 >= ...)`, `(0 !== ...)` (`EqualBinary.php:29`, `LessBinary.php:29`, ...); `in`/`not in` → `CoreExtension::inFilter`; `matches` → `CoreExtension::matches`; `has some/every` → `CoreExtension::arraySome/arrayEvery` | `Zend/zend_operators.c` (`add_function`, `sub_function`, `mul_function`, `div_function`, `mod_function`, `pow_function`, `concat_function`, `bitwise_*`, `zend_compare` for `<=>`, `zend_is_identical`) |
| `Node/Expression/Unary/*` | `-x`, `+x`, `!x`, `(string) x`, `...x` | casts / spread |
| `Node/PrintNode.php` (compile), `Node/TextNode.php` | `yield (string) expr;` (cast omitted when the expression is known to be a string; `yield from` for generators), `yield "text";` | PHP string conversion of the printed value (4.3): int decimal, float precision 14, `true` → `1`, `false`/`null` → `""`, array → `Array` + `Warning: Array to string conversion`, object → `__toString()` or `Error: Object of class X could not be converted to string` |

No `twig_*` global helpers are emitted any more: `Resources/core.php` (46), `debug.php` (1), `escaper.php` (3) and `string_loader.php` (1) only define deprecated (since 3.9) `twig_*` wrappers that forward to `CoreExtension`/`EscaperRuntime`/`DebugExtension`/`StringLoaderExtension` methods; compiled templates call the static methods (`CoreExtension::getAttribute(...)`, `CoreExtension::ensureTraversable(...)`, ...) or `$this->env->getRuntime(...)`/`$this->extensions[...]` directly (`Node/Expression/CallExpression.php:31-70`) [S].

### 3.4 Compile-time builtins with observable effects (C)

| effect | builtin | site |
|---|---|---|
| number literal value | `0 + str_replace('_', '', $match)` (PHP numeric-string → int/float; `"9223372036854775808"` → float, `"1e400"` → INF, `"007"` → 7) [V] | `Lexer.php:394` |
| string literal escapes | Twig's own `stripcslashes` using `ctype_xdigit`, `hexdec`, `chr`, `ctype_digit`, `octdec` (`chr(octdec($o) % 256)`) | `Lexer.php:427-480` |
| whitespace control | `rtrim($text)` (default set ` \t\n\r\0\x0B`), `rtrim($text, " \t\0\x0B")` for `~`; `ltrim` for `-}`/`~}` | `Lexer.php:293,297,498,502,605` |
| blank text detection | `ctype_space` (C locale: ` \t\n\v\f\r`), UTF-8 BOM check | `Node/TextNode.php:44-50` |
| constant regex validation in `matches` | `preg_match` under `set_error_handler` → **SyntaxError** with same message as runtime | `Node/Expression/Binary/MatchesBinary.php:35-39` |
| "Did you mean" suggestions | `levenshtein`, `str_contains`, `asort`, `implode` | `Error/SyntaxError.php:28-45` |
| enum function validation | `enum_exists` on literal class names | `Node/Expression/FunctionNode/EnumCasesFunction.php:44`, `EnumFunction.php:44` |
| dynamic callable names (`*` patterns) | `preg_quote`, `str_replace('\\*', '(.*?)', ...)`, `preg_match('#^...$#')` | `ExtensionSet.php:509-526,240,296,451` |
| lexer tokenization | `preg_match`/`preg_match_all`/`preg_quote` with Twig regexes (lexer survey) | `Lexer.php` |

### 3.5 Infrastructure-only builtins (I) — not ported

Filesystem/opcache (`file_put_contents`, `rename`, `chmod`, `umask`, `tempnam`, `filemtime`, `is_file`, `is_dir`, `mkdir`, `realpath`, `opcache_invalidate`, `opcache_is_script_cached`, `clearstatcache`, `unlink`, `file_get_contents`, `getcwd`, `parse_url`, `pathinfo`, `basename`, `dirname`), profiler (`microtime`, `memory_get_usage`, `memory_get_peak_usage`, `serialize`, `unserialize`), reflection-driven callable analysis (`ReflectionFunction`, `ReflectionMethod`, ...), `ini_get`/`ini_set`/`filter_var`/`extension_loaded`, `class_exists`, `spl_object_id`, `bin2hex(random_bytes(16))` (`Environment::removeCache`), `hash()` for template class names (except `createTemplate`, see 5.11).

---

## 4. Cross-cutting PHP mechanics every port depends on

### 4.1 Weak-mode argument coercion (all builtins)

Neither Twig sources nor compiled templates declare `strict_types`, so every internal call uses weak coercion (`Zend/zend_API.c:505-800`) [S]:

| parameter type | accepted input → result | rejected (TypeError) |
|---|---|---|
| `int` (`Z_PARAM_LONG`, `zend_parse_arg_long_weak` :567) | int; bool → 0/1; null → 0 **+ deprecation**; float: integral and in range → int, fractional → truncated **+ deprecation** `Implicit conversion from float %.*H to int loses precision` (`zend_operators.c:937`, `%.*H` with precision -1, e.g. `1.5`); NaN or out of range → **TypeError**; numeric string (leading/trailing whitespace allowed): `"2"` → 2, `"1.5"` → 1 + deprecation `Implicit conversion from float-string "1.5" to int loses precision` (:941) | arrays, objects, non-numeric/leading-numeric strings (`"12abc"`) |
| `float` (`zend_parse_arg_double_weak` :654) | int → float; numeric string; bool; null → 0.0 + deprecation | as above |
| `int\|float` (`Z_PARAM_NUMBER`, :698) | int/float kept; numeric string → int or float; bool → 0/1 int; null → int 0 + deprecation | arrays, objects, `"12abc"`, `"abc"` |
| `string` (`zend_parse_arg_str_weak` :761) | int/float/bool converted (`convert_to_string`: float via precision 14, true → `"1"`, false → `""`); null → `""` + deprecation; object with `__toString` → its string | arrays, objects without `__toString` |
| `bool` (:533) | any scalar via truthiness; null → false + deprecation | arrays, objects |
| `string\|int\|float` (`range`) | null → int 0 + deprecation; bool → int; object with `__toString` → string | arrays |

Verbatim messages (all [V]):

- `%s(): Passing null to parameter #%u ($%s) of type %s is deprecated` (`zend_API.c:522`), e.g. `round(): Passing null to parameter #1 ($num) of type int|float is deprecated`, `implode(): Passing null to parameter #1 ($separator) of type array|string is deprecated`.
- TypeError: `%s(): Argument #%d ($%s) must be of type %s, %s given` where the given-type name is `zend_zval_value_name()` (`zend_API.c:155`): class name for objects, `"true"`/`"false"` for booleans, else `null|int|float|string|array|resource`. Expected type names come from the `Z_PARAM` macro (`int`, `?int`, `int|float`, `string|int|float`, `array|string`, `Countable|array`, ...). Examples: `abs(): Argument #1 ($num) must be of type int|float, string given`; `array_slice(): Argument #3 ($length) must be of type ?int, string given`; `count(): Argument #1 ($value) must be of type Countable|array, null given`.
- Parameter names (needed for messages) are those of the stubs; signatures of every builtin Twig passes template data to:

```
abs(int|float $num)                      round(int|float $num, int $precision = 0, int|RoundingMode $mode = RoundingMode::HalfAwayFromZero)
floor(int|float $num)  ceil(int|float $num)      max(mixed $value, mixed ...$values)   min(mixed $value, mixed ...$values)
range(string|int|float $start, string|int|float $end, int|float $step = 1)
number_format(float $num, int $decimals = 0, ?string $decimal_separator = ".", ?string $thousands_separator = ",")
sprintf(string $format, mixed ...$values)     strtr(string $string, string|array $from, ?string $to = null)
trim/ltrim/rtrim(string $string, string $characters = " \n\r\t\v\0")    nl2br(string $string, bool $use_xhtml = true)
strip_tags(string $string, array|string|null $allowed_tags = null)     implode(string|array $separator, ?array $array = null)
explode(string $separator, string $string, int $limit = PHP_INT_MAX)
htmlspecialchars(string $string, int $flags = ENT_QUOTES|ENT_SUBSTITUTE|ENT_HTML401, ?string $encoding = null, bool $double_encode = true)
rawurlencode(string $string)   http_build_query(array|object $data, string $numeric_prefix = "", ?string $arg_separator = null, int $encoding_type = PHP_QUERY_RFC1738)
iconv(string $from_encoding, string $to_encoding, string $string): string|false
mb_strlen(string $string, ?string $encoding = null)   mb_substr(string $string, int $start, ?int $length = null, ?string $encoding = null)
mb_strtoupper/mb_strtolower(string $string, ?string $encoding = null)   mb_convert_case(string $string, int $mode, ?string $encoding = null)
json_encode(mixed $value, int $flags = 0, int $depth = 512): string|false
array_merge(array ...$arrays)   array_chunk(array $array, int $length, bool $preserve_keys = false)
array_column(array $array, int|string|null $column_key, int|string|null $index_key = null)
array_slice(array $array, int $offset, ?int $length = null, bool $preserve_keys = false)   array_reverse(array $array, bool $preserve_keys = false)
iterator_to_array(iterable $iterator, bool $preserve_keys = true)   mt_rand(int $min, int $max)
str_contains/str_starts_with/str_ends_with(string $haystack, string $needle)   preg_match(string $pattern, string $subject, ...)
```

### 4.2 Error channels and Twig wrapping

- Exceptions from builtins: `TypeError`, `ValueError`, `ArgumentCountError`, `DivisionByZeroError`, `Error` (e.g. `Object of class X could not be converted to string`), `DateMalformedStringException`, `DateInvalidTimeZoneException`, `JsonException` (only with `JSON_THROW_ON_ERROR`). Twig re-throws `Twig\Error\Error` subclasses unchanged; everything else is wrapped by `Template::handleException()` (`Template.php:615-631`):
  `An exception has been thrown during the rendering of a template ("<php message>")` + ` in "<template>" at line <n>.` (the trailing `.`/`?` of the raw message is moved after the location by `Error::updateRepr()`, `Error/Error.php:129-158`) [V].
- `php_error_docref()` warnings prefix the function name: `"<func>(): <message>"` (e.g. `iconv(): Detected an illegal character in input string`). Twig's `matches` relies on this prefix length (`substr($m, 12)` strips `preg_match()` — 12 bytes).
- Default severities: E_WARNING (`Array to string conversion`, `A non-numeric value encountered`, float→int not representable, NaN coercion), E_NOTICE (iconv illegal/incomplete input, sprintf precision truncation), E_DEPRECATED (null args, implicit float→int, bool-returning comparators).

### 4.3 Float → string

Two code paths, both ending in `zend_gcvt()` (`Zend/zend_strtod.c:4509`) [S][V]:

| path | used by | call |
|---|---|---|
| `zend_double_to_str()` (`Zend/zend_operators.c:3616`) | `(string)`, echo/print (template output), `.`/`~`, `implode`, `sprintf('%s')`, `strval`, `strtr` values, `http_build_query`, `compare_double_to_string` | `zend_gcvt(d, precision ?: 1, '.', 'E', buf)`, `precision` ini = **14**; PHP 8.5: NaN emits `Warning: unexpected NAN value was coerced to string` |
| `smart_str_append_double()` (`Zend/zend_smart_str.c:156`) / `%.*H` | `var_dump` (`float(%.*H)`), `var_export` (zero_frac = true → `1.0`), error messages using `%.*H` | precision = `serialize_precision` = **-1** |
| `php_json_encode_double()` (`ext/json/json_encoder.c:79`) | `json_encode` | `zend_gcvt(d, serialize_precision(-1), '.', 'e', ...)`, `.0` appended only with `JSON_PRESERVE_ZERO_FRACTION` |

`zend_gcvt(value, ndigit, dec_point, exp_char)` port-ready algorithm:

```
mode = ndigit >= 0 ? 2 : 0;  if mode == 0 { ndigit = 17 }
(digits, decpt, sign) = zend_dtoa(value, mode, ndigit)   // mode 0: shortest round-trip digits; mode 2: max(ndigit,1) significant digits, correctly rounded (ties-to-even on the exact binary value), trailing zeros stripped
if decpt == 9999: return (sign && digits[0]=='I' ? "-" : "") + (digits[0]=='I' ? "INF" : "NAN")   // NaN never shows a sign
out = sign ? "-" : ""
if (decpt >= 0 && decpt > ndigit) || decpt < -3:            // exponential
    e = decpt - 1;  esign = e < 0 ? '-' : '+';  e = |e|
    out += digits[0] + dec_point + (digits[1..] or "0") + exp_char + esign + decimal(e)   // no zero padding of exponent
elif decpt < 0:                                            // 0.000ddd
    out += "0" + dec_point + "0"*(-decpt) + digits
else:                                                      // ddd.ddd or ddd000
    out += digits[0..decpt] padded with '0' to decpt chars (at least "0" if decpt==0 and fraction follows)
    if digits longer than decpt: out += dec_point + digits[decpt..]
```

`decpt` is the decimal exponent such that value = 0.d1d2… × 10^decpt. Verified table (precision 14 | json | var_export) [V]:

| value | echo/strval/implode | json_encode | var_export |
|---|---|---|---|
| 0.1+0.2 | `0.3` | `0.30000000000000004` | `0.30000000000000004` |
| 1e15 | `1.0E+15` | `1000000000000000` | `1000000000000000.0` |
| 1e14 | `1.0E+14` | `100000000000000` | `100000000000000.0` |
| 123456789012345.0 | `1.2345678901234E+14` | `123456789012345` | `123456789012345.0` |
| 0.0001 / 0.00001 | `0.0001` / `1.0E-5` | `0.0001` / `1.0e-5` | `0.0001` / `1.0E-5` |
| -0.0 | `-0` | `-0` | `-0.0` |
| 1.0 / 100.0 | `1` / `100` | `1` / `100` | `1.0` / `100.0` |
| 3.0e20 | `3.0E+20` | `3.0e+20` | `3.0E+20` |
| 1e17 (json) | | `1.0e+17` | |
| 5e-324 (json) | | `5.0e-324` | |
| NAN | `NAN` + warning | `false` (error "Inf and NaN cannot be JSON encoded") | `NAN` |
| INF / -INF | `INF` / `-INF` | `false` | `INF` / `-INF` |
| 1/3 | `0.33333333333333` | `0.3333333333333333` | `0.3333333333333333` |
| `ini precision=2`: 0.125 / 0.135 | `0.12` / `0.14` (ties-to-even on exact binary value) | | |

Rust note: mode 0 = shortest round-trip digits (Ryu/Grisu-shortest give the same digit string; UNVERIFIED for all doubles — fuzz against PHP); mode 2 = Rust `format!("{:.*e}", ndigit-1, v)` digits (correctly rounded, ties-to-even), then trim zeros. Only the *layout* above must be ported.

### 4.4 Float → int (PHP 8.5)

`zend_dval_to_lval()` (`Zend/zend_operators.h:126`) [S][V]: finite and in range → truncation; ±INF/NaN → 0 + `Warning: The float %.*H is not representable as an int, cast occurred` (e.g. `The float INF ...`, `The float NAN ...`); finite out of range → same warning and **modular** result `zend_dval_to_lval_slow()` (`zend_operators.c:3886`): `dmod = fmod(d, 2^64); if dmod < 0 { dmod += 2^64 }; (int64)(uint64)dmod`. Examples [V]: `(int)1e30` → `5076964154930102272`; `(int)floor(1e20/1)` → `7766279631452241920`. Exception: `(int)"1e30"` (string cast) saturates to `PHP_INT_MAX` without warning (`zend_dval_to_lval_cap`, :149). Affects `//` (`(int) floor()`), `random()` (`(int)$max`), `batch` (`(int) ceil($size)`), `%` operands, `sprintf('%d')`.

### 4.5 Numeric strings

`_is_numeric_string_ex()` (`Zend/zend_operators.c:3636`) [S]: optional leading whitespace `[ \t\n\r\v\f]`, optional sign, then digits with optional `.` fraction and optional `e/E[+-]digits` exponent, or `.digits`; trailing whitespace allowed; anything else → not numeric (with `allow_errors` the numeric prefix is used and "trailing data" reported). No hex, no underscores, no `INF`. Integers with ≥ `MAX_LENGTH_OF_LONG-1` digits that overflow become doubles (overflow flag used by `zendi_smart_strcmp`). Verified `is_numeric`: `'1'`, `' 1'`, `'1 '`, `'1e3'`, `'.5'`, `'5.'`, `'+1'`, `'-.5e-3'`, `"1\n"` true; `'0x1A'`, `''`, `' '`, `'1_000'` false [V]. Explicit `(float)` cast uses the numeric prefix leniently: `(float)"1e3abc"` = 1000, `"abc"` → 0, `"1e400"` → INF, `[]` → 0.0, `[1]` → 1.0, object → 1.0 + `Warning: Object of class stdClass could not be converted to float` [V].

### 4.6 `zend_compare` (spaceship) summary

Needed by `asort`, `min`/`max`, `in_array` (non-strict, not used by Twig), `CoreExtension::compare` fallback `<=>`. Source `Zend/zend_operators.c:2306` [S]:

- long/long, long/double, double/double: numeric three-way (`ZEND_THREEWAY_COMPARE`: NaN compares as `1`).
- string/string: `zendi_smart_strcmp()` (:3437): if both numeric → numeric compare (with overflow special cases), else `zend_binary_strcmp` (memcmp then length difference) normalised to -1/0/1.
- long/string: `compare_long_to_string()` (:2260): numeric string → numeric compare; else compare `(string)long` with the string bytewise. double/string: NaN → 1; else analogous with `zend_double_to_str` (precision 14).
- null/string: `""` == null else null < string. null/bool/false combos: null == false; null < true.
- bool vs anything: compare truthiness. null vs anything else: `zval_is_true(other) ? -1 : 0`.
- array/array: `zend_hash_compare(ordered=0)` (`Zend/zend_hash.c:3320`): smaller count is smaller; then for each element of op1 in op1 order look up the same key in op2 — missing key → **1** (uncomparable); else first non-zero element comparison. array vs non-array (after scalar conversion attempt): array is greater.
- object/object: same instance → 0; else class `compare` handler (std: same class → property-wise, else uncomparable = 1). Enum vs enum: uncomparable unless identical.
- `php_array_data_compare_unstable_i` (`ext/standard/array.c:286`) adds an enum special case used by `array_unique`/sorting: only when the **right** operand is an enum and `zend_compare` reported "uncomparable": left operand also an enum → order by object address (0 if same case), left operand not an enum → -1 ("shift enums to the end"). Object addresses are not reproducible; for twig-rs any stable per-case order gives the same grouping but sort output order of distinct enum cases may differ (UNVERIFIED impact).

### 4.7 Frameless function variants

PHP 8.4+ compiles direct calls with a fixed argument count to "frameless" implementations when the function name is known at compile time [S]: `class_exists/1,2`, `dirname/1,2`, `implode/1,2`, `in_array/2,3`, `is_numeric/1`, `max/2`, `min/2`, `preg_match/2`, `preg_replace/3`, `property_exists/2`, `str_contains/2`, `str_replace/3`, `str_starts_with/2`, `strpos/2,3`, `strstr/2,3`, `strtr/2,3`, `substr/2,3`, `trim/1,2`, `dechex/1`. Only `min`/`max` differ observably:

| shape | min tie result | max tie result | source |
|---|---|---|---|
| `min(a, b)` / `max(a, b)` compiled directly (Twig `{{ min(a, b) }}`) | **b** (`zend_compare(lhs, rhs) < 0 ? lhs : rhs`) | **a** (`>= 0 ? lhs : rhs`) | `array.c:1282`, `:1410` |
| variadic call (≥3 args, or spread `...`) | first | first | `array.c:1197`, `:1325` |
| single array argument | first | first | `zend_hash_minmax` (`Zend/zend_hash.c:3456`) |

Verified: `{{ min("1.0", 1) }}` → `1`; `{{ min(1, "1.0") }}` → `1.0`; `{{ min("1.0", 1, 5) }}` → `1.0`; `{{ min(...['1.0', 1]) }}` → `1.0`; `{{ max("1.0", 1) }}` → `1.0` [V].

---

## 5. Strings

### 5.1 `sprintf` — `php_formatted_print()`

Twig use: `format` filter `CoreExtension::sprintf($format, ...$values)` = `\sprintf($format ?? '', ...$values)` (`CoreExtension.php:560-563`); every Twig error message. Source `ext/standard/formatted_print.c:423` (`PHP_FUNCTION(sprintf)` :761, `nb_additional_parameters = 1`), helpers `php_sprintf_appendstring` :81, `appendint` :138, `appenduint` :182, `appenddouble` :214, `append2n` :322, `getnumber` :353, `php_sprintf_get_argnum` :377; float conversion `main/snprintf.c` `php_conv_fp` :271, `__cvt` :62 (`NDIG` 320 :261); `zend_gcvt` for g/G/h/H. [S][V]

Grammar per conversion: `%` [argnum `N$`] [flags] [width | `*`[`N$`]] [`.` (precision | `*`[`N$`])] [`l`] specifier.

- `%%` → `%` (only when immediately following `%`; inside a spec, `%` specifier also emits `%`).
- If the char after `%` is a letter: no argnum/flags/width/precision parsing.
- argnum: digits followed by `$`; value ≤ 0 or ≥ INT_MAX → `ValueError: Argument number specifier must be greater than zero and less than 2147483647`.
- flags (any order, repeated): `' '` or `'0'` → padding char; `'-'` → left-align; `'+'` → always sign; `'\''` + char → custom padding char (if no char follows: `ValueError: Missing padding character`).
- width: digits (> INT_MAX → `ValueError: Width must be between 0 and 2147483647`) or `*` (argument must be **int**, else `ValueError: Width must be an integer`; <0 or >INT_MAX → `Width must be between 0 and 2147483647`).
- precision: `.` digits; `.` alone = precision 0; `.*` (int, -1..INT_MAX else `Precision must be an integer` / `Precision must be between -1 and 2147483647`). Precision -1 only for g/G/h/H, else `ValueError: Precision -1 is only supported for %g, %G, %h and %H`. Precision > 53 for floats → `Notice: sprintf(): Requested precision of %d digits was truncated to PHP maximum of 53 digits`.
- optional `l` modifier ignored.
- missing arguments are collected and reported once at the end: `ArgumentCountError: %d arguments are required, %d given` (counts include the format argument: `sprintf('%s %s', 'a')` → `3 arguments are required, 2 given`).
- end of string after `%`/modifiers → `ValueError: Missing format specifier at end of string`; unknown letter → `ValueError: Unknown format specifier "%c"`.

Specifiers:

| spec | conversion | formatting |
|---|---|---|
| `s` | `zval_get_tmp_string` (float precision 14, arrays → `Array` + warning, objects → `__toString` or Error) | precision truncates (`%.3s`); width pads |
| `d` | `zval_get_long` (lenient: `"12abc"` → 12, `1.9` → 1, `true` → 1, `[1]` → 1, `1e30` → modular + warning) | sign `-`, `+` if flag; precision **ignored**; `0` padding not allowed when left-aligned (becomes space) |
| `u` | `zval_get_long` printed as unsigned 64-bit | `-1` → `18446744073709551615` |
| `c` | `(char) zval_get_long` | width/precision ignored (`%5c` → `A`), 321 → `A` |
| `o`,`x`,`X`,`b` | `zval_get_long` as unsigned, base 8/16/2 | **any precision makes output empty** (`%.2x` → `""`, `%5.2x` → 5 spaces) — `append2n` passes `max_width=0` with `expprec` [V] |
| `e`,`E` | `zval_get_double` | `php_conv_fp`: precision (default 6) digits after point, exponent without zero padding: `%e`(1234.5678) → `1.234568e+3`, `%.0e`(12345) → `1e+4`, `%e`(0) → `0.000000e+0` |
| `f`,`F` | `zval_get_double` | fixed, correctly rounded ties-to-even (`%.1f` 0.25 → `0.2`, `%.0f` 2.5 → `2`, 3.5 → `4`); `f` uses locale decimal point (C locale `.`), `F` always `.`; `%.50f`(0.1) shows exact binary expansion `0.10000000000000000555111512312578270211815834045410` |
| `g`,`G`,`h`,`H` | `zval_get_double` | precision 0 → 1, default 6; `zend_gcvt` (see 4.3) with `e` (g/h) or `E` (G/H); `g`/`G` use locale decimal point, `h`/`H` always `.`; `%g`(1e20) → `1.0e+20`, `%g`(0.00001234) → `1.234e-5` |

Special values: NaN → `NaN` (any float spec), ±INF → `INF`/`-INF`; for `%d` INF → `0` + warning. Padding rules (`php_sprintf_appendstring`): right alignment with padding `'0'` and a sign → sign emitted first, then zeros (`%010d`(-42) → `-000000042`, `%+05d`(5) → `+0005`, `%05.1f`(-2.34) → `-02.3`); strings may be zero-padded on either side (`%-05s` → `ab000`). Fatal `Field width %zd is too long` on overflow (E_ERROR, unreachable in practice). PHP 8.5 change: precision-less specs are treated as precision 0 consistently (GH-18897).

Port: M, pure Rust; the float paths reuse the 4.3 digit generators (`zend_dtoa` mode 3 for `%f` = Rust `format!("{:.*}", p, v)` digits; mode 2 with `precision+1` digits for `%e`).

### 5.2 `number_format`

Twig: `formatNumber()` always passes `(float) $number` (`CoreExtension.php:702`) with defaults `[0, '.', ',']` (`:139`). Source `ext/standard/math.c:1345`, `_php_math_number_format_ex` (:~1110-1222), `_php_math_number_format_long` (:1225) [S][V].

```
number_format(float d, long dec, dec_point, thousand_sep):
  if |d| >= 2^52 and d fits in int64: return number_format_long((int64)d, dec, ...)
  dec_int = clamp(dec, INT_MIN, INT_MAX)
  neg = d < 0; d = |d|
  d = _php_math_round(d, dec_int, HALF_UP)          // 9.2
  dec = max(0, dec_int)
  tmp = sprintf("%.*F", dec, d)   // main/spprintf.c: NaN → "nan", INF → "inf" (lowercase, sign dropped since d=|d|); precision capped at 318 (NDIG-2), missing digits padded with '0' later
  if tmp[0] is not a digit: return tmp              // "inf" / "nan"
  if neg and d == 0: neg = false                     // never "-0"
  integer part of tmp: insert thousand_sep every 3 digits from the right (multi-byte separators allowed, empty allowed)
  if dec > 0: append dec_point + fractional digits (padded with '0' to dec)
  prefix "-" if neg
number_format_long(n, dec): if dec < 0: round |n| to 10^-dec with half-up on the remainder (dec < -19 → 0), never "-0"; then same grouping; dec > 0 appends dec_point + dec zeros
```

Verified: `1234.5` → `1,235`; `(1234.5, 2)` → `1,234.50`; `0.5` → `1`; `(1.005, 2)` → `1.01`; `-0.4` → `0`; `(-0.01, 1)` → `0.0`; `(-1234.567, 2, ',', '.')` → `-1.234,57`; `(1234567.891, -3)` → `1,235,000`; `1e20` → `100,000,000,000,000,000,000`; `(9007199254740993.0, 2)` → `9,007,199,254,740,992.00`; `INF` → `inf`; `-INF` → `inf`; `NAN` → `nan`; `(0.123456789, 20)` → `0.12345678899999999734`; `(1234567.891, 2, 'dp', 'ts')` → `1ts234ts567dp89`; `(…, '', '')` → `123456789`; `(1234.5, 1.7)` → `1,234.5` + deprecation; `(1234.5, "x")` → `TypeError: number_format(): Argument #2 ($decimals) must be of type int, string given`; Twig `{{ "abc"|number_format }}` → `0` [V]. Port: S/M pure Rust.

### 5.3 `strtr` (array form) — `replace` filter

Twig: `strtr($str ?? '', self::toArray($from))` (`CoreExtension.php:646`); non-iterable `$from` → Twig RuntimeError `The "replace" filter expects a sequence or a mapping, got "%s".`. Source `ext/standard/string.c:3536` → `php_strtr_array()` :3475 → `php_strtr_array_ex()` :2977 [S][V].

- empty subject → `""` immediately.
- 0 pairs → subject unchanged. 1 pair → key (int keys converted to decimal string) and value converted to string **first** (value conversion side effects/warnings happen even if unused); empty key → `Warning: strtr(): Ignoring replacement of empty string` and subject returned; else plain replace-all of that key (non-overlapping, left to right).
- ≥2 pairs: empty string keys → the same warning per empty key (skipped); keys longer than the subject are skipped; int keys converted to strings. Scan positions left to right; at each position try lengths from `min(maxlen, remaining)` down to `minlen` (only lengths present among keys and only if the first byte is a first byte of some key); first (= **longest**) match wins; append replacement (value converted via `zval_get_string`), continue after the match; replaced text is never rescanned.
- Verified: `"Hello %name%, %name%!"|replace({'%name%': 'Bob', 'Hello': 'Hi', 'H': 'X'})` → `Hi Bob, Bob!`; `'a1b'|replace({1: 'one'})` → `aoneb` [V].
- Port: S/M pure Rust (a set of key lengths + first-byte set + map lookup reproduces it exactly).

### 5.4 `trim`, `ltrim`, `rtrim` + `php_charmask`

Twig: `trim` filter always passes an explicit mask (`DEFAULT_TRIM_CHARS = " \t\n\r\0\x0B"` when null, `CoreExtension.php:136,1209-1224`); `side` other than left/right/both → RuntimeError `Trimming side must be "left", "right" or "both".`; Markup input with default mask keeps Markup safety. `compare()` trims `" \t\n\r\v\f"`; `spaceless` trims default. Source `ext/standard/string.c:524` (`php_trim_int`), `php_charmask` :475 [S][V].

- mask of length 1: compare against that byte.
- otherwise build a 256-entry set: `X..Y` (with Y ≥ X) adds the range; a `..` that cannot form a range emits (and is skipped, other chars still added):
  - at start: `Warning: trim(): Invalid '..'-range, no character to the left of '..'`
  - at end: `Invalid '..'-range, no character to the right of '..'`
  - decreasing: `Invalid '..'-range, '..'-range needs to be incrementing`
  - else: `Invalid '..'-range`
  (prefix uses the actual function name: `ltrim():`/`rtrim():`).
- Byte-wise, no Unicode. Verified `"abcxcba"|trim('a..c')` → `x`; `"abc"|trim('..c')` → warning + `ab` [V]. Port: S.

### 5.5 `nl2br`

Twig: `nl2br($string ?? '')` with `pre_escape: html` (`CoreExtension.php:1235`). Source `string.c:4876` [S][V]: inserts `<br />` (xhtml default) before each line break; `\r\n` and `\n\r` are single breaks (both bytes kept after the tag); lone `\r` or `\n` also. `"a\nb\r\nc\n\rd\re"` → `a<br />\nb<br />\r\nc<br />\n\rd<br />\re` [V]. Port: S.

### 5.6 `strip_tags` — `striptags` filter

Twig: `strip_tags($string ?? '', $allowable_tags)` (`CoreExtension.php:1348`); `$allowable_tags` may be string, array or null. Source `string.c:4961` (`PHP_FUNCTION`), `php_strip_tags_ex` :5257 (state machine, ~300 lines), `php_tag_find` :5174, `PHP_TAG_BUF_SIZE 1023` [S][V].

- Array allowlist → each element `zval_get_string`, wrapped as `<tag>` and concatenated; string allowlist used as is; the allowlist is lower-cased (ASCII).
- States: 0 text; 1 inside HTML tag (`<` followed by non-whitespace); 2 inside `<?...?>` (PHP), with parenthesis depth and quote tracking, `<?xml` switches back to state 1 with `is_xml`; 3 inside `<!...>` (comments/doctype; `<!--` enters state 4; `<!DOCTYPE` (case-insensitive check of `doctype` before `E`) returns to state 1); 4 inside `<!-- ... -->` until `-->`.
- `<` followed by `isspace()` (C locale) is literal text. `>` in text is kept unless nested depth is pending. Quotes (`'`/`"`) inside tags suspend `>` detection. NUL bytes are dropped in states 0/1.
- Allowed tag check: normalise the collected tag (lowercase, strip leading `/`, stop at first whitespace after the name, e.g. `<a href=…>` → `<a>`, `</b>` → `<b>`), then **substring search** (`strstr`) in the allowlist string.
- Verified: `'<p>Hello <b>World</b><br/><!-- c --> <?php echo 1; ?>x</p> a < b'` → `Hello World x a < b`; `'a < b > c'` unchanged; `'1<2'` → `1`; `"a<\nb>c"` unchanged; `'<P>a</P>'` with `'<p>'` → kept; `['b']` allowlist keeps `<b>` [V].
- Port: M, pure Rust (straight transliteration of the state machine; keep byte semantics and the `strstr` quirk).

### 5.7 `implode` / `explode`

- `implode` (`string.c:1085`, `php_implode` :974) [S][V]: 0 elements → `""`; 1 element → `zval_get_string(elem)` (no glue); else each element converted in order: string as is, int decimal, others `zval_get_string` (float precision 14, `true`→`1`, `false`/`null`→`""`, array → `Array` + `Warning: Array to string conversion`, object → `__toString` or `Error: Object of class X could not be converted to string`, enum → that Error). Arg forms: `implode(string, array)`; `implode(array)`; `implode(array, array)` → `TypeError: implode(): Argument #1 ($separator) must be of type string, array given`; `implode(string)` → `TypeError: implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given`. Twig `join` (`CoreExtension.php:838-859`): non-iterable → `[value]` (null → `[]`), `toArray(…, false)`; `$and` path uses `array_slice` + concatenation; 1-element `$and` path returns `$value[0]` raw (coerced by the `: string` return type) [V: `[1, 2.5, true, false, null, 'x']|join(',')` → `1,2.5,1,,,x`].
- `explode` (`string.c:934`) [S][V]: empty separator → `ValueError: explode(): Argument #1 ($separator) must not be empty`; empty subject → `[""]` if limit ≥ 0 else `[]`; limit > 1 → at most `limit` pieces (last holds the rest); limit < 0 → all pieces minus the last `-limit`; limit 0 or 1 → `[subject]`. Twig `split` with empty delimiter uses `splitIntoCharacters` (`/./us`) or `mb_strlen`/`mb_substr` chunks (`CoreExtension.php:882-905`). Verified: `"a,b,c"|split(',', -1)` → `a|b`, `split(',', 0)` → `a,b,c`, `''|split(',')` → `['']`, `''|split('')` → `['']` [V].

### 5.8 `htmlspecialchars` — `escape('html')` and autoescape

Twig calls (`Runtime/EscaperRuntime.php:156-197`): charset `'UTF-8'` (exact) → `htmlspecialchars($s, ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8')`; charset in Twig's list (`ISO-8859-1`, `ISO8859-1`, `ISO-8859-15`, `ISO8859-15`, `utf-8`, `UTF-8`, `CP866`, `IBM866`, `866`, `CP1251`, `WINDOWS-1251`, `WIN-1251`, `1251`, `CP1252`, `WINDOWS-1252`, `1252`, `KOI8-R`, `KOI8-RU`, `KOI8R`, `BIG5`, `950`, `GB2312`, `936`, `BIG5-HKSCS`, `SHIFT_JIS`, `SJIS`, `932`, `EUC-JP`, `EUCJP`, `ISO8859-5`, `ISO-8859-5`, `MACROMAN`; exact or after `strtoupper`) → native call with that charset; otherwise iconv to UTF-8, escape, iconv back. Non-string non-Stringable values are returned unchanged for html/js/css/html_attr/html_attr_relaxed/url; `''` → `''`. Source `ext/standard/html.c:1378` → `php_escape_html_entities_ex()` :1131, `get_next_char()` :89, `determine_charset()` :364, `charset_map` `ext/standard/html_tables.h:40` [S][V].

With flags 11 (ENT_QUOTES|ENT_SUBSTITUTE, doctype HTML401), `all=0`, `double_encode=true`:

- `&` → `&amp;` (always, even inside existing entities: `&amp;` → `&amp;amp;`), `"` → `&quot;`, `'` → `&#039;`, `<` → `&lt;`, `>` → `&gt;`; everything else copied.
- Invalid sequences are replaced by U+FFFD (UTF-8 bytes `EF BF BD`) for UTF-8, or by the literal `&#xFFFD;` for every other charset.
- UTF-8 decoding (`get_next_char`, UTR #36 strategy 2, "maximal subpart"), port-ready:

```
lead(c)  = c < 0x80 || 0xC2 <= c <= 0xF4         trail(c) = 0x80 <= c <= 0xBF
c < 0x80: ok, 1 byte
c < 0xC2: invalid, consume 1
c < 0xE0: need 2; if avail<2: invalid 1; if !trail(b1): invalid (lead(b1) ? 1 : 2); cp<0x80 never happens here
c < 0xF0: if avail<3 || !trail(b1) || !trail(b2):
            invalid: (avail<2 || lead(b1)) ? 1 : (avail<3 || lead(b2)) ? 2 : 3
          cp = ...; if cp < 0x800 or 0xD800<=cp<=0xDFFF: invalid 3
c < 0xF5: if avail<4 || any of b1..b3 not trail:
            invalid: (avail<2||lead(b1)) ? 1 : (avail<3||lead(b2)) ? 2 : (avail<4||lead(b3)) ? 3 : 4
          cp = ...; if cp < 0x10000 or cp > 0x10FFFF: invalid 4
else: invalid 1
```

  Note the asymmetry with the mbstring decoder (6.2): e.g. `E0 80 80` is one invalid 3-byte unit here but `?` + two more in mbstring.
- Non-UTF-8 single-byte charsets (ISO-8859-x, cp125x, KOI8-R, cp866, MacRoman) never produce invalid sequences; multi-byte (BIG5, GB2312, BIG5-HKSCS, SJIS, EUC-JP) have their own lead/trail checks (`html.c` `get_next_char` cases).
- Unknown charset → `Warning: htmlspecialchars(): Charset "%s" is not supported, assuming UTF-8` (Twig avoids this by checking its list first).
- Verified: `"<>&'\""` → `&lt;&gt;&amp;&#039;&quot;`; `"a\xFFb\xC3"` → `a\u{FFFD}b\u{FFFD}`; ISO-8859-1 `"a\xE9b"` unchanged bytes; Shift_JIS `"a\x80b"` → `a&#xFFFD;b` [V].
- Port: S for UTF-8 (pure Rust); M with the other 10 charsets.

### 5.9 `rawurlencode` and `http_build_query` — `url_encode` filter, `escape('url')`

- `rawurlencode` (`ext/standard/url.c:633`, `php_url_encode_impl` :450, `hexchars = "0123456789ABCDEF"` :448) [S][V]: bytes `A-Z a-z 0-9 - _ . ~` kept, every other byte → `%XX` uppercase. `"a b+c/d~e_f.g-h*i'j(k)l!m\x00\xff"` → `a%20b%2Bc%2Fd~e_f.g-h%2Ai%27j%28k%29l%21m%00%FF` [V]. Port: S.
- `http_build_query($array, '', '&', PHP_QUERY_RFC3986)` (`ext/standard/http.c:224`, `php_url_encode_hash_ex` :93, `php_url_encode_scalar` :25) [S][V]: iterate in order; key encoding with raw-urlencode; nested arrays/objects (non-enum) recurse with prefix `key%5B` … `%5D` (`a[x][0]=` → `a%5Bx%5D%5B0%5D=`); integer keys printed decimal (numeric prefix `''`); `null` (and resources) **skipped**; `true` → `1`, `false` → `0`; int decimal; float via `zend_double_to_str` (precision 14) then encoded; backed enum → its value; unbacked enum → `ValueError: Unbacked enum Pure cannot be converted to a string`; objects contribute properties visible from the calling scope (public); recursion guarded. Verified Twig: `{a: 1, b: [1, 2], 'c d': 'e f', g: null, h: true, i: false, j: 1.5, k: {x: 'y'}}|url_encode` → `a=1&b%5B0%5D=1&b%5B1%5D=2&c%20d=e%20f&h=1&i=0&j=1.5&k%5Bx%5D=y` [V]. Port: S/M.

### 5.10 Small string builtins

| builtin | php-src | semantics Twig relies on |
|---|---|---|
| `str_contains`, `str_starts_with`, `str_ends_with` | `string.c:1858/1887/1916` | byte-wise; empty needle → true. `in` with string haystack: `'' === $value \|\| str_contains($compare, (string) $value)` for string/int/float needles only (`CoreExtension.php:1080-1086`) |
| `strtolower`, `strtoupper` | `string.c:1263/1250` | **ASCII only** (locale-independent since PHP 8.2): `"ÀBC İ Ä"` → `Àbc İ Ä` [V] |
| `ucfirst` | `string.c:2767` | ASCII only; used only in deprecation/compile messages |
| `strcasecmp` | `Zend/zend_builtin_functions.c:422` | ASCII case-folding; Twig only tests `=== 0` (macro name lookup, `MacroNamespace.php:82,102`) |
| `substr`, `strlen`, `strpos`, `strrpos` | `string.c:2318`, `zend_builtin_functions.c:374` | byte-based; used on PHP-internal strings |
| `ord`, `chr` | `string.c:2708/2731` | PHP 8.5 deprecations: `ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead`; `chr(): Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256` (Twig's usages are single-byte) |
| `levenshtein` | `ext/standard/levenshtein.c:21,89` | byte-wise DP, costs 1/1/1; used for `Did you mean "x"?` (keep alternatives with `lev <= strlen(name)/3` or `str_contains(item, name)`, `asort` by distance (stable), join `", "`) |
| `ctype_digit`, `ctype_space`, `ctype_xdigit`, `ctype_alpha` | `ext/ctype/ctype.c:129/164/178/115` | C locale classes; `''` → false; int arguments are deprecated (`ctype_digit(): Argument of type int will be interpreted as string in the future`) — Twig passes strings. `convertDate` treats `ctype_digit($s) \|\| ('-' === $s[0] && ctype_digit(substr($s, 1)))` as a timestamp |
| `str_replace`, `addcslashes`, `str_repeat`, `substr_count`, `preg_quote` | `string.c` | compile time/infrastructure only |

### 5.11 `hash('xxh128')` — names of `template_from_string` templates

`Environment::createTemplate()` names templates `__string_template__<hash>` or `<name> (string template <hash>)` with `hash('xxh128', $template)` (`Environment.php:469-477`); names appear in error messages [V: `Unexpected "}" in "__string_template__bc3540ba8d8676228047b5a35f275ff8" at line 1 column 9.`]. Source `ext/hash/hash.c:417`, `ext/hash/hash_xxhash.c:281` (`XXH128_canonicalFromHash(XXH3_128bits_digest())`, bundled xxHash 0.8.1 `ext/hash/xxhash.h`). Test vectors [V]: `''` → `99aa06d3014798d86001c324468d497f`; `'abc'` → `06b05ab6733a618578af5f94892f3950`; `'Hi {{ x }'` → `bc3540ba8d8676228047b5a35f275ff8`. Rust: `xxhash-rust` `xxh3_128` → 32 lowercase hex digits big-endian (UNVERIFIED crate parity; check with the vectors). Port: S (crate).

### 5.12 Escaper strategies built on these builtins (Twig logic, `Runtime/EscaperRuntime.php:118-352`)

Not PHP builtins themselves, but they are thin compositions of `htmlspecialchars`, `rawurlencode`, `preg_match('//u')`, `preg_replace_callback`, `mb_ord`, `ord`, `strlen` and `sprintf`, so the exact outputs are listed here [S][V]:

- Input: autoescape + `Markup` safe for the strategy (incl. implied: `html_attr` ⇒ `html_attr_relaxed`, `html`; `html_attr_relaxed` ⇒ `html`) → returned as is; `Stringable` objects of a registered safe class (also via `class_parents()` + `class_implements()`) → `(string)`; other `Stringable` → cast; non-string non-`Stringable` values with strategy `html|js|css|html_attr|html_attr_relaxed|url` → **returned unchanged** (then printed with PHP string conversion: `{{ 1.5|e }}` → `1.5`, `{{ true|e('js') }}` → `1`, `{{ null|e('css') }}` → empty) [V]; `''` → `''`.
- `js`: non-UTF-8 charset → iconv to UTF-8 first and back at the end; invalid UTF-8 → RuntimeError `The string to escape is not a valid UTF-8 string.`; each code point outside `[a-zA-Z0-9,._]`: `\` → `\\`, `/` → `\/`, U+0008 → `\b`, U+000C → `\f`, U+000A → `\n`, U+000D → `\r`, U+0009 → `\t`; other BMP → `\u%04X` (uppercase hex, at least 4 digits); astral → `\u%04X\u%04X` surrogate pair (`high = 0xD800 | ((cp-0x10000) >> 10)`, `low = 0xDC00 | ((cp-0x10000) & 0x3FF)`). `"a b\né/<😀"` → `a\u0020b\n\u00E9\/\u003C\uD83D\uDE00`; U+0080 U+009F U+00A0 → `\u0080\u009F\u00A0` [V].
- `css`: code points outside `[a-zA-Z0-9]` → `\` + uppercase hex without padding + one space: `"a bé<"` → `a\20 b\E9 \3C `; U+1F600 U+007F → `\1F600 \7F ` [V].
- `html_attr` (allowed `[a-zA-Z0-9,.\-_]`) / `html_attr_relaxed` (also `:@[]`): if the **first byte** is ≤ 0x1F (except tab, LF, CR) or in 0x7F..0x9F → `&#xFFFD;` (for valid UTF-8 this means U+0000–U+001F except TAB/LF/CR, and U+007F; C1 controls U+0080–U+009F are two-byte sequences and are **not** replaced); single byte → `&quot;` (0x22), `&amp;` (0x26), `&lt;` (0x3C), `&gt;` (0x3E), else `&#x%02X;`; multi-byte → `&#x%04X;` of the code point. Verified: `"a bé<\x01\x7F,.-_:@[]"` → `a&#x20;b&#x00E9;&lt;&#xFFFD;&#xFFFD;,.-_&#x3A;&#x40;&#x5B;&#x5D;`; relaxed `"a bé<:@[]"` → `a&#x20;b&#x00E9;&lt;:@[]`; `"\t\n\r"` → `&#x09;&#x0A;&#x0D;`; U+0080 U+009F U+00A0 → `&#x0080;&#x009F;&#x00A0;`; U+1F600 U+007F → `&#x1F600;&#xFFFD;` [V].
- `url`: `rawurlencode()` on the raw bytes (no UTF-8 validation): `"a b/é?&=~"` → `a%20b%2F%C3%A9%3F%26%3D~` [V].
- other: custom escaper callable `($string, $charset)`; else RuntimeError `Invalid escaping strategy "foo" (valid ones: "html", "js", "url", "css", "html_attr", "html_attr_relaxed").` (custom strategy names appended) [V].
- Port: S, pure Rust (no regex engine).

### 5.13 Strings not used

`ucwords`, `str_split`, `strrev`, `vsprintf`, `str_pad`, `wordwrap`, `html_entity_decode`, `htmlentities`, `urlencode` (the PHP function), `mb_str_split`, `mb_convert_encoding`, `mb_check_encoding`, `mb_ucfirst` — not referenced by `src/` (scan) [S].

---

## 6. mbstring

### 6.1 Encoding resolution

Twig passes `$charset` = `Environment::getCharset()` (default `UTF-8`; `setCharset()` upper-cases and maps `UTF8` → `UTF-8`, `Environment.php:642-648`). `php_mb_get_encoding()` (`ext/mbstring/mbstring.c:234`) resolves names/aliases case-insensitively; unknown → `ValueError: mb_strtoupper(): Argument #2 ($encoding) must be a valid encoding, "BOGUS" given` (argument number per function: mb_strlen #2, mb_substr #4, mb_strtoupper/lower #2, mb_convert_case #3) [V]. Default illegal-character handling for conversions: mode CHAR with substitute `?` (U+003F).

### 6.2 UTF-8 decoder (`mb_utf8_to_wchar`)

`ext/mbstring/libmbfl/filters/mbfilter_utf8.c:446` [S][V]. Each `MBFL_BAD_INPUT` is one "character" (counts 1 in `mb_strlen`, becomes `?` in conversions):

```
c < 0x80: cp
c < 0xC2: BAD (1 byte)
c <= 0xDF: if next exists and (next & 0xC0) == 0x80: 2-byte cp else BAD (consume 1; if no next: consume 1)
c <= 0xEF: if >= 2 bytes remain:
              if b1 not trail || (c==E0 && b1<A0) || (c==ED && b1>=A0): BAD, consume 1
              elif b2 not trail: BAD, consume 2
              else cp (3 bytes)
           else (truncated): BAD; also consume b1 if it is a valid 2nd byte for c, and then b2 if trail
c <= 0xF4: if >= 3 bytes remain:
              if b1 not trail || (c==F0 && b1<90) || (c==F4 && b1>=90): BAD consume 1
              elif b2 not trail: BAD consume 2
              elif b3 not trail: BAD consume 3
              else cp (4 bytes)
           else (truncated): BAD; if b1 exists and is a valid 2nd byte for c, skip all following trail bytes
else: BAD (1 byte)
```

Verified: `mb_strlen("a\xFFb\xE2\x82")` = 4, `mb_strlen("\xF0\x9F\x98")` = 1, `mb_strlen("\xED\xA0\x80")` = 3; `mb_substr("a\xFF\xC3\xA9\xE2\x82b", 0)` → `a?é?b`; `mb_strtoupper("a\xFFb\xE2\x82")` → `A?B?` [V].

### 6.3 `mb_strlen` (`length` filter, `Markup::count()`)

`mbstring.c:1881` → `mb_get_strlen()` :1858: fixed-width encodings → bytes/width; UTF-8 strings flagged valid → count non-continuation bytes (`mb_fast_strlen_utf8` :1813); otherwise decode and count (6.2). Twig `length` (`CoreExtension.php:1289-1312`): null → 0; scalar → `mb_strlen((string))` (`12.5` → 4, `true` → 1); Countable/array/SimpleXMLElement → `count()`; Traversable → `iterator_count()`; Stringable → `mb_strlen((string))`; else 1 [V]. Port: S.

### 6.4 `mb_substr` (`slice`, `first`, `last`, `capitalize`, `split` with limit)

`mbstring.c:2354` [S][V]:

```
from == PHP_INT_MIN or len == PHP_INT_MIN → ValueError "mb_substr(): Argument #2 ($start) must be between -9223372036854775807 and 9223372036854775807" (resp. #3 ($length))
mblen = mb_get_strlen(str) if from < 0 or (len given and < 0)
real_from = from >= 0 ? from : (-from < mblen ? mblen + from : 0)
real_len  = len null ? UNTIL_END : len >= 0 ? len : (real_from < mblen && -len < mblen - real_from ? mblen - real_from + len : 0)
mb_get_substr(:2156): len == 0 or real_from >= byte length → ""; fixed-width encodings slice bytes;
otherwise decode → skip real_from code points → re-encode real_len code points (invalid input → '?')
```

Twig `slice` on strings: `mb_substr((string) $item, $start, $length, $charset)` (`CoreExtension.php:783`); on arrays `array_slice`; on Traversable a `LimitIterator` when start ≥ 0 and length ≥ 0 (null counts as ≥ 0), else `iterator_to_array` + `array_slice` (`:761-784`). `first`/`last` call `slice(…, 0, 1)`/`(…, -1, 1)` then `current()` for arrays (empty array → `false` → prints `""`). Verified: `"héllo wörld"|slice(1,3)` → `éll`, `slice(-3)` → `rld`, `slice(0,-2)` → `héllo wör`, `slice(20)` → `''`, `slice(-20, 2)` → `hé`, `inv|slice(1,1)` → `?`; `12345|first` → `1` [V]. Port: S.

### 6.5 Case conversion — `mb_strtoupper`, `mb_strtolower`, `mb_convert_case(MB_CASE_TITLE)`

Twig: `upper` → `mb_strtoupper`, `lower` → `mb_strtolower`, `title` → `mb_convert_case($s, MB_CASE_TITLE, $charset)`, `capitalize` → `mb_strtoupper(mb_substr($s, 0, 1)) . mb_strtolower(mb_substr($s, 1))` (`CoreExtension.php:1321-1373`). Source `mbstring.c:2958-3015` → `php_unicode_convert_case()` (`ext/mbstring/php_unicode.c:282`), lookups `php_unicode_toupper_raw/tolower_raw/totitle_raw` (:118-178, minimal perfect hash over `_uccase_{upper,lower,title,fold}_table`), properties `UC_CASED`(35)/`UC_CASE_IGNORABLE`(36) (`php_unicode.h:73-74,172-173`), tables `ext/mbstring/unicode_data.h` (5457 lines) generated by `ext/mbstring/ucgendat/ucgendat.php` from UnicodeData, CaseFolding, SpecialCasing, DerivedCoreProperties, EastAsianWidth — **Unicode 17.0** (`UPGRADING`: "Unicode data tables have been updated to Unicode 17.0"; `NEWS:1952`) [S].

Algorithm (full modes MB_CASE_UPPER/LOWER/TITLE), processing the input in chunks of 64 decoded code points [S]:

- Invalid input code points (> 0xFFFFFF marker) pass through and become `?` on output.
- Mapping results > 0xFFFFFF encode a special-casing sequence (length in the top byte, index into `_uccase_extra_table`): multi-code-point results are emitted (`ß` → `SS`, `ﬁ` → `FI`, `ŉ` → `ʼN`, `İ` → `i̇` in lower).
- Only **unconditional** SpecialCasing entries are used (`ucgendat.php:343-366` skips conditional/language rules) — no Turkish/Lithuanian rules, except encoding `ISO-8859-9` which maps `i`↔`İ`, `I`↔`ı` (`php_unicode.c:124-160`).
- ASCII fast paths: upper for cp < 0xB5, lower for cp < 0xC0.
- **Final sigma** (LOWER, and TITLE in lowercase mode): `Σ` (U+03A3) becomes `ς` (U+03C2) when preceded (skipping Case_Ignorable code points) by a Cased letter and not followed (skipping Case_Ignorable) by a Cased letter; look-behind scans only the current 64-code-point chunk and the not-yet-overwritten tail of the previous converted buffer (`scan_back_for_cased_letter`), look-ahead decodes further input (`scan_ahead_for_cased_letter`). Exact replication requires the same chunking.
- **TITLE**: `title_mode` starts false; for each code point: if `title_mode` → lowercase (with final-sigma rule) else → titlecase (fallback uppercase); then if the code point is not Case_Ignorable: `title_mode = is_cased(cp)`. Consequences [V]: `"hello wORLD o'neil mc-donald ǆungla straße"` → `Hello World O'neil Mc-Donald ǅungla Straße` (apostrophe is Case_Ignorable, hyphen is not); `"a.b,c_d1e 2f\tg's x’y «z» ŉx"` → `A.b,C_D1E 2F\tG's X’y «Z» ʼNx` (`.` and `’` are Case_Ignorable; digits end a word).
- Verified upper/lower: `'straße ǆ ﬁ ŉ'|upper` → `STRASSE Ǆ FI ʼN`; `'İSTANBUL ΣΑΣ ǅ'|lower` → `i̇stanbul σας ǆ` [V].

Port: L (pure Rust feasible: convert `unicode_data.h` or regenerate from Unicode 17.0 UCD with a script mirroring `ucgendat.php`; ~1-2k lines of tables + ~300 lines of logic). Do not use generic Rust case-mapping crates unless they match Unicode 17.0 *and* PHP's unconditional-only policy (UNVERIFIED).

### 6.6 `mb_ord`

`mbstring.c:5792`: empty string → `ValueError: mb_ord(): Argument #1 ($string) must not be empty`; first decoded code point, or `false` if invalid [S][V]. Twig only calls it on strings already validated as UTF-8 (escaper), so it equals the Rust `char as u32`.

### 6.7 Other encodings

libmbfl implements ~80 encodings (`ext/mbstring/libmbfl/filters/`). Supporting Twig `charset` values other than UTF-8 in `upper/lower/title/length/slice/split` requires per-encoding decoders/encoders with identical invalid-byte behaviour: XL. `ISO-8859-1` behaves as expected [V: `"a\xe9b"|upper` → `A\xC9B` with charset ISO-8859-1].

---

## 7. iconv and charsets

Twig: `convert_encoding` filter → `iconv($from, $to, $string ?? '')` (`CoreExtension.php:1257-1264`); escaper non-UTF-8 paths (`EscaperRuntime.php:194-197,203,248,255,269,277,335,354-361`); `reverse`/`shuffle`/`random` convert to UTF-8 and back when charset ≠ UTF-8. Source `ext/iconv/iconv.c:2260` (`PHP_FUNCTION(iconv)`), `_php_iconv_show_error()` :1806, `ICONV_CSNMAXLEN 64` (`php_iconv.h:65`) [S][V]:

- Charset name ≥ 64 bytes → `Warning: iconv(): Encoding parameter exceeds the maximum allowed length of 64 characters`, returns false.
- Errors (return `false`): `Notice: iconv(): Detected an illegal character in input string` (invalid input **or unmappable output character**), `Notice: iconv(): Detected an incomplete multibyte character in input string`, `Warning: iconv(): Wrong encoding, conversion from "%s" to "%s" is not allowed`, `Warning: iconv(): Cannot open converter`, `Warning: iconv(): Malformed string`, `Notice: iconv(): Unknown error (%d)`.
- `//TRANSLIT`, `//IGNORE` suffixes are glibc features: `iconv('UTF-8', 'ASCII//TRANSLIT', "aéb€")` → `aebEUR`; `'ISO-8859-1//IGNORE'` on `"a€b"` → `ab` [V].
- In Twig, `convertEncoding(): string` receives `false` and coerces it to `""` (weak return type) — output is empty plus the notice [V]. Escaper with a bogus charset prints `""` plus two warnings [V].

Port: pure-Rust parity with glibc iconv (alias names, TRANSLIT tables) is XL. Recommendation: support UTF-8 natively; optionally map a fixed set of charsets via `encoding_rs` (UNVERIFIED parity, especially TRANSLIT/IGNORE and error cases) or bind the system iconv.

---

## 8. PCRE (preg_*)

### 8.1 Library and configuration

- Bundled PCRE2 **10.44** (2024-06-07) (`ext/pcre/pcre2lib/pcre2.h:44-47`); PCRE Unicode version 15.0.0; JIT enabled; `pcre.backtrack_limit=1000000`, `pcre.recursion_limit=100000` (match/depth limits, `php_pcre.c:319,329`) [V].
- PHP 8.5 builds without `PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK` (UPGRADING) [S].
- Character tables: PCRE2 default (C locale) unless `setlocale(LC_CTYPE)` changed (Twig does not).

### 8.2 PHP pattern parsing (`pcre_get_compiled_regex_cache_ex`, `ext/pcre/php_pcre.c:576`, :620-790) [S][V]

1. Skip leading `isspace()`; nothing left → `Warning: preg_match(): Empty regular expression`.
2. Delimiter = next byte; alphanumeric, `\` or NUL → `Delimiter must not be alphanumeric, backslash, or NUL byte`. Bracket pairs `( [ { <` close with `) ] } >` (nesting counted); otherwise same char; backslash escapes skip the next byte. Not found → `No ending delimiter '%c' found` / `No ending matching delimiter '%c' found`.
3. Modifiers: `i` CASELESS, `m` MULTILINE, `n` NO_AUTO_CAPTURE, `s` DOTALL, `x` EXTENDED, `A` ANCHORED, `D` DOLLAR_ENDONLY, `r` EXTRA_CASELESS_RESTRICT, `S` ignored, `X` ignored, `U` UNGREEDY, `u` → `PCRE2_UTF | PCRE2_UCP | PCRE2_NEVER_BACKSLASH_C`, `J` DUPNAMES, space/`\n`/`\r` ignored; anything else (incl. `e`) → `Unknown modifier '%c'`, NUL → `NUL byte is not a valid modifier`.
4. Compile failure → `Compilation failed: %s at offset %zu` (PCRE2 message from `pcre2_get_error_message`; `\C` with `u` → `using \C is incompatible with the 'u' modifier`).
5. All of the above: warning + function returns `false` (and `preg_last_error()` = `PREG_INTERNAL_ERROR`).

`preg_last_error_msg()` (`php_pcre.c:143`): `No error`, `Internal error`, `Malformed UTF-8 characters, possibly incorrectly encoded`, `The offset did not correspond to the beginning of a valid UTF-8 code point`, `Backtrack limit exhausted`, `Recursion limit exhausted`, `JIT stack limit exhausted`.

### 8.3 Twig usages

| use | pattern | equivalent pure-Rust implementation |
|---|---|---|
| `matches` operator (`CoreExtension::matches`, `:1182-1196`) | user pattern | needs PCRE2 10.44 (see 8.4) |
| escaper validity (`EscaperRuntime.php:206,258,280`) | `//u` | `std::str::from_utf8(bytes).is_ok()` (PCRE2 UTF check = RFC 3629). Vectors [V]: rejected `C0 80`, `F0 8F 80 80` (overlong), `ED A0 80`, `ED BF BF` (surrogates), `F4 90 80 80` (> U+10FFFF), `F5 80 80 80`, `F8 88 80 80`, `E2 82` (truncated); accepted `F4 8F BF BF`, `EF BF BF` (noncharacter), `EE BF BF`, `C2 A0`, `00`, `7F`, `E0 A0 80`, `F0 90 80 80` — identical to Rust's validator |
| escaper js/css/html_attr | `#[^a-zA-Z0-9,\._]#Su`, `#[^a-zA-Z0-9]#Su`, `#[^a-zA-Z0-9,\.\-_]#Su`, `#[^a-zA-Z0-9,\.\-_:@\[\]]#Su` | iterate `char`s (valid UTF-8 guaranteed); callbacks see one code point |
| `splitIntoCharacters` (`:1273-1280`) | `/./us` | valid UTF-8 → one string per `char` (code points, not graphemes); invalid → error; empty → `['']` |
| `spaceless` (`:1245-1248`) | `/>\s+</` | replace `>` + run of `[ \t\n\x0B\f\r]` + `<` with `><`, then `trim()` |
| dynamic filter/function/test names (`ExtensionSet.php:509-526`) | `#^prefix(.*?)suffix$#` | simple wildcard matcher (lazy groups) |

Twig error texts [V]:
- runtime invalid regex: `Regexp "%s" passed to "matches" is not valid` + `substr($warning, 12)`, e.g. `Regexp "/(b/" passed to "matches" is not valid: Compilation failed: missing closing parenthesis at offset 2`; constant regexes give the same text as **SyntaxError** at compile time (`MatchesBinary.php:35-39`).
- match failure: `Regexp "/./u" passed to "matches" failed: Malformed UTF-8 characters, possibly incorrectly encoded.`
- `Unable to split the string passed to "reverse" into characters: Malformed UTF-8 characters, possibly incorrectly encoded.`
- escaper: `The string to escape is not a valid UTF-8 string.`
- `matches` returns PHP int `1`/`0` (prints `1`/`0`).

### 8.4 Port strategy for `matches`

Bind PCRE2 10.44 (C, BSD licence) via FFI (`pcre2-sys` style, building the exact 10.44 sources), compile with the flags of 8.2, JIT on, match limit 1,000,000 and depth limit 100,000, and port PHP's delimiter/modifier parser and warning texts. Pure-Rust engines (`regex`, `fancy-regex`) differ in syntax and semantics (backreferences, possessive quantifiers, `\K`, recursion, UCP classes, error messages): XL to emulate.

---

## 9. Math

### 9.1 `abs` (filter)

`ext/standard/math.c:252` [S][V]: `Z_PARAM_NUMBER` coercion (4.1); int → int except `PHP_INT_MIN` → float `9.2233720368547758E+18`; float → `fabs` (`-0.0` → `0.0`). Examples: `abs("-3")` → 3, `abs("-3.5")` → 3.5, `abs("1e3")` → 1000.0, `abs(" 12")`/`("12 ")` → 12, `abs(null)` → 0 + deprecation, `abs(false)` → 0, `abs("12abc")`/`("x")`/object → TypeError `abs(): Argument #1 ($num) must be of type int|float, <type> given`. Port: S.

### 9.2 `round` (filter) — PHP 8.4+ algorithm

Twig (`CoreExtension.php:660-673`): `$value = (float) $value`; `'common'` → `round($value, $precision)`; `'ceil'`/`'floor'` → `$method($value * 10 ** $precision) / 10 ** $precision`; other → RuntimeError `The "round" filter only supports the "common", "ceil", and "floor" methods.`. Source `math.c:339` (`PHP_FUNCTION(round)`), `_php_math_round` :172, `php_round_helper` :70, `php_intpow10` :40 [S][V]:

```
PHP_FUNCTION(round): precision via weak int (float → deprecation + truncation); places = clamp to int range;
  mode validated (1..8 or RoundingMode) else ValueError "round(): Argument #3 ($mode) must be a valid rounding mode (RoundingMode::*)"
  int input with places >= 0 → (float) value; otherwise _php_math_round((double) value, places, mode); always returns float

_php_math_round(value, places, mode):
  if !finite(value) || value == 0.0: return value
  if places == 0 && value == trunc(value): return value
  places = max(places, INT_MIN+1)
  exponent = php_intpow10(|places|)          // exact table 1e0..1e22, else pow(10.0, |places|)
  if value >= 0: tmp = floor(places > 0 ? value*exponent : value/exponent); tmp2 = tmp + 1
  else:          tmp = ceil (places > 0 ? value*exponent : value/exponent); tmp2 = tmp - 1
  if (places > 0 ? tmp2/exponent : tmp2*exponent) == value: tmp = tmp2     // pre-rounding error fix
  if |tmp| >= 1e16: return value
  tmp = php_round_helper(tmp, value, exponent, places, mode)
  if |places| < 23: return places > 0 ? tmp/exponent : tmp*exponent
  else: buf = snprintf("%15fe%d", tmp, -places); r = zend_strtod(buf); return finite(r) ? r : value

php_round_helper(integral, value, exponent, places, mode) [HALF_UP, the only mode Twig uses]:
  edge = places > 0 ? |(integral + copysign(0.5, integral)) / exponent| : |(integral + copysign(0.5, integral)) * exponent|
  return |value| >= edge ? integral + copysign(1.0, integral) : integral
  (HALF_DOWN: >, HALF_EVEN/ODD: tie → make even/odd, CEILING/FLOOR/AWAY/TOWARD_ZERO use edge = |integral/exponent| or |integral*exponent|)
```

Verified: `round(2.5)` → 3.0; `(-2.5)` → -3.0; `(0.285, 2)` → 0.29; `(1.955, 2)` → 1.96; `(5.045, 2)` → 5.05; `(5.055, 2)` → 5.06; `(1.45, 1)` → 1.5; `(1234567.891, -3)` → 1235000.0; `(0.1+0.2, 15)` → 0.3; `(1.23456789, 25)` → 1.23456789; `(123456.0, -30)` → 0.0; `round(35, -1)` → 40.0; Twig `1.255|round(2)` → `1.26`, `|round(2, 'floor')` → `1.25`, `|round(2, 'ceil')` → `1.26`, `42|round` → `42` [V]. Port: S (needs C-compatible `floor/ceil/fabs/copysign/fmod`; `pow()` only for |places| > 22 — result may differ in the last ulp between libm implementations: UNVERIFIED for Rust `powf` on non-glibc targets).

### 9.3 `floor`, `ceil`

`math.c:294/275`: `Z_PARAM_NUMBER`; int → float (no rounding), float → C `floor`/`ceil`; always float (`ceil(-0.5)` → `-0.0`, prints `-0`). Used by `round` methods, `batch` (`(int) ceil($size)`), `//` (`(int) floor(a / b)`: `7 // 2` → 3, `-7 // 2` → -4, `7.5 // 2` → 3, `1e20 // 1` → `7766279631452241920` + warning) [V]. Port: S.

### 9.4 `max`, `min`

See 4.7 for tie rules. Source `ext/standard/array.c:1197-1460` [S][V]:

- one argument: must be array else `TypeError: max(): Argument #1 ($value) must be of type array, int given`; empty → `ValueError: max(): Argument #1 ($value) must contain at least one element`; result `zend_hash_minmax(ht, php_data_compare, flag)` (`compar(res, cur) < 0` replaces for max, `> 0` for min).
- ≥2 arguments: fast paths for int/int, float/float and int↔float when the int is exactly representable as double; otherwise `zend_compare(arg_i, current) > 0` (max) / `< 0` (min).
- Examples: `max(1,"2",3.5)` → 3.5; `max("abc", 0)` → `"abc"`; `max("10","9a")` → `"9a"`; `max([1,2],[1,3])` → `[1,3]`; `max(1,[0])` → `[0]`; `max(null,false)` → `null`; `max(false,null)` → `false`; `max(0,-0.0)` → 0; `max(-0.0,0)` → `-0.0`; `max(NAN,1)` → 1; `max(1,NAN)` → NAN; `min(NAN,1)` → 1; `min(1,NAN)` → NAN; `min(['10',9,'9a','abc'])` → 9; `max(['10',9,'9a','abc'])` → `'abc'`; `max(new stdClass, 1)` → object + `Notice: Object of class stdClass could not be converted to int` [V].
- Port: S once `zend_compare` exists.

### 9.5 Operators used by templates (not builtins; `Zend/zend_operators.c`)

| Twig | PHP | key behaviours [V] |
|---|---|---|
| `/` | `div_function` | int/int exact → int (`10/5` → 2), else float (`10/4` → 2.5); `/ 0` → `DivisionByZeroError: Division by zero` |
| `//` | `(int) floor(a / b)` | see 9.3 |
| `%` | `mod_function` | operands converted to int (float with fraction → deprecation `Implicit conversion from float 7.5 to int loses precision`); `% 0` → `DivisionByZeroError: Modulo by zero`; sign of dividend (`-7 % 3` → -1); `PHP_INT_MIN % -1` → 0; non-numeric string → `TypeError: Unsupported operand types: string % int` |
| `**` | `pow_function` | int**int stays int unless overflow/negative exponent (`2**-1` → 0.5, `2**63` → float); `0 ** -1` → INF + `Deprecated: Power of base 0 and negative exponent is deprecated`; `(-8)**(1/3)` → NAN |
| `+ - *` | arithmetic | int overflow → float; `"5" + "5.0"` → 10.0; `"5abc" + 1` → 6 + `Warning: A non-numeric value encountered`; `"abc" + 1`, `[] + 1` → `TypeError: Unsupported operand types: string + int` / `array + int`; `null + 1` → 1 |
| `b-and b-or b-xor` | bitwise | float operands → int with deprecation if fractional; string & string → bytewise (`"a" & "b"` → `` ` ``) |
| `is even/odd/divisible by` | `%` | as `%` (`3.5 is odd` → deprecation, true) |

---

## 10. Arrays, sorting, iteration

### 10.1 `range` (function `range()` and operator `a..b`) — PHP 8.3+ rules

Source `ext/standard/array.c:2999`, `php_range_process_input` :2932, size macros :2890-2925, `HT_MAX_SIZE` = `0x40000000` on 64-bit (`Zend/zend_types.h:471`) [S][V]. Port-ready:

```
step (optional, int|float, weak coercion):
  float: INF/NAN → ValueError "range(): Argument #3 ($step) must be a finite number, INF provided" (NAN provided);
         negative → is_step_negative, step = -step; step_long = (int)step_double (silent); is_step_double = (double)step_long != step_double
  int:   PHP_INT_MIN → ValueError "range(): Argument #3 ($step) must be greater than -9223372036854775808"; negative → flag + negate
  step_double == 0 → ValueError "range(): Argument #3 ($step) cannot be 0"
process_input(x, argnum) → type:
  int → IS_LONG; float → INF/NAN ValueError "range(): Argument #N ($start|$end) must be a finite number, INF provided" else IS_DOUBLE
  string: "" → Warning "range(): Argument #N ($name) must not be empty, casted to 0" → IS_LONG 0
          numeric float string → IS_DOUBLE (finite check); numeric int string → IS_ARRAY if length 1 else IS_LONG
          non-numeric: length != 1 → Warning "range(): Argument #N ($name) must be a single byte, subsequent bytes are ignored"; → IS_STRING (numeric fallback 0)
if start_type >= IS_STRING or end_type >= IS_STRING:          // ordering IS_LONG < IS_DOUBLE < IS_STRING < IS_ARRAY
  if one side < IS_STRING:
     if start < IS_STRING and end != IS_ARRAY: Warning "range(): Argument #1 ($start) must be a single byte string if argument #2 ($end) is a single byte string, argument #2 ($end) converted to 0"
     elif end < IS_STRING and start != IS_ARRAY: Warning "... Argument #2 ($end) must be a single byte string if argument #1 ($start) is a single byte string, argument #1 ($start) converted to 0"
     → numeric handling (the string side becomes its numeric fallback)
  elif is_step_double: if either is IS_STRING: Warning "range(): Argument #3 ($step) must be of type int when generating an array of characters, inputs converted to 0"; both → 0; numeric handling
  else character range on first bytes low..high stepping by step (unsigned bytes, stops before going below 0 / above 255);
       decreasing allowed with positive step; increasing with negative step → negative_step_error; |high-low| < step → boundary_error; equal → [char]
numeric: if any double or is_step_double: float range: element_i = start ± i*step (computed from start, not accumulated),
           size = round_half_up((|end-start|/step) + 1) with HT_MAX_SIZE check, loop while i < size and element within end
         else int range with unsigned arithmetic, size = (|end-start| / step) + 1
errors: negative_step_error → ValueError "range(): Argument #3 ($step) must be greater than 0 for increasing ranges"
        boundary_error → ValueError "range(): Argument #3 ($step) must be less than the range spanned by argument #1 ($start) and argument #2 ($end)"
        too large → ValueError "The supplied range exceeds the maximum array size by %.1f elements: start=%.1f, end=%.1f, step=%.1f. Max size: %.0f" (float)
                    or "The supplied range exceeds the maximum array size by %u elements: start=%d, end=%d, step=%d. Calculated size: %u. Maximum size: %u." (int)
        (the macros always print the smaller bound as "start" and the larger as "end": range(0, 1e10) and range(1e10, 0) both give
         "... by 8926258177.0 elements: start=0.0, end=10000000000.0, step=1.0. Max size: 1073741824"; range(PHP_INT_MAX, 0) gives
         "... by 9223372035781033984 elements: start=0, end=9223372036854775807, step=1. Calculated size: 9223372036854775807. Maximum size: 1073741824." [V])
```

Verified examples: `range(1,2,0.3)` → `[1.0,1.3,1.6,1.9]`; `range(0,1,0.1)` → `[0.0,0.1,0.2,0.30000000000000004,0.4,0.5,0.6000000000000001,0.7000000000000001,0.8,0.9,1.0]`; `range(1,5,2.0)` → ints `[1,3,5]`; `range("1","5")` → strings `["1",…,"5"]`; `range("1","10")` → ints; `range("A","z",10)` → `["A","K","U","_","i","s"]`; `range("aa","cc")` → `["a","b","c"]` + 2 warnings; `range("a",5)` → `[0..5]` + warning; `range("",3)` → `[0..3]` + warning; `range("1.5","3")` → `[1.5,2.5]`; `range("a","c",1.5)` → `[0.0]` + warning; `range(null,3)` → `[0..3]` + deprecation; `range([],3)` → TypeError `range(): Argument #1 ($start) must be of type string|int|float, array given`; `range("0","a")` → 50 chars `"0"…"a"` [V]. Port: M, pure Rust.

### 10.2 Sorting — `sort` filter (`asort` / `uasort`), method-name `sort`

Twig (`CoreExtension.php:1049-1066`): Traversable → `iterator_to_array` (keys preserved); non-array → RuntimeError `The "sort" filter expects a sequence or a mapping, got "%s".`; arrow given → `uasort($array, $arrow)` (non-Closure callables: sandbox RuntimeError `The callable passed to the "sort" filter must be a Closure in sandbox mode.` or deprecation); else `asort($array)`. **Keys are preserved** (`[3,1,2]|sort` → `{"1":1,"2":2,"0":3}`) [V].

Pipeline [S]: `PHP_FUNCTION(asort)` (`array.c:754`) → `php_get_data_compare_func(SORT_REGULAR)` → stable wrapper of `php_array_data_compare_unstable_i` (:286, `zend_compare` + enum special case) → `zend_array_sort()` → `zend_hash_sort_internal()` (`Zend/zend_hash.c:3200`): copies buckets without holes, stores original order in `Z_EXTRA` (stable fallback `stable_sort_fallback`, `array.c:88-105`), calls `zend_sort()` (`Zend/zend_sort.c:248`) with swap of whole buckets; `renumber=false` for asort/uasort (keys kept), `true` for sort/usort.

`uasort` (`array.c:936` → `php_usort` :894, compare `php_array_user_compare_unstable` :837): the array is duplicated before sorting (callback sees the original); callback result converted with `php_get_long()` (`zval_get_long_func`, :649) and normalised to -1/0/1 — floats are truncated toward zero **silently** (no deprecation), so `sort((a, b) => a - b)` on `[1.5, 1.2, 1.4]` treats all pairs as ties and keeps the input order; numeric strings convert, `null` → 0, non-empty array → 1 [V]; if the callback returns **bool**: one-time `Deprecated: uasort(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero`; `true` → 1; `false` → call again with swapped arguments and return `-normalize(result)`. Ties fall back to original order.

`zend_sort` = hybrid: `nmemb <= 16` → `zend_insert_sort` (special networks for 2-5 elements; for ≥6: plain insertion for the first 6, then a "step-2" backwards search) else median-of-3 (or median-of-5 when `nmemb >= 1024`) quicksort recursing into the smaller side. **Validated port in section 19** (126 arrays with an inconsistent comparator, sizes 1..2000, 0 mismatches; mixed-type `asort` identical) [V]. Example test vector: `['10', 9, '9a', 'abc', 1.5, true, null]|sort` → `{"6":null,"4":1.5,"1":9,"0":"10","2":"9a","3":"abc","5":true}`; `['b','B','a','A','_','1']|sort` → `{"5":"1","3":"A","1":"B","4":"_","2":"a","0":"b"}` [V]. Port: M (needs `zend_compare`).

`getAttribute` method-cache construction also calls `sort($methods)` (`CoreExtension.php:1914`) to fix precedence `get` > `is` > `has` among `get_class_methods()` results — host-object specific in twig-rs.

### 10.3 Other array builtins

| builtin (php-src) | Twig use | semantics to replicate [V unless noted] |
|---|---|---|
| `array_merge` (`array.c:4457`) | `merge` filter (`CoreExtension.php:734-747`), `include` (`array_merge($context, $variables)`), emitted block/trait merges | string keys: later wins, keep first position; int keys renumbered 0.. in order: `{a:1,5:'x','7':'y'} + {a:2,5:'z',b:3}` → `{"a":2,"0":"x","1":"y","2":"z","b":3}`; Twig error `The "merge" filter expects a sequence or a mapping, got "%s" for argument %d.` |
| `array_chunk` (`:7133`) | `batch` | size < 1 → `ValueError: array_chunk(): Argument #2 ($length) must be greater than 0`; Twig passes `preserveKeys` (default **true**): `[1,2,3,4,5]\|batch(2)` → `[[1,2],{"2":3,"3":4},{"4":5}]`; fill appended with `[]` (next int key): `{a:1,b:2,c:3}\|batch(2,'x')` → `[{"a":1,"b":2},{"c":3,"0":"x"}]` |
| `array_column` (`:4740`) | `column` filter | rows: arrays (key lookup) or objects (accessible props, `__isset`/`__get`); missing column → row skipped; null column → whole row; index key values: int/string used, float → int (+ deprecation if fractional), bool → int, null → `""` (+ PHP 8.5 deprecation `Using null as an array offset is deprecated, use an empty string instead`), array → `TypeError: Cannot access offset of type array on array`; `column_key` array → `TypeError: array_column(): Argument #2 ($column_key) must be of type string\|int\|null, array given` |
| `array_slice` (`:3962`) | `slice`, `join` | see 6.4 for offsets; `length` null → to end; string keys always kept; int keys renumbered unless `preserve_keys` |
| `array_reverse` (`:4803`) | `reverse` | int keys renumbered unless preserve: `{a:1,2:'x',5:'y'}` → `{"0":"y","1":"x","a":1}` |
| `array_keys` / `array_values` (`:4485/:4625`) | `keys` filter, `toArray(…, false)`, error messages (`implode(', ', array_keys($object))`) | numeric-string keys are ints (`'10'` → 10) |
| `array_filter` (`:6667`) | `filter` filter on arrays with `ARRAY_FILTER_USE_BOTH` → callback `(value, key)`, keys preserved, truthiness of result | on Traversable Twig returns `new \CallbackFilterIterator(new \IteratorIterator($it), $arrow)` (lazy, keys preserved) |
| `array_pad` (`:4857`) | sequence destructuring | pads right (positive) / left (negative), no-op when long enough |
| `array_intersect_key` (`:5546`) + `+=` | for-loop context restore | keys compared as hash keys |
| `array_key_exists` (`:7091`) | `getAttribute` with `(string)` key; emitted `NameExpression` | numeric strings match int keys (`'1'` ↔ 1, `'01'` ≠ 1); null key deprecated in 8.5 (`Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead`) |
| `in_array` strict (`:1749`) | `in` with object/resource needle, `ARRAY_LIKE_CLASSES` check | identity |
| `array_is_list` (`ext/standard/type.c:324`) | `sequence`/`mapping` tests | keys exactly 0..n-1 in order; `[]` is a list (`[] is sequence` → true, `[] is mapping` → false) |
| `count` (`array.c:661`) | many | arrays, Countable (`->count()` result via `php_get_long`); other → `TypeError: count(): Argument #1 ($value) must be of type Countable\|array, %s given` |
| `current` (`:1157`) | `first`/`last` | empty array → `false` |
| `array_unique`, `array_key_first`, `array_diff_key`, `array_combine`, `array_shift/unshift/pop/push`, `ksort`, `arsort` | internal bookkeeping | not template-observable |
| `iterator_to_array` (`ext/spl/spl_iterators.c:3041`) | `toArray`, slice, reverse, sort, column, tests, capture | arrays: copy or `array_values`; iterators with keys: `array_set_zval_key` (`Zend/zend_API.c:2268`): string → symtable (numeric strings → int), int, bool → 0/1, float → int (+ deprecation), null → `""` (+ 8.5 deprecation), resource → int + warning, array/object → `TypeError: Cannot access offset of type array on array` (`... of type stdClass on array`); duplicate keys overwrite (`yield 'a'=>1; yield 'a'=>2` → `{"a":2}`); `preserve_keys=false` → list |
| `iterator_count` (`:3075`) | `length`, `empty` test | consumes the iterator |

### 10.4 Iterators (`slice`, `keys`, `filter`)

- `LimitIterator` (`spl_iterators.c:2202`, seek :2163): Twig `slice` on `Iterator` with start ≥ 0, length ≥ 0 → `iterator_to_array(new \LimitIterator($it, $start, $length ?? -1), $preserveKeys)`; `OutOfBoundsException` (messages `Cannot seek to %d which is below the offset %d`, `Cannot seek to %d which is behind offset %d plus count %d`) is caught → `[]` (`CoreExtension.php:768-774`). Non-seekable inner iterators are rewound and advanced with `next()`.
- `IteratorAggregate` unwrapped with `getIterator()` loops (`:764-766`, `:933-935`).
- `keys` on iterators: explicit `rewind/valid/key/next` loop.

---

## 11. JSON (json_encode)

Twig: `json_encode` filter = PHP `json_encode($value, $flags = 0, $depth = 512)` directly (`CoreExtension.php:253`); `Markup` implements `JsonSerializable` (returns its string). Source `ext/json/json.c:220`, encoder `ext/json/json_encoder.c` (`php_json_encode_zval` :644, `php_json_encode_array` :109, `php_json_escape_string` :366, `php_json_encode_double` :79) [S][V].

- Errors return `false` (Twig prints `""`) and set `json_last_error_msg()`: `Malformed UTF-8 characters, possibly incorrectly encoded`, `Inf and NaN cannot be JSON encoded`, `Maximum stack depth exceeded`, `Non-backed enums have no default serialization`, recursion, `Type is not supported` (resources). `JSON_PARTIAL_OUTPUT_ON_ERROR` substitutes `null`; `JSON_THROW_ON_ERROR` throws `JsonException` with the same message (wrapped by Twig). `depth` outside int range → `ValueError: json_encode(): Argument #3 ($depth) must be between -2147483648 and 2147483647`.
- null/true/false/int as usual; float: `zend_gcvt(d, -1, '.', 'e')` (4.3), INF/NaN → error; `JSON_PRESERVE_ZERO_FRACTION` adds `.0` when no `.` (`[1.0,1e25]` → `[1.0,1.0e+25]`).
- arrays: list (`zend_array_is_list`) → `[...]`, otherwise object with keys as strings (`{"1":"a"}`, `[0=>'a',2=>'b']` → `{"0":"a","2":"b"}`); `JSON_FORCE_OBJECT`; empty array → `[]` (Twig `{}` literal is an empty array → `[]`).
- objects: `JsonSerializable` → its result; backed enum → value; unbacked → error; others → public properties (declared order, then dynamic; uninitialised typed props skipped; DateTime family uses its property table: `{"date":"2024-02-29 13:14:15.123456","timezone_type":3,"timezone":"Europe\/Vilnius"}`); empty object → `{}`.
- strings: `JSON_NUMERIC_CHECK` first (numeric strings → numbers: `['1','1.5','1e3',' 1','abc','0x1A']` → `[1,1.5,1000,1,"abc","0x1A"]`); escape `"` → `\"` (or `\u0022` with HEX_QUOT), `\` → `\\`, `/` → `\/` unless UNESCAPED_SLASHES, `\b \f \n \r \t`, other bytes < 0x20 → `\u00XX` (lowercase hex digits `0123456789abcdef`), `<`/`>` → `\u003C`/`\u003E` with HEX_TAG, `&` → `\u0026` with HEX_AMP, `'` → `\u0027` with HEX_APOS; non-ASCII decoded with `php_next_utf8_char` (= html.c decoder, 5.8): invalid → error / `INVALID_UTF8_IGNORE` drops / `INVALID_UTF8_SUBSTITUTE` → `\ufffd` (or raw U+FFFD with UNESCAPED_UNICODE); valid → `\uXXXX` (surrogate pairs above U+FFFF, lowercase hex) unless UNESCAPED_UNICODE, but U+2028/U+2029 stay escaped unless UNESCAPED_LINE_TERMINATORS too. Byte 0x7F is **not** escaped.
- `JSON_PRETTY_PRINT`: newline + 4 spaces per depth, `": "` separator, empty arrays/objects stay `[]`/`{}`.
- Verified: `"\x00\x01\x1f\x7f\u{2028}\u{2029}é😀"` → `"\u0000\u0001\u001f` + raw 0x7F + `\u2028\u2029\u00e9\ud83d\ude00"`; floats `[1.0,0.1,1e100,-0.0,1e15,1e17,123456789.12345678,5e-324]` → `[1,0.1,1.0e+100,-0,1000000000000000,1.0e+17,123456789.12345678,5.0e-324]` [V].
- Flag constants: see section 18. Port: M, pure Rust.

---

## 12. Random (random(), shuffle)

- `random()` (`CoreExtension.php:460-511`): no args → `mt_rand()` (0..2^31-1); `max` only → `mt_rand(0, (int)$max)`; int/float `values` → range `[min(values,0), max(values,0)]` or `[values, max]` via `mt_rand((int)$min, (int)$max)` (float → int casts, 8.5 warnings for huge values); string → characters (`splitIntoCharacters`, charset conversion if not UTF-8), `''` → `''`; iterable → `toArray` then `$values[array_rand($values, 1)]`; empty → RuntimeError `The "random" function cannot pick from an empty sequence or mapping.`; other scalars returned as is.
- `mt_rand` (`ext/random/random.c:523`): `max < min` → `ValueError: mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)` [V]; uniform via `php_mt_rand_common` (:462) on the global MT19937 state (seeded randomly per process unless `mt_srand`).
- `array_rand` (`array.c:6505`, `php_array_pick_keys` :6355): empty → `ValueError: array_rand(): Argument #1 ($array) must not be empty`.
- `shuffle` (`array.c:3351`, `php_array_data_shuffle` :3253): Fisher–Yates with the default engine; result is a list.
- `cycle()` uses `$values[$position % $count]` (PHP `%`; for non-list arrays the computed index may be missing → `Warning: Undefined array key 1` and `null`) [V].
- Exact random sequences are not reproducible across processes anyway; port with any PRNG but keep range semantics and error messages. Port: S.

---

## 13. Date/time and timelib

### 13.1 Twig usage

- `date` filter `formatDate($date, $format = null, $timezone = null)` (`CoreExtension.php:522-534`): DateInterval → `$date->format($format ?? '%d days')`; else `convertDate($date, $timezone)->format($format ?? 'F j, Y H:i')` (defaults `:138`).
- `convertDate` (`:585-630`): timezone `false` = keep, null = default (`new \DateTimeZone(date_default_timezone_get())` = `UTC` by default, `:184-191`), string → `new \DateTimeZone($tz)`; DateTimeImmutable → `setTimezone`; DateTime → clone + `setTimezone`; null/`'now'` → `new \DateTime('now', tz)`; `(string)$date` all digits or `-digits` → `new \DateTime('@'.$date)` (UTC offset +00:00) then `setTimezone`; else `new \DateTime($date)` (parsed in the default timezone unless the string contains a zone) then `setTimezone` unless `false`.
- `date_modify` filter → `convertDate($date, false)->modify($modifier)` (`:548-551`).
- `date()` function → `convertDate`.
- `getAttribute` on `DateTimeInterface` exposes `date`, `timezone`, `timezone_type` from `((array) $object)[$item]` (`:1882-1888`); any public method is callable (`format, modify, add, sub, getTimezone, setTimezone, getOffset, getMicrosecond, setTime, setDate, setISODate, setTimestamp, setMicrosecond, getTimestamp, diff`, statics `createFromFormat`, `createFromTimestamp`, `createFromInterface`, `createFromMutable/Immutable`, `getLastErrors`); `DateTimeZone`: `getName, getOffset, getTransitions, getLocation, listAbbreviations, listIdentifiers`; `DateInterval`: `format`, `createFromDateString`; props `y m d h i s f invert days from_string` [V].

### 13.2 Construction and errors (`ext/date/php_date.c`)

- `php_date_initialize()` (:2398): empty string → `"now"`; `timelib_strtotime()`; constructor errors throw `DateMalformedStringException: Failed to parse time string (%s) at position %d (%c): %s` with the first timelib error (position, offending char or space, message) [V: `Failed to parse time string (not a date) at position 0 (n): The timezone could not be found in the database`; `( 1700000000) at position 8 (0): Unexpected character`]. Missing fields filled from "now" in the target zone (`timelib_fill_holes`), then `timelib_update_ts` / `timelib_update_from_sse`.
- `DateTimeZone::__construct` (:4076) → `timezone_initialize()` (:4018): `timelib_parse_zone`; offsets ≥ ±100 h → `Timezone offset is out of range (%s)`; unknown → `DateInvalidTimeZoneException: DateTimeZone::__construct(): Unknown or bad timezone (%s)` [V].
- `DateTime::modify` / `DateTimeImmutable::modify` (:3364/:3389) → `php_date_modify()` (:3263): parse with timelib; errors become `DateMalformedStringException: DateTime::modify(): Failed to parse time string (%s) at position %d (%c): %s` (EH_THROW turns the warning into the exception; text keeps the `DateTime::modify(): ` prefix) [V]; copies y/m/d/h/i/s/us that were set, resets i/s when only h set, applies relative parts, `@ts` resets zone to UTC.
- `date.timezone` INI default `"UTC"` (:254); `date_default_timezone_get()` (:5486) → `"UTC"` without warning [V].
- Odd inputs (all [V], all timelib-specific): `1700000000.5|date('Y-m-d H:i:s.u')` → `1699-11-30 00:05:00.000000`; `1.5|date('U')` → a "now"-relative value; `'2024-01-31'|date_modify('+1 month')` → `2024-03-02`; `'last day of next month'` → `2024-02-29`.

### 13.3 `DateTime::format` characters (`date_format()`, `php_date.c:672`) [S][V]

| char | output | char | output |
|---|---|---|---|
| `d` | day `%02d` | `D` | `Sun..Sat` |
| `j` | day `%d` | `l` | `Sunday..Saturday` (`Unknown` if weekday < 0) |
| `S` | English suffix: 10-19 → `th`, else last digit 1/2/3 → `st/nd/rd`, else `th` | `w` | 0 (Sun)..6 |
| `N` | ISO 1 (Mon)..7 | `z` | day of year 0-based |
| `W` | ISO week `%02d` | `o` | ISO year (`%lld`) |
| `F` | `January..December` | `m` | `%02d` |
| `M` | `Jan..Dec` | `n` | `%d` |
| `t` | days in month | `L` | 1/0 leap |
| `y` | `%02d` of `y % 100` (negative years give e.g. `-5`) | `Y` | `-` if negative, then at least 4 digits |
| `x` | like `Y` but `+` prefix when ≥ 10000 | `X` | always sign (`+2024`) |
| `a`/`A` | `am`/`pm`, `AM`/`PM` | `B` | Swatch beat `%03d` (from UTC seconds +3600) |
| `g`/`h` | 12-hour `%d`/`%02d` (0 → 12) | `G`/`H` | 24-hour `%d`/`%02d` |
| `i`/`s` | `%02d` | `u` | microseconds `%06d` | 
| `v` | milliseconds `%03d` | `e` | zone id (TYPE_ID), abbreviation (TYPE_ABBR), `+HH:MM[:SS]` (TYPE_OFFSET); `UTC` if not local |
| `I` | DST 1/0 | `O` / `P` / `p` | `+HHMM` / `+HH:MM` / `Z` when UTC, `Z`, or `GMT+0000`, else as `P` |
| `T` | abbreviation; for offset zones `GMT+HHMM`-style abbr (`GMT%c%02d%02d`) | `Z` | offset seconds |
| `c` | `Y-m-d\TH:i:sP` with `%04lld` year | `r` | `D, d M Y H:i:s O` (`%3s, %02d %3s %04lld ...`) |
| `U` | Unix seconds | `\` | next char literal; a **trailing** `\` emits a NUL byte (`'Y\\'` → `2024\0`) [V] |
| other | copied | `''` format | `''` |

Verified sample: `'2024-03-04 05:06:07'|date('d D j l N S w z W F m M n t L o X x Y y a A B g G h H i s')` → `04 Mon 4 Monday 1 th 1 63 10 March 03 Mar 3 31 1 2024 +2024 2024 2024 24 am AM 254 5 5 05 05 06 07` (B computed at UTC) [V].

### 13.4 `DateInterval::format` (`date_interval_format()`, `php_date.c:4957`) [S][V]

`%Y` `%02d` years, `%y` `%d`; `%M`/`%m` months; `%D`/`%d` days; `%H`/`%h`; `%I`/`%i`; `%S`/`%s`; `%F` `%06d` microseconds, `%f` `%d`; `%a` total days or `(unknown)`; `%R` `+`/`-`; `%r` `-` or empty; `%%` → `%`; unknown `%q` → `%q`; a trailing lone `%` is dropped. Verified: diff 2024-01-01 → 2024-03-05 10:20:30 formatted `'%y %Y %m %M %d %D %h %H %i %I %s %S %f %F %a %R %r %% %q %'` → `0 00 2 02 4 04 10 10 20 20 30 30 0 000000 64 +  % %q ` [V].

### 13.5 Property table (`(array)` cast, json, var_dump)

`date_object_to_hash()` (`php_date.c:1930`): `date` = `date_format("x-m-d H:i:s.u")`, `timezone_type` (1 offset, 2 abbreviation, 3 id), `timezone` (`+05:00`/`+05:00:01` via `date_create_tz_offset_str` :1904, abbreviation, or id). `DateTimeZone` exposes `timezone_type`, `timezone` [V].

### 13.6 timelib in php-src (`ext/date/lib`)

| file | lines | role |
|---|---|---|
| `parse_date.re` | 2791 (79 KB) | re2c source of the lenient parser (`timelib_strtotime`, `timelib_parse_from_format`, `timelib_parse_zone`); ~96 named sub-pattern definitions and **51** scanner rule/action blocks (`yesterday`, `now`, `noon`, `midnight\|today`, `tomorrow`, `timestamp`, `timestampms`, `firstdayof\|lastdayof`, `backof\|frontof`, `weekdayof`, `timetiny12\|timeshort12\|timelong12`, `mssqltime`, `timetiny24\|...\|iso8601long`, `gnunocolon`, `gnunocolontz`, `iso8601nocolon`, `americanshort\|american`, `iso8601date4\|iso8601dateslash\|dateslash`, `iso8601date2`, `iso8601datex`, `gnudateshorter`, `gnudateshort`, `datefull`, `pointeddate4`, `pointeddate2`, `datenoday`, `datenodayrev`, `datetextual\|datenoyear`, `datenoyearrev`, `datenocolon`, `xmlrpc\|...\|exif`, `pgydotd`, `isoweekday`, `isoweek`, `pgtextshort`, `pgtextreverse`, `clf`, `year4`, `ago`, `daytext`, `relativetextweek`, `relativetext`, `monthfull\|monthabbr`, `tzcorrection\|tz`, `dateshortwithtime*` (2 blocks), `relative`, `[.,]`, `space`, NUL/newline, `any`) |
| `parse_date.c` | 25641 (530 KB) | re2c-generated C (committed) |
| `parse_iso_intervals.re/.c` | 453 / 1095 | ISO 8601 intervals (DatePeriod; not used by Twig) |
| `parse_tz.c` | 973 | tzfile reader, zone lookup, `timelib_get_time_zone_info` |
| `parse_posix.c` | 567 | POSIX TZ strings (transitions after the table) |
| `tm2unixtime.c` | 486 | normalisation & relative arithmetic (`timelib_update_ts`, do_adjust_relative, "last day of", weekday rules) |
| `unixtime2tm.c` | 247 | epoch → broken-down local time |
| `timelib.c` | 391 | helpers, `timelib_fill_holes`, ctor/dtor |
| `interval.c` | 389 | `timelib_diff` (DateTime::diff) |
| `dow.c` | 216 | day of week / ISO week |
| `astro.c` | 313 | sunrise/sunset (unused) |
| `timelib.h` / `timelib_private.h` | 1104 / 188 | API (`TIMELIB_VERSION 202217`, `"2022.17"`) |
| `timezonemap.h` / `fallbackmap.h` | 1127 / 42 | abbreviation → offset tables |
| `timezonedb.h` | 70723 (6.48 MB) | bundled tz database `"2026.6"`, 598 entries (`timezone_identifiers_list()` 419; `ALL_WITH_BC` 598) [V] |

Licence: MIT (`ext/date/lib/LICENSE.rst`, Derick Rethans / MongoDB). Docker uses the bundled db (version matches `timezone_version_get()` = `2026.6`) [V].

Port options:
1. **Bind timelib** (C, MIT, standalone; compile the php-src copy with `cc`): exact parsing, normalisation, transitions, diff; ~XL to port by hand, S/M to bind. Formatting (`date_format`, `date_interval_format`) is easy to port in Rust on top of timelib's broken-down time (S).
2. Pure Rust: the parser could be regenerated with **re2c's Rust backend** (re2c 3.x ships `re2rust`; UNVERIFIED that `parse_date.re`'s C-specific scanner configuration translates mechanically) from `parse_date.re`, rewriting the C actions (~1.5k lines, UNVERIFIED estimate) by hand; plus ports of `tm2unixtime.c`, `unixtime2tm.c`, `parse_tz.c`, `interval.c`, `dow.c` and the timezonedb (or tzdb via `jiff`/`chrono-tz` — version and transition data must match 2026.6, UNVERIFIED). Estimated XL.

---

## 14. var_dump (dump function)

Twig `dump()` (`Extension/DebugExtension.php:39-61`): only when `debug`; no args → dumps the context minus `Template`/`TemplateWrapper` values as one array; else `var_dump(...$vars)`; output captured with `ob_start()/ob_get_clean()`. HTML-safety: output is marked safe when Xdebug overloads var_dump in develop mode with html_errors, **or when `PHP_SAPI === 'cli'`** (`:24-29`) — i.e. under the CLI oracle `dump` output is not escaped.

Format (`ext/standard/var.c:118` `php_var_dump`) [S][V]:

```
NULL | bool(true) | bool(false) | int(N) | float(%.*H with serialize_precision -1: 0.30000000000000004, 1, -0, 1.0E+100, NAN, -INF)
string(LEN) "raw bytes"
array(COUNT) {\n  [0]=>\n  int(1)\n  ["key"]=>\n  ...}\n          (indent: level+1 spaces before [key], value at level+2)
object(Class)#HANDLE (COUNT) {\n  ["pub"]=>\n  ...  ["prot":protected]=>\n  ["priv":"Class":private]=>\n  ["typed"]=>\n  uninitialized(int)\n}\n
enum(Suit::Hearts)
*RECURSION*
resource(ID) of type (stream)
references with refcount > 1 are prefixed "&"; lazy objects prefixed "lazy ghost "/"lazy proxy "
```

COUNT excludes uninitialised typed properties although they are printed; `#HANDLE` is PHP's object store handle (not reproducible in Rust unless handles are emulated); closures dump `name/file/line`; internal classes may dump synthetic properties (`ArrayObject` `storage`, `DateTimeZone` `timezone_type/timezone`). Port: M (pure Rust over the twig-rs value model).

---

## 15. Introspection, constants, enums

| builtin | php-src | Twig use & semantics |
|---|---|---|
| `get_debug_type` | `ext/standard/type.c:39` | in ~25 error messages: `null`, `bool`, `int`, `float`, `string`, `array`, class name (anonymous: `class@anonymous` / `Parent@anonymous`), `Closure`, `resource (stream)`, `resource (closed)` [V] |
| `gettype` | `type.c:~20` | **not used** (`NULL`, `boolean`, `integer`, `double`, ... for reference) |
| `is_iterable` | `type.c:444` | arrays and Traversable (strings are not iterable) — the `iterable` test and many guards |
| `is_countable` | `type.c:457` | `cycle()` |
| `is_scalar` | `type.c:389` | int/float/string/bool (not null) — `length` |
| `is_numeric` | `type.c:367` | `CoreExtension::compare` after trimming `" \t\n\r\v\f"` (see 4.5) |
| `is_nan` | `math.c:588` | `compare` float/string |
| `method_exists` | `Zend/zend_builtin_functions.c:972` | case-insensitive (`'COUNT'` on ArrayObject → true) [V]; compile/infrastructure only |
| `is_callable` | `ext/standard/type.c:414` | function names case-insensitive (`'STRTOUPPER'` → true) [V]; only in `Node::__toString()` debugging output (`Node/Node.php:83`), not at runtime |
| `property_exists` | `:1067` | **case-sensitive**, true for declared properties of any visibility (incl. uninitialised typed) and dynamic properties [V]; used by `getAttribute` for properties not declared on the class (`CoreExtension.php:2284-2288`) |
| `get_class_methods` | `:948` | methods visible from the calling scope (CoreExtension → public), declared case; `getAttribute` lower-cases and indexes them (`get`/`is`/`has` prefixes) |
| `class_parents`, `class_implements` | `ext/spl/php_spl.c:56/90` | safe-class lookup for autoescaping Stringable objects |
| `defined` | `zend_builtin_functions.c:608` | `constant()` function/`is defined` and `getAttribute` class constants; `'Class::CONST'` supported (undefined class → false, no error); global names case-sensitive (`defined('php_eol')` → false) [V] |
| `constant` | `ext/standard/basic_functions.c:515` | only called after `defined()`; enum cases returned as objects; PHP 8.5 deprecated constants emit deprecations (`Constant DATE_RFC7231 is deprecated since 8.5, as this format ignores the associated timezone and always uses GMT`) [V] |
| `enum_exists`, `UnitEnum::cases()`, `BackedEnum::from/tryFrom` | `zend_builtin_functions.c:1194`, `Zend/zend_enum.c` | `enum_cases()`/`enum()` functions (literal names checked at compile time: SyntaxError `The first argument of the "enum_cases" function must be the name of an enum, "Obj" given`); runtime errors `Enum "%s" does not exist.`, `"%s" is not an enum.`, `"%s" is an empty enum.` |
| `get_class` | `zend_builtin_functions.c:625` | emitted in `constant` test with object argument |

Twig `constant()` messages (`CoreExtension.php:1681-1704`) [V]: `Constant "NOPE" is undefined.`; `You cannot use the Twig function "constant" to access "Suit::class". You could provide an object and call constant("class", $object) or use the class name directly as a string.`; `constant('class', obj)` → class name. For twig-rs, PHP's predefined constants must be provided as a table (section 18).

---

## 16. SPL classes and interfaces used

| class/interface | where | semantics needed |
|---|---|---|
| `Traversable`, `Iterator`, `IteratorAggregate` | everywhere (`instanceof`), emitted destructuring | iteration protocol (`getIterator` unwrapping loops) |
| `ArrayAccess` | `getAttribute`, emitted fast path | `offsetExists`/`offsetGet` |
| `Countable` | `length`, `empty`, for-loop `loop.length` | `count()` |
| `Stringable` | `length`, `empty`, escaper, getAttribute key normalisation | `__toString` |
| `JsonSerializable` | `Markup` | json |
| `Closure` | arrows, `invoke`, `checkArrow` | callable values |
| `UnitEnum`/`BackedEnum` | enum functions | cases/from/tryFrom/name/value |
| `LimitIterator`, `IteratorIterator`, `CallbackFilterIterator` | `slice`, `filter` (`CoreExtension.php:770,2077`) | see 10.4 |
| `ArrayIterator`, `ArrayObject`, `CachingIterator`, `RecursiveArrayIterator`, `RecursiveCachingIterator`, `SplDoublyLinkedList`, `SplFixedArray`, `SplObjectStorage`, `SplQueue`, `SplStack`, `WeakMap` | `CoreExtension::ARRAY_LIKE_CLASSES` (`:122-134`): sandbox exempts these from property checks; `ArrayObject::getArrayCopy()` in sequence/mapping tests | in twig-rs: "array-like host objects" |
| `SimpleXMLElement` | `length` | not portable (ignore) |
| `DateTimeInterface`, `DateTime`, `DateTimeImmutable`, `DateTimeZone`, `DateInterval` | date functions | section 13 |
| `Generator` | yield-based templates | internal |
| exceptions: `LogicException`, `RuntimeException`, `InvalidArgumentException`, `BadMethodCallException` (caught in getAttribute for `__call`), `OutOfBoundsException` (LimitIterator), `TypeError`, `Throwable` | | |
| `Reflection*`, `RecursiveDirectoryIterator`, `RecursiveIteratorIterator`, `RegexIterator`, `SplObjectStorage`/`WeakMap` caches | compile/infrastructure | not ported |

---

## 17. Output buffering, flush, error handlers, misc

- `ob_start`/`ob_get_clean`/`ob_get_level`/`ob_end_clean` (`Template.php`, `CoreExtension::captureOutput`, `DebugExtension::dump`): mechanism only; twig-rs uses string buffers. Non-debug `renderBlock` installs a discarding callback `ob_start(static fn () => '')` while displaying, with level cleanup on exceptions.
- `flush()` (`ext/standard/basic_functions.c:1127`) emitted by `{% flush %}` → flush the output sink (no-op for string rendering).
- `set_error_handler`/`restore_error_handler`: `matches` converts warnings into `RuntimeError` (`CoreExtension.php:1184-1195`); twig-rs needs the warning text of the regex front-end (8.2).
- `debug_backtrace` (`Error/Error.php:168`): template line guessing from PHP backtraces — not portable; twig-rs must track template lines directly.
- `func_num_args`/`func_get_arg(s)`: BC shims (deprecation triggers), not ported.
- `trigger_deprecation()` is symfony/deprecation-contracts (`@trigger_error(..., E_USER_DEPRECATED)`), not a PHP builtin.

---

## 18. PHP constants templates commonly reference

Templates reach PHP constants through `constant('NAME')`, `constant('Class::NAME')`, `is constant(...)` and `json_encode(constant('JSON_...'))`. Values from PHP 8.5.13 (64-bit Linux) [V] (full dump of Core/date/json/standard/mbstring/pcre/ctype/iconv/random/spl constants: `scratchpad/scripts/consts.tsv`, 570 entries):

- **DATE_*** (= `DateTimeInterface::*`): `ATOM 'Y-m-d\TH:i:sP'`, `COOKIE 'l, d-M-Y H:i:s T'`, `ISO8601 'Y-m-d\TH:i:sO'`, `ISO8601_EXPANDED 'X-m-d\TH:i:sP'`, `RFC822 'D, d M y H:i:s O'`, `RFC850 'l, d-M-y H:i:s T'`, `RFC1036 'D, d M y H:i:s O'`, `RFC1123 'D, d M Y H:i:s O'`, `RFC7231 'D, d M Y H:i:s \G\M\T'` (deprecated 8.5), `RFC2822 'D, d M Y H:i:s O'`, `RFC3339 'Y-m-d\TH:i:sP'`, `RFC3339_EXTENDED 'Y-m-d\TH:i:s.vP'`, `RSS 'D, d M Y H:i:s O'`, `W3C 'Y-m-d\TH:i:sP'`.
- **JSON_***: `HEX_TAG 1`, `HEX_AMP 2`, `HEX_APOS 4`, `HEX_QUOT 8`, `FORCE_OBJECT 16`, `NUMERIC_CHECK 32`, `UNESCAPED_SLASHES 64`, `PRETTY_PRINT 128`, `UNESCAPED_UNICODE 256`, `PARTIAL_OUTPUT_ON_ERROR 512`, `PRESERVE_ZERO_FRACTION 1024`, `UNESCAPED_LINE_TERMINATORS 2048`, `OBJECT_AS_ARRAY 1`, `BIGINT_AS_STRING 2`, `INVALID_UTF8_IGNORE 1048576`, `INVALID_UTF8_SUBSTITUTE 2097152`, `THROW_ON_ERROR 4194304`, `ERROR_NONE 0` … `ERROR_NON_BACKED_ENUM 11`.
- **Numbers**: `PHP_INT_MAX 9223372036854775807`, `PHP_INT_MIN -9223372036854775808`, `PHP_INT_SIZE 8`, `PHP_FLOAT_EPSILON 2.220446049250313E-16`, `PHP_FLOAT_MAX 1.7976931348623157E+308`, `PHP_FLOAT_MIN 2.2250738585072014E-308`, `PHP_FLOAT_DIG 15`, `INF`, `NAN`, `M_PI 3.141592653589793` (prints `3.1415926535898` with precision 14 [V]), `M_E`, `M_LOG2E`, `M_LOG10E`, `M_LN2`, `M_LN10`, `M_PI_2`, `M_PI_4`, `M_1_PI`, `M_2_PI`, `M_SQRTPI`, `M_2_SQRTPI`, `M_LNPI`, `M_EULER`, `M_SQRT2`, `M_SQRT1_2`, `M_SQRT3`.
- **Environment**: `PHP_EOL "\n"`, `PHP_VERSION '8.5.13'`, `PHP_MAJOR_VERSION 8`, `PHP_MINOR_VERSION 5`, `PHP_RELEASE_VERSION 13`, `PHP_VERSION_ID 80513`, `PHP_OS 'Linux'`, `PHP_OS_FAMILY 'Linux'`, `DIRECTORY_SEPARATOR '/'`, `PATH_SEPARATOR ':'`, `PHP_SAPI 'cli'`.
- **Errors**: `E_ERROR 1`, `E_WARNING 2`, `E_PARSE 4`, `E_NOTICE 8`, `E_CORE_ERROR 16`, `E_CORE_WARNING 32`, `E_COMPILE_ERROR 64`, `E_COMPILE_WARNING 128`, `E_USER_ERROR 256`, `E_USER_WARNING 512`, `E_USER_NOTICE 1024`, `E_STRICT 2048`, `E_RECOVERABLE_ERROR 4096`, `E_DEPRECATED 8192`, `E_USER_DEPRECATED 16384`, `E_ALL 30719`.
- **standard**: `ENT_COMPAT 2`, `ENT_QUOTES 3`, `ENT_NOQUOTES 0`, `ENT_IGNORE 4`, `ENT_SUBSTITUTE 8`, `ENT_DISALLOWED 128`, `ENT_HTML401 0`, `ENT_XML1 16`, `ENT_XHTML 32`, `ENT_HTML5 48`; `SORT_REGULAR 0`, `SORT_NUMERIC 1`, `SORT_STRING 2`, `SORT_DESC 3`, `SORT_ASC 4`, `SORT_LOCALE_STRING 5`, `SORT_NATURAL 6`, `SORT_FLAG_CASE 8`; `COUNT_NORMAL 0`, `COUNT_RECURSIVE 1`; `ARRAY_FILTER_USE_BOTH 1`, `ARRAY_FILTER_USE_KEY 2`; `PHP_ROUND_HALF_UP 1`, `HALF_DOWN 2`, `HALF_EVEN 3`, `HALF_ODD 4`; `PHP_QUERY_RFC1738 1`, `PHP_QUERY_RFC3986 2`; `STR_PAD_LEFT 0`, `STR_PAD_RIGHT 1`, `STR_PAD_BOTH 2`; `MB_CASE_UPPER 0`, `MB_CASE_LOWER 1`, `MB_CASE_TITLE 2`, `MB_CASE_FOLD 3`, `MB_CASE_UPPER_SIMPLE 4`, `MB_CASE_LOWER_SIMPLE 5`, `MB_CASE_TITLE_SIMPLE 6`, `MB_CASE_FOLD_SIMPLE 7`; `PREG_PATTERN_ORDER 1`, `PREG_SET_ORDER 2`, `PREG_OFFSET_CAPTURE 256`, `PREG_UNMATCHED_AS_NULL 512`, `PCRE_VERSION '10.44 2024-06-07'`; `MT_RAND_MT19937 0`.

---

## 19. Reference: zend_sort port (validated)

PHP userland transliteration of `Zend/zend_sort.c` (`zend_sort_2..5` :24-82, `zend_insert_sort` :84, `zend_sort` :248). Validated in PHP 8.5.13 against native `usort()` with the inconsistent comparator `(($x*31 + $y*17 + ($x ^ $y)) % 5) - 2` on 126 arrays (n ∈ {1..8, 15, 16, 17, 18, 31, 33, 64, 100, 257, 1023, 1024, 1025, 2000}, 6 seeds each): 0 mismatches; also identical to native `asort()` on `['10', 9, '9a', 'abc', 1.5, true, null, '1e1', ' 9', 'ABC', 0, '', [], -1]` (keys `6,11,13,10,4,1,8,0,2,3,5,7,9,12`) [V] (`scratchpad/scripts/t/zsort.php`). `cmp` must already include the stable fallback (`r = sign(user(a,b)); return r != 0 ? r : sign(pos(a) - pos(b))`). Indices are element indices (C uses byte pointers).

```php
function cmpw(array &$a, int $i, int $j, callable $cmp): int { return $cmp($a[$i], $a[$j]); }
function swp(array &$a, int $i, int $j): void { $t = $a[$i]; $a[$i] = $a[$j]; $a[$j] = $t; }
function sort2(&$a, $x, $y, $c) { if (cmpw($a, $x, $y, $c) > 0) swp($a, $x, $y); }
function sort3(&$a, $x, $y, $z, $c) {
    if (!(cmpw($a, $x, $y, $c) > 0)) {
        if (!(cmpw($a, $y, $z, $c) > 0)) return;
        swp($a, $y, $z);
        if (cmpw($a, $x, $y, $c) > 0) swp($a, $x, $y);
        return;
    }
    if (!(cmpw($a, $z, $y, $c) > 0)) { swp($a, $x, $z); return; }
    swp($a, $x, $y);
    if (cmpw($a, $y, $z, $c) > 0) swp($a, $y, $z);
}
function sort4(&$a, $x, $y, $z, $w, $c) {
    sort3($a, $x, $y, $z, $c);
    if (cmpw($a, $z, $w, $c) > 0) { swp($a, $z, $w);
        if (cmpw($a, $y, $z, $c) > 0) { swp($a, $y, $z);
            if (cmpw($a, $x, $y, $c) > 0) swp($a, $x, $y); } }
}
function sort5(&$a, $x, $y, $z, $w, $v, $c) {
    sort4($a, $x, $y, $z, $w, $c);
    if (cmpw($a, $w, $v, $c) > 0) { swp($a, $w, $v);
        if (cmpw($a, $z, $w, $c) > 0) { swp($a, $z, $w);
            if (cmpw($a, $y, $z, $c) > 0) { swp($a, $y, $z);
                if (cmpw($a, $x, $y, $c) > 0) swp($a, $x, $y); } } }
}
function insert_sort(&$a, int $start, int $n, $c) {
    switch ($n) {
        case 0: case 1: return;
        case 2: sort2($a, $start, $start + 1, $c); return;
        case 3: sort3($a, $start, $start + 1, $start + 2, $c); return;
        case 4: sort4($a, $start, $start + 1, $start + 2, $start + 3, $c); return;
        case 5: sort5($a, $start, $start + 1, $start + 2, $start + 3, $start + 4, $c); return;
    }
    $end = $start + $n; $sentry = $start + 6;
    for ($i = $start + 1; $i < $sentry; $i++) {          // plain insertion for the first 6
        $j = $i - 1;
        if (!(cmpw($a, $j, $i, $c) > 0)) continue;
        while ($j != $start) { $j--; if (!(cmpw($a, $j, $i, $c) > 0)) { $j++; break; } }
        for ($k = $i; $k > $j; $k--) swp($a, $k, $k - 1);
    }
    for ($i = $sentry; $i < $end; $i++) {                 // step-2 backwards search
        $j = $i - 1;
        if (!(cmpw($a, $j, $i, $c) > 0)) continue;
        while (true) {
            $j -= 2;
            if (!(cmpw($a, $j, $i, $c) > 0)) { $j++; if (!(cmpw($a, $j, $i, $c) > 0)) $j++; break; }
            if ($j == $start) break;
            if ($j == $start + 1) { $j--; if (cmpw($a, $i, $j, $c) > 0) $j++; break; }
        }
        for ($k = $i; $k > $j; $k--) swp($a, $k, $k - 1);
    }
}
function zsort(&$a, int $base, int $n, $c) {
    while (true) {
        if ($n <= 16) { insert_sort($a, $base, $n, $c); return; }
        $start = $base; $end = $start + $n; $offset = $n >> 1; $pivot = $start + $offset;
        if ($n >> 10) { $delta = $offset >> 1; sort5($a, $start, $start + $delta, $pivot, $pivot + $delta, $end - 1, $c); }
        else { sort3($a, $start, $pivot, $end - 1, $c); }
        swp($a, $start + 1, $pivot);
        $pivot = $start + 1; $i = $pivot + 1; $j = $end - 1;
        while (true) {
            while (cmpw($a, $pivot, $i, $c) > 0) { $i++; if ($i == $j) goto done; }
            $j--; if ($j == $i) goto done;
            while (cmpw($a, $j, $pivot, $c) > 0) { $j--; if ($j == $i) goto done; }
            swp($a, $i, $j);
            $i++; if ($i == $j) goto done;
        }
        done:
        swp($a, $pivot, $i - 1);
        if (($i - 1) - $start < $end - $i) { zsort($a, $start, ($i - $start) - 1, $c); $base = $i; $n = $end - $i; }
        else { zsort($a, $i, $end - $i, $c); $n = ($i - $start) - 1; }
    }
}
```

---

## 20. Reference: complete call-site table (generated)

Generated by `scratchpad/scripts/gen_callsites.py` from the tokenizer scan (`scratchpad/scripts/scan_raw.tsv`). Columns: **R\*** string callables invoked at runtime; **R** runtime helpers; **E** emitted into compiled templates (string literals in `Node/**` compile methods); **C** compile time; **I** infrastructure. Paths relative to `twig-3.31.1/src`. Emitted-code hits inside error-message strings (e.g. `constant (` in a message) were excluded. Not shown because they are language constructs or non-builtins: `isset`, `unset`, `empty`, `trigger_deprecation` (symfony), `(int)`/`(string)` casts, operators.

| builtin | R* (string callable) | R (runtime) | E (emitted code) | C (compile time) | I (infrastructure) |
|---|---|---|---|---|---|
| `abs` | Extension/CoreExtension.php:248 |  |  |  |  |
| `addcslashes` |  |  |  | Compiler.php:152 |  |
| `array_chunk` |  | Extension/CoreExtension.php:1723 |  | Node/Expression/ArrayExpression.php:41,104 |  |
| `array_column` |  | Extension/CoreExtension.php:2056 |  |  |  |
| `array_combine` |  |  |  | Lexer.php:622 |  |
| `array_diff_key` |  | TwigMacro.php:104 |  |  |  |
| `array_filter` |  | Extension/CoreExtension.php:2073 |  | ExpressionParser/Infix/AssignmentExpressionParser.php:56; Node/CheckSecurityNode.php:92,93,94,95 |  |
| `array_intersect` |  |  |  | NodeVisitor/SafeAnalysisNodeVisitor.php:149 |  |
| `array_intersect_key` |  |  | Node/ForNode.php:119 |  |  |
| `array_is_list` |  | Extension/CoreExtension.php:1501,1524; TwigMacro.php:94 |  |  |  |
| `array_key_exists` |  | Environment.php:928; Extension/CoreExtension.php:1782; Runtime/EscaperRuntime.php:344; TwigMacro.php:161,179 | Node/Expression/NameExpression.php:49,57,81 | Extension/EscaperExtension.php:124; Node/Expression/CallExpression.php:197,198,213; Node/Node.php:167,172; Util/CallableArgumentsExtractor.php:94,95,110 |  |
| `array_key_first` |  |  |  | Extension/EscaperExtension.php:130 |  |
| `array_key_last` |  |  |  | NodeVisitor/CorrectnessNodeVisitor.php:228; Parser.php:352 |  |
| `array_keys` |  | BlockChain.php:90; Error/SyntaxError.php:44; Extension/CoreExtension.php:959,1814; Runtime/EscaperRuntime.php:348; Template.php:319 |  | ExtensionSet.php:119; Node/CheckSecurityNode.php:100,101,102,103; Node/Expression/CallExpression.php:260; Parser.php:254,527,556,604; Util/CallableArgumentsExtractor.php:164 | Loader/FilesystemLoader.php:71; Test/IntegrationTestCase.php:311,373 |
| `array_map` |  | Extension/CoreExtension.php:1915; Sandbox/SecurityPolicy.php:56 |  | ExpressionParser/Prefix/GroupingExpressionParser.php:62; Lexer.php:622; Util/CallableArgumentsExtractor.php:102,164 |  |
| `array_merge` |  | Environment.php:946,952; Extension/CoreExtension.php:743,1554; Runtime/EscaperRuntime.php:89,132,348; Template.php:319,322,474,496,515 | Node/ModuleNode.php:301,322,395 | AbstractTwigCallable.php:31; ExtensionSet.php:404; Lexer.php:81,621; Node/Expression/CallExpression.php:209,214,243; TokenStream.php:44; TwigFilter.php:33; TwigFunction.php:33; TwigTest.php:32; Util/CallableArgumentsExtractor.php:106,111,147 | Environment.php:122; Test/IntegrationTestCase.php:251 |
| `array_pad` |  |  | Node/Expression/Binary/SequenceDestructuringSetBinary.php:68 |  |  |
| `array_pop` |  | Error/Error.php:189 |  | Compiler.php:82,107; Lexer.php:259,540,557,660,674; Node/Expression/CallExpression.php:294,296; NodeVisitor/CorrectnessNodeVisitor.php:161; NodeVisitor/EscaperNodeVisitor.php:93; Parser.php:158,269,312; Util/CallableArgumentsExtractor.php:193,195 | Loader/FilesystemLoader.php:263 |
| `array_push` |  |  |  | Node/Expression/ArrayExpression.php:95 |  |
| `array_rand` |  | Extension/CoreExtension.php:510 |  |  |  |
| `array_reverse` |  | Extension/CoreExtension.php:985,989,998 |  | Node/ModuleNode.php:515 | Cache/ChainCache.php:86 |
| `array_shift` |  | Error/Error.php:192 |  | ExpressionParser/Prefix/LiteralExpressionParser.php:138; ExtensionSet.php:241,297,452; NodeVisitor/OptimizerNodeVisitor.php:140,202,208,209,210; Parser.php:425 | Extension/ProfilerExtension.php:41; Util/ReflectionCallable.php:98,101,104,107,110,113 |
| `array_slice` |  | Extension/CoreExtension.php:780,858 |  | NodeVisitor/SandboxNodeVisitor.php:159,175; TokenStream.php:44 |  |
| `array_unique` |  | Runtime/EscaperRuntime.php:132; Template.php:325 |  |  |  |
| `array_unshift` |  | Error/Error.php:191 |  | NodeVisitor/OptimizerNodeVisitor.php:131,132,133,140,203,204; Parser.php:420 | Extension/ProfilerExtension.php:32; Loader/FilesystemLoader.php:131 |
| `array_values` |  | Extension/CoreExtension.php:1416 |  |  | Extension/AttributeExtension.php:197,198,199; Util/ReflectionCallable.php:116 |
| `arsort` |  |  |  | Lexer.php:623 |  |
| `asort` |  | Error/SyntaxError.php:42; Extension/CoreExtension.php:1062 |  |  |  |
| `basename` |  |  |  |  | Cache/FilesystemCache.php:67 |
| `bin2hex` |  |  |  |  | Environment.php:265 |
| `call_user_func` |  |  |  | Extension/EscaperExtension.php:136 |  |
| `ceil` | Extension/CoreExtension.php:668 | Extension/CoreExtension.php:1721 |  |  |  |
| `chmod` |  |  |  |  | Cache/FilesystemCache.php:69 |
| `chr` |  |  |  | Lexer.php:466,472; Node/TextNode.php:48 |  |
| `class_exists` |  |  |  | ExtensionSet.php:32 | Cache/ChainCache.php:53; Environment.php:423,430,438,447; Test/IntegrationTestCase.php:364; Util/CallableParameters.php:127 |
| `class_implements` |  | Runtime/EscaperRuntime.php:130 |  |  |  |
| `class_parents` |  | Runtime/EscaperRuntime.php:130 |  |  |  |
| `clearstatcache` |  |  |  |  | Cache/FilesystemCache.php:58 |
| `constant` |  | Extension/CoreExtension.php:1703,1897 | Node/Expression/Test/ConstantTest.php:33 |  |  |
| `count` |  | Environment.php:522; Extension/CoreExtension.php:438,506,846,854,858,1300,1426,1467,1726,1727; TwigMacro.php:95,96,104,143,146,191 | Node/ForNode.php:76 | AbstractTwigCallable.php:197; Extension/EscaperExtension.php:128; ExtensionSet.php:554,555,576; Lexer.php:269,280,656; Node/Expression/Binary/SequenceDestructuringSetBinary.php:69; Node/Expression/CallExpression.php:202,205,260; Node/Expression/Filter/DefaultFilter.php:58; Node/Expression/FunctionNode/EnumCasesFunction.php:32; Node/Expression/FunctionNode/EnumFunction.php:32; Node/IfNode.php:29,43; Node/MacrosNode.php:52; Node/ModuleNode.php:242,449,457,462; Node/Node.php:97,287; Node/SetNode.php:37,58,76; NodeVisitor/EscaperNodeVisitor.php:168,169; NodeVisitor/SafeAnalysisNodeVisitor.php:83,84,85; NodeVisitor/SandboxNodeVisitor.php:186; Parser.php:237,263,288,307,380,410,415,476; TokenParser/ForTokenParser.php:53; TokenParser/MacroTokenParser.php:85; TokenParser/SetTokenParser.php:46,52; Util/CallableArgumentsExtractor.php:58,59,99,102,164 | Extension/ProfilerExtension.php:43; Profiler/Dumper/BaseDumper.php:61 |
| `ctype_alpha` |  |  |  | Lexer.php:630,635 | Loader/FilesystemLoader.php:319 |
| `ctype_digit` |  | Extension/CoreExtension.php:619 |  | Lexer.php:467,469; Node/Expression/ArrayExpression.php:32; Node/Expression/TempNameExpression.php:39 |  |
| `ctype_space` |  |  |  | Node/TextNode.php:44,50; TokenParser/SandboxTokenParser.php:48 |  |
| `ctype_xdigit` |  |  |  | Lexer.php:461,463 |  |
| `current` |  | Extension/CoreExtension.php:799,815 |  |  |  |
| `date_default_timezone_get` |  | Extension/CoreExtension.php:187 |  |  |  |
| `debug_backtrace` |  | Error/Error.php:168; Resources/core.php:553 |  |  |  |
| `defined` |  | Extension/CoreExtension.php:1691,1892 |  |  |  |
| `dirname` |  |  |  |  | Cache/FilesystemCache.php:55 |
| `end` |  |  |  | Lexer.php:555; Node/Expression/CallExpression.php:291; Util/CallableArgumentsExtractor.php:190 | Loader/FilesystemLoader.php:262 |
| `enum_exists` |  | Extension/CoreExtension.php:1637,1657 |  | Node/Expression/FunctionNode/EnumCasesFunction.php:44; Node/Expression/FunctionNode/EnumFunction.php:44 |  |
| `explode` |  | Extension/CoreExtension.php:887 |  | Node/Node.php:102 | Cache/ChainCache.php:86; Loader/FilesystemLoader.php:261,301; Test/IntegrationTestCase.php:363 |
| `extension_loaded` |  | Extension/DebugExtension.php:24 |  |  |  |
| `file_exists` |  |  |  |  | Cache/FilesystemCache.php:89 |
| `file_get_contents` |  |  |  |  | Loader/FilesystemLoader.php:141; Test/IntegrationTestCase.php:189; Util/TemplateDirIterator.php:25 |
| `file_put_contents` |  |  |  |  | Cache/FilesystemCache.php:68 |
| `filemtime` |  |  |  | ExtensionSet.php:181 | Cache/FilesystemCache.php:100; Extension/AbstractExtension.php:58,62; Extension/AttributeExtension.php:109,110; Extension/EscaperExtension.php:69; Extension/SandboxBridgeExtension.php:36; Loader/FilesystemLoader.php:178 |
| `filter_var` |  |  |  |  | Cache/FilesystemCache.php:36,73 |
| `floor` | Extension/CoreExtension.php:668 |  | Node/Expression/Binary/FloorDivBinary.php:21 |  | Profiler/Dumper/BlackfireDumper.php:60,66 |
| `flush` |  |  | Node/FlushNode.php:38 |  |  |
| `func_get_arg` |  |  |  | ExpressionParser.php:222,226; Node/Node.php:65,176,191,231,249 | Extension/EscaperExtension.php:78; Loader/FilesystemLoader.php:99,119 |
| `func_get_args` |  | TemplateWrapper.php:60 |  |  |  |
| `func_num_args` |  | Extension/CoreExtension.php:1543; Extension/SandboxExtension.php:109; Sandbox/SecurityPolicy.php:100 |  | ExpressionParser.php:61,221,224; Node/CheckSecurityNode.php:36; Node/Expression/CallExpression.php:72; Node/ImportNode.php:31; Node/Node.php:64,176,191,231,249 | Extension/EscaperExtension.php:78; Loader/FilesystemLoader.php:99,119 |
| `function_exists` |  | Extension/CoreExtension.php:1259; Runtime/EscaperRuntime.php:356 |  |  | Cache/FilesystemCache.php:34,73,75 |
| `get_class` |  |  | Node/Expression/Test/ConstantTest.php:38 | Node/Expression/CallExpression.php:53 | Environment.php:271 |
| `get_class_methods` |  | Extension/CoreExtension.php:1910 |  |  |  |
| `get_debug_type` |  | BlockChain.php:58; Extension/CoreExtension.php:643,740,1054,1718,1804,1806,1809,1820,1825,1848,2034,2067,2088,2110,2131,2152,2174 |  | ExtensionSet.php:551; Node/MacrosNode.php:34,44; Node/Node.php:57 | Environment.php:988; Util/ReflectionCallable.php:61 |
| `get_object_vars` |  |  |  | Parser.php:96 |  |
| `getcwd` |  |  |  |  | Loader/FilesystemLoader.php:42 |
| `hash` |  | Environment.php:471 |  | ExtensionSet.php:160 | Cache/FilesystemCache.php:41; Environment.php:339; Profiler/NodeVisitor/ProfilerNodeVisitor.php:36 |
| `hexdec` |  |  |  | Lexer.php:466 |  |
| `htmlspecialchars` |  | Runtime/EscaperRuntime.php:160,184,191,195 |  |  | Profiler/Dumper/HtmlDumper.php:55 |
| `http_build_query` |  | Extension/CoreExtension.php:715 |  |  |  |
| `iconv` |  | Extension/CoreExtension.php:1263; Runtime/EscaperRuntime.php:197,248,269,335,360 |  |  |  |
| `implode` |  | Environment.php:544; Error/SyntaxError.php:44; Extension/CoreExtension.php:851,858,998,1024,1814; Runtime/EscaperRuntime.php:348 | Node/CaptureNode.php:38 | Lexer.php:576,645; Node/Expression/CallExpression.php:205,260; Node/Node.php:94,106; Node/NodeDocumentation.php:31; TokenStream.php:39; Util/CallableArgumentsExtractor.php:102,164 | Environment.php:993; Loader/ChainLoader.php:77,111,130; Loader/FilesystemLoader.php:237,268; Test/IntegrationTestCase.php:380 |
| `in_array` |  | Extension/CoreExtension.php:1103,1769,1882,1930; Markup.php:73; Runtime/EscaperRuntime.php:143; Sandbox/SecurityPolicy.php:105,117,123,137,157,173; TwigMacro.php:63 | Node/Expression/GetAttrExpression.php:89 | Lexer.php:381,666,668; Node/Expression/AssignNameExpression.php:29; Node/Expression/TempNameExpression.php:31,41; Node/MacroNode.php:92; NodeVisitor/EscaperNodeVisitor.php:160; NodeVisitor/OptimizerNodeVisitor.php:157; NodeVisitor/SafeAnalysisNodeVisitor.php:47,52,141,145; Parser.php:657; Token.php:95,97,108,122; TokenParser/GuardTokenParser.php:29 | FileExtensionEscapingStrategy.php:36 |
| `ini_get` |  | Extension/DebugExtension.php:26,27 |  |  | Cache/FilesystemCache.php:35,36,73 |
| `ini_set` |  |  |  | Compiler.php:166,171 |  |
| `is_a` |  |  |  |  | Environment.php:681; Util/CallableParameters.php:135 |
| `is_array` |  | Environment.php:518; Extension/CoreExtension.php:423,714,779,955,988,1053,1093,1299,1412,1501,1524,1782,1803,1810,1845,2072; Sandbox/SecurityChecker.php:169,228; Sandbox/SecurityPolicy.php:56,173; Template.php:334 | Node/Expression/GetAttrExpression.php:66,198; Node/ForNode.php:74 | Compiler.php:177; ExtensionSet.php:550; Node/Expression/CallExpression.php:46,52; Parser.php:249; Token.php:122 | Loader/ChainLoader.php:54; Loader/FilesystemLoader.php:79; Util/ReflectionCallable.php:36,66,67,72 |
| `is_bool` |  | Extension/CoreExtension.php:1763 |  | AbstractTwigCallable.php:182; Compiler.php:175 |  |
| `is_callable` |  |  |  | Node/Node.php:83 |  |
| `is_countable` |  | Extension/CoreExtension.php:428 |  |  |  |
| `is_dir` |  |  |  |  | Cache/FilesystemCache.php:56,59; Loader/FilesystemLoader.php:101,121 |
| `is_file` |  |  |  | ExtensionSet.php:180 | Cache/FilesystemCache.php:48,96; Extension/AbstractExtension.php:54,61; Extension/AttributeExtension.php:110; Loader/FilesystemLoader.php:228 |
| `is_float` |  | Extension/CoreExtension.php:466,1081,1150,1161,1763 |  | Compiler.php:164,166 |  |
| `is_int` |  | Extension/CoreExtension.php:466,1081,1126,1137; TwigMacro.php:115 |  | Compiler.php:164; ExpressionParser/Infix/ArgumentsTrait.php:31; Node/Expression/CallExpression.php:154,234; Node/Expression/TempNameExpression.php:39; NodeVisitor/SandboxNodeVisitor.php:172,191; Token.php:78; Util/CallableArgumentsExtractor.php:46,132 |  |
| `is_iterable` | Extension/CoreExtension.php:326 | Extension/CoreExtension.php:500,642,739,840,1033,1088,1396,1717,2033,2066,2087,2109,2130,2151,2173; Resources/core.php:399 | Node/WithNode.php:50 |  |  |
| `is_nan` |  | Extension/CoreExtension.php:1151,1162 |  |  |  |
| `is_numeric` |  | Extension/CoreExtension.php:1128,1139,1155,1166 |  |  |  |
| `is_object` |  | Extension/CoreExtension.php:1092,1524,1793,1803,1808,1834,2047 | Node/ForNode.php:74 |  |  |
| `is_resource` |  | Extension/CoreExtension.php:1092 |  | ExtensionSet.php:551 |  |
| `is_scalar` |  | Extension/CoreExtension.php:1295 |  |  |  |
| `is_string` |  | BlockChain.php:54; Extension/CoreExtension.php:482,799,815,1017,1080,1081,1126,1137,1150,1161; Runtime/EscaperRuntime.php:124; TwigMacro.php:196 | Node/Expression/Binary/EndsWithBinary.php:24,26; Node/Expression/Binary/StartsWithBinary.php:24,26 | Extension/EscaperExtension.php:135; Node/Expression/Binary/SetBinary.php:32; Node/Expression/CallExpression.php:37,44,46; Node/Expression/FunctionNode/EnumCasesFunction.php:40; Node/Expression/FunctionNode/EnumFunction.php:40; Node/Expression/FunctionNode/RenderSandboxedFunction.php:39,72,78; Node/Expression/MacroReferenceExpression.php:39; Node/Expression/Test/DefinedTest.php:51; Node/PrintNode.php:67; NodeVisitor/EscaperNodeVisitor.php:56; NodeVisitor/OptimizerNodeVisitor.php:107 | Environment.php:300,984; Util/ReflectionCallable.php:32 |
| `is_writable` |  |  |  |  | Cache/FilesystemCache.php:63 |
| `iterator_count` |  | Extension/CoreExtension.php:1304,1471 |  |  |  |
| `iterator_to_array` |  | Extension/CoreExtension.php:770,776,985,1052,1409,1498,1521,2038; Sandbox/SecurityChecker.php:159,210 | Node/CaptureNode.php:38 | Node/ModuleNode.php:45 | Loader/ChainLoader.php:55 |
| `json_encode` | Extension/CoreExtension.php:253 |  |  |  |  |
| `ksort` |  |  |  | Compiler.php:217; NodeTraverser.php:50 |  |
| `levenshtein` |  | Error/SyntaxError.php:32 |  |  |  |
| `ltrim` |  | Extension/CoreExtension.php:1217; Runtime/EscaperRuntime.php:85 |  | ExtensionSet.php:85,90; Lexer.php:605; Node/Expression/CallExpression.php:58; Node/Node.php:106 | Loader/FilesystemLoader.php:300 |
| `max` | Extension/CoreExtension.php:299 |  |  | ExpressionParser/ExpressionParsers.php:125; ExtensionSet.php:177,181 | Extension/AbstractExtension.php:62; Extension/AttributeExtension.php:108; Extension/EscaperExtension.php:67; Extension/SandboxBridgeExtension.php:34 |
| `mb_convert_case` |  | Extension/CoreExtension.php:1360 |  |  |  |
| `mb_ord` |  | Runtime/EscaperRuntime.php:233,265,331 |  |  |  |
| `mb_strlen` |  | Extension/CoreExtension.php:894,1296,1308; Markup.php:87 |  |  |  |
| `mb_strtolower` |  | Extension/CoreExtension.php:1335,1372 |  |  |  |
| `mb_strtoupper` |  | Extension/CoreExtension.php:1323,1372 |  |  |  |
| `mb_substr` |  | Extension/CoreExtension.php:783,901,1372 |  |  |  |
| `memory_get_peak_usage` |  |  |  |  | Profiler/Profile.php:142,154 |
| `memory_get_usage` |  |  |  |  | Profiler/Profile.php:141,153 |
| `method_exists` |  |  |  | ExpressionParser/ExpressionParsers.php:147; ExtensionSet.php:136,141,146,151,545; Node/Expression/CallExpression.php:322; NodeVisitor/SandboxNodeVisitor.php:303 | Util/ReflectionCallable.php:36,72 |
| `microtime` |  |  |  |  | Profiler/Dumper/BlackfireDumper.php:27; Profiler/Profile.php:140,152 |
| `min` | Extension/CoreExtension.php:300 |  |  | ExpressionParser/ExpressionParsers.php:124 |  |
| `mkdir` |  |  |  |  | Cache/FilesystemCache.php:57 |
| `mt_rand` |  | Extension/CoreExtension.php:463,479 |  |  |  |
| `nl2br` |  | Extension/CoreExtension.php:1235 |  |  |  |
| `number_format` |  | Extension/CoreExtension.php:702 |  |  |  |
| `ob_end_clean` |  | Extension/CoreExtension.php:2218; Template.php:217,260,451 |  |  |  |
| `ob_get_clean` |  | Extension/CoreExtension.php:2224; Extension/DebugExtension.php:60; Template.php:223,266,457 |  |  |  |
| `ob_get_level` |  | Extension/CoreExtension.php:2209,2217; Template.php:207,216,250,259,441,450 |  |  |  |
| `ob_start` |  | Extension/CoreExtension.php:2210; Extension/DebugExtension.php:45; Template.php:209,211,252,254,443,445 |  |  |  |
| `octdec` |  |  |  | Lexer.php:472 |  |
| `opcache_invalidate` |  |  |  |  | Cache/FilesystemCache.php:74 |
| `opcache_is_script_cached` |  |  |  |  | Cache/FilesystemCache.php:48 |
| `ord` |  | Runtime/EscaperRuntime.php:265,297 |  |  |  |
| `parse_url` |  |  |  |  | Loader/FilesystemLoader.php:323 |
| `pathinfo` |  |  |  |  | FileExtensionEscapingStrategy.php:44 |
| `preg_last_error_msg` |  | Extension/CoreExtension.php:1189,1276 |  |  |  |
| `preg_match` |  | Extension/CoreExtension.php:1188; Runtime/EscaperRuntime.php:206,258,280 |  | Compiler.php:260; ExpressionParser.php:307; ExpressionParser/Infix/DotExpressionParser.php:55; ExpressionParser/Prefix/LiteralExpressionParser.php:84; ExtensionSet.php:240,296,451; Lexer.php:320,325,349,359,370,379,388,393,404,409,415,486,512,531,536,539,556; Node/Expression/Binary/MatchesBinary.php:37; TokenParser/AbstractTokenParser.php:52 | Loader/FilesystemLoader.php:250; Test/IntegrationTestCase.php:181,191,198 |
| `preg_match_all` |  | Extension/CoreExtension.php:1275 |  | Lexer.php:227 | Test/IntegrationTestCase.php:204,390 |
| `preg_quote` |  |  |  | ExtensionSet.php:510,518,526; Lexer.php:100,108,110,112,120,122,124,130,137,139,141,150,152,154,161,163,165,167,169,177,179,181,185,190,192,194,196,198,200,203,204,629 | Util/DeprecationCollector.php:41 |
| `preg_replace` |  | Extension/CoreExtension.php:1247 |  | Lexer.php:380,640; Node/Expression/CallExpression.php:277; Util/CallableArgumentsExtractor.php:181 |  |
| `preg_replace_callback` |  | Runtime/EscaperRuntime.php:210,262,289 |  |  |  |
| `printf` |  |  |  |  | Test/IntegrationTestCase.php:371 |
| `property_exists` | Extension/CoreExtension.php:2287 |  |  |  |  |
| `random_bytes` |  |  |  |  | Environment.php:265 |
| `range` | Extension/CoreExtension.php:301 |  | Node/Expression/Binary/RangeBinary.php:23 |  |  |
| `rawurlencode` |  | Extension/CoreExtension.php:718; Runtime/EscaperRuntime.php:341 |  |  |  |
| `realpath` |  |  |  |  | Loader/FilesystemLoader.php:43,229; Test/IntegrationTestCase.php:177 |
| `rename` |  |  |  |  | Cache/FilesystemCache.php:68 |
| `restore_error_handler` |  | Extension/CoreExtension.php:1194 |  | Node/Expression/Binary/MatchesBinary.php:39 | Test/IntegrationTestCase.php:326,358; Util/DeprecationCollector.php:73 |
| `round` |  | Extension/CoreExtension.php:665 |  |  |  |
| `rtrim` |  | Extension/CoreExtension.php:1218 |  | Lexer.php:293,297,498,502 | Cache/FilesystemCache.php:29; Loader/FilesystemLoader.php:106,126 |
| `serialize` |  |  |  | ExtensionSet.php:119,160 | Profiler/Profile.php:171 |
| `set_error_handler` |  | Extension/CoreExtension.php:1184 |  | Node/Expression/Binary/MatchesBinary.php:35 | Test/IntegrationTestCase.php:301,334; Util/DeprecationCollector.php:57 |
| `shuffle` |  | Extension/CoreExtension.php:1023,1035 |  |  |  |
| `sort` |  | Extension/CoreExtension.php:1914 |  | ExtensionSet.php:155,156,157,158 |  |
| `spl_object_id` |  | BlockChain.php:203 |  | ExpressionParser/ExpressionParsers.php:95,103 |  |
| `sprintf` |  | BlockChain.php:58,204,226; Environment.php:544; Error/Error.php:149,152,154; Error/SyntaxError.php:44; Extension/CoreExtension.php:562,643,740,1054,1185,1189,1276,1638,1658,1662,1697,1700,1718,1804,1806,1809,1812,1814,1818,1820,1823,1825,1844,1846,1848,1975,2034,2067,2088,2110,2131,2152,2174,2198; MacroNamespace.php:62; Runtime/EscaperRuntime.php:235,244,265,323,331,350; Sandbox/SecurityPolicy.php:111,118,131,141,165,181; Template.php:517,519,542,601,627; TemplateWrapper.php:129; TwigMacro.php:134,137 | Node/DeprecatedNode.php:48; Node/ModuleNode.php:274 | AbstractTwigCallable.php:45,71; Compiler.php:152,203,251; DeprecatedCallableInfo.php:43,46,48,51,57,59; ExpressionParser.php:215,261,365; ExpressionParser/AbstractExpressionParser.php:18; ExpressionParser/ExpressionParserType.php:31; ExpressionParser/Infix/ArgumentsTrait.php:71; ExpressionParser/Infix/AssignmentExpressionParser.php:45,50; ExpressionParser/Infix/DotExpressionParser.php:59; ExpressionParser/Prefix/LiteralExpressionParser.php:93,224; Extension/EscaperExtension.php:208; Extension/StagingExtension.php:38,52,76,90; ExtensionSet.php:93,198,202,211,267,422,551,555,566,577,585; Lexer.php:260,374,423,542,549,671,676; Node/BlockNode.php:38; Node/BlockReferenceNode.php:35; Node/DeprecatedNode.php:49; Node/EmbedNode.php:50; Node/Expression/AssignNameExpression.php:30; Node/Expression/Binary/EndsWithBinary.php:24,26,28; Node/Expression/Binary/MatchesBinary.php:35; Node/Expression/Binary/ObjectDestructuringSetBinary.php:41; Node/Expression/Binary/SequenceDestructuringSetBinary.php:41; Node/Expression/Binary/StartsWithBinary.php:24,26,28; Node/Expression/BlockReferenceExpression.php:84; Node/Expression/CallExpression.php:48,50,56,58,61,63,158,171,173,199,203,227,258,299; Node/Expression/ConditionalExpression.php:22; Node/Expression/FunctionNode/EnumCasesFunction.php:45,48; Node/Expression/FunctionNode/EnumFunction.php:45,49,52; Node/Expression/GetAttrExpression.php:37; Node/Expression/InlinePrint.php:27; Node/Expression/NullCoalesceExpression.php:33; Node/Expression/TempNameExpression.php:32; Node/ForNode.php:35,39; Node/ImportNode.php:32,36; Node/IncludeNode.php:47,58,61,65; Node/MacroNode.php:48,52,65,68,71; Node/MacrosNode.php:34,44; Node/ModuleNode.php:40,43,48,251,256,265,270,284,286,288,307,340,515; Node/Node.php:52,57,65,74,78,90,94,106,173,228; Node/WithNode.php:41,47,50,57,66,73; NodeVisitor/CorrectnessNodeVisitor.php:229; NodeVisitor/OptimizerNodeVisitor.php:60; NodeVisitor/SafeAnalysisNodeVisitor.php:84; NodeVisitor/YieldNotReadyNodeVisitor.php:41; Parser.php:80,246,250,253,337,526,555,603,662,672; Sandbox/Sandbox.php:43; Token.php:62,155,219,259; TokenParser/BlockTokenParser.php:52; TokenParser/DeprecatedTokenParser.php:52; TokenParser/GuardTokenParser.php:30; TokenParser/IfTokenParser.php:67; TokenParser/MacroTokenParser.php:53,101,104; TokenParser/SandboxTokenParser.php:37; TokenStream.php:31,77,80,81; Util/CallableArgumentsExtractor.php:49,61,69,72,96,100,125,137,162,198 | Cache/FilesystemCache.php:60,64,83,90; Environment.php:271,448,473,475,528,622,719,929; Extension/AttributeExtension.php:43,47,145,168,189; Loader/ArrayLoader.php:47,61,70; Loader/ChainLoader.php:77,111,130; Loader/FilesystemLoader.php:102,122,214,237,282,311; Profiler/Dumper/BaseDumper.php:58; Profiler/Dumper/BlackfireDumper.php:27,49,51; Profiler/Dumper/HtmlDumper.php:40,45,50; Profiler/Dumper/TextDumper.php:23,28,33; Profiler/Node/EnterProfileNode.php:34,37; Profiler/Node/LeaveProfileNode.php:35; Profiler/NodeVisitor/ProfilerNodeVisitor.php:36; Test/IntegrationTestCase.php:206,317,324,348,353,355; Test/NodeTestCase.php:115; Util/ReflectionCallable.php:48 |
| `str_contains` |  | Error/SyntaxError.php:33; Extension/CoreExtension.php:1082; Extension/DebugExtension.php:26 |  | ExtensionSet.php:509,517,525; Lexer.php:398,595; Node/Expression/CallExpression.php:37; Node/TextNode.php:48 | Loader/FilesystemLoader.php:296; Test/IntegrationTestCase.php:185; Util/ReflectionCallable.php:52 |
| `str_ends_with` |  |  | Node/Expression/Binary/EndsWithBinary.php:28 | Lexer.php:520 | Extension/AbstractExtension.php:61; FileExtensionEscapingStrategy.php:40 |
| `str_repeat` |  |  |  | Compiler.php:92,133; Node/Node.php:103 | Test/IntegrationTestCase.php:257 |
| `str_replace` |  |  |  | Compiler.php:152; ExtensionSet.php:510,518,526; Lexer.php:394,600,675; Node/Expression/FunctionNode/RenderSandboxedFunction.php:78; Node/ModuleNode.php:204,515; Node/Node.php:78,88; NodeVisitor/SandboxNodeVisitor.php:209; Source.php:89; Util/CallableArgumentsExtractor.php:176 | Loader/FilesystemLoader.php:248; Test/IntegrationTestCase.php:206,209 |
| `str_starts_with` |  | Extension/CoreExtension.php:1921,1924,1927; MacroNamespace.php:42,61 | Node/Expression/Binary/StartsWithBinary.php:28 | Lexer.php:100,416; Node/MacroNode.php:165; TokenParser/MacroTokenParser.php:97 | Loader/FilesystemLoader.php:272; Profiler/Profile.php:32 |
| `strcasecmp` |  | MacroNamespace.php:82,102 |  |  |  |
| `strip_tags` |  | Extension/CoreExtension.php:1348 |  |  |  |
| `strlen` |  | Error/SyntaxError.php:33; Extension/CoreExtension.php:1382; MacroNamespace.php:42,61; Runtime/EscaperRuntime.php:265,311 |  | Compiler.php:206; Lexer.php:218,430,469,592,605; Node/Expression/MethodCallExpression.php:40,51; Node/MacroNode.php:165; Node/Node.php:100; Source.php:92; TokenParser/MacroTokenParser.php:98 | Environment.php:985; Loader/FilesystemLoader.php:149,319; Test/IntegrationTestCase.php:318 |
| `strncmp` |  |  |  |  | Loader/FilesystemLoader.php:150 |
| `strpos` |  |  |  | Lexer.php:434 | Loader/FilesystemLoader.php:255,281; Util/ReflectionCallable.php:32 |
| `strrpos` |  | Template.php:344 |  | Source.php:90 |  |
| `strspn` |  |  |  |  | Loader/FilesystemLoader.php:318,321 |
| `strtolower` | Extension/CoreExtension.php:1915; Sandbox/SecurityPolicy.php:56 | Extension/CoreExtension.php:1696,1957; Sandbox/SecurityPolicy.php:149 |  | Node/Expression/AssignNameExpression.php:29; Node/Expression/CallExpression.php:277; Node/Expression/FunctionNode/RenderSandboxedFunction.php:78; Node/Expression/TempNameExpression.php:31; NodeVisitor/SandboxNodeVisitor.php:209; Util/CallableArgumentsExtractor.php:176,181 |  |
| `strtoupper` |  | Environment.php:644; Runtime/EscaperRuntime.php:187 |  |  |  |
| `strtr` |  | Extension/CoreExtension.php:646 |  |  |  |
| `substr` |  | Error/Error.php:144,146; Extension/CoreExtension.php:619,1185,1382,1696,1923,1926,1929,1940; MacroNamespace.php:42,61; Template.php:345 |  | Lexer.php:100,270,287,405,417,435,439,491,516,521,607,611; Node/Expression/Binary/MatchesBinary.php:35; Node/Expression/MethodCallExpression.php:40,51; Node/MacroNode.php:165; Node/TextNode.php:49; Source.php:89; TokenParser/MacroTokenParser.php:98 | Extension/AbstractExtension.php:61; FileExtensionEscapingStrategy.php:36,41; Loader/FilesystemLoader.php:151,256,257,285,286; Test/IntegrationTestCase.php:318; Util/ReflectionCallable.php:33 |
| `substr_count` |  |  |  | Compiler.php:205; Lexer.php:595 |  |
| `tempnam` |  |  |  |  | Cache/FilesystemCache.php:67 |
| `trim` |  | Extension/CoreExtension.php:1127,1138,1154,1165,1216,1247 |  | Lexer.php:575,585 | Test/IntegrationTestCase.php:317,345,348,355,368; Test/NodeTestCase.php:70,72 |
| `uasort` |  | Extension/CoreExtension.php:1060 |  |  |  |
| `ucfirst` |  |  |  | DeprecatedCallableInfo.php:43; Node/Expression/CallExpression.php:63 |  |
| `umask` |  |  |  |  | Cache/FilesystemCache.php:69 |
| `unlink` |  |  |  |  | Cache/FilesystemCache.php:89 |
| `unserialize` |  |  |  |  | Profiler/Profile.php:176 |
| `var_dump` |  | Extension/DebugExtension.php:55,57 |  |  |  |
| `var_export` |  |  |  | Compiler.php:168; Node/ModuleNode.php:515; Node/Node.php:88 |  |

---

## 21. Prioritised porting table

Size: S < 300 lines, M 300–1500, L 1500–5000, XL > 5000 or external-library scale. "Pure Rust?" = feasible to match PHP 8.5.13 byte-for-byte without binding C.

| prio | builtin / mechanism | php-src location | size | pure Rust? | notes |
|---|---|---|---|---|---|
| P0 | float/int/bool/null → string (`zend_double_to_str`, precision 14) and serialize_precision -1 variants | `Zend/zend_strtod.c:4509` (`zend_gcvt`), `Zend/zend_operators.c:3616`, `Zend/zend_smart_str.c:156` | M | yes | digit generation via shortest/fixed algorithms; exponent layout of 4.3; NaN warning (8.5) |
| P0 | weak argument coercion + TypeError/deprecation texts | `Zend/zend_API.c:505-800`, `:155`, `:119` | M | yes | one shared module used by every builtin port |
| P0 | `zend_compare` / `zendi_smart_strcmp` / numeric strings | `Zend/zend_operators.c:2260-2480`, `:3437`, `:3636` | M | yes | shared with type-juggling survey; needed by sort/min/max/compare |
| P0 | `htmlspecialchars` (UTF-8) | `ext/standard/html.c:89,1131,1378` | S | yes | autoescape default path; exact U+FFFD substitution |
| P0 | escaper js/css/html_attr(+relaxed)/url helpers (`preg_match('//u')`, per-char callbacks, `mb_ord`, `rawurlencode`) | `ext/pcre` (validity), `ext/standard/url.c:450,633` | S | yes | no regex engine needed; `from_utf8` = PCRE UTF check |
| P0 | `sprintf` / `php_formatted_print` | `ext/standard/formatted_print.c:423`, `main/snprintf.c:62,271` | M | yes | `format` filter + all error messages; quirks in 5.1 |
| P0 | `implode`, `explode`, `trim/ltrim/rtrim`, `str_contains/starts/ends`, `strtolower/upper` (ASCII) | `ext/standard/string.c:974,934,475-683,1858-1930,1250,1263` | S | yes | |
| P0 | `count`, `array_merge`, `array_keys/values`, `array_slice`, `array_key_exists`, `in_array` (strict), `array_is_list`, `current`, `iterator_to_array`, `iterator_count` | `ext/standard/array.c`, `ext/spl/spl_iterators.c:3041,3075`, `Zend/zend_API.c:2268` | S | yes | key coercion rules incl. 8.5 deprecations |
| P0 | `mb_strlen`, `mb_substr`, `mb_strtoupper`, `mb_strtolower`, `mb_convert_case(TITLE)` for UTF-8 | `ext/mbstring/mbstring.c:1858-3015`, `php_unicode.c:282`, `libmbfl/filters/mbfilter_utf8.c:446`, `unicode_data.h` | L | yes | Unicode 17.0 tables; final sigma chunking; invalid → `?` |
| P0 | `range` (function + `..`) | `ext/standard/array.c:2932-3246` | M | yes | PHP 8.3 rules, warnings, ValueErrors |
| P0 | `asort`/`uasort` + `zend_sort` + stable fallback + bool-comparator deprecation | `Zend/zend_sort.c:24-307`, `Zend/zend_hash.c:3200`, `ext/standard/array.c:88-120,286,754,837-950` | M | yes | validated port in section 19 |
| P0 | `json_encode` | `ext/json/json_encoder.c`, `json.c:220` | M | yes | flags, errors → false, float format |
| P1 | `number_format`, `round` (8.4 algorithm), `floor`, `ceil`, `abs` | `ext/standard/math.c:40-420,1110-1390` | S | yes | libm `pow` only for abs(places) > 22 |
| P1 | `max`/`min` incl. frameless tie rules | `ext/standard/array.c:1197-1460`, `Zend/zend_hash.c:3456` | S | yes | 4.7 |
| P1 | `strtr` (array) | `ext/standard/string.c:2977-3570` | S | yes | longest-match semantics, empty-key warning |
| P1 | `nl2br`, `rawurlencode`, `http_build_query` | `string.c:4876`, `url.c:450-650`, `http.c:25-245` | S | yes | |
| P1 | `strip_tags` | `ext/standard/string.c:5163-5560` | M | yes | state machine transliteration |
| P1 | `array_chunk`, `array_column`, `array_reverse`, `array_filter`, `array_pad`, `array_intersect_key` | `ext/standard/array.c` | S | yes | |
| P1 | operators `/ // % ** + - * b-and…` with PHP 8.5 int/float rules and errors | `Zend/zend_operators.c` | M | yes | shared with type-juggling survey |
| P1 | `DateTime`/`DateTimeImmutable`/`DateTimeZone` construction, `setTimezone`, `modify`, timestamps, defaults | `ext/date/php_date.c:2398,3263,4018`, `ext/date/lib/*` | XL | partially (bind timelib recommended) | lenient parser, tz transitions, relative arithmetic |
| P1 | `DateTime::format`, `DateInterval::format`, property table | `php_date.c:672,4957,1930` | S | yes (on top of timelib data) | trailing-backslash NUL quirk |
| P1 | timezone database 2026.6 | `ext/date/lib/timezonedb.h` | XL (data) | yes via data embedding | keep version in sync |
| P2 | `matches` operator: PHP regex front-end + PCRE2 10.44 | `ext/pcre/php_pcre.c:576-860`, `pcre2lib/` | XL (pure) / M (bind) | no (bind PCRE2) | warning texts reused by Twig messages |
| P2 | `var_dump` (`dump`) | `ext/standard/var.c:43-250` | M | yes | object handles not reproducible |
| P2 | `get_debug_type`, `is_*`, `is_numeric`, `defined`/`constant` + constants table, enums | `ext/standard/type.c`, `Zend/zend_builtin_functions.c`, `basic_functions.c:515` | S (+ data) | yes | constants list (section 18) |
| P2 | `hash('xxh128')` for string-template names | `ext/hash/hash_xxhash.c:281`, `xxhash.h` 0.8.1 | S | yes (crate) | verify with 3 vectors (5.11) |
| P2 | `levenshtein` + suggestion logic | `ext/standard/levenshtein.c:21-110` | S | yes | |
| P2 | `mt_rand`, `array_rand`, `shuffle` | `ext/random/random.c:447-545`, `ext/standard/array.c:3253,6355` | S | yes | randomness not reproducible anyway; keep errors |
| P2 | `LimitIterator`/`CallbackFilterIterator` semantics | `ext/spl/spl_iterators.c:1753,2163-2250` | S | yes | |
| P3 | non-UTF-8 charsets in htmlspecialchars | `ext/standard/html.c` (+ `html_tables.h`) | M | yes | 10 charsets |
| P3 | `iconv` (convert_encoding, non-UTF-8 escaping) | `ext/iconv/iconv.c` + glibc | XL | no (glibc behaviour) | recommend UTF-8 only or system iconv binding |
| P3 | libmbfl non-UTF-8 encodings for `upper/lower/title/length/slice` | `ext/mbstring/libmbfl` | XL | yes but large | |
| — | output buffering, `flush`, `debug_backtrace`, `func_get_args`, reflection, filesystem/opcache | various | — | n/a | replaced by twig-rs design |
