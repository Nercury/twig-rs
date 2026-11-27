# Research

Investigation projects and ongoing investigation notes. Conclusions that turn into facts move to `reference/`;
decisions move to `plans/` and then into code.

| Item | Purpose |
|---|---|
| [php-oracle/](php-oracle/) | Docker image with PHP 8.5.13 + Twig 3.31.1 that renders templates, evaluates PHP expressions, runs fixtures and Twig's own PHPUnit suite. The source of truth for behaviour. |
| [fuzzing.md](fuzzing.md) | Differential fuzzing against the oracle (`crates/twig-fuzz`): how a round is run and triaged, what the six rounds of roadmap phase 5 found, the vectors that stay marked and the harness's open gaps. |
| [benchmarks/](benchmarks/) | Criterion results against PHP Twig 3.31.1 (with and without OPcache/JIT) on the same Docker host, before/after the profiling passes, how to re-run the comparison, remaining hot spots. |
| [timelib-port/](timelib-port/) | How timelib 2022.17 was ported to pure Rust (re2rust scanners, embedded timezonedb), the C differential harness against PHP's own timelib, coverage and status. |
| [rust-ecosystem.md](rust-ecosystem.md) | Crate survey (2026-11), minijinja VM design notes and lessons for the LLTL VM, Rust 2026 conventions, toolchain and PCRE2 build checks. |
