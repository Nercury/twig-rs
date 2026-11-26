# Changelog

All notable changes to twig-rs are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crates follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Every crate of the workspace has the same version.

## Unreleased — 0.2.0

The revival of twig-rs. The 2015–2017 work in progress (0.1: a lexer, an unfinished parser and an early compiler on
the `little` crate) was not usable; 0.2.0 is a new implementation whose target is to render templates exactly like
**Twig 3.31.1 on PHP 8.5.13**: the same output bytes, the same error classes and messages with template names and
lines, the same PHP warnings and deprecations, so that templates work copy-pasted from PHP projects. Deliberate and
known differences are listed in [`reference/deviations.md`](reference/deviations.md); anything else that differs is a
bug.

### Added

- The engine (crate `twig`): the lexer, the parser with the whole expression grammar and every core tag, the node
  visitors (correctness checks, auto-escaping with every strategy, the optimizer, the sandbox), a compiler to a
  low-level instruction set and a virtual machine with PHP's value semantics, every core filter, function and test,
  the sandbox, debug and string-loader extensions, and the array, filesystem and chain loaders. Data, filters,
  functions, tests, tags, extensions, loaders and security policies are Rust; one `Environment` serves many threads.
  PHP diagnostics (warnings, notices, deprecations) go to a policy of the environment instead of being printed.
- Conformance: all 500 runnable cases of Twig 3.31.1's integration fixtures pass, Twig's behaviour-level PHPUnit
  tests are ported, and the differences found by differential fuzzing against PHP are pinned by regression vectors.
- The crates under the engine, which reproduce the PHP layer Twig runs on:
  - `twig-value`: PHP 8.5's value model (null, bool, int, float, byte strings, ordered arrays, objects), its
    operators, comparisons and conversions, float formatting, and the PHP builtins Twig calls.
  - `twig-unicode`: Unicode 17.0 tables and mbstring-compatible case mapping and UTF-8 functions.
  - `twig-pcre`: PCRE2 10.44 as bundled with PHP (compiled from the vendored C sources, with the JIT) and PHP's
    `preg_match()` semantics, for the `matches` operator.
  - `twig-timelib`: a Rust port of timelib 2022.17 with timezonedb 2026.6.
  - `twig-date`: PHP's `DateTime`, `DateTimeImmutable`, `DateTimeZone`, `DateInterval` and `DatePeriod` and the date
    functions, on twig-timelib.
  - `twig-iconv` (optional, LGPL-2.1-or-later): glibc 2.41's `iconv()` for every charset it converts.
- Cargo features of `twig`:
  - `serde`: `twig::to_context` and `twig::to_value` turn anything that implements serde's `Serialize` into template
    data.
  - `glibc-iconv`: twig-iconv becomes the default charset converter of environments (the `convert_encoding` filter,
    escaping with a non-UTF-8 charset). Without it, environments use twig-value's clean-room converter for UTF-8,
    UTF-16, UTF-32, UCS-2, UCS-4, ASCII and ISO-8859-1, and other charsets fail with twig-rs's "not supported" error.
- The crate documentation is a guide (data and the `context!` macro, serde, autoescaping, filters, functions, tests,
  extensions and tags, loaders, the sandbox, errors and PHP diagnostics, threads), with runnable examples in
  `examples/`.
- `NOTICE` files listing the third-party code and data each crate contains and their licences.

### Changed

- Rust 2024 edition; the minimum supported Rust version is 1.88. Building needs a C compiler (PCRE2).
- Licensing: twig-rs remains BSD-3-Clause; each crate's `license` field adds the licences of the code it ports (PHP
  License 3.01, Zend Engine License 2.00 and others, see `NOTICE`). twig-timelib is MIT like timelib, and the
  optional twig-iconv is LGPL-2.1-or-later.

### Removed

- The 0.1 API and the `little` crate it compiled templates with.
