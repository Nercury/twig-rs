# Benchmarks: twig-rs and PHP Twig

A comparison of twig-rs with PHP Twig 3.31.1 on PHP 8.5.13, over the scenarios and micro templates of the criterion
benchmarks in `benches/` (see `benches/README.md`), measured on 2026-11-27 before and after the second profiling pass:
twig-rs at `a258816` and at `9e99e9d` (the pass, merged with the fixes after fuzz round 7). Both engines produce
byte-identical output for every scenario (checked with `TWIG_BENCH_DUMP` and `bench.php --dump`).

## Summary

- **Rendering** with a warm template cache: the second pass made rendering **1.2-1.7x faster** on Linux (1.1-1.7x on
  Windows). twig-rs now renders five of the six scenarios faster than PHP Twig (`escaping` 4.7x, `inheritance` 2.1x,
  `loops` 1.5x, `macros` 1.4x, `dates` 1.2x) and `filters` 1.02x slower; against PHP with OPcache and the tracing JIT it
  is 1.1-4.1x faster on the same five and 1.21x slower on `filters`. With the first pass (measured a day earlier),
  rendering is about 2.4-5.2x faster than at `595fce0`.
- **Single constructs** (the micro templates): only an empty `for` loop is still clearly slower than PHP Twig (2.4x;
  2.0x with the JIT). `{% set x = a + 1 %}` is 1.06x slower (11.6 ns against 10.9 ns) and `{{ name|length }}` 1.05x
  slower than with the JIT (1.20x faster than PHP Twig without it); every other construct is faster than both. An
  auto-escaped `{{ a }}` now costs 11.3 ns more than `{{ a|raw }}` (64 ns before the pass; 31 ns in PHP, 17 ns with the
  JIT).
- **Compiling** (lexing, parsing, node visitors, code generation) is **10.3-11.8x faster** than PHP Twig's
  `compileSource()` and 7.1-8.0x faster than with OPcache and the JIT; the second pass made it 7-16% slower, mostly
  through its instruction fusion.
