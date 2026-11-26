# Benchmarks: twig-rs and PHP Twig

A comparison of twig-rs with PHP Twig 3.31.1 on PHP 8.5.13, over the scenarios and micro templates of the criterion
benchmarks in `benches/` (see `benches/README.md`), measured on 2026-11-26 before and after the first profiling pass
of the render path (twig-rs at `595fce0`, and with the commits of the pass). Both engines produce byte-identical
output for every scenario (checked with `TWIG_BENCH_DUMP` and `bench.php --dump`).

## Summary

- **Compiling** (lexing, parsing, node visitors, code generation): twig-rs is **11.5-13.5x faster** than PHP Twig's
  `compileSource()`, and 7.6-8.6x faster than PHP with OPcache and the tracing JIT (unchanged by the pass, within
  the noise).
- **Rendering** with a warm template cache: the profiling pass made rendering **1.9-3.4x faster** on Linux (2.0-3.8x
  on Windows). twig-rs now renders the escaping scenario 3.1x and the inheritance scenario 1.4x faster than PHP Twig,
  the macros scenario 1.1x faster, and the dates, filters and loops scenarios 1.1-1.2x slower; against PHP with
  OPcache and the JIT it is 2.5x faster for escaping, 1.1x faster for inheritance and 1.1-1.5x slower for the rest.
- Allocations per render fell by 63-96% (escaping: from 23,702 to 1,521). On Windows, where the system allocator is
  slow, mimalloc made rendering 1.3-1.6x faster before the pass and 1.05-1.4x after it.
