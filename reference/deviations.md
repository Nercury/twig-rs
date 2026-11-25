# Known deviations from PHP 8.5.13 / Twig 3.31.1

Behaviour that cannot or deliberately does not match the target. Each entry names the PHP behaviour, what twig-rs
does instead, and why. Anything not listed here is expected to match exactly; a mismatch is a bug.

## Value model (`twig-value`)

| Area | PHP | twig-rs | Why |
|---|---|---|---|
| Object handles | `var_dump` prints per-request handle numbers `#N` that PHP reuses after frees | `ObjectRef::id()` is a process-wide counter, never reused | Allocation history is not reproducible |
| Rust closures in dumps | PHP 8.4+ closures dump `name`/`file`/`line` properties | `object(Closure)#N (0) {}` | Host closures have no PHP source location |
| Closure rebinding | `bind`/`bindTo`/`call`/`fromCallable` | `Error` for Rust-backed closures | No PHP scope to bind |
| Object comparison path | Switches from per-slot to property-table comparison once the table is materialised | Table comparison only when one object has dynamic properties | Differs only for uninitialised/unset declared properties |
| `ArrayIterator` | Sees modifications made during iteration | Iterates a snapshot taken at `rewind` | Twig converts to arrays first; unreachable from templates |
| `ArrayObject` extras | Object storage, custom iterator classes, sort/serialize methods | Not supported; methods return an `Error` | Not reachable from templates |
| Property model | Typed-property coercion on write, `__get`/`__isset` recursion guards | Not modelled | Host objects implement their own semantics |
| Userland TypeError suffix | `…, called in <compiled file> on line N` | Omitted | Refers to PHP's generated template class files |
| Resources, references | Exist | Not modelled; arrays are never recursive | Not reachable from Twig templates |
| Memory exhaustion | Fatal error at `memory_limit` (128M default) | `Error: Allowed memory size of 134217728 bytes exhausted (tried to allocate N bytes)` for oversized sprintf/number_format/json/range results; N is the zend_string size | Avoids unbounded allocation; the oracle itself runs with `memory_limit=-1` |
| Stack limits | `Maximum call stack size of N bytes … reached` (json_encode depth, http_build_query, deep nesting) | Not reproduced; deep recursion is bounded by twig-rs limits instead | PHP's byte count is machine dependent |
| `Field width %zd is too long` | Uncatchable fatal error | Catchable `Error` | No fatal errors in a library |
| `JsonException` code | Exception code set | Not on `PhpError`; available as `JsonError::code()` | `PhpError` carries class and message only |
| json_encode property snapshot | Declared properties read lazily | Property table read when the object is reached | Differs only if `jsonSerialize()` mutates a later property of the same parent |
| Throwing host cast handler in sprintf | PHP continues formatting | Error returned immediately | Built-in objects never throw there |
| Exceptions mid-loop (strtr, implode, strip_tags, str_replace) | Without an error handler PHP may continue | First exception returned at once | Equals PHP with a user error handler installed, which every diagnostics policy models |
| iconv charsets | glibc converts UTF-7, EUC-TW, ISO-2022-CN(-EXT), JOHAB, IBM932/943, the stateful EBCDIC double-byte codepages, TSCII and the ISO 6937 family | `Error: iconv(): conversion of charset "X" is not supported by twig-rs`; unknown names behave like PHP | Not ported; every other glibc module (single-byte, CJK, ISO-2022-JP/-JP-2/-JP-3/-KR, GB18030, BIG5-HKSCS, …) is converted with tables measured from the oracle's glibc |
| iconv UHC `A2 E8` under `//IGNORE` at the end of input | glibc reads past the input buffer (undefined behaviour) | The "incomplete multibyte character" notice the oracle shows | No UB |
| iconv GB2312→BIG5 at `F7 FF` | glibc reads one entry past its table | The value measured on the oracle | No out-of-bounds read |
| `str_repeat` huge results | Memory fatal error | `Error` with a twig-rs text | Twig does not call it at runtime |
| `array_rand` with holes | Depends on packed/hash layout and `nNumUsed` | Layout not exposed | Templates cannot create holes |
| Enum ordering in `SORT_REGULAR` | Distinct cases ordered by heap address | Descending object id (matches observed `Suit::cases()` order) | Heap addresses are not reproducible |
| Nested `usort` deprecation flag | Global flag reset by a nested sort | Per-call flag | Global engine state |
| Throwing comparator | PHP finishes the sort with the fallback order | Array left unchanged | Partial sort state is engine specific |
| Unseeded randomness | CSPRNG / per-process state | std keyed hasher for `mt_srand(null)`; `random_int` draws come from the caller | Seeded MT19937 matches PHP exactly |
| `pow` / `**` | glibc 2.41 FMA+AVX2 variant on the oracle machine | Port of that variant | Regenerating vectors on a CPU without FMA/AVX2 would make PHP itself differ in the last bit |

