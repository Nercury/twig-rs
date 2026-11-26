# Benchmarks: twig-rs and PHP Twig

A one-off comparison of twig-rs with PHP Twig 3.31.1 on PHP 8.5.13, over the scenarios of the criterion benchmarks in
`benches/` (see `benches/README.md`), measured on 2026-11-26. Both engines produce byte-identical output for every
scenario (checked with `TWIG_BENCH_DUMP` and `bench.php --dump`).

## Summary

- **Compiling** (lexing, parsing, node visitors, code generation): twig-rs is **11-13.5x faster** than PHP Twig's
  `compileSource()`, and 7.5-8.5x faster than PHP with OPcache and the tracing JIT.
- **Rendering** with a warm template cache: twig-rs is **1.7-2.6x slower** than PHP Twig (on par for the escaping
  scenario), and 1.3-3.5x slower than PHP with OPcache and the JIT.
- The micro-benchmarks below locate most of the render gap: an auto-escaped `{{ value }}` costs about 190 ns more in
  twig-rs than the same print with `|raw`, against about 30 ns in PHP, and a plain `{% set %}` costs 45 ns against
  12 ns. Fixed per-render costs and literal text are cheaper in twig-rs.
- On Windows (native, system allocator) the same twig-rs code compiles and renders 1.9-3.4x slower than in the Linux
  container; with mimalloc as the allocator rendering is only 1.3-1.7x slower, so the render path allocates a lot.

The render numbers are the input for the profiling pass of `plans/product.md`; they describe the engine as of this
date, not a target.

## Results

Linux, both engines in containers on the same Docker host (medians per iteration; criterion for twig-rs, `bench.php`
for PHP):

| benchmark | twig-rs | PHP Twig | PHP Twig, OPcache + JIT | twig-rs vs PHP Twig | twig-rs vs OPcache + JIT |
|---|---:|---:|---:|---:|---:|
| compile/dates | 26.6 µs | 358.0 µs | 208.8 µs | 13.5x faster | 7.8x faster |
| compile/escaping | 35.3 µs | 451.2 µs | 267.2 µs | 12.8x faster | 7.6x faster |
| compile/filters | 120.5 µs | 1.47 ms | 968.3 µs | 12.2x faster | 8.0x faster |
| compile/inheritance | 126.9 µs | 1.44 ms | 954.2 µs | 11.3x faster | 7.5x faster |
| compile/loops | 52.4 µs | 634.6 µs | 409.5 µs | 12.1x faster | 7.8x faster |
| compile/macros | 71.5 µs | 864.7 µs | 609.7 µs | 12.1x faster | 8.5x faster |
| render/dates | 1.31 ms | 615.1 µs | 533.4 µs | 2.1x slower | 2.5x slower |
| render/escaping | 1.41 ms | 1.49 ms | 1.13 ms | 1.1x faster | 1.3x slower |
| render/filters | 1.57 ms | 609.9 µs | 498.1 µs | 2.6x slower | 3.1x slower |
| render/inheritance | 144.1 µs | 83.4 µs | 64.4 µs | 1.7x slower | 2.2x slower |
| render/loops | 1.32 ms | 650.6 µs | 374.4 µs | 2.0x slower | 3.5x slower |
| render/macros | 1.36 ms | 759.6 µs | 630.0 µs | 1.8x slower | 2.2x slower |

PHP's `compile` time does not include PHP compiling the generated class (an `eval()` or an OPcache miss), which a
template pays once more before its first render.

### Micro-benchmarks

One-off templates (not in `benches/scenarios/`), rendered the same way; `a` is `1`, `items` the integers 1 to 1000:

| template | twig-rs | PHP Twig | PHP Twig, OPcache + JIT |
|---|---:|---:|---:|
| `Hello {{ name }}!` | 0.8 µs | 1.3 µs | 1.2 µs |
| 1000 x `x{# c #}` (text only) | 7.3 µs | 27.6 µs | 22.3 µs |
| 1000 x `{{ 1 }}` | 37.1 µs | 33.7 µs | 25.8 µs |
| 1000 x `{{ a\|raw }}` | 44.9 µs | 38.4 µs | 34.6 µs |
| 1000 x `{{ a }}` (auto-escaped) | 235.4 µs | 68.0 µs | 54.2 µs |
| 1000 x `{% set x = a %}` | 45.5 µs | 12.0 µs | 11.1 µs |
| 1000 x `{% set x = a + 1 %}` | 64.3 µs | 42.8 µs | 11.0 µs |
| `{% for i in items %}{{ i }}{% endfor %}` | 325.1 µs | 86.1 µs | 62.4 µs |