- What is left is listed under [Remaining hot spots](#remaining-hot-spots): the context copies of `for` loops and arrow
  functions (atomic reference counts), name hints that collide in some processes, the charset reads of callables such as
  `length`, case conversion, markup objects and the date objects.

## Results

Linux, both engines in containers on the same Docker host; medians per iteration (criterion for twig-rs, `bench.php` for
PHP), each the median of three rounds (see [Setup](#setup)); "before" is twig-rs at `a258816`:

| benchmark | twig-rs before | twig-rs | speed-up | PHP Twig | PHP Twig, OPcache + JIT | twig-rs vs PHP Twig | twig-rs vs OPcache + JIT |
|---|---:|---:|---:|---:|---:|---:|---:|
| compile/dates | 26.5 µs | 30.3 µs | 0.87x | 353.9 µs | 222.4 µs | 11.67x faster | 7.33x faster |
| compile/escaping | 33.0 µs | 36.9 µs | 0.89x | 434.1 µs | 275.7 µs | 11.76x faster | 7.47x faster |
| compile/filters | 120.3 µs | 139.1 µs | 0.86x | 1.46 ms | 1.01 ms | 10.53x faster | 7.27x faster |
| compile/inheritance | 126.1 µs | 139.8 µs | 0.90x | 1.44 ms | 990.9 µs | 10.33x faster | 7.09x faster |
| compile/loops | 53.1 µs | 59.0 µs | 0.90x | 641.9 µs | 424.7 µs | 10.88x faster | 7.20x faster |
| compile/macros | 73.1 µs | 78.2 µs | 0.94x | 877.8 µs | 628.3 µs | 11.22x faster | 8.03x faster |
| micro/attribute | 143.4 µs | 52.2 µs | 2.75x | 180.4 µs | 136.8 µs | 3.45x faster | 2.62x faster |
| micro/empty_loop | 572.0 µs | 379.2 µs | 1.51x | 158.9 µs | 187.6 µs | 2.39x slower | 2.02x slower |
| micro/filter | 164.4 µs | 89.4 µs | 1.84x | 107.2 µs | 85.5 µs | 1.20x faster | 1.05x slower |
| micro/for | 129.0 µs | 48.9 µs | 2.64x | 80.9 µs | 64.0 µs | 1.65x faster | 1.31x faster |
| micro/hello | 0.63 µs | 0.45 µs | 1.39x | 1.40 µs | 1.14 µs | 3.10x faster | 2.52x faster |
| micro/macro | 674.1 µs | 589.8 µs | 1.14x | 916.0 µs | 892.7 µs | 1.55x faster | 1.51x faster |
| micro/print_escaped | 84.1 µs | 27.8 µs | 3.02x | 63.4 µs | 47.2 µs | 2.28x faster | 1.70x faster |
| micro/print_int | 15.0 µs | 13.9 µs | 1.07x | 26.6 µs | 25.1 µs | 1.91x faster | 1.80x faster |
| micro/print_raw | 20.1 µs | 16.5 µs | 1.22x | 32.5 µs | 30.3 µs | 1.96x faster | 1.83x faster |
| micro/set | 27.0 µs | 9.84 µs | 2.75x | 10.4 µs | 10.4 µs | 1.06x faster | 1.05x faster |
| micro/set_add | 46.7 µs | 11.6 µs | 4.04x | 10.9 µs | 10.8 µs | 1.06x slower | 1.08x slower |
| micro/text | 6.39 µs | 4.39 µs | 1.45x | 23.6 µs | 22.1 µs | 5.37x faster | 5.03x faster |
| render/dates | 708.0 µs | 499.6 µs | 1.42x | 605.6 µs | 558.8 µs | 1.21x faster | 1.12x faster |
| render/escaping | 441.5 µs | 284.9 µs | 1.55x | 1.35 ms | 1.17 ms | 4.74x faster | 4.12x faster |
| render/filters | 767.3 µs | 644.4 µs | 1.19x | 633.7 µs | 534.4 µs | 1.02x slower | 1.21x slower |
| render/inheritance | 58.6 µs | 39.7 µs | 1.48x | 82.4 µs | 66.6 µs | 2.07x faster | 1.68x faster |
| render/loops | 511.0 µs | 309.0 µs | 1.65x | 450.2 µs | 386.5 µs | 1.46x faster | 1.25x faster |
| render/macros | 703.2 µs | 525.2 µs | 1.34x | 739.3 µs | 665.4 µs | 1.41x faster | 1.27x faster |

PHP's `compile` time does not include PHP compiling the generated class (an `eval()` or an OPcache miss), which a
template pays once more before its first render.

### Micro-benchmarks

The `micro` group of `benches/render.rs` (and of `bench.php`): one-off templates that repeat one construct 1000 times
(`hello` and `for` once), rendered like the scenarios:

| name | template |
|---|---|
| `hello` | `Hello {{ name }}!` |
| `text` | `x{# c #}` |
| `print_int` | `{{ 1 }}` |
| `print_raw` | `{{ a\|raw }}` |
| `print_escaped` | `{{ a }}` (auto-escaped) |
| `set` | `{% set x = a %}` |
| `set_add` | `{% set x = a + 1 %}` |
| `for` | `{% for i in items %}{{ i }}{% endfor %}` (1000 iterations) |
| `attribute` | `{{ user.name }}` |
| `filter` | `{{ name\|length }}` |
| `empty_loop` | `{% for i in [1, 2] %}{% endfor %}` |
| `macro` | `{{ _self.m(a) }}` with `{% macro m(v) %}{{ v }}{% endmacro %}` |

Their rows are in the table above. Per construct, after the pass (in brackets: before it, PHP Twig, PHP with OPcache and
the JIT):

- An auto-escaped print of a variable costs 11.3 ns more than the same print with `|raw` (64, 31, 17 ns): it is one
  fused instruction that hands the value to the `escape` filter where it is in the context and prints a value that needs
  no escaping in place.
- `{% set x = a %}` costs 9.8 ns (27.0, 10.4, 10.4 ns) and `{% set x = a + 1 %}` 11.6 ns (46.7, 10.9, 10.8 ns); an
  iteration of a `for` loop that prints its value 48.9 ns (129, 80.9, 64.0 ns); `{{ user.name }}` 52.2 ns (143, 180,
  137 ns); a macro call 590 ns (674, 916, 893 ns); the fixed cost of a render (`hello`) 0.45 µs (0.63, 1.40, 1.14 µs).
- Setting up and leaving an empty `for` loop costs 379 ns (572, 159, 188 ns) and `{{ name|length }}` 89.4 ns (164, 107,
  85.5 ns): see [Remaining hot spots](#remaining-hot-spots).

### Windows

The same sources natively on the Windows host (rustc 1.100.0, x86_64-pc-windows-msvc, criterion defaults, the system
allocator), run like the Linux ones (a process per benchmark, both trees in turn, three rounds, medians), with every
process pinned to one logical processor of a Zen 5 core (`start /affinity 4`):

| benchmark | before (`a258816`) | after | speed-up |
|---|---:|---:|---:|
| compile/dates | 32.6 µs | 36.6 µs | 0.89x |
| compile/escaping | 39.5 µs | 45.5 µs | 0.87x |
| compile/filters | 145.9 µs | 170.5 µs | 0.86x |
| compile/inheritance | 153.0 µs | 169.8 µs | 0.90x |
| compile/loops | 63.8 µs | 72.4 µs | 0.88x |
| compile/macros | 86.2 µs | 94.8 µs | 0.91x |
| micro/attribute | 138.5 µs | 64.4 µs | 2.15x |
| micro/empty_loop | 599.3 µs | 369.0 µs | 1.62x |
| micro/filter | 161.6 µs | 97.9 µs | 1.65x |
| micro/for | 123.5 µs | 46.7 µs | 2.65x |
| micro/hello | 1.01 µs | 0.65 µs | 1.55x |
| micro/macro | 840.7 µs | 762.4 µs | 1.10x |
| micro/print_escaped | 92.9 µs | 25.9 µs | 3.59x |
| micro/print_int | 15.1 µs | 13.6 µs | 1.11x |
| micro/print_raw | 18.7 µs | 16.3 µs | 1.15x |
| micro/set | 24.8 µs | 11.6 µs | 2.15x |
| micro/set_add | 43.9 µs | 12.9 µs | 3.40x |
| micro/text | 6.96 µs | 4.81 µs | 1.45x |
| render/dates | 936.2 µs | 604.5 µs | 1.55x |
| render/escaping | 467.8 µs | 306.7 µs | 1.53x |
| render/filters | 874.1 µs | 762.2 µs | 1.15x |
| render/inheritance | 67.9 µs | 45.2 µs | 1.50x |
| render/loops | 504.2 µs | 299.0 µs | 1.69x |
| render/macros | 760.7 µs | 596.7 µs | 1.27x |

The pinning matters on this CPU: it has four Zen 5 cores (logical processors 0-7, 16 MB of L3) and eight Zen 5c cores
(8-23, 8 MB of L3, lower clocks), and without it Windows ran some of the benchmark processes on the compact cores: the
slowest of three rounds of a benchmark was typically 24% slower than the fastest, sometimes 50-90% (the container runs
were not pinned; their rounds differed by 2% typically and 6% at most, the name-hint collisions aside). The Windows
system allocator is slower than glibc's: in the first pass, making `mimalloc` the global allocator of the render
benchmark made rendering 1.3-1.6x faster before that pass and 1.05-1.4x after it. twig-rs does not choose an allocator
for its host; one that renders a lot on Windows can make `mimalloc` (or another fast allocator) its global allocator.

## The first pass

The first profiling pass (2026-11-26, `595fce0` to `fdc54d8`) made rendering 1.9-3.4x faster on Linux and 2.0-3.8x on
Windows, left compiling as it was, and cut the allocations per render by 63-96% (`escaping`: from 23,702 to 1,521). By
area:

- **Calls of filters and functions**: argument vectors reused within a render; the `escape` filter called with its
  arguments as they are when they have the declared types; core callables bound without allocating when PHP's binding
  cannot fail or reorder anything; closures invoked without resolving them through the environment; method lookups
  without allocating the lowercase name.
- **Arrays**: large arrays indexed by stored hashes and an open-addressing table of positions, so that a copy-on-write
  copy is two `memcpy`s; shared arrays iterated in place; the variable names of a program prepared (normalised and
  hashed) once.
- **Interpreter**: instructions dispatched inside the interpreter loop; frame vectors reused; prints write strings and
  ints without converting them; `getAttribute()` reads existing elements with one borrowed lookup; `loop` fields updated
  through prepared keys; the context after a `for` loop restored in place when no int key is involved.
- **Strings, markup, dates**: concatenations, int strings and `rawurlencode()` results built in one allocation;
  well-formed UTF-8 escaped by `htmlspecialchars()` in one pass; hex escapes written without formatting a string per
  character; `Twig\Markup` and the date filters without dynamic method calls; date formats write their numbers directly.

## The second pass

A second round over the remaining hot spots, starting from `a258816`, profiled the same way (`perf` on the criterion
binaries run with `--profile-time`) and decided by interleaved A/B runs: each candidate was built next to its
predecessor (`cargo bench --no-run` in `rust:1.100-slim-trixie`, the binaries kept side by side) and the two were run
alternately, round by round (3 to 6 rounds of criterion with 1 s of warm-up and 2-3 s of measurement). The ratios below
are medians of the per-round ratios, new / old (below 1 is faster). Other work ran on the machine, so single rounds
varied by up to ±10%; only changes that measured faster in most rounds were kept. By area, in the order they were made:

- **Per-render fixed costs**: `Environment::load()` built the class key by copying the loader's cache key (for an
  `ArrayLoader`, the whole template code) three times and looked it up with SipHash; the key is now built in place and
  looked up with foldhash: `text` 0.79, `hello` 0.91, `set` 0.92.
- **Array lookups**: keys of up to 16 bytes are compared with word loads instead of a `memcmp` call (most comparisons of
  a linear search fail on the length or the first bytes): `set` 0.94, `empty_loop` 0.95. twig-value's hot array,
  comparison and arithmetic functions are `#[inline]` (no call from `twig` into twig-value is inlined without it), and
  int comparisons have a fast path: `set` 0.84, `set_add` 0.85, `loops` 0.91 (with the first date changes). Frames
  remember where they last found each variable in their context (16 slots by key hash; the hint is checked inline and
  the search is out of line; the entries of a context mostly keep their positions): `for` 0.81, `set` 0.88, `set_add`
  0.88 against the same code without hints.
- **Instruction fusion** (`lltl::fuse`, see `plans/engine.md`): a pass at the end of the compilation of every unit
  replaces frequent sequences with fused instructions. Calls take their arguments from the operand stack, and their
  trailing constant arguments and the `Print` of their result become part of the call (`CallStack`); the auto-escaped
  print of a variable is one instruction that hands the value to the `escape` filter where it is in the context
  (`CallName`: a value the filter returns unchanged is printed in place); the core `escape` runtime is called without an
  argument list; `Const`/`LoadName` then `StoreName`, attribute reads without arguments, an operator with a constant
  operand, an operator then `StoreName`, and a comparison then a jump are single instructions; jumps to jumps are
  threaded. Only instructions that cannot fail, warn or run other code are removed, and the line table is rebuilt from
  the lines of the remaining ones: `attribute` 0.40, `print_escaped` 0.56, `inheritance` 0.58, `filter` 0.58, `for`
  0.59, `escaping` 0.74, `loops` 0.75, `filters` 0.84, `macros` 0.85, then `set_add` 0.81 (the operators). Last, a
  lenient read of a variable joins the operator and store after it (`NameBinaryStore`, `{% set x = a + 1 %}`): `set_add`
  0.68; and the fused operators add, subtract and multiply two ints inline (the copy of the generic operator's result
  out of its `Result` was the hottest code of the `set_add` profile): `set_add` 0.68 again, `loops` 0.97.
- **Constant arrays**: the constant prefix of an array literal is built once, as `zend_try_ct_eval_array()` does in PHP
  (an array holding `NAN` is left alone: the same-array shortcut of the comparisons would make sharing it observable):
  `empty_loop` 0.76 (with the restore below).
- **Loops and context writes**: several writes in a row separate an array once (`Array::edit()`): the two loop variables
  of an iteration, the removals after a loop, the `loop` fields; the restore after a loop keeps a context whose keys are
  the parent's, in order; copies of an array get room for a few more entries; and twig-value's array tables are
  `triomphe::Arc`s, whose uniqueness check before every write is a load instead of the compare-and-swap of
  `std::sync::Arc::make_mut()` (about 5 ns on this machine): `loops` 0.93 and `for` 0.86 (with the hints), then `set`
  0.69, `set_add` 0.90, `macro` 0.92, `for` 0.93, `loops` 0.95 for triomphe (`empty_loop` 1.04).
- **Output**: a render to a string writes into its buffer without the dynamic call of the sink: `text` 0.85 (`escaping`
  0.92 together with the loop changes).
- **Dates**: `@<timestamp>` is parsed without the scanner and without allocating (a test checks the result against the
  scanner's over many inputs), and the date of an int timestamp is built without its string; `DateTime::modify()` is
  called directly; zone names are parsed from a stack copy; the default timezone's data, the clock and the POSIX
  transitions of each year (twig-timelib's `PosixCache`) are not looked up or computed again; `format()` formats the
  time where it is and computes the zone offset only for the format letters that print it: `dates` 0.95, 0.86 and 0.88.
- **Measured and dropped**: remembering positions for attribute reads as well, together with fusing `StoreName('_seq')`
  with `IterStart`, measured slower (`escaping` 1.24, `attribute` 1.09, `filters` 1.07).

The compile benchmarks became 7-16% slower: the fusion passes are about 10% of the `compile/filters` profile, and the
rest of the pass costs 2-4% (measured with the passes switched off). Allocations per render, counted with a counting
global allocator in a scratch harness (reallocations counted as allocations, unlike the first pass's count), before /
after: `dates` 12,936 / 6,626 (596 / 455 KB), `loops` 1,247 / 841, `inheritance` 476 / 354, `filters` 9,211 / 8,648,
`macros` 6,644 / 6,424, `escaping` 1,539 / 1,531.

## Remaining hot spots

From `perf` profiles of the final code (Linux, shares of the samples of one benchmark), in order of weight. They are why
`empty_loop` is still 2.4x slower than PHP Twig (2.0x with the JIT), why `filters` is 1.02x slower than PHP Twig and
1.21x slower than with OPcache and the JIT, and why `filter` is 1.05x slower than with the JIT:

- **Atomic reference counts of context copies**: template values must be `Send + Sync`, so strings and arrays are `Arc`s
  with atomic counts where PHP increments plain integers. Every `for` loop copies the context into `_parent` (as PHP
  does); the first write of the loop then copies the context's table, with an atomic increment for every string key and
  every string or array value, and the restore after the loop drops the copy with as many atomic decrements, on top of
  allocating and freeing the copy and the `loop` array with glibc's `malloc` (PHP's allocator serves these from per-size
  free lists). In the `empty_loop` profile, copying the context's table is 15%, dropping it 10%, and writing, finding
  and removing the loop variables (`_parent`, `_seq`, `loop`, `_key`, `i`) about 40%: most of its 379 ns per loop (PHP:
  159 ns). The same copy and drop comes with every call of an arrow function: in `filters`, the arrow functions of
  `map`, `filter`, `reduce` and `sort` are 26% of the profile and copying and dropping array tables 10%; in `loops`, the
  copies are 11%. PHP copies the table on the first write as well, with plain increments; the counts cannot become plain
  integers while values are `Send + Sync`.
- **Name hints that collide in some processes**: the 16 slots in which a frame remembers where it last found a variable
  are chosen by the key's hash, and the key hasher's seed is random per process (foldhash, from the time and addresses).
  In a process where two names that a loop or a row of statements reads and writes in turn share a slot, every access
  misses its hint and searches: in 16 fresh processes each, `{% set x = a %}` ran at 17-18 ns instead of 10 ns in 4,
  `{% set x = a + 1 %}` at 19.6 ns instead of 11.4 ns in 1 and `{{ a }}` at 37 ns instead of 28 ns in 1 of 12
  (`a258816`, without hints, varied by at most 8% between processes; the scenarios of the final code vary by 5-7%).
  Slots assigned to the names of a program when it is compiled would not collide.
- **Callables that need the charset**: `length` itself is about a quarter of the `filter` profile (`{{ name|length }}`):
  every call reads the environment's charset under the options lock and clones it (an `RwLock` read and an `Arc` clone
  and drop), and `mb_strlen()` then looks the encoding up by its name again. The other callables that take the charset
  (`upper`, `lower`, `title`, `capitalize`, `slice`, `first`, `last`, `reverse`, `split`, ...) read it the same way.
- **Filters**: case conversion goes through code points also for ASCII strings (`twig_unicode::case::convert_case` and
  `from_wchar`, 6% of `filters`), `mb_substr()` formats the text of its range error on every call before it checks the
  range (`substr_range_error`, 2%), userland-typed methods bind and check their arguments, and calls of several
  arguments still collect them in a vector.
- **The interpreter loop**: code that fusion does not cover still pays a dispatch per instruction and moves its values
  through the operand stack (`execute` itself is 24% of `loops`, 17% of `macros` and 9% of `filters`); the set-up and
  the end of a `for` loop take several instructions each.
- **Markup** (macros, captures, `include()`): creating a `Twig\Markup` builds a `PropertyStore` (a lock and a slot
  vector) and reading its content locks it again: about 6% of `macros`.
- **Dates**: the work PHP also does, in Rust instead of C: constructing the `DateTime` objects of the filters is 29% of
  `dates`, formatting 14%, converting time zones 12%, `date_modify` 12%.
- **Compiling**: the fusion passes rebuild the code, the line table and the jump flow of a unit once per pass that
  changes something (10% of `compile/filters`), and the code generator's constant table and alias map hash with std's
  SipHash (about 5%); fewer, merged passes and foldhash would win part of it back.
- **Windows**: the system allocator (see [Windows](#windows)).

## Setup

- Machine: AMD Ryzen AI 9 HX 370 (12 cores, 24 threads), Windows 11 Pro, Docker Desktop 28.5 (WSL 2 backend, all 24
  threads). Other work (builds, tests and oracle runs in other checkouts) used the same machine that day, so the runs
  went one at a time: each started only after 15 s without other builds (`cargo`, `rustc`, `clippy-driver`, `rustdoc`)
  and other containers, and one during which such work started (checked every 4 s) was stopped and run again: 7 of the
  150 Linux runs and 1 of the 144 Windows runs. The Linux runs took two hours, mostly waiting, for 40 minutes of
  measurement.
- twig-rs: rustc 1.100.0 in `rust:1.100-slim-trixie`, the benchmarks built by `cargo bench --no-run` (release profile:
  `opt-level = 3`, no LTO), each tree in a target volume of its own, criterion 0.8.2 defaults (3 s warm-up, 5 s
  measurement, 100 samples). Before: `a258816`, checked out in a scratch worktree; after: `9e99e9d`. Every benchmark ran
  as a criterion run of its own (`<bench binary> --bench '^<id>$'`, a fresh process), the two trees one after the other
  with the order alternating between rounds, and the tables show the median of three rounds.
- PHP: the oracle image `twig-oracle:3.31.1` (PHP 8.5.13 NTS CLI, Twig 3.31.1). `bench.php --time=4` warms up for 0.8 s,
  then takes about 400 samples of about 10 ms each in 4 s and reports the median per iteration; it ran over everything
  once per round and configuration, as `run-php.sh` runs it. (Run per scenario in a process of its own, the JIT's
  medians moved by up to 15% either way, `loops` and `inheritance` slower and `empty_loop` faster: what the tracing JIT
  compiles depends on what ran before. Without the JIT the two ways agree within 3%.)
  - "PHP Twig": default CLI settings (OPcache off), Twig without a cache, so compiled templates are `eval()`'d once.
  - "OPcache + JIT": `opcache.enable_cli=1`, `opcache.jit=tracing`, `opcache.jit_buffer_size=128M`, and Twig's
    filesystem cache so that OPcache keeps the compiled templates (a typical production set-up).
- Both sides use Twig's default options (`autoescape: html`, `strict_variables: false`, `debug: false`) and the `UTC`
  timezone; contexts come from the same `data.json` files (the micro templates: `name` `'World'`, `a` `1`, `items` the
  integers 1 to 1000, `user` `{name: 'Ada'}`).
- Profiles: Linux `perf` in a privileged container on the same Docker host (`docker run --privileged`, Debian's
  `linux-perf` installed in `rust:1.100-slim-trixie`): `perf record -F 4000 -g` on the criterion binaries run with
  `--profile-time`, built with `RUSTFLAGS="-C force-frame-pointers=yes"` (v0 symbols read through `rustfilt`).
  Allocations were counted with a counting `#[global_allocator]` in a scratch harness. Neither is part of the
  repository.

## Reproducing

```bash
python research/benchmarks/gen_data.py          # only after changing the data generator
bash research/benchmarks/run-rust.sh            # criterion in rust:1.100-slim-trixie -> results/criterion/
bash research/benchmarks/run-php.sh --time=4    # bench.php in twig-oracle:3.31.1 -> results/php.jsonl, php-jit.jsonl
python research/benchmarks/compare.py           # the table above
```

`run-php.sh` passes its arguments to `bench.php` (`--time=<seconds>`, `--only=<name>`, `--dump=<dir>` needs a writable
mount); `run-rust.sh` takes a source directory and criterion arguments. Results vary by a few percent between runs (and
between processes: see the name hints under [Remaining hot spots](#remaining-hot-spots)); compare ratios, not absolute
times, across machines.

For a before/after table, run `run-rust.sh` on a copy of the earlier commit (`git archive <commit> | tar -x -C <dir>` or
a detached `git worktree add`) with a target volume of its own (`TWIG_BENCH_TARGET=twig-bench-before`) and keep a copy
of `results/` for `compare.py --before <dir>`. For medians over several rounds, keep a copy of `results/` after every
round of both sides and pass them all:
`compare.py after-1 after-2 after-3 --before before-1 --before before-2 --before before-3`. Without PHP results (on
Windows) the table leaves out the PHP columns. On a CPU with two kinds of cores, pin the benchmark processes to one kind
(on Windows `start /affinity <mask>`), or the rounds differ by tens of percent.

| file | role |
|---|---|
| `bench.php` | PHP counterpart of `benches/compile.rs` and `benches/render.rs` |
| `run-php.sh`, `run-rust.sh` | run both sides in containers on the same Docker host |
| `compare.py` | the comparison tables from criterion's estimates and `bench.php`'s output, medians over runs |
| `gen_data.py` | writes `benches/scenarios/*/data.json` |
