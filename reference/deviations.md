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
| iconv charsets | glibc converts CJK, stateful, EBCDIC and many codepages | `Error: iconv(): conversion of charset "X" is not supported by twig-rs` for charsets without tables; unknown names behave like PHP | Tables only for the common single-byte charsets and UTF variants |
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

## Unicode (`twig-unicode`)

| Area | PHP | twig-rs | Why |
|---|---|---|---|
| Encodings | mbstring supports ~80 encodings | UTF-8 only | Twig's default charset; others come later |
| `mb_substitute_character` modes | none / long / entity | Only the default `?` | Twig does not change it |

## Engine (`twig`)

| Area | PHP | twig-rs | Why |
|---|---|---|---|
| Non-UTF-8 bytes in error messages | Raw bytes | Lossy UTF-8 conversion | Error messages are Rust strings |