- The escaper is the largest single cost. For an integer the `escape` filter returns the value unchanged, so the
  190 ns are call overhead: `escape()` in `src/extension/runtime/strings/escaper.rs` binds its four parameters with
  `Call::bind()` and checks three of them with `verify_user_arg()` on every print, where Twig compiles a direct
  `EscaperRuntime::escape()` call.
- Variable assignment and loop iterations cost about 3.8x PHP's. Literal output is close to PHP, and the addition
  itself (the difference between the two `set` rows) costs 19 ns against 31 ns in PHP without the JIT.

### Windows

The same twig-rs sources natively on the Windows host (rustc 1.100.0, x86_64-pc-windows-msvc), with the system
allocator and, as an experiment that is not part of the repository, with `mimalloc` as the global allocator of the
render benchmark:

| benchmark | Linux (container) | Windows | Windows, mimalloc |
|---|---:|---:|---:|
| compile/dates | 26.6 µs | 80.0 µs | - |
| compile/escaping | 35.3 µs | 96.5 µs | - |
| compile/filters | 120.5 µs | 339.3 µs | - |
| compile/inheritance | 126.9 µs | 350.0 µs | - |
| compile/loops | 52.4 µs | 153.8 µs | - |
| compile/macros | 71.5 µs | 133.5 µs | - |
| render/dates | 1.31 ms | 4.49 ms | 2.07 ms |
| render/escaping | 1.41 ms | 4.03 ms | 2.45 ms |
| render/filters | 1.57 ms | 4.12 ms | 2.52 ms |
| render/inheritance | 144.1 µs | 319.5 µs | 201.5 µs |
| render/loops | 1.32 ms | 3.42 ms | 1.93 ms |
| render/macros | 1.36 ms | 3.82 ms | 1.83 ms |

Windows results vary more between runs (up to 20%: `render/filters` measured 5.0 ms in an earlier run). mimalloc
removes 60-80% of the difference to Linux, which suggests many small allocations per rendered node; the rest is not
explained yet.

## Setup

- Machine: AMD Ryzen AI 9 HX 370 (12 cores, 24 threads), Windows 11 Pro, Docker Desktop 28.5 (WSL 2 backend, all 24
  threads). Load was not controlled beyond checking that the CPU was idle before the runs; expect a few percent of
  noise.
- twig-rs: rustc 1.100.0 in `rust:1.100-slim-trixie`, `cargo bench` (release profile: `opt-level = 3`, no LTO),
  criterion 0.8.2 defaults (3 s warm-up, 5 s measurement, 100 samples). The crate as of commit `2b04a2b` plus the
  benchmarks.
- PHP: the oracle image `twig-oracle:3.31.1` (PHP 8.5.13 NTS CLI, Twig 3.31.1). `bench.php --time=4` warms up for
  0.8 s, then takes about 400 samples of about 10 ms each in 4 s and reports the median per iteration.
  - "PHP Twig": default CLI settings (OPcache off), Twig without a cache, so compiled templates are `eval()`'d once.
  - "OPcache + JIT": `opcache.enable_cli=1`, `opcache.jit=tracing`, `opcache.jit_buffer_size=128M`, and Twig's
    filesystem cache so that OPcache keeps the compiled templates (a typical production set-up).
- Both sides use Twig's default options (`autoescape: html`, `strict_variables: false`, `debug: false`) and the `UTC`
  timezone; contexts come from the same `data.json` files.

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

| file | role |
|---|---|
| `bench.php` | PHP counterpart of `benches/compile.rs` and `benches/render.rs` |
| `run-php.sh`, `run-rust.sh` | run both sides in containers on the same Docker host |
| `compare.py` | the comparison table from criterion's estimates and `bench.php`'s output |
| `gen_data.py` | writes `benches/scenarios/*/data.json` |