- An auto-escaped `{{ value }}` now costs about 76 ns more than the same print with `|raw` (193 ns before, about 33 ns
  in PHP). What remains of the gap is listed under [Remaining hot spots](#remaining-hot-spots): the interpreter loop,
  atomic reference counts, `for` loop set-up, the date objects and markup creation.

## Results

Linux, both engines in containers on the same Docker host (medians per iteration; criterion for twig-rs, `bench.php`
for PHP; "before" is twig-rs at `595fce0`, the other twig-rs columns are the code after the pass):

| benchmark | twig-rs before | twig-rs | speed-up | PHP Twig | PHP Twig, OPcache + JIT | twig-rs vs PHP Twig | twig-rs vs OPcache + JIT |
|---|---:|---:|---:|---:|---:|---:|---:|
| compile/dates | 26.4 µs | 26.5 µs | 1.00x | 360.3 µs | 215.5 µs | 13.6x faster | 8.1x faster |
| compile/escaping | 33.3 µs | 35.6 µs | 0.94x | 437.4 µs | 273.9 µs | 12.3x faster | 7.7x faster |
| compile/filters | 120.7 µs | 130.5 µs | 0.92x | 1.51 ms | 985.7 µs | 11.6x faster | 7.6x faster |
| compile/inheritance | 126.3 µs | 122.9 µs | 1.03x | 1.49 ms | 984.0 µs | 12.1x faster | 8.0x faster |
| compile/loops | 59.9 µs | 52.6 µs | 1.14x | 654.9 µs | 414.7 µs | 12.4x faster | 7.9x faster |
| compile/macros | 81.9 µs | 69.2 µs | 1.18x | 920.5 µs | 596.2 µs | 13.3x faster | 8.6x faster |
| micro/attribute | 384.8 µs | 151.5 µs | 2.54x | 188.9 µs | 130.8 µs | 1.2x faster | 1.2x slower |
| micro/empty_loop | 1.00 ms | 579.6 µs | 1.73x | 163.5 µs | 179.9 µs | 3.5x slower | 3.2x slower |
| micro/filter | 563.2 µs | 168.8 µs | 3.34x | 106.3 µs | 79.8 µs | 1.6x slower | 2.1x slower |
| micro/for | 344.3 µs | 139.8 µs | 2.46x | 82.4 µs | 63.4 µs | 1.7x slower | 2.2x slower |
| micro/hello | 0.9 µs | 0.6 µs | 1.37x | 1.4 µs | 1.1 µs | 2.2x faster | 1.7x faster |
| micro/macro | 1.08 ms | 694.1 µs | 1.56x | 883.1 µs | 842.5 µs | 1.3x faster | 1.2x faster |
| micro/print_escaped | 242.2 µs | 97.0 µs | 2.50x | 66.1 µs | 46.0 µs | 1.5x slower | 2.1x slower |
| micro/print_int | 39.7 µs | 14.5 µs | 2.74x | 27.5 µs | 23.8 µs | 1.9x faster | 1.6x faster |
| micro/print_raw | 48.8 µs | 20.8 µs | 2.35x | 33.3 µs | 28.7 µs | 1.6x faster | 1.4x faster |
| micro/set | 46.4 µs | 27.1 µs | 1.72x | 10.9 µs | 9.9 µs | 2.5x slower | 2.7x slower |
| micro/set_add | 67.6 µs | 48.4 µs | 1.40x | 11.2 µs | 10.8 µs | 4.3x slower | 4.5x slower |
| micro/text | 7.9 µs | 6.6 µs | 1.19x | 24.2 µs | 21.2 µs | 3.7x faster | 3.2x faster |
| render/dates | 1.36 ms | 698.1 µs | 1.94x | 598.0 µs | 553.8 µs | 1.2x slower | 1.3x slower |
| render/escaping | 1.51 ms | 450.1 µs | 3.37x | 1.40 ms | 1.14 ms | 3.1x faster | 2.5x faster |
| render/filters | 1.55 ms | 776.8 µs | 1.99x | 641.1 µs | 531.6 µs | 1.2x slower | 1.5x slower |
| render/inheritance | 122.5 µs | 58.6 µs | 2.09x | 83.2 µs | 64.3 µs | 1.4x faster | 1.1x faster |
| render/loops | 1.27 ms | 521.9 µs | 2.43x | 461.0 µs | 373.4 µs | 1.1x slower | 1.4x slower |
| render/macros | 1.35 ms | 677.4 µs | 1.99x | 738.6 µs | 631.3 µs | 1.1x faster | 1.1x slower |

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

Their rows are in the table above. Per construct, after the pass:

- An auto-escaped print costs about 76 ns more than the same print with `|raw` (193 ns before; about 33 ns in PHP,
  17 ns with the JIT): the escape filter is now called with its arguments as they are and returns an int or a string
  that needs no escaping as it is, but the call is still eleven instructions.
- Literal text, prints, fixed per-render costs (`hello`) and macro calls are cheaper than in PHP, also with the JIT;
  attribute reads are cheaper than in PHP Twig and slightly dearer than with the JIT.
- `{% set %}` costs about 27 ns (11 ns in PHP), an iteration of a `for` loop that prints its value about 140 ns
  (82 ns), and setting up an empty loop about 580 ns (164 ns): see [Remaining hot spots](#remaining-hot-spots).

### Windows

The same sources natively on the Windows host (rustc 1.100.0, x86_64-pc-windows-msvc, criterion defaults), with the
system allocator and, as an experiment that is not part of the repository, with `mimalloc` 0.1.52 as the global
allocator of the render benchmark:

| benchmark | before | after | speed-up | before, mimalloc | after, mimalloc | speed-up |
|---|---:|---:|---:|---:|---:|---:|
| render/dates | 3.18 ms | 1.51 ms | 2.10x | 2.00 ms | 1.08 ms | 1.84x |
| render/escaping | 2.81 ms | 740.1 µs | 3.80x | 2.08 ms | 707.7 µs | 2.93x |
| render/filters | 2.96 ms | 1.39 ms | 2.13x | 2.19 ms | 1.09 ms | 2.00x |
| render/inheritance | 235.6 µs | 104.4 µs | 2.26x | 169.6 µs | 86.0 µs | 1.97x |
| render/loops | 2.30 ms | 785.0 µs | 2.93x | 1.79 ms | 745.3 µs | 2.41x |
| render/macros | 2.62 ms | 1.31 ms | 2.00x | 1.90 ms | 1.02 ms | 1.86x |
| micro/attribute | 738.6 µs | 215.4 µs | 3.43x | 575.0 µs | 219.1 µs | 2.62x |
| micro/empty_loop | 1.97 ms | 936.8 µs | 2.10x | 1.43 ms | 827.1 µs | 1.73x |
| micro/filter | 1.23 ms | 278.4 µs | 4.41x | 733.5 µs | 253.0 µs | 2.90x |
| micro/for | 700.4 µs | 199.9 µs | 3.50x | 496.2 µs | 212.8 µs | 2.33x |
| micro/hello | 2.1 µs | 1.5 µs | 1.44x | 1.1 µs | 0.9 µs | 1.22x |
| micro/macro | 2.51 ms | 1.27 ms | 1.99x | 1.59 ms | 1.01 ms | 1.58x |
| micro/print_escaped | 497.3 µs | 134.1 µs | 3.71x | 361.1 µs | 140.1 µs | 2.58x |
| micro/print_int | 113.3 µs | 21.8 µs | 5.21x | 57.0 µs | 21.5 µs | 2.66x |
| micro/print_raw | 121.5 µs | 28.9 µs | 4.20x | 65.5 µs | 28.0 µs | 2.34x |
| micro/set | 91.8 µs | 36.9 µs | 2.49x | 68.7 µs | 37.4 µs | 1.84x |
| micro/set_add | 130.0 µs | 63.6 µs | 2.05x | 95.4 µs | 65.2 µs | 1.46x |
| micro/text | 12.2 µs | 10.9 µs | 1.11x | 11.3 µs | 9.7 µs | 1.16x |

Windows results vary more between runs than the container's: the CPU mixes fast and compact cores, and one run of
`render/macros` measured 0.74 ms where the run in the table measured 1.31 ms. Before the pass, mimalloc made rendering
1.3-1.6 times faster, which pointed at many small allocations per rendered node; after it, 1.05-1.4 times, and the
render path allocates far less (allocations per render of each scenario, counted with a counting global allocator in
a scratch harness, before / after):

| scenario | allocations before | allocations after | bytes allocated before | bytes allocated after |
|---|---:|---:|---:|---:|
| dates | 32,326 | 11,820 | 1.46 MB | 0.60 MB |
| escaping | 23,702 | 1,521 | 1.55 MB | 0.28 MB |
| filters | 24,477 | 8,151 | 2.24 MB | 0.91 MB |
| inheritance | 1,813 | 408 | 176 KB | 74 KB |
| loops | 18,839 | 826 | 1.68 MB | 0.36 MB |
| macros | 21,261 | 4,843 | 2.25 MB | 0.95 MB |

twig-rs does not choose an allocator for its host. A host that renders a lot on Windows can still gain from making
mimalloc (or another fast allocator) its global allocator.

## The profiling pass

Profiled with `perf` (software clock sampling in a privileged container, binaries built with frame pointers) and a
counting global allocator in a scratch harness, then optimised without changing behaviour (the fixture suite, all
tests and the fuzz regression vectors stay green; every fast path below falls back to the general code whenever its
result could differ). By area:

- **Calls of filters and functions**: argument vectors are reused within a render; the `escape` filter takes its four
  arguments as they are when they have the declared types (no `Call::bind()`, no `verify_user_arg()`, no parameter
  list built per call) and returns strings that a strategy leaves unchanged as they are; the core callables bind
  their arguments without allocating when PHP's binding cannot fail or reorder anything (no named arguments, enough
  positional ones), and `CallArgs::bind()` skips its slot vector for positional-only calls; closures are invoked
  without resolving them through the environment; method lookups no longer allocate the lowercase name.
- **Arrays**: large arrays index their entries with stored hashes and an open-addressing table of positions, so a
  copy-on-write copy is two `memcpy`s instead of re-hashing and cloning every key; shared arrays are iterated in place
  instead of copied; the variable names of a program are prepared once (normalised and hashed with the process-wide
  key seed) and looked up and stored without building or hashing keys.
- **Interpreter**: instructions are dispatched inside the interpreter loop; the vectors of a frame (operand stack,
  argument lists, saved macros) are reused; prints write strings and ints without converting them; existing array
  elements are read by `getAttribute()` with one borrowed lookup; `loop` fields are updated through prepared keys and
  without copying `$context['loop']`; the context after a `for` loop is restored in place when no int key is involved.
- **Strings**: concatenations, int strings and `rawurlencode()` results are built in one allocation; well-formed
  UTF-8 is escaped by `htmlspecialchars()` in one pass; the `js`, `css` and `html_attr` strategies write their hex
  escapes without formatting a string per character.
- **Markup and macros**: `Twig\Markup` checks its strategies and converts to a string without dynamic method calls and
  shares its default strategy list; positional macro arguments bind without a slot vector.
- **Dates**: the date filters construct, convert and format their own `DateTime` objects without dynamic method calls;
  date formats write their numbers directly; date methods name themselves in messages without formatting the name.

### Profiling

The profiles of this pass came from Linux `perf` in a privileged container on the same Docker host (`docker run
--privileged`, Debian's `linux-perf` installed in `rust:1.100-slim-trixie`): WSL 2 exposes no hardware counters, but
`perf record -F 4000 -g` samples the CPU clock, and building with `RUSTFLAGS="-C force-frame-pointers=yes"` gives
usable call graphs (v0 symbols need `rustfilt` to read). A scratch harness rendering a scenario in a loop served as
the profiled program, and the same harness with a counting `#[global_allocator]` reported allocations per render and
their call sites. Neither is part of the repository.

## Remaining hot spots

From the profiles after the pass (Linux, `perf`), in order of weight:

- **The interpreter loop** (20-28% self time in the `loops`, `macros` and `filters` profiles): each instruction costs
  roughly 5-10 ns of dispatch, operand-stack traffic and `Result`-based control flow, and simple statements take
  several instructions (`{% set x = a + 1 %}` is four; an auto-escaped print is eleven plus a call). Instruction
  fusion would need new `Instr` variants (a public type). Recording the current instruction only when its line run
  changes was measured and gave nothing.
- **Atomic reference counts**: strings and arrays are `Arc`s, so pushing a string constant or reading a string or
  array variable costs an atomic increment and later a decrement (about 8.6 ns per pair on this machine), and every
  array write goes through `Arc::make_mut` (about 4.7 ns): every context write (`set`, the loop variables) pays it.
  PHP's reference counts are plain integers. This is most of the difference in the `set` and `set_add` rows.
- **`for` loops**: setting up a loop copies the context into `_parent` (as PHP does) and inserts and removes the loop
  variables; an empty loop costs about 3.5 times PHP's. The context copy is a copy of the entry vector with an atomic
  increment per key and value.
- **Date filters**: what remains is `twig-date` itself: `DateTime::modify()` and `setTimezone()` on immutable dates
  still go through dynamic method calls with argument binding (their implementations are private to `twig-date`),
  then timelib parses `@<timestamp>` and computes timezone transitions on every call.
- **Markup** (macros, captures, `include()`): creating a `Twig\Markup` builds a `PropertyStore` (a lock and a slot
  vector) and reading its content locks it again, about 0.2 µs per object; the captured output is collected in a
  buffer of `Output` (`src/vm/output.rs`, not changed in this pass) and copied into the string.
- **Per-render fixed costs**: `Environment::load()` hashes the template name with SipHash and builds class keys on
  every render (visible in tiny templates such as `hello`); `src/runtime/environment.rs` was not changed in this pass.
- **Windows**: the system allocator is still slower than glibc's for what remains (mostly the strings that templates
  build: escaped output, filter results); hosts that render a lot on Windows can make `mimalloc` their global
  allocator (twig-rs does not choose an allocator).

## Setup

- Machine: AMD Ryzen AI 9 HX 370 (12 cores, 24 threads), Windows 11 Pro, Docker Desktop 28.5 (WSL 2 backend, all 24
  threads). Load was not controlled beyond checking that the CPU was idle before the runs; other work ran on the
  machine at times, so expect 5-10% of noise between runs (the before and after runs ran back to back).
- twig-rs: rustc 1.100.0 in `rust:1.100-slim-trixie`, `cargo bench` (release profile: `opt-level = 3`, no LTO),
  criterion 0.8.2 defaults (3 s warm-up, 5 s measurement, 100 samples). Before: commit `595fce0` with the benchmarks
  of the pass (in a target volume of its own); after: the last commit of the pass.
- PHP: the oracle image `twig-oracle:3.31.1` (PHP 8.5.13 NTS CLI, Twig 3.31.1). `bench.php --time=4` warms up for
  0.8 s, then takes about 400 samples of about 10 ms each in 4 s and reports the median per iteration.
  - "PHP Twig": default CLI settings (OPcache off), Twig without a cache, so compiled templates are `eval()`'d once.
  - "OPcache + JIT": `opcache.enable_cli=1`, `opcache.jit=tracing`, `opcache.jit_buffer_size=128M`, and Twig's
    filesystem cache so that OPcache keeps the compiled templates (a typical production set-up).
- Both sides use Twig's default options (`autoescape: html`, `strict_variables: false`, `debug: false`) and the `UTC`
  timezone; contexts come from the same `data.json` files (the micro templates: `name` `'World'`, `a` `1`, `items` the
  integers 1 to 1000, `user` `{name: 'Ada'}`).

## Reproducing

```bash
python research/benchmarks/gen_data.py      # only after changing the data generator
bash research/benchmarks/run-rust.sh        # criterion in rust:1.100-slim-trixie -> results/criterion/
bash research/benchmarks/run-php.sh         # bench.php in twig-oracle:3.31.1 -> results/php.jsonl, php-jit.jsonl
python research/benchmarks/compare.py       # the table above
```

`run-php.sh` passes its arguments to `bench.php` (`--time=<seconds>`, `--only=<scenario>`, `--dump=<dir>` needs a
writable mount); `run-rust.sh` takes a source directory and criterion arguments. Results vary by a few percent between
runs; compare ratios, not absolute times, across machines.

For a before/after table, run `run-rust.sh` on a copy of the earlier commit (`git archive <commit> | tar -x -C <dir>`)
with a target volume of its own (`TWIG_BENCH_TARGET=twig-bench-before`), keep a copy of `results/`, run it again on
the current tree and pass the copy to `compare.py --before <dir>`.

| file | role |
|---|---|
| `bench.php` | PHP counterpart of `benches/compile.rs` and `benches/render.rs` |
| `run-php.sh`, `run-rust.sh` | run both sides in containers on the same Docker host |
| `compare.py` | the comparison table from criterion's estimates and `bench.php`'s output |
| `gen_data.py` | writes `benches/scenarios/*/data.json` |