## Regex (`twig-pcre`)

| Area | PHP | twig-rs | Why |
|---|---|---|---|
| `(*UTF)` without `u` on invalid UTF-8 | Undefined behaviour in C | `false` with `PREG_BAD_UTF8_ERROR` | No UB in Rust |
| PHP's "known valid UTF-8" string flag | Hidden zend_string flag | Callers pass `SubjectUtf8` explicitly | Only `/u` patterns with `(*NOTEMPTY)` are affected |
| Allocation failures | PHP refuses to start or returns false with an internal error | JIT switched off, or panic | Out-of-memory handling is process specific |
| Flags ValueError | `$matches` already reset to `[]` | Not expressible with `Err` | API shape |
| Locale character tables | `setlocale`-dependent tables | Not supported | Twig never calls `setlocale` |
| Architectures | — | Only x86-64 tested | — |

## Dates (`twig-timelib`)

No deviation observable through PHP. Port-level differences (shared zone data via `Arc`, clamped month-table indexes
where C reads out of bounds, defined error codes where C leaves them uninitialised) are unreachable from PHP.

## Date objects and functions (`twig-date`)

| Area | PHP | twig-rs | Why |
|---|---|---|---|
| Not ported | `strftime`/`gmstrftime`, `date_sunrise`/`date_sunset`/`date_sun_info` | Absent | Deprecated/locale dependent; astro.c is not ported and Twig cannot reach it |
| Host subclasses of date classes | `__debugInfo` consulted by var_dump; class lookup on unserialize of private keys; user-class detection in "not initialised" messages | Not consulted / skipped / any non-built-in class counts as user class | Host subclasses are a twig-rs API concern |
| `#[\NoDiscard]` warnings on immutable mutators, constant deprecation notices (`DATE_RFC7231`, `SUNFUNCS_*`) | Emitted by the engine | Data available (`classes::GLOBAL_CONSTANTS`, `class_constant_deprecation()`); the twig engine must emit them | Engine-level diagnostics |
| `date.timezone` INI warning | Warning text on invalid ini value | `set_ini_timezone` returns false only | No INI layer |
| Failing `DatePeriod::__construct` overloads | Diagnostics of each failed overload | Dropped (hand-ported overload resolution) | Engine overload machinery |
| Concurrent renders | Per-request last errors and default timezone | Shared by renders that share one `DateContext` | The engine should use one context per render |
| Out-of-range month names | Undefined behaviour | `""` | No UB in Rust |

## Fixture data (`twig-test-support`, dev only)

Differences in the rebuilt `--DATA--` values that no fixture output depends on: `class_implements()` order for some
userland classes, anonymous class names and the `SplFileInfo` path (taken from the oracle's eval site), the
`DateTimeImmutable('now')` of `expressions/attributes.test` (oracle run time), resources (handed to the harness),
next-free integer keys derived from keys, partial `SimpleXMLElement`, `CallbackFilterIterator` callbacks rebuilt from
recorded decisions, unsupported `Generator::send()/throw()` and rare SPL methods.

The fixture harness (`tests/fixtures/`) gives the one resource of the corpus (`tests/in.test`) to templates as a
`TwigRs\Fixtures\ResourceStandIn` object that converts to `Resource id #1`, and does not define the PHP functions
`Twig\Tests\html()` (it only checks that a PHP function named like a strategy does not disturb escaping) and
`test_foo()` of IntegrationTest.php: twig-rs has no PHP function table, and the test extension's callables that use
them are ported in Rust.

## Unicode (`twig-unicode`)

| Area | PHP | twig-rs | Why |
|---|---|---|---|
| Encodings | mbstring supports ~80 encodings | UTF-8 only | Twig's default charset; others come later |
| `mb_substitute_character` modes | none / long / entity | Only the default `?` | Twig does not change it |

## Engine (`twig`)

| Area | PHP | twig-rs | Why |
|---|---|---|---|
| Non-UTF-8 bytes in error and deprecation messages | Raw bytes | Lossy UTF-8 conversion | Error messages are Rust strings |
| Trim options containing PCRE metacharacters | Inserted unquoted into the `endverbatim` pattern (e.g. `+` breaks every `{% verbatim %}` with a PHP warning) | Always taken literally | Configuration accident in Twig |
| `whitespace_line_chars` option | PCRE character-class body; empty `[]` is a compile error that disables closing delimiters | Literal byte set; empty set matches nothing | Same unless class syntax is used |
| Lexer settings that make Twig loop forever | Infinite loop | Both interpolation delimiters empty disables interpolation; empty operator tokens are ignored | No hangs |
| `{% line N %}` overflow | TypeError after `PHP_INT_MAX` | Line numbers saturate | No integer overflow |
| `Token` deprecations for ARROW/SPREAD types and punctuation-typed tests (3.21) | Raised by `Token::test()` | Not raised | The lexer never produces such tokens; no sink there |
| Past-the-end token stream access (e.g. `{% line 0 %}` then an unclosed tag) | TypeError / `Error` on `null` | Last token returned / syntax error kept | No null dereference |
| Expression nesting | No limit (PHP's stack is on the heap) | `Maximum nesting level of 128 reached.` (configurable; tags and `embed` count too) | Deep recursion would overflow the native stack |
| Legacy node classes and APIs | `NameExpression`, `MethodCallExpression`, `InlinePrint`, …; callable options `deprecated`/`deprecating_package`/`alternative`; `getOperators()`; "not ready for first-class callables" deprecation | Not ported | Deprecated since 3.x; no core use |
| Embedded-template indices after a failed parse | Cached parser keeps counting (2, then 3) | Fresh parser per template; indices restart at 1 | Parser state is not shared between templates |
| Documentation comment targets | Any node object | Blocks, macros and child paths of the returned node | AST shape |
| Node traverser | Visitors may restructure anything | A visitor changes only the visited node and its subtree; removing the root or a non-container child is a LogicException-kind error | Static AST |
| Recursion depth | Templates recurse (includes, blocks, macros, parents) until PHP runs out of stack or memory; templates using each other as traits recurse forever | `Maximum template call depth of N reached (twig-rs limit).` (option `max_depth`, default 200); constructors of templates loading their traits are limited to 64 nested levels (only a cycle of traits gets there) | Bounded native stack |
| Output encoding | Output is a byte string | `render()`/`render_block()` return `String` and fail with `The output of template "%s" is not valid UTF-8 (render it to bytes instead).`; `render_bytes()`, `display()` and the sinks get the bytes | Rust strings are UTF-8 |
| Output sink failures | `echo` cannot fail | A failing `Sink` gives a `RuntimeError` `Unable to write the template output: ...` | Host sinks can fail |
| Warnings of compiled PHP code | PHP warns once while compiling the generated class (constant-folded array literals, e.g. an undefined key of a literal array) | Raised when the expression runs, on every render | No PHP compiler step |
| Line guessing with `cache => false` | Every template is `eval()`'d from the same line of `Environment.php`, so `Error::guess()` also matches the frames of other templates (an exception in a macro imported from another template takes the handling template's marker at or before the macro's PHP line) | Templates are told apart as with a filesystem cache | Depends on the layout of generated PHP |
| `{% sandbox %}` and `include(..., sandboxed = true)` | Toggle the checker of the environment (`setSandboxed()`), seen by every render sharing it | Toggle the sandbox for the current thread; `SandboxExtension::enable_sandbox()` still applies to every thread | Concurrent renders share one environment |
| `render_sandboxed()` | `$this->env->getRuntime(SandboxBridgeRuntime::class)` before the arguments are evaluated | The runtime is fetched when the call runs, after the arguments | Runtime-class callables are looked up at call time |
| `Twig\TemplateWrapper` objects in templates | Every public method can be called | `render`, `renderBlock`, `hasBlock`, `getBlockNames`, `getTemplateName`, `getDefaultEscapeStrategy` and `getSourceContext` work; `display`, `displayBlock`, `stream`, `streamBlock`, `unwrap`, `isOwnedBy`, `__construct` (and `Twig\Source::__construct`) throw a `LogicException` `Calling Twig\TemplateWrapper::%s() from a template is not supported by twig-rs.` | They echo or return engine internals |
| `{% from %}` macro variables | Named `_vN` by the compiler | Context key `"\0_v<id>"` | Visible only if a user alias is literally `_vN` |
| String callables | `'strtoupper'`, `'Class::method'` and `[$class, 'method']` name PHP functions | Cannot be resolved: `Call to undefined function X()`; in `sort` and `filter` PHP's callable-check `TypeError` (`... must be a valid callback, function "x" not found or invalid function name`, `class "X" not found`) | No PHP function table |
| Closure names and source locations in messages | `{closure:<file>:<line>}` in arrow-function `ArgumentCountError`s; userland `TypeError`/`ArgumentCountError` messages end with `, called in <compiled file> on line N` or `passed in <file> on line N` (`Template::load()`, `Template::yield()`, `renderBlock()` without `$blocks`, the typed parameters of the template functions, `invoke` with a non-`Closure`, spreads binding too few arguments) | `{closure}` and no location suffix | The files are PHP's generated template classes |
| `needEscaping()`'s `TypeError` (`{% autoescape null %}` around a print or block) | A raw `TypeError` (`compileSource()` does not wrap it) | A `LogicException`-kind error with PHP's message, which `compile_source()` wraps (`An exception has been thrown during the compilation of a template ("...")`) | `Error` has no `TypeError` kind for compile errors |
| Invalid optimizer mode, deprecated modes 4 and 8 | Reported when the extensions initialize | Reported by the first parse (a template that also has a syntax error reports the syntax error first) | The visitor is built lazily |
| Parameter types in the sandbox | `CallableParameters::isStringCoercionSafe()` reflects every PHP parameter type | Twig's own 8 safe parameters are known; host callables declare theirs with `CallableParameter::string_coercion_safe()`, otherwise untyped (more `CheckToStringNode`s) | No reflection of Rust closures |
| Expression classes defined by extensions | Escaped and sandbox-wrapped according to their PHP class (`FunctionExpression` subclasses, `OperatorEscapeInterface`, ...) | Always unsafe for escaping and never wrapped themselves; the sandbox wraps the children they list in `CustomExpr::string_coerced_child_names()` / `CustomNode::string_coerced_child_names()` | Custom node traits do not model PHP class hierarchies |
| Escaper default strategy values | Any value a callable returns, `is_safe_callback`s that throw | A string, `false` or a callback returning a constant; a non-UTF-8 strategy name is stored lossily in the module's `strategy` attribute | API shape |
| Custom node classes in compiled code | Compiled by their `compile()` method | `The "%s" node class cannot be compiled by twig-rs.` | No code generation hook for extensions yet |
| Literal `enum()`/`enum_cases()` names that equal an alias of compiled templates (`Markup`, `Source`, `Template`, `Environment`, ...) | The inlined `Name::CASE` resolves to the `use`d Twig class | The enum checked at compile time | The compiled PHP class header is not modelled |
| Deprecated escaper API | `EscaperExtension::setEscaper()`, `getEscapers()`, `setSafeClasses()`, `addSafeClass()` (deprecated since 3.10); escapers are PHP callables | Not provided: `Environment::escaper_runtime()` with Rust closures | Deprecated; no PHP callables |

## Callables (`twig::extension::runtime`)

| Area | PHP | twig-rs | Why |
|---|---|---|---|
| mbstring encodings (`upper`, `lower`, `title`, `capitalize`, `length`, `slice`, `first`, `last`, `split`) | mbstring converts ~80 encodings with its tables | Case conversion supports UTF-8, ISO-8859-1, 8bit, ASCII and 7bit; character counting and cutting supports UTF-8 and the 26 single-byte encodings; other encodings mbstring knows fail with `Error: <function>(): encoding "X" is not supported by twig-rs` (unknown names give PHP's `ValueError`) | No multi-byte or table-based encodings yet |
| Randomness (`random`, `shuffle`) | One MT19937 per request, seeded from the CSPRNG unless `mt_srand()` ran | One MT19937 per thread, seeded from entropy; `twig::extension::runtime::mt_srand()` seeds it, and seeded sequences match PHP's | A render runs on one thread |
| Re-entering a `CallbackFilterIterator` (`filter` on a `Traversable`) | Host code may call back into the iterator during one of its own calls | `Error` | Templates cannot reach it |
| PHP constants (`constant()`, the `constant` test) | Every constant of the build | The constants of Core, standard, pcre, json, mbstring, iconv and random as the target defines them (build-specific values such as `PHP_BINARY` copied from the oracle), `DATE_*`, and what the host registers; namespaced global constant names are matched exactly (PHP lower-cases the namespace part) | Only what templates plausibly use |
| Non-UTF-8 template names given to `template_from_string()` | Bytes | Converted lossily | Template names are Rust strings |

## Loaders (`twig::loader`)

| Area | PHP | twig-rs | Why |
|---|---|---|---|
| PHP warnings of `FilesystemLoader` | Without an error handler PHP warns and carries on: a cached file that was deleted or became a directory loads as `Source('')` (`file_get_contents(…): Failed to open stream: …`, `file_get_contents(): Read of <st_size+8192> bytes failed with errno=21 Is a directory`), `isFresh()` on a vanished file compares `0` (`filemtime(): stat failed for …`) | The `ErrorException` of a converted warning (`ErrorKind::Php`), what PHP throws with warnings turned into exceptions; templates wrap it like any other non-Twig exception | The loader has no diagnostics sink |
| Stream wrappers | `phar://`, `data://`, `glob://`, `php://`, `http(s)://`, `ftp(s)://`, `compress.zlib://`; unknown wrappers warn `Unable to find the wrapper` and fall back to a plain path | Only plain files and `file://`; other wrappers never exist, without a warning | Plain files only |
| PHP's stat and realpath caches | Stat cache, realpath cache (120 s TTL), `MAXPATHLEN` limit of `tsrm_realpath()` | Not modelled: the operating system is asked every time | Process-level caches |
| Numeric namespaces | `getNamespaces()` returns int keys for numeric names | Strings | Typed API |
| `new FilesystemLoader('')` / `('0')` | A falsy string registers nothing | No string form (the constructor takes a list) | API shape |
| Platforms | Linux (the oracle) | Windows follows PHP-on-Windows rules (`\` separator, `realpath()` without the `\\?\` prefix, PHP's Windows `file://` forms), not covered by the oracle; non-UTF-8 paths are exact bytes on Unix (`Source::path` lossy) and never exist on Windows | OS path APIs |
| Host loader class names | `get_class($loader)` | `Loader::class_name()`, by default the Rust type name | No PHP class |
