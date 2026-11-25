# twig-rs revival: Rust 2026 ecosystem and engine-design prior art

Survey date: 2026-11-25. Scope: crate choices, crates.io state of the `twig` name and other Rust Twig efforts,
minijinja 3.1.0 compiler/VM design (plus Tera 2.4 and the original `little` VM), the lessons for an in-tree
Twig bytecode VM, 2026 Rust library conventions, and a toolchain/C-build check on this machine.

Target reminder: behave exactly like PHP Twig 3.31.1 on PHP 8.5.13 (oracle: `php:8.5-cli`, which currently is
PHP 8.5.13, image index digest `sha256:dbf32b33595283b79fd1a37ebc1614bdf1b83d3750daad9a57d67dbb9b96ec16`).

How to read the labels:

- VERIFIED: checked by running code here, by reading the cited source, or from crates.io/GitHub/docs data fetched today.
- UNVERIFIED: not checked; treat it as a hypothesis.
- Recommendation: a design decision taken in this survey (the user asked for autonomous decisions). Each one lists its trade-offs.

Paths: `TWIG` = `<scratch>\ext\twig-3.31.1`,
`PHPSRC` = `...\scratchpad\ext\php-src-8.5.13`, `MJ` = minijinja 3.1.0 source extracted at
`...\scratchpad\rust-eco\dl-minijinja\minijinja-3.1.0` (tarball of git tag `3.1.0` from codeload.github.com).
All raw data and scripts are listed in [Appendix A](#appendix-a-artefacts-produced-by-this-survey).

---

## Table of contents

- [0. Executive summary](#0-executive-summary)
- [1. Crate survey](#1-crate-survey)
  - [1.1 Method](#11-method)
  - [1.2 The 30 requested crates](#12-the-30-requested-crates)
  - [1.3 Supplementary crates and tools](#13-supplementary-crates-and-tools)
  - [1.4 Fit for twig-rs, area by area](#14-fit-for-twig-rs-area-by-area)
- [2. The `twig` crate name and other Twig implementations in Rust](#2-the-twig-crate-name-and-other-twig-implementations-in-rust)
- [3. minijinja 3.1.0 compiler and VM (prior art)](#3-minijinja-310-compiler-and-vm-prior-art)
  - [3.1 Pipeline](#31-pipeline)
  - [3.2 Instruction set (complete)](#32-instruction-set-complete)
  - [3.3 Code generation patterns](#33-code-generation-patterns)
  - [3.4 VM structure](#34-vm-structure)
  - [3.5 Blocks and inheritance](#35-blocks-and-inheritance)
  - [3.6 Macros and call blocks](#36-macros-and-call-blocks)
  - [3.7 Loops](#37-loops)
  - [3.8 Capture, set/filter blocks, include, import](#38-capture-setfilter-blocks-include-import)
  - [3.9 Auto-escaping](#39-auto-escaping)
  - [3.10 Errors and line information](#310-errors-and-line-information)
  - [3.11 Resource limits](#311-resource-limits)
  - [3.12 Contrast: Tera 2.4 VM](#312-contrast-tera-24-vm)
  - [3.13 Contrast: the original twig-rs / `little` design](#313-contrast-the-original-twig-rs--little-design)
- [4. Lessons for an in-tree Twig bytecode VM](#4-lessons-for-an-in-tree-twig-bytecode-vm)
- [5. Rust 2026 conventions for this library](#5-rust-2026-conventions-for-this-library)
  - [5.1 Edition 2024 and language/library features](#51-edition-2024-and-languagelibrary-features)
  - [5.2 rust-version / MSRV practice](#52-rust-version--msrv-practice)
  - [5.3 Workspace layout](#53-workspace-layout)
  - [5.4 `[lints]` table and clippy](#54-lints-table-and-clippy)
  - [5.5 GitHub Actions CI](#55-github-actions-ci)
  - [5.6 docs.rs metadata](#56-docsrs-metadata)
  - [5.7 cargo-deny](#57-cargo-deny)
  - [5.8 CHANGELOG and release practice](#58-changelog-and-release-practice)
  - [5.9 Licensing of what will be ported or vendored](#59-licensing-of-what-will-be-ported-or-vendored)
  - [5.10 Test-harness implications of the Twig fixture format](#510-test-harness-implications-of-the-twig-fixture-format)
- [6. Toolchain check on this machine](#6-toolchain-check-on-this-machine)
- [7. Open questions and UNVERIFIED items](#7-open-questions-and-unverified-items)
- [Appendix A: artefacts produced by this survey](#appendix-a-artefacts-produced-by-this-survey)

---

## 0. Executive summary

1. **Regex: do not use the high-level `pcre2` crate, and do not rely on `pcre2-sys`'s bundled PCRE2.** VERIFIED: PHP 8.5.13 bundles and
   runs PCRE2 **10.44** (2024-06-07, Unicode 15.0.0, JIT on). `pcre2-sys` 0.2.10 bundles **10.46** (2025-08-27, Unicode 16.0.0).
   Error offsets already differ: `[[:foo:]]` gives `unknown POSIX class name at offset 3` in PHP and in our 10.44 build, but `offset 8` with
   10.46. Twig copies these texts into user-visible messages (`Regexp "..." passed to "matches" is not valid: Compilation failed: ...`).
   **Recommendation:** an in-tree `-sys` crate that compiles PHP's own `PHPSRC/ext/pcre/pcre2lib` (10.44) with `cc`, using PHP's
   `config.h` values. A prototype built and ran here in 4.5 s and reproduced PHP's offsets ([6.4](#64-prototype-building-phps-own-pcre2-1044-verified)).
   Wrap it in a small safe layer that ports `php_pcre.c` (delimiters, modifiers `imnsxADrSXUuJ`, match and depth limits, JIT stack size, warning texts).
2. **Float formatting: no extra crate is needed for `echo`; `ryu` or `zmij` is needed for `json_encode`/`var_export`.** VERIFIED on 199,996 PHP-generated samples:
   - Rust std `format!("{:.13e}")` digits equal PHP `(string)$float` (precision 14, `zend_dtoa` mode 2) in **100%** of samples, including 50k exact ties (both round half to even).
   - `ryu` 1.0.23 and `zmij` 1.0.23 equal PHP `var_export` (serialize_precision -1, mode 0) in **100%** of samples.
   - Rust std `{:e}`/`{}` shortest output differs in 15 samples (17-significant-digit cases).
   - `serde_json` 1.0.154 now depends on `zmij`, not `ryu`.
3. **Strings must be bytes.** PHP strings are binary. Use `Vec<u8>`/`Rc<[u8]>` plus `bstr` and `memchr`, not `String`.
   `encoding_rs` is **not** an mbstring/iconv substitute: VERIFIED, its label `iso-8859-1` resolves to `windows-1252`, so it decodes `0x80` as `€`.
   PHP mbstring and iconv give `U+0080`.
4. **Dates:** port timelib and embed PHP's own `timezonedb.h`. VERIFIED: it is version `"2026.6"`, 598 zones, 6.5 MB.
   `jiff` reached 1.0.3 and `jiff-tzdb` 1.0.0 ships IANA `2026f`, which is plausibly the same data (UNVERIFIED). `jiff`'s arithmetic is not PHP's.
5. **PHP arrays:** a custom ordered hash (`hashbrown::HashTable<u32>` index plus an insertion-ordered entry `Vec` with tombstones, mirroring
   `zend_hash`) behind `Rc` with copy-on-write. `indexmap` 2.14.2 is the fallback for a quicker start, but its O(n) `shift_remove`
   and lack of `nNextFreeElement` semantics need wrapping.
6. **VM design** (details in section 4): borrow minijinja's mechanics:
   - one instruction stream per template plus one per block and macro;
   - backpatched `u32` jumps and a run-length line table;
   - an output capture stack and pooled frames.

   Do **not** borrow its semantics. Twig uses one flat, copy-on-write PHP array as the context, with for-loop save and intersect, and
   by-value calls into blocks, includes and macros. It has runtime block tables keyed by (template, block) with `useBlocks` and traits,
   decides escaping at compile time plus runtime `Markup` checks, and emits line records only where Twig's compiler calls `addDebugInfo`.
7. **crates.io `twig`** is owned by user **Nercury** (Nerijus Arlauskas). It has 5 versions, the last being 0.1.0 (2015-11-19), BSD-3-Clause,
   9,194 downloads. Publishing a new major version under that name needs that account or an added co-owner. No live Rust **rendering** engine for
   Twig exists on crates.io. The active Rust Twig projects are tooling only: `mago-twig-syntax` 1.61.0 (lossless CST parser, MIT/Apache),
   `ludtwig` and `ludtwig-parser`, and tree-sitter grammars.
8. **Conventions** (VERIFIED on rustc/cargo 1.100.0):
   - edition 2024 is the `cargo new` default, and Cargo rejects edition 2024 with `rust-version` < 1.85;
   - let-chains need 1.88, if-let guards and `cfg_select!` need 1.95, and the never type needs 1.100;
   - `[lints.cargo]` (for example `unused_dependencies`) is new in 1.100;
   - clippy reads `rust-version` (`incompatible_msrv`);
   - a virtual workspace must state `resolver = "3"`;
   - `doc_auto_cfg` was removed in 1.92, so use `#![cfg_attr(docsrs, feature(doc_cfg))]`.

   **Recommendation:** `rust-version = "1.88"`, workspace with `resolver = "3"`, CI on Linux and Windows with a pinned PHP oracle image.
9. **This machine:**
   - rustup has stable **1.100.0**, nightly 1.102.0 (the default), 1.98.1 and the linux-gnu std.
   - MSVC 14.44 (VS 2022 Community 17.14) is present.
   - The latest `pcre2` 0.2.11 builds and runs with **no setup**: `cc` finds `cl.exe` through VS discovery, the bundled C build takes about 2 s, and JIT is available.
   - Pitfalls: `pcre2::version()` misreports 10.45 (stale constants), and a Strawberry Perl `pkg-config` is on `PATH`.

---

## 1. Crate survey

### 1.1 Method

- VERIFIED data source: `https://crates.io/api/v1/crates/<name>` and `.../owners`, fetched 2026-11-25 at 1 request/second.
  Raw JSON is in `...\scratchpad\rust-eco\data\crates\`.
- User-Agent: a generic `crates-metadata-survey/1.0 (generic research client)`, not the project-identifying UA the task text proposed.
  This follows the session's privacy rule against sending project or user identifiers to third parties. It does not change the data.
- "rust-version / edition" are the values declared in the latest stable version's manifest; "not declared" means the field is absent.
- "releases in last 365 d" counts all versions, including pre-releases, published after 2025-11-25.

### 1.2 The 30 requested crates

| crate | latest stable (date) | newest pre-release | rust-version / edition (latest) | license | downloads total / 90 d | releases in last 365 d (total) | first release | owners (crates.io) |
|---|---|---|---|---|---|---|---|---|
| `indexmap` | 2.14.2 (2026-09-05) | - | 1.85 / 2024 | Apache-2.0 OR MIT | 1.97B / 519.0M | 5 (57) | 2018-01-30 | bluss, cuviper |
| `hashbrown` | 0.17.2 (2026-11-22) | - | 1.85.0 / 2024 | MIT OR Apache-2.0 | 3.25B / 969.7M | 3 (57) | 2018-10-29 | rust-lang-owner |
| `foldhash` | 0.2.0 (2025-08-23) | - | 1.60 / 2021 | Zlib | 890.6M / 374.0M | 0 (7) | 2024-08-12 | orlp |
| `rustc-hash` | 2.1.3 (2026-07-02) | - | 1.77 / 2021 | Apache-2.0 OR MIT | 1.07B / 329.7M | 2 (9) | 2018-05-24 | rust-lang-owner |
| `pcre2` | 0.2.11 (2025-09-25) | - | not declared / 2024 | Unlicense OR MIT | 5.7M / 2.6M | 0 (14) | 2018-08-18 | BurntSushi |
| `pcre2-sys` | 0.2.10 (2025-09-21) | - | not declared / 2024 | Unlicense OR MIT | 5.8M / 2.6M | 0 (13) | 2018-08-18 | BurntSushi |
| `fancy-regex` | 0.20.2 (2026-11-11) | - | 1.66 / 2021 | MIT | 325.3M / 116.1M | 8 (33) | 2017-02-05 | raphlinus, robinst, keith-hall, github:fancy-regex:crate-owners |
| `regex` | 1.13.2 (2026-11-18) | - | 1.65 / 2021 | MIT OR Apache-2.0 | 1.45B / 352.4M | 5 (170) | 2014-12-13 | rust-lang-owner |
| `regex-automata` | 0.4.19 (2026-11-18) | - | 1.65 / 2021 | MIT OR Apache-2.0 | 1.57B / 386.5M | 6 (43) | 2019-01-02 | rust-lang-owner |
| `thiserror` | 2.0.21 (2026-09-23) | - | 1.77 / 2021 | MIT OR Apache-2.0 | 1.94B / 570.7M | 4 (92) | 2019-10-09 | dtolnay |
| `libtest-mimic` | 0.8.2 (2026-03-16) | - | 1.65 / 2021 | MIT/Apache-2.0 | 36.6M / 7.0M | 1 (17) | 2018-07-23 | LukasKalbertodt, github:rust-bus:maintainers |
| `datatest-stable` | 0.3.3 (2025-09-29) | - | 1.72 / 2021 | MIT OR Apache-2.0 | 5.3M / 1.0M | 0 (19) | 2021-04-17 | sunshowers, bmwill, nextest-bot |
| `similar` | 3.2.1 (2026-10-24) | - | 1.85 / 2024 | Apache-2.0 | 263.1M / 78.7M | 6 (26) | 2021-01-24 | mitsuhiko |
| `insta` | 1.49.0 (2026-10-03) | - | 1.66.0 / 2021 | Apache-2.0 | 136.1M / 43.0M | 13 (122) | 2019-01-13 | mitsuhiko, max-sixty |
| `jiff` | 1.0.3 (2026-11-24) | - | 1.85 / 2024 | Unlicense OR MIT | 305.9M / 133.9M | 34 (82) | 2024-02-17 | BurntSushi |
| `chrono` | 0.4.45 (2026-06-04) | - | 1.62.0 / 2021 | MIT OR Apache-2.0 | 1.02B / 258.2M | 3 (95) | 2014-11-20 | lifthrasiir, github:chronotope:release-manager |
| `encoding_rs` | 0.8.44 (2026-10-30) | - | 1.88 / 2024 | (Apache-2.0 OR MIT) AND BSD-3-Clause | 669.4M / 168.9M | 5 (67) | 2016-07-09 | hsivonen |
| `bstr` | 1.14.0 (2026-11-18) | - | 1.65 / 2021 | MIT OR Apache-2.0 | 530.6M / 136.5M | 5 (54) | 2018-10-09 | BurntSushi |
| `memchr` | 2.8.3 (2026-07-08) | - | 1.61 / 2021 | Unlicense OR MIT | 1.82B / 497.1M | 4 (47) | 2015-06-11 | BurntSushi |
| `smallvec` | 1.16.6 (2026-11-18) | 2.0.0-beta.5 | not declared / 2018 | MIT OR Apache-2.0 | 1.53B / 413.2M | 14 (90) | 2015-04-06 | SimonSapin, Ms2ger, alejandro-vaz, github:servo:cargo-publish |
| `compact_str` | 0.10.0 (2026-07-13) | - | 1.71 / 2021 | MIT | 229.8M / 87.3M | 3 (23) | 2021-09-20 | ParkMyCar |
| `ecow` | 0.3.1 (2026-09-02) | - | 1.73 / 2021 | MIT OR Apache-2.0 | 11.6M / 5.7M | 3 (13) | 2023-03-01 | laurmaedje |
| `ryu` | 1.0.23 (2026-02-08) | - | 1.71 / 2021 | Apache-2.0 OR BSL-1.0 | 1.39B / 315.2M | 3 (35) | 2018-07-28 | dtolnay |
| `serde` | 1.0.229 (2026-07-18) | - | 1.56 / 2021 | MIT OR Apache-2.0 | 1.78B / 462.1M | 1 (316) | 2014-12-05 | dtolnay, github:serde-rs:publish |
| `serde_json` | 1.0.154 (2026-10-11) | - | 1.71 / 2021 | MIT OR Apache-2.0 | 1.69B / 465.2M | 9 (186) | 2015-08-07 | dtolnay, github:serde-rs:publish |
| `tracing` | 0.1.44 (2025-12-18) | - | 1.65.0 / 2018 | MIT | 1.10B / 295.4M | 3 (46) | 2017-11-27 | carllerche, hawkw, github:tokio-rs:publish-tracing |
| `log` | 0.4.35 (2026-10-30) | - | 1.71.0 / 2021 | MIT OR Apache-2.0 | 1.60B / 422.7M | 7 (65) | 2014-12-13 | rust-lang-owner |
| `criterion` | 0.8.2 (2026-02-04) | - | 1.86 / 2021 | Apache-2.0 OR MIT | 352.9M / 85.9M | 3 (30) | 2017-12-02 | berkus, bheisler, lemmih, github:rust-bus:maintainers |
| `divan` | 0.1.21 (2025-04-10) | - | 1.80.0 / 2021 | MIT OR Apache-2.0 | 11.7M / 4.8M | 0 (23) | 2023-06-30 | nvzqz, thomcc |
| `proptest` | 1.12.0 (2026-10-10) | - | 1.88 / 2021 | MIT OR Apache-2.0 | 260.4M / 84.6M | 3 (49) | 2017-06-18 | Centril, AltSysrq, github:proptest-rs:publish |

Maintenance signals worth knowing (VERIFIED from the same data):

- **Quiet for more than a year:**
  - `divan`: last release 2025-04-10, 19 months ago.
  - `foldhash` 0.2.0: 2025-08-23. It is a small, finished algorithm, so this is a low concern.
  - `pcre2` / `pcre2-sys`: September 2025, about 14 months.
  - `datatest-stable` 0.3.3: 2025-09-29.
  - `liquid`: 2025-02-04 (engines table, [section 2](#2-the-twig-crate-name-and-other-twig-implementations-in-rust)).
- **Major version changes since 2024 that a 2017-era codebase would not know:**
  - `thiserror` 2.x.
  - `similar` 3.x (edition 2024, MSRV 1.85).
  - `jiff` 1.0 (2026).
  - `hashbrown` 0.15 to 0.17. 0.15 removed `raw` in favour of `HashTable` (UNVERIFIED from memory), and the 0.17.2 feature list still shows `raw-entry`.
  - `criterion` 0.8; its repository moved to `github.com/criterion-rs/criterion.rs`.
  - `smallvec` 2.0 is still in beta (2.0.0-beta.5).
- **Feature flags of note** (VERIFIED from crates.io `features`):
  - `hashbrown` 0.17.2: `default-hasher`, `inline-more`, `raw-entry`, `serde`.
  - `indexmap` 2.14.2: `std`, `serde`.
  - `bstr` 1.14.0: `std`, `unicode`, `serde`.
  - `jiff` 1.0.3: `tz-system`, `tzdb-bundle-always`, `tzdb-bundle-platform`, `tzdb-zoneinfo`, `tz-fat`, `static-tz`, `serde`.
  - `fancy-regex` 0.20.2: `variable-lookbehinds`, `leftmost_longest`.
  - `serde_json` 1.0.154: `preserve_order`, `float_roundtrip`, `arbitrary_precision`, `raw_value`.

### 1.3 Supplementary crates and tools

Fetched because they came up while verifying the above (VERIFIED, same method):

| crate | latest stable (date) | newest pre-release | rust-version / edition (latest) | license | downloads total / 90 d | releases in last 365 d (total) | first release | owners (crates.io) |
|---|---|---|---|---|---|---|---|---|
| `zmij` | 1.0.23 (2026-07-13) | - | 1.71 / 2021 | MIT | 751.1M / 385.2M | 37 (37) | 2025-12-18 | dtolnay |
| `itoa` | 1.0.19 (2026-11-09) | - | 1.68 / 2021 | MIT OR Apache-2.0 | 1.72B / 460.1M | 4 (38) | 2016-06-25 | dtolnay |
| `ahash` | 0.8.14 (2026-11-25) | - | 1.60.0 / 2018 | MIT OR Apache-2.0 | 943.3M / 208.9M | 3 (98) | 2019-02-28 | tkaitchuck |
| `stacker` | 0.1.25 (2026-08-02) | - | 1.63 / 2021 | MIT OR Apache-2.0 | 146.9M / 49.5M | 3 (26) | 2015-07-15 | rust-lang-owner |
| `jiff-tzdb` | 1.0.0 (2026-11-15) | - | 1.85 / 2024 | Unlicense OR MIT | 84.7M / 46.0M | 7 (13) | 2024-07-09 | BurntSushi |

Cargo tooling:

| crate | latest stable (date) | newest pre-release | rust-version / edition (latest) | license | downloads total / 90 d | releases in last 365 d (total) | first release | owners (crates.io) |
|---|---|---|---|---|---|---|---|---|
| `cargo-deny` | 0.22.1 (2026-11-24) | - | 1.95.0 / 2024 | MIT OR Apache-2.0 | 8.0M / 2.7M | 20 (121) | 2019-05-13 | embark-studios |
| `cargo-semver-checks` | 0.51.0 (2026-10-03) | - | 1.93 / 2024 | Apache-2.0 OR MIT | 763k / 178k | 6 (80) | 2022-07-13 | obi1kenobi |
| `cargo-msrv` | 0.19.6 (2026-10-21) | 0.20.0-beta.7 | 1.94.1 / 2024 | Apache-2.0 OR MIT | 539k / 33k | 13 (71) | 2019-10-02 | foresterre |
| `cargo-hack` | 0.6.46 (2026-11-01) | - | 1.85 / 2021 | Apache-2.0 OR MIT | 2.0M / 206k | 7 (105) | 2019-10-30 | taiki-e |
| `cargo-nextest` | 0.9.152 (2026-11-13) | - | 1.91 / 2024 | Apache-2.0 OR MIT | 14.2M / 1.9M | 44 (176) | 2022-02-13 | sunshowers, nextest-bot |
| `release-plz` | 0.4.3 (2026-11-02) | - | not declared / 2024 | MIT OR Apache-2.0 | 367k / 29k | 30 (245) | 2022-02-01 | marcoieni |
| `cargo-release` | 1.1.7 (2026-10-21) | - | 1.97 / 2024 | MIT OR Apache-2.0 | 556k / 19k | 9 (151) | 2016-03-09 | sunng87, epage |
| `git-cliff` | 2.14.2 (2026-09-18) | - | 1.88.0 / 2024 | MIT OR Apache-2.0 | 372k / 52k | 7 (69) | 2021-07-01 | orhun |

### 1.4 Fit for twig-rs, area by area

Each entry gives the verdict, the reasons, and the risks.

#### Ordered maps and hashing (`indexmap`, `hashbrown`, `foldhash`, `rustc-hash`, `ahash`)

- PHP arrays are ordered hash maps with `int|string` keys. They have:
  - insertion order preserved across deletes;
  - integer-like string keys normalised to ints;
  - `nNextFreeElement` for `[]=` appends;
  - "packed" optimisation;
  - value semantics with copy-on-write.

  Twig's runtime leans on all of these: `merge`, `keys`, `slice` with `preserve_keys`, `sort` (stable since PHP 8.0), `loop.length`, context
  ordering after for-loops (`array_intersect_key` + `+=`, see [4.1](#41-context-model-copy-a-php-array-not-a-jinja-frame-stack)).
- **Recommendation:** an in-tree `PhpArray`:
  - a `hashbrown::HashTable<u32>` (indices into an entries `Vec`), keyed by `ArrayKey::{Int(i64), Str(Rc<[u8]>)}`;
  - entries stored as `Vec<Option<(ArrayKey, Value)>>` with tombstones and compaction, like `zend_hash`;
  - plus a `next_free: i64` field;
  - wrapped in `Rc` and mutated with `Rc::make_mut`, which gives PHP's separate-on-write semantics.

  Strengths: exact iteration order, O(1) delete, `nNextFreeElement` fidelity. Risk: more code to own. Failure mode: subtle divergence in
  compaction or `next_free` after deletes. Test it against the oracle with property tests (`proptest`) that replay random operation sequences in PHP.
- **Quick-start alternative:** `indexmap` 2.14.2. Mature (2B downloads), MSRV 1.85.
  - Deleting while keeping order needs `shift_remove`, which is O(n).
  - `swap_remove` breaks PHP order and must never be used.
  - `next_free` must be tracked outside the map.
- Hasher:
  - `hashbrown`'s default hasher is `foldhash` (feature `default-hasher`). It is "minimally DoS-resistant" (per-process seed) and fast.
  - Context keys can come from request data (hash keys), so prefer `foldhash` or `ahash` over `rustc-hash` (FxHash has no seed).
  - PHP itself uses unseeded DJBX33A (VERIFIED, `PHPSRC/Zend/zend_string.h:501-535`, initial value `5381`). Order comes from
    insertion, never from hash order, so the choice of Rust hasher cannot affect output.

#### Regular expressions (`pcre2`, `pcre2-sys`, `fancy-regex`, `regex`, `regex-automata`)

Where Twig 3.31.1 uses PCRE at runtime (VERIFIED, `grep preg_ TWIG/src`):

- user-supplied patterns:
  - the `matches` operator, `CoreExtension::matches` at `TWIG/src/Extension/CoreExtension.php:1182-1197`;
  - compile-time validation of a constant pattern in `TWIG/src/Node/Expression/Binary/MatchesBinary.php:33-41`;
- fixed patterns:
  - spaceless: `preg_replace('/>\s+</', '><', ...)`, CoreExtension.php:1247;
  - splitting into characters: `preg_match_all('/./us', ...)`, CoreExtension.php:1275;
  - escaper callbacks: `#[^a-zA-Z0-9,\._]#Su` and `#[^a-zA-Z0-9]#Su`, `TWIG/src/Runtime/EscaperRuntime.php:206-289`;
  - lexer and parser regexes (`Lexer::REGEX_NAME` and others).

Exact messages Twig builds from PHP's warning (VERIFIED in source and by running the oracle):

- compile time: `sprintf('Regexp "%s" passed to "matches" is not valid: %s.', $regexp, substr($m, 14))`, MatchesBinary.php:35. It throws SyntaxError.
- runtime: `sprintf('Regexp "%s" passed to "matches" is not valid', $regexp).substr($m, 12)`, CoreExtension.php:1185. Note there is **no trailing period**.
- runtime failure: `sprintf('Regexp "%s" passed to "matches" failed: %s.', $regexp, preg_last_error_msg())`, CoreExtension.php:1189.
- oracle output for `~(abc~`:
  - runtime: `Regexp "~(abc~" passed to "matches" is not valid: Compilation failed: missing closing parenthesis at offset 4`
  - compile: `Regexp "~(abc~" passed to "matches" is not valid: Compilation failed: missing closing parenthesis at offset 4.`

PHP side (VERIFIED, `PHPSRC/ext/pcre/php_pcre.c`):

- Warning texts:
  - `Empty regular expression` (line 630)
  - `Delimiter must not be alphanumeric, backslash, or NUL byte` (642)
  - `No ending delimiter '%c' found` (686)
  - `No ending matching delimiter '%c' found` (688)
  - `Unknown modifier '%c'` (741)
  - `NUL byte is not a valid modifier` (743)
  - `Compilation failed: %s at offset %zu` (789)
  - `JIT compilation failed: %s` (812)
- Modifiers (lines 702-746):
  - `i` CASELESS, `m` MULTILINE, `n` NO_AUTO_CAPTURE, `s` DOTALL, `x` EXTENDED;
  - `A` ANCHORED, `D` DOLLAR_ENDONLY, `r` EXTRA_CASELESS_RESTRICT;
  - `S` and `X` are no-ops;
  - `U` UNGREEDY;
  - `u` = UTF | UCP | NEVER_BACKSLASH_C;
  - `J` DUPNAMES;
  - spaces, `\n` and `\r` are ignored.
- JIT stack: 32 KiB to 192 KiB (lines 68-69).
- `pcre2_set_match_limit` uses `pcre.backtrack_limit` and `pcre2_set_depth_limit` uses `pcre.recursion_limit` (lines 319, 329).
  The oracle values are 1000000 and 100000 (VERIFIED with `php -i`).

Crate verdicts:

- **`pcre2` 0.2.11 (high level): not sufficient** (VERIFIED by reading its source):
  - `RegexBuilder` only exposes `caseless`, `dotall`, `extended`, `multi_line`, `crlf`, `ucp`, `utf`, `disable_utf_check`, `jit`, `jit_if_available` and `max_jit_stack_size`.
  - It has no ANCHORED, DOLLAR_ENDONLY, UNGREEDY, DUPNAMES, NO_AUTO_CAPTURE, NEVER_BACKSLASH_C or EXTRA options, and no match or depth limits.
  - `build(&self, pattern: &str)` takes a `&str`, so non-UTF-8 pattern bytes, which PHP allows without `u`, cannot be passed.
  - `pcre2::version()` returns compile-time binding constants that say 10.45, while the linked library is 10.46 (VERIFIED, [6.3](#63-pcre2-throwaway-project-verified)).
- **`pcre2-sys` 0.2.10 (raw FFI):** 222 bindings, including `pcre2_set_match_limit_8`, `pcre2_set_depth_limit_8`,
  `pcre2_set_compile_extra_options_8`, `pcre2_jit_stack_create_8` and `pcre2_get_error_message_8` (VERIFIED, `src/bindings.rs`).
  - The API is adequate.
  - The bundled C version (10.46) is wrong for exact parity.
  - Its `build.rs` first tries `pkg_config::probe_library("libpcre2-8")` unless `PCRE2_SYS_STATIC=1` (build.rs:31-35), so a system
    PCRE2 of any version can silently be picked up.
- **`fancy-regex` / `regex` / `regex-automata`:** not PCRE-compatible. `regex` has no backreferences or lookaround, and its Unicode
  class and `\d` semantics differ. Do not use them for user patterns. They are acceptable for internal lexer patterns only if every
  pattern is proven equivalent. Simpler still: hand-write the lexer scanning with `memchr`, as Twig's patterns are fixed.
- **Recommendation:** in-tree `php-pcre2-sys`, built from `PHPSRC/ext/pcre/pcre2lib`. Same 28-file list as `config.w32`. Its
  `config.h` is PHP's minus the PHP includes, plus `HAVE_PCRE_JIT_SUPPORT=1`. This gives byte-identical PCRE2 behaviour
  (prototype in [6.4](#64-prototype-building-phps-own-pcre2-1044-verified)). On top of it, a `php-regex` crate ports
  `pcre_get_compiled_regex_cache_ex` (delimiters, modifiers, the warning texts above) and `php_pcre_match_impl`.
  - Trade-off: we pin 10.44 even though upstream has security fixes in 10.45/10.46. Risk is low for a template engine, and must be revisited when the PHP target moves.
  - Bump the vendored copy only together with the PHP oracle version.

#### Errors (`thiserror`)

- `thiserror` 2.0.21: fine for internal error enums. However, Twig error *messages* must be produced by porting
  `Twig\Error\Error::updateRepr` (`TWIG/src/Error/Error.php:131-160`), not by `#[error("...")]` strings:
  - strip a trailing `.` or `?`;
  - append ` in "<name>"`, then ` at line N` and optionally ` column N`;
  - re-append the punctuation.

  Keep raw message, template name, line and column as fields, and implement `Display` by hand.

#### Testing (`libtest-mimic`, `datatest-stable`, `insta`, `similar`, `proptest`)

- **`libtest-mimic` 0.8.2: Recommendation for the Twig fixture harness** (488 `.test` files, see [5.10](#510-test-harness-implications-of-the-twig-fixture-format)).
  - One `Trial` per fixture and per DATA/EXPECT pair.
  - `harness = false` in `[[test]]`.
  - Works with `cargo test` filtering and with nextest's custom-harness protocol (UNVERIFIED for nextest; documented by nextest).
- `datatest-stable` 0.3.3: a thin file-per-test layer over libtest-mimic. Each Twig fixture expands into several cases (multiple DATA
  blocks), so plain libtest-mimic is simpler. It had no release in the last 365 days.
- `insta` 1.49.0: useful for snapshotting bytecode listings and AST dumps (compiler regression tests), not for oracle parity, where the expected output comes from PHP.
- `similar` 3.2.1: readable diffs for failed fixtures.
- `proptest` 1.12.0 (MSRV 1.88): differential testing against PHP. Generate inputs in Rust, evaluate them in a batch through the Docker oracle, and compare.

#### Dates and time (`jiff`, `chrono`)

- Twig's `date` and `date_modify` filters and `date()` function go through PHP `DateTime`, which is timelib: a re2c parser for
  `strtotime` formats, PHP's own month-overflow arithmetic (VERIFIED in the oracle: `(new DateTime("2021-01-31"))->modify("+1 month")`
  gives `2021-03-03`), and `DateTime::format` letters. Neither `jiff` nor `chrono` implements these semantics.
- tz data (VERIFIED):
  - `timezone_version_get()` in the oracle returns `2026.6`;
  - `PHPSRC/ext/date/lib/timezonedb.h:70723` reads `const timelib_tzdb timezonedb_builtin = { "2026.6", 598, ...}`, and the file is 6,481,653 bytes;
  - `jiff-tzdb` 1.0.0 bundles IANA `2026f` (`tzname.rs:1`).
- **Recommendation:** port timelib (MIT, `PHPSRC/ext/date/lib`) and embed PHP's `timezonedb.h` data. Use `jiff` only, if at all, as
  a test cross-check. Risk: timelib's parser is large (re2c output) and needs a faithful port or a regenerated scanner.

#### Encodings (`encoding_rs`)

- VERIFIED (`...\scratchpad\tmp-pcre2\src\bin\enccheck.rs`):
  - labels `iso-8859-1`, `latin1` and `ascii` all resolve to `windows-1252`;
  - label `utf-16` resolves to `UTF-16LE`;
  - decoding bytes `80 9F` through the `iso-8859-1` label yields UTF-8 `E2 82 AC C5 B8` (`€Ÿ`);
  - PHP 8.5.13 `mb_convert_encoding("\x80\x9f","UTF-8","ISO-8859-1")` and `iconv(...)` both yield `C2 80 C2 9F`.
- Verdict: `encoding_rs` implements the WHATWG Encoding Standard, which is a different contract from mbstring/libmbfl and iconv.
  Twig's `convert_encoding` and charset handling need an mbstring port for the encodings Twig users actually hit (UTF-8, ISO-8859-x,
  Windows-125x, ASCII). Licensing caveat: libmbfl is LGPL-2.1, see [5.9](#59-licensing-of-what-will-be-ported-or-vendored).

#### Strings and buffers (`bstr`, `memchr`, `smallvec`, `compact_str`, `ecow`)

- PHP strings are byte strings and Twig may output invalid UTF-8. Strings in Twig values therefore have to be bytes.
- **Recommendation:** `Rc<[u8]>` or a small custom `PhpStr` (inline up to around 22 bytes, `Rc` otherwise) for values, plus `bstr` for
  byte-string algorithms (`find`, `split`, `trim`, char iteration with substitution) and `memchr` for lexer scanning.
  - `compact_str` and `ecow` are `str`-based; `ecow::EcoVec<u8>` could work but is a niche crate (11.6M downloads).
  - `smallvec` 1.x (edition 2018, no MSRV declared) is fine for argument vectors; 2.0 is still beta.

#### Number formatting (`ryu`, `zmij`, `itoa`)

- Experiment (VERIFIED, `...\scratchpad\tmp-pcre2\src\bin\floatcmp.rs`, data `...\rust-eco\data\float\samples.tsv`):
  - 199,996 doubles, generated in the oracle: random bit patterns, decimal-looking values, template-like products, and exact ties at the 15th significant digit;
  - compared as (sign, significant digits, decimal exponent).

  | PHP output | Rust candidate | equal |
  |---|---|---|
  | `(string)$f`, precision=14, `zend_gcvt` mode 2 (`PHPSRC/Zend/zend_strtod.c:4509-4515`, `zend_operators.c:3616-3621`) | `format!("{x:.13e}")` (std) | **199,996 / 199,996** |
  | `var_export($f)`, serialize_precision=-1, mode 0 | `ryu::Buffer::format_finite` | **199,996 / 199,996** |
  | same | `zmij::Buffer::format_finite` | **199,996 / 199,996** |
  | same | `format!("{x:e}")` and `format!("{x}")` (std shortest; both give identical digits on all samples) | 199,981 (15 mismatches, all 17-digit cases, e.g. bits `c314560194435469`: PHP `-1431016079152410.2`, std `-1.4310160791524103e15`) |

- Exact half-way cases at precision 14 round **half to even** in PHP: `23119411616519.5` becomes `23119411616520`, and `63151216125196.5`
  becomes `63151216125196`. Rust's exact formatting does the same.
- **Recommendation:** use std `{:.Ne}` for `precision`-based conversions (echo, string concatenation, `(string)`), then port `zend_gcvt`'s layout rules:
  - `1.0E+25`, `-0`, the `E` threshold;
  - the `.0` suffix that `smart_str_append_double(..., zero_frac=true)` adds (`PHPSRC/Zend/zend_smart_str.c:156-165`).

  Use `zmij` (dtolnay, now `serde_json`'s backend) or `ryu` for the shortest form. `itoa` is optional because std integer
  formatting is exact.

#### Serialization and logging (`serde`, `serde_json`, `tracing`, `log`)

- `serde` 1.0.229 and `serde_json` 1.0.154. VERIFIED: serde_json now depends on `serde_core ^1.0.220` and `zmij ^1.0` (the crates.io dependencies endpoint).
  - Keep serde optional (a feature) for converting Rust data into Twig values.
  - `json_encode` must be a port of PHP's `ext/json` encoder (flags, `serialize_precision`, escaping), not `serde_json` output.
- `log` 0.4.35 (lightweight facade) versus `tracing` 0.1.44 (spans). A template engine library should emit at most debug-level events.
  - **Recommendation:** optional `tracing` behind a feature, or no logging at all.
  - Deprecation notices (Twig's `trigger_deprecation`) must be a first-class callback API, because Twig fixtures assert on them.

#### Benchmarks (`criterion`, `divan`)

- `criterion` 0.8.2: maintained (rust-bus, new org), MSRV 1.86.
- `divan` 0.1.21: no release since 2025-04-10.
- **Recommendation:** `criterion`, behind a separate `benches/` crate so its dependencies do not affect the library's MSRV.

---

## 2. The `twig` crate name and other Twig implementations in Rust

### 2.1 `twig` on crates.io (VERIFIED, `/api/v1/crates/twig` and `/owners`)

- Owner: user `Nercury` (crates.io user id 440, name "Nerijus Arlauskas", `github_username_matches: true`). There are no team owners.
- Versions:

  | version | published | downloads | license | crate size |
  |---|---|---|---|---|
  | 0.1.0 | 2015-11-19 | 2,955 | BSD-3-Clause | 41,853 B |
  | 0.0.4 | 2015-10-24 | 1,591 | BSD-3-Clause | 37,604 B |
  | 0.0.3 | 2015-10-11 | 1,547 | BSD-3-Clause | 21,050 B |
  | 0.0.2 | 2015-09-16 | 1,555 | BSD-3-Clause | 13,616 B |
  | 0.0.1 | 2015-08-15 | 1,545 | (none) | 12,182 B |

- Crate metadata:
  - description "Twig templating engine for Rust; work in progress.";
  - repository `https://github.com/Nercury/twig-rs`;
  - documentation `http://nercury.github.io/twig-rs/twig/index.html`;
  - total downloads 9,194, 14 in the last 90 days;
  - no versions yanked;
  - `trustpub_only: false`.
- Consequence: a revived crate can publish `twig` 0.2.0 or 1.0.0 only from the owner account, or after `cargo owner --add`.
  Old 0.0.x/0.1.0 versions remain downloadable (yanking is optional).
- Candidate sub-crate names were all free on 2026-11-25 (HTTP 404 from the crates.io API): `twig-syntax`, `twig-parser`, `twig-vm`,
  `twig-compiler`, `twig-runtime`, `twig-core`, `twig-php`, `twig-rs`, `php-rt`, `php-runtime`, `zend-strtod`, `php-pcre`,
  `php-pcre2-sys`, `pcre2-php-sys`, `twig-fixtures`.

### 2.2 Other Twig-related crates (VERIFIED, crates.io search `q=twig`, `q=twig template`, `q=twig php`)

| crate | latest (updated) | kind | state |
|---|---|---|---|
| `mago-twig-syntax` | 1.61.0 (2026-11-21) | lossless lexer + CST + parser "targeting Twig 3 syntactic compatibility" (carthage-software/mago, MIT OR Apache-2.0) | very active. The repository has `crates/twig-syntax/tests/fixtures/**` and fuzz seeds (VERIFIED by `git ls-tree`). Useful as a syntax reference and fuzz corpus. **No evaluator.** |
| `mago-twig-formatter`, `mago-twig-linter` | 1.61.0 | tooling on the above | active |
| `ludtwig`, `ludtwig-parser` | 0.15.0 / 0.12.0 (2026-11-12) | linter/formatter and lossless HTML+Twig parser | active (last commit 2026-11-12) |
| `oak-twig` | 0.0.11 (2026-03-30) | parser example in the ygg-lang/oaks project | early |
| `tree-sitter-shopware-twig`, `tree-sitter-jinja-dialects` | 0.2.3 / 0.1.1 | tree-sitter grammars | grammars only |
| `markup_fmt` | 0.28.0 | formatter that supports Twig | formatter |
| `sawyer` | 0.0.0 (2022-11-07) | "A Twig interpreter." | placeholder. Its GitHub repo is not reachable (`git clone` failed). |
| `twig` | 0.1.0 (2015) | the original twig-rs | dormant |
| `twig-doc`, `twig-kit`, `twig-sys`, `twig-cli`, `twig-tmux`, `git-twig`, `twiggy*` | - | unrelated projects that use the word "twig" | n/a |

Historical Rust Twig efforts (VERIFIED, last commit dates from shallow clones):

| repository | last commit | note |
|---|---|---|
| `Nercury/twig-rs` | 2017-10-01 "Make it work." | the codebase being revived. It depends on `little = "0.3"`. |
| `Nercury/little-rs` | 2019-06-25 | the "LLTL" VM crate (`little` 0.3.0, MIT) |
| `rust-web/twig` | 2016-10-07 | attempt to merge Nercury/twig-rs and colin-kiegel/twig-rust. COMPATIBILITY.md says "only supports UTF8 encoded templates". |
| `colin-kiegel/twig-rust` | 2015-12-23 | earlier independent port |

Not done: a GitHub-wide code search. The unauthenticated GitHub API rate limit was exhausted on this shared IP, so unpublished
GitHub-only engines may exist (UNVERIFIED).

### 2.3 Jinja-family engines in Rust (context, VERIFIED crates.io data)

| crate | latest stable (date) | newest pre-release | rust-version / edition (latest) | license | downloads total / 90 d | releases in last 365 d (total) | first release | owners (crates.io) |
|---|---|---|---|---|---|---|---|---|
| `minijinja` | 3.1.0 (2026-11-07) | - | 1.70 / 2021 | Apache-2.0 | 48.5M / 17.3M | 25 (124) | 2021-09-23 | mitsuhiko |
| `tera` | 2.4.0 (2026-09-11) | - | not declared / 2024 | MIT | 44.7M / 8.2M | 15 (128) | 2016-04-04 | Keats |
| `askama` | 0.16.1 (2026-09-04) | - | 1.88 / 2024 | MIT OR Apache-2.0 | 64.6M / 19.0M | 9 (44) | 2017-02-15 | GuillaumeGomez, djc, Kijewski, github:askama-rs:maintainer |
| `rinja` | 0.4.0+deprecated (2025-03-29) | - | 1.81 / 2021 | MIT OR Apache-2.0 | 4.8M / 314k | 0 (9) | 2024-06-17 | GuillaumeGomez, Kijewski, github:askama-rs:maintainer |
| `handlebars` | 6.4.4 (2026-08-12) | - | 1.85 / 2024 | MIT | 108.3M / 19.0M | 5 (187) | 2014-12-27 | sunng87 |
| `liquid` | 0.26.11 (2025-02-04) | - | 1.83.0 / 2021 | MIT OR Apache-2.0 | 9.8M / 2.0M | 0 (78) | 2015-02-15 | johannhof, github:cobalt-org:owners |
| `upon` | 0.11.0 (2026-07-25) | - | 1.66 / 2021 | MIT OR Apache-2.0 | 570k / 131k | 1 (14) | 2022-03-26 | rossmacarthur |
| `sailfish` | 0.11.9 (2026-10-26) | - | not declared / 2024 | MIT | 807k / 116k | 11 (44) | 2020-06-05 | vthg2themax, Kogia-sima |

Notes:

- minijinja has reached 3.x. A Go port, `minijinja-go/v3.1.0`, lives in the same repository.
- Tera 2 is released (2.4.0) and has a bytecode VM ([3.12](#312-contrast-tera-24-vm)).
- `rinja` merged back into `askama`.

---

## 3. minijinja 3.1.0 compiler and VM (prior art)

Source: git tag `3.1.0`, released 2026-11-07, the latest GitHub release (VERIFIED, `releases/latest`). The release notes mention
fuel now also limiting memory, `render_with_spans`, and many Jinja2-compatibility fixes. Crate facts: edition 2021, `rust-version = "1.70"`.
Default features: `builtins`, `debug`, `macros`, `multi_template`, `adjacent_loop_items`, `std_collections`. Optional features: `fuel`,
`loop_controls`, `stacker`, `json`, `urlencode`, `speedups` (`v_htmlescape`, `zmij`, `itoa`), `preserve_order` (`indexmap`).
Source: `MJ/minijinja/Cargo.toml`.

### 3.1 Pipeline

- Lexer (`compiler/lexer.rs`), then parser (`compiler/parser.rs`, `MAX_RECURSION = 150` at line 14), then AST (`compiler/ast.rs`), then `CodeGenerator` (`compiler/codegen.rs`).
- Codegen output, `CompiledTemplate` (`template.rs:438-449`):
  - `instructions: Instructions` (root stream);
  - `blocks: BTreeMap<&str, Instructions>` (one stream per `{% block %}`);
  - `buffer_size_hint`;
  - `syntax_config`;
  - `initial_auto_escape`.
- Instructions borrow from the template source (`Instruction<'source>` holds `&'source str` names). Templates own their source through a vendored `self_cell` (`template.rs:7, 64`).
- Size checks:
  - `size_of::<Instruction>() == 32` (`instructions.rs:566-570`);
  - `size_of::<Value>() == 24` (`value/mod.rs:2395`);
  - `ValueRepr` variants are `None`, `Undefined(UndefinedType, UndefinedOrigin)`, `Bool`, `U64`, `I64`, `F64`, `Invalid(Arc<Error>)`,
    `U128`, `I128`, `String(Arc<str>, StringType)`, `SmallStr`, `Bytes(Arc<Vec<u8>>)`, `Object(DynObject)` (`value/mod.rs:577-591`).

### 3.2 Instruction set (complete)

From `MJ/minijinja/src/compiler/instructions.rs:47-263`. The machine is stack-based; "TOS" means top of stack.

| group | instruction | semantics |
|---|---|---|
| output | `EmitRaw(&str)` | write template text |
| | `Emit` | pop and write, applying auto-escape and formatter |
| variables | `StoreLocal(&str)` | pop into the current frame's locals (and into the frame's closure if any) |
| | `Lookup(&str)` | push variable (locals, then loop var, then closure, then frame ctx, then globals), or undefined |
| | `GetAttr(&str)` / `SetAttr(&str)` | attribute get; `SetAttr` only on `namespace()` objects |
| | `GetItem`, `Slice` | subscript; slice pops `step`, `stop`, `start`, `obj` |
| constants | `LoadConst(Value)` | push constant (constant folding produces these) |
| builders | `BuildMap(n)`, `BuildKwargs(n)`, `MergeKwargs(n)`, `BuildList(Option<n>)`, `BuildTuple(Option<n>)` | `None` means the count is on the stack |
| unpack | `UnpackList(n)`, `UnpackLists(n)` | destructuring; splat argument batches push the item count |
| arithmetic | `Add`, `Sub`, `Mul`, `Div`, `IntDiv`, `Rem`, `Pow`, `Neg` | |
| compare | `Eq`, `Ne`, `Gt`, `Gte`, `Lt`, `Lte`, `In`, `CompareAndPreserve(CompareOp)` | the last one is for chained comparisons (`a < b < c`) |
| logic / strings | `Not`, `StringConcat` | |
| calls | `ApplyFilter(&str, Option<u16>, LocalId)`, `PerformTest(&str, Option<u16>, LocalId)` | `LocalId` indexes a per-evaluation cache of resolved filters/tests (`MAX_LOCALS = 50`, line 22) |
| | `CallFunction(&str, Option<u16>)`, `CallMethod(&str, Option<u16>)`, `CallObject(Option<u16>)` | `None` means the argument count is on the stack (splats) |
| control flow | `Jump(u32)`, `JumpIfFalse(u32)`, `JumpIfFalseOrPop(u32)`, `JumpIfTrueOrPop(u32)` | absolute targets, backpatched |
| loops | `PushLoop(flags)`, `Iterate(end)`, `PushDidNotIterate`, `PopLoopFrame` | flags `LOOP_FLAG_WITH_LOOP_VAR=1`, `LOOP_FLAG_RECURSIVE=2` |
| frames | `PushWith`, `PopFrame` | `{% with %}`, imports |
| escaping | `PushAutoEscape`, `PopAutoEscape` | runtime auto-escape mode stack |
| capture | `BeginCapture(CaptureMode::{Capture,Discard})`, `EndCapture` | output redirection; `EndCapture` pushes the captured string |
| stack | `DupTop`, `DiscardTop`, `Swap` | |
| fast paths | `FastSuper`, `FastRecurse` | `{{ super() }}` / `{{ loop(x) }}` emitted directly, without capture |
| templates | `CallBlock(&str)`, `LoadBlocks`, `Include(bool ignore_missing)`, `ExportLocals` | multi-template feature |
| macros | `BuildMacro(&str name, u32 offset, u8 flags)`, `Return`, `IsUndefined`, `Enclose(&str)`, `GetClosure` | `MACRO_CALLER = 2` flag |

### 3.3 Code generation patterns

- **Backpatching** (`codegen.rs:52-63, 204-317`):
  - a `pending_block: Vec<PendingBlock>` stack with variants `Branch { jump_instr }`, `Loop { iter_instr, jump_instrs }` and `ScBool { jump_instrs }`;
  - jumps are emitted with target `!0` and patched at `end_if`, `end_for_loop` or `end_sc_bool`;
  - `start_else` patches the pending `JumpIfFalse` to `jump_instr + 1`.
  - Buffers are recycled through thread-local pools (lines 65-115).
- **Short-circuit** (`codegen.rs:266-302, 1014-1022`): `a and b` compiles to `<a> JumpIfFalseOrPop(end) <b>`. `or` uses `JumpIfTrueOrPop`.
- **Chained comparison** (`codegen.rs:953-988`): `CompareAndPreserve` + `JumpIfFalseOrPop`, then a cleanup tail `Swap; DiscardTop`.
- **if expression without else** pushes a *silent undefined* (`codegen.rs:733-750`). This is Jinja-specific.
- **for with a filter** (`for x in seq if cond`, `codegen.rs:606-624`) runs two passes: the first loop counts and keeps matching items on
  the stack, `BuildList(None)` collects them, and the real loop follows.
- **Constant folding**: `expr.as_const()` (`codegen.rs:672-676`). The 3.1.0 release notes say folding is capped at 1 KB results.
- **Calls** (`codegen.rs:842-951`): positional arguments are pushed; splats force batching into lists and then `UnpackLists`.
  All-constant kwargs fold into one `LoadConst(Kwargs)`, while dynamic ones use `BuildKwargs`/`MergeKwargs`.
- **Blocks** (`codegen.rs:472-482`): a sub-generator compiles the body into a separate `Instructions`. It is stored in `blocks[name]`,
  and the parent stream gets `CallBlock(name)`.
- **Macros** (`codegen.rs:485-532`) are compiled *inline* behind a jump:
  - `Jump(after)`;
  - argument binding in reverse order, with defaults via `DupTop; IsUndefined; JumpIfFalse; DiscardTop; <default>`;
  - the body, then `Return`;
  - at `after`: `Enclose(name)` for every undeclared name the body references (computed by `compiler/meta.rs:40 find_macro_closure`),
    then `GetClosure`, `LoadConst([arg names])` and `BuildMacro(name, body_offset, flags)`;
  - `{% macro %}` then does `StoreLocal(name)`.
- **Line tracking during codegen**: `set_line`, `push_span` and `pop_span` (`codegen.rs:145-178`).

### 3.4 VM structure

- Entry: `vm::eval(env, instructions, root, blocks, out, auto_escape)` (`vm/mod.rs:114-123`). It creates a
  `State { ctx: Context, current_block, auto_escape, instructions, temps, extensions, blocks: BTreeMap<&str, BlockStack>, loaded_templates, id, known_instructions, closures, macro_context_pool, fuel_tracker, ... }`
  (`vm/state.rs:43-...`).
- Context: a `Vec<Frame>` (`vm/context.rs:132-139`). `Frame { locals: BTreeMap<&str, Value>, ctx: Value, current_loop: Option<LoopState>, closure, closure_context }` (lines 15-29).
  - `Context::load` walks frames from the top (`context.rs:281-...`): locals, then the `loop` variable when the frame has a loop with
    `with_loop_var`, then the closure, then `frame.ctx.get_attr_fast(key)`, and finally `env.get_global(key)`.
- Value stack: `Stack { values: Vec<Value> }` with capacity 24 (`context.rs:77-131`). Call arguments are a slice of the stack top (`get_call_args`).
- Main loop: `eval_impl` (`vm/mod.rs:254-1080`). It is a `loop { match instr { ... } pc += 1 }`. Local macros `bail!`/`ctx_ok!` attach
  location information to errors (`process_err`). Filter and test lookups are memoised in `[Option<&Value>; MAX_LOCALS]` per evaluation.
- Nested evaluations (blocks, includes, super, macros) go through `State::with_execution_state` (`vm/state.rs:163-235`). It saves and
  restores `instructions`, `auto_escape`, `current_block` and the block table (`BlockState::{Keep, Isolate, Replace}`), and restores the frame-stack depth.
- Optional `stacker` grows the native stack: `maybe_grow(32 KiB, 1 MiB)`, `vm/mod.rs:241-246`.

### 3.5 Blocks and inheritance

- `prepare_blocks` wraps each block stream in a `BlockStack { instructions: Vec<&Instructions>, depth }` (`vm/mod.rs:54-62`, `vm/state.rs:897-936`).
- `{% extends expr %}` compiles to `<expr> LoadBlocks` (`codegen.rs:421-425`). At runtime (`vm/mod.rs:1009-1019, 1305-1332`):
  - a second extends is an error: `tried to extend a second time in a template`;
  - the parent is loaded and every parent block stream is **appended** to `state.blocks[name]` (child first, then parents);
  - `parent_instructions` is set;
  - `out.begin_capture(CaptureMode::Discard)` starts;
  - the rest of the child keeps executing with output discarded, so `set` and macros still run;
  - at the end of the child stream the VM swaps in the parent's root instructions, ends the discard capture, resets the filter/test caches and starts at `pc = 0` (`vm/mod.rs:346-371`).
  - Cycle detection uses `loaded_templates`: `cycle in template inheritance. {name:?} was referenced more than once` (line 1318).
- `CallBlock(name)` runs only when not extending and not discarding (`vm/mod.rs:1042-1046`). `call_block` runs `blocks[name].instructions()`
  (the current depth) in a new frame, with errors `Required block '{name}' not found` (line 1344) and `block '{name}' not found` (line 1362).
- `super()` (`perform_super`, `vm/mod.rs:1252-1302`):
  - `BlockStack::push()` increments depth; when there is no deeper stream the error is `no parent block exists`;
  - the parent stream runs in a new frame, then depth is popped;
  - outside a block the error is `cannot super outside of block`;
  - errors are wrapped as `error in super block`.
- Included templates get `BlockState::Replace(prepare_blocks(their blocks))`. Macros run with `BlockState::Isolate`, which truncates
  block stacks back to a checkpoint afterwards (`vm/state.rs:171-233`).

### 3.6 Macros and call blocks

- `BuildMacro` (`vm/mod.rs:1474-1493`) creates a `Macro { name, arg_spec, instructions_id, offset, state_id, closure, caller_reference }`
  object (`vm/macro_object.rs:12-23`). The instruction stream is registered in `state.known_instructions` under its address, because
  values must be `'static`.
- Calling it (`macro_object.rs:116-142`):
  - `prepare_args` maps positional and keyword arguments, with errors such as `duplicate argument` and `unknown keyword argument`;
  - `vm::eval_macro(...)` takes a pooled `Context`, adds a base frame with the root ctx, then a frame whose `closure_context` is the
    macro's closure, stores `caller`, and charges `MACRO_RECURSION_COST = 4` (`vm/mod.rs:168-226`);
  - output is collected into a fresh `String`;
  - the result is a safe string if auto-escaping is on (`Value::from_safe_string`).
- Closures (`context.rs:218-245`): once a frame has a closure, every `StoreLocal` is mirrored into it, emulating Python closures over the declaring scope.
- `{% call %}` compiles the caller body as an anonymous macro passed as the `caller` kwarg (`codegen.rs:541-544, 919-926`).

### 3.7 Loops

- `PushLoop(flags)` pops the iterable and pushes a frame with `LoopState { with_loop_var, current_recursion_jump, object: Arc<Loop>, iter }` (`vm/mod.rs:1388-1448`, `loop_object.rs:11-57`).
  `Loop.len` is known only when the iterator's `size_hint` bounds are equal.
- `Iterate(end)` pushes the next item or jumps to `end`, which must be `PopLoopFrame`. `PushDidNotIterate` feeds `{% else %}`.
- Attributes (`loop_object.rs:210-245`): `index0`, `index`, `length`, `revindex`, `revindex0`, `first`, `last`, `depth`, `depth0`,
  `previtem`, `nextitem`, plus methods `changed(...)` and `cycle(...)` (lines 161-187).
- Recursive loops: `loop(x)` becomes `FastRecurse`/`CallFunction("loop")`. Recursion jumps back to the `PushLoop` pc and remembers the
  return pc in `next_loop_recursion_jump` (`vm/mod.rs:303-342`). Recursing from another stream (an include or block) runs a nested
  evaluation (`recurse_loop_from_other_instructions`, lines 1193-1249).
- `break`/`continue` (feature `loop_controls`) are jumps patched into the loop's pending block (`codegen.rs:440-464`).

### 3.8 Capture, set/filter blocks, include, import

- `{% set x %}...{% endset %}` compiles to `BeginCapture(Capture) body EndCapture [filter] StoreLocal` (`codegen.rs:359-370`).
- `{% filter f %}...{% endfilter %}` compiles to `BeginCapture body EndCapture <filter expr> Emit` (`codegen.rs:380-389`).
- `EndCapture` pushes a safe string when auto-escape is on, a plain string otherwise, and undefined for `Discard` (`output.rs:101-114`).
  `Output` keeps `capture_stack: Vec<Option<String>>` and a raw `target` pointer retargeted on push and pop (`output.rs:33-128`).
- `{% include %}` compiles to `<name or list> Include(ignore_missing)`. At runtime (`vm/mod.rs:1108-1185`):
  - it tries each name; the include shares the **current context frames** (Jinja semantics);
  - failures wrap as `error in "<tmpl>"` (`BadInclude`);
  - a missing template gives `tried to include non-existing template {:?}` / `tried to include one of multiple templates, none of which existed {}`.
- `{% import %}` compiles to `BeginCapture PushWith <expr> Include(false) EndCapture ExportLocals PopFrame StoreLocal` (`codegen.rs:395-404`).
  The imported module's top-level locals, minus `_`-prefixed names, become a `Module` object (`vm/mod.rs:1026-1040`).

### 3.9 Auto-escaping

- Escaping is a **runtime** mode. `State.auto_escape: AutoEscape::{None, Html, Json, Custom}` (`utils.rs:222-...`) is set per template
  by a callback (`initial_auto_escape`).
- `Emit` calls `write_escaped(out, &state.auto_escape, &value)`. Safe strings (`StringType::Safe`) bypass escaping (`utils.rs:198-215`).
- `{% autoescape %}` evaluates its argument and pushes or pops the mode (`vm/mod.rs:835-844, 1367-1386`).
- Adding a plain string to a safe string escapes it in 3.1.0 (`ops::needs_markup_concat`, `vm/mod.rs:648-663`).

### 3.10 Errors and line information

- **Line table** (`instructions.rs:265-269, 383-449`). `Vec<LineInfo { first_instruction: u32, line: u16 }>` gets a record only when
  the line changes (run-length). `get_line(pc)` is a binary search for the greatest `first_instruction <= pc`.
  - With `debug`, a parallel `Vec<SpanInfo>` holds full spans (start/end line and column).
- **Attaching locations**: `process_err(err, pc, state)` (`vm/mod.rs:1496-1527`) runs only if the error has no line yet. It sets the
  template name plus span or line from the current instruction stream, so the innermost failing stream wins.
  - In debug mode it attaches `DebugInfo`: template source and the referenced locals, found by `get_referenced_names` walking the
    instructions backwards (`instructions.rs:479-529`).
  - It also describes undefined origins such as `` `user.name` is undefined (from line 3) `` (lines 1535-1619).
- **Error type** (`error.rs:40-...`): `Error { repr: Box<ErrorRepr { kind, detail, name, lineno, span, source, debug_info, undefined_origin }> }`.
  Display is `{kind}: {detail} (in {name}:{line})` (lines 183-201). Nested failures chain through `with_source`
  (`error in super block`, `error in "<tmpl>"`).

### 3.11 Resource limits

- Recursion: `MAX_RECURSION = 500` (`environment.rs:51`), configurable but capped unless `stacker` is enabled (`environment.rs:676-685`).
  An include costs 10 (`INCLUDE_RECURSION_COST`, `vm/mod.rs:45`) and a macro call costs 4 (`MACRO_RECURSION_COST`, line 49). The error is `recursion limit exceeded`.
- Fuel (feature `fuel`, `vm/fuel.rs`):
  - `UNITS_PER_FUEL = 64`, `INSTRUCTION_COST = 64` units, `BYTE_COST = 1`, `ITEM_COST = 32`, `COMPARE_COST = 64`, `SERIALIZED_BYTE_COST = 16`;
  - output bytes, created values, filter input bytes and comparisons are charged;
  - once exhausted, the tracker stays exhausted.

### 3.12 Contrast: Tera 2.4 VM

VERIFIED from the `tera-2.4.0` crate source:

- Instruction enum (`src/parsing/instructions.rs:9-131`):
  - `LoadConst`, `LoadName`, `LoadAttr`, `LoadAttrOpt`, `BinarySubscript(Opt)`, `Slice(Opt)`;
  - `WriteText`, `WriteTop`, `Set`, `SetGlobal`, `Include`, `BuildMap`, `BuildList`, `BuildMapWithSpreads`, `BuildListWithSpreads`;
  - `CallFunction`, component rendering ops, `ApplyFilter`, `RunTest`, `RenderBlock`;
  - `Jump`, `PopJumpIfFalse`, `JumpIfFalseOrPop`, `JumpIfTrueOrPop`, `Capture`, `EndCapture`;
  - `StartIterate`, `StartIterateComprehension`, `Iterate`, `StoreLocal`, `StoreDidNotIterate`, `Break`, `PopLoop`, `AppendToList`;
  - arithmetic and comparison ops, `Not`, `Negative`;
  - fused super-instructions `LoadPath(Vec<String>)` and `WritePath(Vec<String>)`.
- `Chunk` stores `Vec<(Instruction, Vec<Span>)>`, one span list per instruction (lines 135-...). That is heavier than minijinja's run-length table.
- Inheritance is **static**. `extends` must be a string literal and the first tag: `` `extends` needs to be the first tag of the template ``
  (`src/parsing/parser.rs:1599-1630`). Block lineage is precomputed across all templates at load time (`finalize_templates`, `src/tera.rs:622, 763`).
  This is **incompatible with Twig**, where the parent is an arbitrary runtime expression (`TWIG/src/Node/ModuleNode.php:365-408`).

### 3.13 Contrast: the original twig-rs / `little` design

- `little` 0.3.0 (`...\scratchpad\ext\little-rs\src\lib.rs`) is a 9-instruction machine:
  - `Output{location}`, `Property{name}`, `Push{location}`, `Pop{times}`, `Jump{pc}`, `CondJump{pc, location, test}`, `Call{call, argc, push_result_to_stack}`, `Load{binding, location}`, `Interupt`;
  - operands are `Mem::{Const, Binding, Parameter{name}, Parameters, StackTop1, StackTop2}`;
  - it has a constants pool, a calls table resolved at build time, SHA-1 `Fingerprint`s for caching compiled blobs, and pull-based
    output (`Execute::execute(V) -> impl io::Read`).
- twig-rs lowering (`...\ext\twig-rs-orig\src\instructions\compiler\*.rs`) implemented only `Print(Name)`. Every other node is `unreachable!(...)`.
  Nothing there constrains the new design beyond the intent: AST, then low-level IR, then VM.

---

## 4. Lessons for an in-tree Twig bytecode VM

Each lesson cites Twig 3.31.1 behaviour that the VM must reproduce and says what to take from, or reject in, minijinja.
Confidence is high unless stated.

### 4.1 Context model: copy a PHP array, not a Jinja frame stack

Twig compiles every scope as one PHP array, `$context`:

- **for-loops** (`TWIG/src/Node/ForNode.php:52-122`):
  - `$context['_parent'] = $context;` and `$context['_seq'] = CoreExtension::ensureTraversable(<seq>);`
  - an optional `$context['_iterated'] = false;`
  - the `loop` array: `'parent' => $context['_parent'], 'index0' => 0, 'index' => 1, 'first' => true`, then, if the sequence is an
    array or `Countable`: `revindex0`, `revindex`, `length`, `last`;
  - `foreach`;
  - then `$_parent = $context['_parent']; unset($context['_seq'], $context['<key>'], $context['<value>'], $context['_parent'][, $context['_iterated']][, $context['loop']]); $context = array_intersect_key($context, $_parent); $context += $_parent;`

  Effects: variables that existed before the loop **keep values assigned inside it**; variables first created inside are dropped; loop
  target variables are restored to their pre-loop values; key order changes (surviving keys keep inner order, restored keys are
  appended). minijinja's frames drop all inner assignments, which is wrong for Twig.
- **loop bookkeeping** runs at the **end** of each iteration (`ForLoopNode.php:30-...`): `++index0; ++index; first = false;`, and if
  `revindex0`/`revindex` are set: `--revindex0; --revindex; last = 0 === revindex0`.
  `loop` is an ordinary array value. A template can read `loop.parent`, overwrite `loop`, or iterate it, and key order is observable.
- **blocks**: `block_<name>(array $context, array $blocks)` (`BlockNode.php:31-...`). **includes**: `yield($context)` / `CoreExtension::merge($context, vars)` / `toArray(vars)` for `only` (`IncludeNode.php:99-118`).
  **macros**: a fresh array (`MacroNode.php:79-...`). All of these receive a **copy**, PHP array value semantics.
- **captures** (`{% set x %}`, `{% apply %}`, macro bodies) run in a closure with `use (&$context, $macros, $blocks)` (`CaptureNode.php:30-54`),
  which **shares** the context by reference.
- Design:
  - one `Rc<PhpArray>` context per call frame, `Rc::make_mut` on write (O(1) "copy", copy-on-write like zval refcounting);
  - dedicated opcodes `ForBegin{key, value, has_else, with_loop}`, `ForNext{end}`, `ForLoopUpdate`, `ForEnd{...}` whose effects
    reproduce the exact PHP statements above (including `unset` order and `+=`);
  - captures run in the *same* frame with an output capture (minijinja's `BeginCapture`/`EndCapture` is the right mechanism).
  - Reject minijinja's closures and `Enclose`: Twig macros see only arguments, `varargs` and globals ([4.4](#44-macros)).

### 4.2 Name and attribute access are several opcodes, with explicit lines

- `NameExpression::compile` (`TWIG/src/Node/Expression/NameExpression.php:38-94`) has five forms:
  1. defined-test: `array_key_exists('n', $context)`, or `true` for special and always-defined names;
  2. special variables: `_self`, `_context`, `_charset`;
  3. always-defined: `$context['n']`. The `always_defined` attribute is set by:
     - `OptimizerNodeVisitor` for `loop` and loop-target names inside a loop (`TWIG/src/NodeVisitor/OptimizerNodeVisitor.php:150-159`);
       the same visitor also decides `ForNode`'s `with_loop`, i.e. whether a `loop` array is built at all;
     - `MethodCallExpression` (macro calls) on its object operand;
     - `??` and `?:` on their left operand (`NullCoalesceBinary.php:45`, `ElvisBinary.php:31`).

     So the optimizer must be ported with identical decisions.
  4. lenient, when `ignore_strict_check` or not strict: `($context['n'] ?? null)`;
  5. strict: `(isset(...) || array_key_exists(...)) ? $context['n'] : throw new RuntimeError('Variable "n" does not exist.', <lineno>, $this->source)`.
- Design:
  - `LoadName{name, mode: Defined|Special(k)|Always|Lenient|Strict{line}}`;
  - attribute access maps to one opcode `GetAttr{type: Any|Array|Method, defined_test, ignore_strict, sandboxed, line, argc}` mirroring
    `CoreExtension::getAttribute($env, $source, $object, $item, $arguments, $type, $isDefinedTest, $ignoreStrictCheck, $sandboxed, $lineno)`.
- minijinja's `Undefined` value with lazy failure must **not** be copied. Twig fails, or returns `null`, at lookup time.

### 4.3 Blocks and inheritance: runtime tables of (template, block)

- Parent selection is a runtime expression. A constant parent is loaded eagerly (`$this->parent = $this->load(<const>, <line>)`);
  otherwise `$this->getParent($context)` (`ModuleNode.php:365-408`). The child body runs first, then
  `yield from <parent>->unwrap()->yield($context, array_merge($this->blocks, $blocks));`.
- `Template::yield` does `$context += globals; $blocks = array_merge($this->blocks, $blocks);` (`TWIG/src/Template.php:471-482`).
  Incoming (child) blocks win, and globals never override context keys.
- `yieldBlock($name, $context, $blocks, $useBlocks = true, $templateContext = null)` (`Template.php:487-533`) resolves in this order:
  1. `$blocks[$name]` if `$useBlocks`;
  2. otherwise own `$this->blocks[$name]`, also merging own blocks into `$blocks`;
  3. otherwise the parent's `yieldBlock(..., array_merge($this->blocks, $blocks), false, $templateContext ?? $this)`;
  4. otherwise throw `Block "%s" should not call parent() in "%s" as the block does not exist in the parent template "%s".` or `Block "%s" on template "%s" does not exist.`
- `parent()` maps to `yieldParentBlock` (`Template.php:535-544`): traits (`use`) first, then the parent's `yieldBlock(..., false)`,
  else `The template has no parent and no traits defining the "%s" block.`
- Child templates may not have output outside blocks. `CorrectnessNodeVisitor` throws
  `A template that extends another one cannot include content outside Twig blocks. Did you forget to put the content inside a {% block %} tag?`
  (`TWIG/src/NodeVisitor/CorrectnessNodeVisitor.php:96-107`, throw at line 104). Unlike minijinja's `Discard` capture, the VM never needs to discard child output.
- Design:
  - a block table `IndexMap<BlockName, (TemplateHandle, BlockId)>` per render call, merged exactly like `array_merge`;
  - runtime functions `yield_block` and `yield_parent_block` ported line by line, with their error texts;
  - per template: `blocks`, `traits` (with `use ... with a as b` aliases) and `parent: Option<Expr|Const>`.

  minijinja's `BlockStack` (depth index into appended streams) cannot express `useBlocks=false`, traits or `block()` called with a template argument, so reject it.

### 4.4 Macros

- 3.31.1 compiles each macro to
  `new TwigMacro('name', function (<args with defaults>, ...$varargs): string|Markup { $macros = $this->macros; $context = [<args>, 'varargs' => $varargs] + $this->env->getGlobals(); $blocks = []; return <CaptureNode>; }, <signature>, <variadic>)`
  (`MacroNode.php:79-...`). A macro body sees only its arguments, `varargs` and globals; its output is captured as `''` or a `Markup`.
- Calls go through a `MacroNamespace` (`TWIG/src/MacroNamespace.php:54-69`):
  - own macros (exact name, then case-insensitive with a deprecation);
  - then the parent template's namespace;
  - then the deprecated `macro_` prefix;
  - else `Macro "%s" is not defined in template "%s".`
- Macros imported in a child template's body are unavailable in blocks if the body did not run:
  `Macros imported in the body of template "%s" are not available because the body was not rendered; move the "import" or "from" tag inside the block or the macro that uses it.` (`Template.php:599-602`).
- Design:
  - macro bodies as separate instruction streams (like blocks), not inline-behind-jump; this is simpler with `known_instructions`-free handles;
  - `CallMacro{namespace_slot, name, argc, line}`;
  - no closures.

### 4.5 Loops

There is no `break`/`continue`, no recursive `loop()` and no `loop.changed`/`cycle` in Twig core. The loop variable is a plain array
([4.1](#41-context-model-copy-a-php-array-not-a-jinja-frame-stack)). minijinja's `Loop` object, `FastRecurse` and adjacent-item
iterator are not needed. Take only the `Iterate(end)` + `PopLoopFrame` control shape.

### 4.6 Capture, `apply` and `set`

- `ApplyTokenParser` produces `SetNode(capture=true, <LocalVariable>, body)` followed by `PrintNode(<filter applied to the local>)` (`TWIG/src/TokenParser/ApplyTokenParser.php:36-53`).
- Capture results are `''` for empty output, else `new Markup($tmp, $charset)` (`CaptureNode.php:30-54`).
- `SetNode` with `safe`: a constant `''` stays `""`, other constants become `new Markup(<const>, charset)`, and non-constants become `('' === $tmp = <expr>) ? '' : new Markup(...)` (`SetNode.php:54-...`).
- Design: `BeginCapture`, `EndCapture{as_markup: bool}`, `StoreLocal`. This matches minijinja's mechanics with Twig's value rules.

### 4.7 Escaping: compile-time decision plus runtime `Markup`

- `EscaperNodeVisitor` (`TWIG/src/NodeVisitor/EscaperNodeVisitor.php:35-...`) wraps print expressions in an `escape` filter call
  (`getEscaperFilter`, line 175) unless `isSafeFor($type, $expr)` holds (line 147). Safety comes from filter and function `is_safe`
  metadata and `SafeAnalysisNodeVisitor`.
- At runtime `EscaperRuntime::escape($string, $strategy, $charset, $autoescape)` (`TWIG/src/Runtime/EscaperRuntime.php:118-...`) returns
  a `Markup` unchanged when `$autoescape && $string->isSafeFor($strategy)`. `Markup` carries `safeStrategies` (default `['all']`, `TWIG/src/Markup.php:40-70`).
  For other `Stringable` values it consults `safeClasses`. Non-string, non-Stringable values are returned as-is for `html`, `js`, `css`,
  `html_attr`, `html_attr_relaxed` and `url`.
- Design:
  - port both visitors at the AST level;
  - emit ordinary `ApplyFilter("escape", [strategy, charset, autoescape=true])` (or a dedicated `Escape{strategy}` opcode);
  - `Value::Markup{bytes, charset, safe_strategies}`.

  Reject minijinja's runtime-only `AutoEscape` mode: Twig's `{% autoescape %}` is compile-time.

### 4.8 Error lines must follow Twig's `addDebugInfo` granularity

- Twig records a debug-info entry only in `Compiler::addDebugInfo($node)`, and only when the node's line differs from the last one
  recorded (`TWIG/src/Compiler.php:200-213`). Nodes that call it:
  - BlockNode, BlockReferenceNode, DeprecatedNode, DoNode, FlushNode, ForElseNode, ForNode, IfNode, IncludeNode, MacroNode, ModuleNode,
    PrintNode, SandboxNode, SetNode, TextNode, WithNode;
  - expressions: NameExpression, BlockReferenceExpression, ParentExpression, ArrowFunctionExpression, AssignMacroVariable,
    Object/SequenceDestructuringSetBinary.

  (VERIFIED with `grep -l addDebugInfo`.)
- For exceptions without a line, `Error::guess()` walks the PHP backtrace and takes the debug entry with the greatest PHP line `<=` the
  failing line (`TWIG/src/Error/Error.php:162-215`). Non-Twig exceptions are wrapped as
  `An exception has been thrown during the rendering of a template ("%s").` and guessed (`Template.php:614-631`).
- Design:
  - a minijinja-style run-length table (`first_pc -> line`, binary search), with records emitted **only** at the Twig-equivalent points
    and in Twig's compile order. Emitting per expression, as minijinja's spans do, would report different lines for multi-line statements;
  - errors Twig raises with an explicit line (strict variables, `getAttribute`, macro calls, `include`) carry the line as an instruction operand;
  - message assembly ports `updateRepr` ([1.4](#errors-thiserror)), including ` column %d` support (`Error.php:151-156`).
- Constant folding must not move runtime errors to compile time. For example, `{{ 1/0 }}` must stay a render-time
  `An exception has been thrown during the rendering of a template ("Division by zero").`

  Both parts are VERIFIED: PHP 8.5.13 throws `DivisionByZeroError: Division by zero` (oracle), and Twig wraps it per `Template.php:614-631`.

  Fold only what Twig's own optimizer folds.

### 4.9 Output

Twig 3.31 compiles to generators (`yield`) or `echo`. Either way the observable output is the byte concatenation of yielded scalars and
`Stringable`s, converted with PHP's `(string)`. A `Vec<u8>` sink with a capture stack (minijinja `Output`) is the right shape. Write
`&[u8]`, not `fmt::Write`, because output may be non-UTF-8.

### 4.10 Instruction design advice

- Keep minijinja's practical choices:
  - a `#[repr]`-free Rust enum, about 16-32 bytes, with interned names, `u32` jump targets and `Rc` constants;
  - separate streams per root, block and macro;
  - backpatching with a pending-block stack;
  - filter, function and test resolution cached per stream (minijinja's `LocalId`);
  - pooled frames and buffers.
- Add the Twig-specific opcodes above (`LoadName` modes, `GetAttr` flags, For* family, `CallMacro`, `YieldBlock`, `YieldParentBlock`,
  `Include{only, ignore_missing, with}`, `Embed{index}`, `Sandbox`, `Deprecated`, `Guard`).
- Keep the AST, the node visitors (escaper, safe analysis, optimizer, sandbox, correctness, macro auto-import) and the compile order
  identical to Twig's, then lower. This gives a faithful line table and identical compile-time errors.
- Optional: serialize bytecode (the `little` `Fingerprint` idea) to cache compiled templates. Defer it; there is no parity impact.

### 4.11 Deliberate deviation: recursion limits

PHP 8.5 has no Twig-level recursion limit. Native-stack exhaustion surfaces as PHP's `Error`
`Maximum call stack size of %zu bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion?`
(VERIFIED, `PHPSRC/Zend/zend_execute.c:2617`; the oracle has `zend.max_allowed_stack_size=0`, meaning auto-detect). Twig would then
wrap that error like any other exception.

The byte count depends on the machine, so this can never match exactly. The Rust VM needs a depth limit (minijinja: 500 with
weighted costs) to avoid overflowing the native stack. Document it as a known deviation and make it configurable.

---

## 5. Rust 2026 conventions for this library

### 5.1 Edition 2024 and language/library features

Stabilization versions come from rust-lang/rust `RELEASES.md` (fetched 2026-11-25; it already lists 1.100.1 for 2026-11-27, a
fix for an `Iterator` inference regression). Each feature was also compiled on this machine with stable 1.100.0
(`...\scratchpad\tmp-pcre2\features-check\probe.rs`, `letchains.rs`).

| feature | stable since | local check (1.100.0, `--edition 2024`) |
|---|---|---|
| Edition 2024 | 1.85.0 | `cargo new` writes `edition = "2024"` |
| Cargo: `rust-version` must be >= 1.85 with edition 2024 | 1.85 | `rust-version 1.80 is incompatible with the version (1.85.0) required by the specified edition (2024)` |
| resolver v3 (MSRV-aware) | 1.84.0 | `Locking 12 packages to highest Rust 1.100.0 compatible versions` |
| let-chains (edition 2024 only) | 1.88.0 | ok; edition 2021 gives `error: let chains are only allowed in Rust 2024 or later` |
| `<[T]>::as_chunks`, `HashMap::extract_if` | 1.88.0 | ok |
| `Vec::pop_if`, `get_disjoint_mut`, `f64::next_up`/`next_down`, trait upcasting | 1.86.0 | ok |
| `<iN>::midpoint`, `Vec::extract_if` | 1.87.0 | ok |
| async closures | 1.85.0 | ok |
| `&raw const`/`&raw mut`, `unsafe extern` + `safe fn`, `use<..>` precise capturing | 1.82.0 | ok |
| `#[expect]` lint attribute | 1.81.0 | ok |
| `LazyLock` | 1.80.0 | ok |
| `str::floor_char_boundary` | 1.91.0 | ok |
| `std::fmt::from_fn` | 1.93.0 (listed again under 1.95.0; the reason is not checked) | ok |
| `if let` guards on match arms | 1.95.0 | ok |
| `cfg_select!` | 1.95.0 | ok |
| `core::hint::cold_path` | 1.95.0 | ok |
| never type `!` (fallback changed to `!` on all editions) | 1.100.0 | ok (`Option<!>` compiles) |
| `std::alloc::Allocator` (custom allocators) | 1.100.0 | not probed |
| `u32::from_ascii` | **not stable** | E0599 |

Cargo items relevant to this project (VERIFIED, `RELEASES.md` Cargo sections):

- 1.100: Cargo lints (`[lints.cargo]`, e.g. `unused_dependencies`); dependency cooldowns (`[registry] global-min-publish-age = "7 days"`,
  `[resolver] incompatible-publish-age = "deny"`, VERIFIED in the Cargo config reference); `cargo install` uses the packaged lockfile by
  default; a new build-dir layout.
- 1.99: a built-in `debug` profile. Edition-2024 members can override an inherited `default-features`.
- 1.97: `build.warnings` config (`"warn"|"allow"|"deny"`, env `CARGO_BUILD_WARNINGS`), replacing `-Dwarnings`; `resolver.lockfile-path`; `-m` for `--manifest-path`.
- 1.96: fixes for CVE-2026-5222 and CVE-2026-5223 (Cargo).
- 1.94: config `include`; registry `pubtime`; TOML 1.1 manifests.
- 1.93: `cargo clean --workspace`.
- 1.91: `build.build-dir`.
- 1.90: multi-package publishing (`cargo publish --workspace`).
- 1.88: automatic cache GC.

### 5.2 rust-version / MSRV practice

- Facts (VERIFIED):
  - the resolver honours `rust-version` when locking dependencies;
  - clippy reads `rust-version` and warns with `clippy::incompatible_msrv`, e.g. "current MSRV ... is `1.85.0` but this item is stable since `1.86.0`" (probe `...\tmp-pcre2\lints-check`);
  - minijinja keeps MSRV 1.70 on edition 2021, while mago (Twig/PHP tooling, 2026) sets `rust-version = "1.100.0"` (VERIFIED from its manifest).
- **Recommendation:** `rust-version = "1.88"`, the minimum for let-chains in edition 2024. Revisit when a needed API requires more (if-let guards and `cfg_select!` need 1.95).
  - Trade-off: older distro toolchains can build it, at the cost of not using 1.89-1.100 APIs.
  - Debian 13 ships rustc 1.85 (UNVERIFIED), so 1.88 already excludes it. A library whose users run web apps on rustup stable can afford this.
  - Enforce it with a CI job on `dtolnay/rust-toolchain@1.88.0` running `cargo check --workspace --all-features --locked`.

### 5.3 Workspace layout

Recommendation, with each crate's purpose. Keep the count small at first; splitting later is cheap.

```
twig-rs/
├─ Cargo.toml                # virtual manifest: [workspace] resolver = "3", members, package/deps/lints inheritance
├─ crates/
│  ├─ twig/                  # public crate (crates.io name `twig`): Environment, loaders, Template API, errors
│  │   └─ src/{lexer,parser,ast,visitors,compiler,vm,ext/...}   # start monolithic; split into twig-syntax / twig-vm later if useful
│  ├─ php-rt/                # PHP semantics, no Twig knowledge: Value/zval-like types, PhpArray, type juggling,
│  │                         #   zend_strtod/dtoa layout, sprintf/number_format/round, strcmp/sort, htmlspecialchars,
│  │                         #   mbstring subset, json_encode, date (timelib port + timezonedb data)
│  ├─ php-pcre2-sys/         # vendored PCRE2 10.44 from php-src, built by cc with PHP's config.h; raw FFI only
│  └─ php-regex/             # safe preg_* port (delimiters/modifiers/limits/warnings) over php-pcre2-sys
├─ tests/
│  ├─ fixtures/              # copied from TWIG/tests/Fixtures (BSD-3-Clause, keep LICENSE) + converted DATA/CONFIG
│  └─ integration.rs         # libtest-mimic harness (harness = false)
├─ benches/                  # criterion, separate package so its deps do not affect MSRV
├─ xtask/                    # oracle tooling: run php:8.5.13-cli in Docker, regenerate expectations, convert DATA
├─ deny.toml  rustfmt.toml  clippy.toml  CHANGELOG.md  LICENSE(s)
```

Manifest skeleton. VERIFIED: the same structure, reduced to `bstr`/`memchr` dependencies, passes `cargo +stable clippy` on 1.100.0
(probe `...\tmp-pcre2\vws-check`). The resolver reported `Locking 9 packages to highest Rust 1.88 compatible versions`, and Cargo warned
`unused workspace dependency 'memchr'`: `cargo::unused_workspace_dependencies` is `warn` by default, so only list dependencies that some
member actually uses.

```toml
[workspace]
resolver = "3"                       # required explicitly in a virtual manifest (VERIFIED warning otherwise)
members = ["crates/*", "xtask"]

[workspace.package]
edition = "2024"
rust-version = "1.88"
license = "BSD-3-Clause"             # see 5.9: ported php-src code may require adding PHP-3.01 / Zend-2.0 notices
repository = "https://github.com/<owner>/twig-rs"

[workspace.dependencies]
bstr = "1.14"
memchr = "2.8"
hashbrown = "0.17"
zmij = "1"
thiserror = "2"
libtest-mimic = "0.8"
similar = "3"
cc = "1"

[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "deny"
missing_docs = "warn"
unreachable_pub = "warn"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
must_use_candidate = "allow"

[workspace.lints.cargo]
unused_dependencies = "warn"         # new in 1.100 (VERIFIED working)
```

Member crates use `edition.workspace = true`, `rust-version.workspace = true` and `[lints] workspace = true`.
`unsafe_code = "forbid"` goes in all crates except `php-pcre2-sys` and `php-regex`.

### 5.4 `[lints]` table and clippy

- Syntax VERIFIED on 1.100.0: `[lints.rust]`, `[lints.clippy]` with `{ level, priority }` (group at `priority = -1`, so single-lint overrides win), and `[lints.cargo]`.
- `clippy.toml`: none is needed for MSRV, because clippy reads `rust-version`. Possible keys: `avoid-breaking-exported-api = false`
  (pre-1.0) and `doc-valid-idents`.
- Do not enable the whole `restriction` group. mago does, and has to allow `blanket_clippy_restriction_lints` and about 20 lints to cope (VERIFIED from its manifest).
- Recommended extra lints for a parity engine:
  - `clippy::float_cmp` (warn);
  - `clippy::cast_possible_truncation` / `cast_sign_loss` (warn, because PHP integer and float conversions must be explicit);
  - `clippy::dbg_macro`, `clippy::print_stdout`, `clippy::print_stderr` (deny in library crates).

### 5.5 GitHub Actions CI

Action versions verified on 2026-11-25 with `git ls-remote --tags`:

| action | latest tag |
|---|---|
| `actions/checkout` | v7.0.1 |
| `Swatinem/rust-cache` | v2.9.3 |
| `taiki-e/install-action` | v2.90.8 |
| `EmbarkStudios/cargo-deny-action` | v2.1.4 |
| `rust-lang/crates-io-auth-action` | v1.0.6 |
| `release-plz/action` | v0.5.148 |
| `obi1kenobi/cargo-semver-checks-action` | v2.9 |

`dtolnay/rust-toolchain` has branches per version (`1.85` … `1.100.0`; it pre-creates branches up to `1.120.0`) plus `stable`,
`beta` and `nightly`. Its README (VERIFIED):

- use `uses: dtolnay/rust-toolchain@stable`, `@nightly` or `@1.89.0`;
- with an explicit `toolchain:` input, use `@master`;
- sliding windows are available: `toolchain: stable 18 months ago` and `toolchain: stable minus 8 releases`;
- pin by a full SHA within `master` history.

Recommended workflow (Recommendation):

```yaml
name: ci
on: [push, pull_request]
env:
  CARGO_TERM_COLOR: always
  CARGO_BUILD_WARNINGS: deny          # cargo >= 1.97 replacement for RUSTFLAGS=-Dwarnings (VERIFIED docs)
jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]      # Windows matters: MSVC build of vendored PCRE2, dev machine is Windows
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@stable
        with: { components: "clippy, rustfmt" }
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets --all-features --locked
      - run: cargo test --workspace --all-features --locked
  msrv:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@1.88.0
      - run: cargo check --workspace --all-features --locked
  oracle:                                         # differential tests against PHP; Docker is available on ubuntu runners
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@stable
      - run: docker pull php:8.5.13-cli@sha256:<digest>   # pin: tag 8.5-cli floats to newer patch releases
      - run: cargo xtask oracle-check
  docs:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo doc --workspace --no-deps --all-features
        env: { RUSTDOCFLAGS: "--cfg docsrs -D warnings" }
  deny:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: EmbarkStudios/cargo-deny-action@v2
```

- The `php:8.5.13-cli` tag exists (VERIFIED, `docker manifest inspect`). The local `php:8.5-cli` currently resolves to index digest
  `sha256:dbf32b33595283b79fd1a37ebc1614bdf1b83d3750daad9a57d67dbb9b96ec16` (created 2026-11-24, `PHP_VERSION=8.5.13`).
- Pinning third-party actions by commit SHA is GitHub's security recommendation. Trade-off: more Dependabot churn.

### 5.6 docs.rs metadata

- VERIFIED from `https://docs.rs/about/metadata`:
  - keys: `features`, `all-features`, `no-default-features`, `default-target`, `targets`, `additional-targets`, `rustc-args`, `rustdoc-args`, `cargo-args`;
  - default targets: `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, `aarch64-unknown-linux-gnu`, `i686-pc-windows-msvc`;
  - "Set this to `[]` to only build the default target."
- VERIFIED locally:
  - `#![feature(doc_auto_cfg)]` now fails: `error[E0557]: feature has been removed ... removed in 1.92.0 ... merged into doc_cfg`;
  - `#![cfg_attr(docsrs, feature(doc_cfg))]` produces automatic "Available on crate feature `a` only" badges on nightly rustdoc;
  - Cargo treats `docsrs` as an expected cfg ("expected names are: `docsrs`, `feature`, and `test` and 34 more"), so no `check-cfg` entry is needed.
- Recommendation:

```toml
[package.metadata.docs.rs]
all-features = true
rustdoc-args = ["--cfg", "docsrs"]
targets = []          # pure library: one target is enough (docs.rs default target x86_64-unknown-linux-gnu)
```

With `#![cfg_attr(docsrs, feature(doc_cfg))]` in each `lib.rs`. For `php-pcre2-sys`, docs.rs builds C code. That works with `cc` on
docs.rs (UNVERIFIED for this crate), and the crate must not need network access in `build.rs`.

### 5.7 cargo-deny

- cargo-deny 0.22.1 (2026-11-24, MSRV 1.95). Configuration sections in its own template (VERIFIED, `deny.template.toml` at tag 0.22.1):
  `[graph]`, `[output]`, `[advisories]` (`ignore`; cargo-deny's own config uses `unmaintained = "all"`, `unsound = "all"`), `[licenses]`
  (`allow`, `confidence-threshold`, `exceptions`, `[licenses.private]`), `[bans]` (`multiple-versions`, `wildcards`, `deny` with
  `use-instead`, `skip`, `skip-tree`, `[bans.std-replacements] scope = "workspace"`), and `[sources]` (`unknown-registry`, `unknown-git`, `allow-registry`, `allow-git`, `allow-org`).
- Recommendation:

```toml
[graph]
all-features = true
[advisories]
unmaintained = "all"
unsound = "all"
[licenses]
confidence-threshold = 0.93
allow = ["MIT", "Apache-2.0", "BSD-3-Clause", "Unlicense", "Zlib", "Unicode-3.0"]
# ryu is "Apache-2.0 OR BSL-1.0", so Apache-2.0 already satisfies it; foldhash is Zlib.
# Add "PHP-3.01" and "Zend-2.0" only if the workspace's own crates declare them after porting php-src code (see 5.9).
[bans]
multiple-versions = "warn"
wildcards = "deny"
[bans.std-replacements]
scope = "workspace"
[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

- Caveat: cargo-deny sees only Cargo `license` fields. `pcre2-sys` declares `Unlicense OR MIT` but bundles PCRE2 C code under
  BSD-3-Clause (University of Cambridge header in the sources). An in-tree `php-pcre2-sys` must declare the C license itself, e.g.
  `license = "BSD-3-Clause"` with the PCRE2 notice, and ship the notice.

### 5.8 CHANGELOG and release practice

- CHANGELOG: a "Keep a Changelog" style `CHANGELOG.md` with an `Unreleased` section. Generate it from Conventional Commits with
  `git-cliff` 2.14.2 or maintain it by hand.
- Release automation options, each VERIFIED to be current:
  - `release-plz` 0.4.3 / `release-plz/action` v0.5.148: opens release PRs, bumps versions with `cargo-semver-checks`, updates the changelog and publishes.
  - `cargo-release` 1.1.7 (MSRV 1.97): local, explicit.
  - `cargo publish --workspace` (stable since Cargo 1.90) publishes multiple packages in dependency order.
- Publishing credentials: crates.io **Trusted Publishing**. `rust-lang/crates-io-auth-action@v1` exchanges the GitHub OIDC JWT for a
  temporary token and revokes it in its post step (VERIFIED, action README; tags up to v1.0.6). The crates.io API exposes a per-crate
  `trustpub_only` flag (`false` for `twig`).
- Supply chain: consider Cargo's new dependency cooldown (`[registry] global-min-publish-age`) in CI config. Trade-off: delays security fixes too.
- Semver: run `cargo-semver-checks` (0.51.0) only after 1.0, or from the first public 0.x release.

**Recommendation:** release-plz with Trusted Publishing once a first usable version exists. Until then, use manual `cargo publish`
from the owner account (`twig` is owned by `Nercury`).

### 5.9 Licensing of what will be ported or vendored

All VERIFIED from the license files in the local trees. This is not legal advice.

| source | license | implication |
|---|---|---|
| Twig 3.31.1 (`TWIG/LICENSE`) | BSD-3-Clause, "Copyright (c) 2009-present by the Twig Team" | porting and copying fixtures is fine. Keep the notice. |
| php-src (`PHPSRC/LICENSE`) | PHP License 3.01 | BSD-like, plus clause 3 (no "PHP" endorsement), clause 4 (derived products may not be called "PHP": "Foo for PHP" is fine, "PHP Foo" is not) and clause 6 (acknowledgment "This product includes PHP software, freely available from <http://www.php.net/software/>"). Line-by-line ports of ext/standard etc. are derivative works: keep notices and add the acknowledgment. Crate names such as `php-rt` may conflict with clause 4. **Prefer names without a leading "php"** (e.g. `twig-phpcompat`, `twig-pcre2-sys`) or get permission (Recommendation; low confidence on the legal threshold). |
| Zend engine (`PHPSRC/Zend/LICENSE`) | Zend Engine License 2.00 | BSD-style. Clause 3: "The names "Zend" and "Zend Engine" must not be used to endorse or promote products derived from this software without prior permission". Acknowledgment: "This product includes the Zend Engine, freely available at http://www.zend.com" (lines 18-41). It applies to ports of `zend_operators.c`, `zend_hash.c`, `zend_smart_str.c`. Avoid crate names such as `zend-compat` too. |
| `PHPSRC/Zend/zend_strtod.c` (dtoa/strtod) | David M. Gay / Lucent permissive notice ("Permission to use, copy, modify, and distribute this software for any purpose without fee is hereby granted, provided that this entire notice is included...") | porting `zend_dtoa`/`zend_strtod`/`zend_gcvt` needs this notice kept. The PHP-added parts (e.g. `zend_gcvt`) fall under the Zend license. |
| libmbfl (`PHPSRC/ext/mbstring/libmbfl/LICENSE`) | **LGPL-2.1** | porting libmbfl code (encoding filters) into a statically linked Rust crate creates LGPL obligations. **Avoid deriving from libmbfl**; implement conversions from the encoding specifications and test them against the oracle. |
| timelib (`PHPSRC/ext/date/lib/LICENSE.rst`) | MIT | fine. Keep the notice. |
| PCRE2 (bundled sources, header text) | BSD-3-Clause (University of Cambridge); sljit is its own permissive license (UNVERIFIED which) | fine. Ship notices. |

Given clause 4, the crate names in [5.3](#53-workspace-layout) (`php-rt`, `php-regex`, `php-pcre2-sys`) are placeholders. Final names
should avoid a leading "PHP".

### 5.10 Test-harness implications of the Twig fixture format

VERIFIED from `TWIG/src/Test/IntegrationTestCase.php` and the fixture tree.

- 488 `.test` files in `TWIG/tests/Fixtures`, 33 of them `*.legacy.test`:
  - tags: 195; filters: 72; functions: 70; expressions: 55; macros: 30; tests: 26; exceptions: 14; regression: 10; whitespace: 6; operators: 5; autoescape: 3; extensions: 2.
  - section counts: 103 with `--EXCEPTION--`, 17 with `--CONFIG--`, 21 with `--DEPRECATION--`, 15 with `--CONDITION--` (PHP-version checks such as `\PHP_VERSION_ID >= 80100`);
  - 63 fixtures construct PHP objects (`new ...`) in `--DATA--`.
- Parsing regexes, verbatim:
  - exception form, line 191:
    `/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)\s*(?:--DATA--\s*(.*))?\s*--EXCEPTION--\s*(.*)/sx`
  - output form, line 198:
    `/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)--DATA--.*?--EXPECT--.*/s`
  - outputs, line 204: `/--DATA--(.*?)(?:--CONFIG--(.*?))?--EXPECT--(.*?)(?=\-\-DATA\-\-|$)/s`
  - templates, line 390: `/--TEMPLATE(?:\((.*?)\))?--(.*?)(?=\-\-TEMPLATE|$)/s` (the default name is `index.twig`).
- Run rules:
  - default environment config `['cache' => false, 'strict_variables' => true]`, merged with `--CONFIG--` (lines 251-254);
  - `addGlobal('global', 'global')` (line 261);
  - each additional DATA/EXPECT pair appends `str_repeat(' ', $i)` to every template source to force recompilation (line 257);
  - output is compared as `trim($output, "\n ")` against `trim($expected, "\n ")` (lines 345, 368);
  - load-time exceptions use `assertSame(trim($exception), trim(sprintf('%s: %s', $e::class, $message)))`, and the message must end with `.` or `?`;
  - render-time exceptions use PHPUnit `assertStringMatchesFormat` (`%s`, `%d` and other placeholders; line 348; 1 fixture uses a placeholder);
  - deprecations are compared as `implode("\n", $deprecations)` (line 380).
- Consequences:
  - `--DATA--` and `--CONFIG--` are PHP code. Convert them once, in the oracle (`xtask`), into a typed JSON form, and give PHP test
    classes (from `TWIG/tests/IntegrationTest.php`) Rust equivalents.
  - Exception expectations include the PHP class name (`Twig\Error\SyntaxError: ...`), so Rust errors need a "PHP class name" accessor.
  - A small `StringMatchesFormat` implementation is needed.

---

## 6. Toolchain check on this machine

### 6.1 Rust toolchains (VERIFIED)

- `rustup 1.29.3 (edbc831ce 2026-11-02)`.
- Installed toolchains:
  - `stable-x86_64-pc-windows-msvc`: **rustc 1.100.0 (0ef1bc386 2026-11-09)**, cargo 1.100.0 (3acc79c0c 2026-11-05), LLVM 23.1.5.
    Components: cargo, clippy, rust-docs, rust-src, rust-std (msvc + `x86_64-unknown-linux-gnu`), rustc, rustfmt.
  - `nightly-x86_64-pc-windows-msvc`: **active default**, rustc 1.102.0-nightly (6905905bb 2026-11-19), cargo 1.102.0-nightly.
  - `1.98.1-x86_64-pc-windows-msvc`: rustc 1.98.1 (48a229cea 2026-09-01).
  - `stable-x86_64-unknown-linux-gnu`, `nightly-x86_64-unknown-linux-gnu`: present, but not runnable as host toolchains on Windows (`rustc.exe ... not applicable`).
- Because nightly is the default, use `cargo +stable ...`, or add a `rust-toolchain.toml` (`channel = "stable"` or `"1.100.0"`) to the
  repository so contributors build on stable. All checks in this document used `+stable`.

### 6.2 C toolchain and other tools (VERIFIED)

- Visual Studio Community 2022 17.14.36623.8, including `Microsoft.VisualStudio.Component.VC.Tools.x86.x64` (found via `vswhere`).
  MSVC tools are 14.44.35207 (from `cc`'s link-search output), and the Windows SDK is 10.0.26100.0.
- Also on `PATH`: CMake, Strawberry Perl's `gcc`, `nasm`, `perl` and **`pkg-config`**, Python 3.12.10, Git Bash, Docker (with the `php:8.5-cli` image).
- Strawberry's `pkg-config` has no `libpcre2-8.pc` (VERIFIED: no pcre files under `C:\Strawberry\c\lib`), so `pcre2-sys` fell back to its bundled build.
  - Risk: if a MinGW PCRE2 ever appears there, `pcre2-sys` would try to link a MinGW library into an MSVC build.
  - Mitigation: set `PCRE2_SYS_STATIC=1`, or use the in-tree sys crate, which never probes pkg-config.

### 6.3 pcre2 throwaway project (VERIFIED)

Location: `<scratch>\tmp-pcre2`.

- Steps:
  1. `cargo +stable new --vcs none --name tmp_pcre2 tmp-pcre2` (wrote `edition = "2024"`);
  2. `cargo +stable add pcre2` (resolved `pcre2 v0.2.11`, `pcre2-sys v0.2.10`, `cc v1.11.3`);
  3. add `pcre2-sys` and `libc` for raw FFI, then build with `cargo +stable build -vv` (log in `tmp-pcre2\build-vv.log`).
- Result: **success with no setup**.
  - The `pcre2-sys` build script compiled the bundled C sources with MSVC `cl.exe` (MSVC warnings C4244 and C4127 in the log), found through
    `cc`/`find-msvc-tools`, with no Developer Command Prompt or `vcvars` (`VCINSTALLDIR = None`, `INCLUDE = None`, `LIB = None`).
  - It emitted `cargo:rustc-link-lib=static=pcre2`.
  - A clean rebuild of `pcre2-sys` + `pcre2` + the binary took **1.96 s**.
  - One Rust warning in `pcre2-sys`: `use of deprecated constant std::usize::MAX`.
- Output of `cargo +stable run`:

  ```
  pcre2::version() (compile-time constants in bindings): 10.45
  runtime PCRE2_CONFIG_VERSION: 10.46 2025-08-27
  runtime PCRE2_CONFIG_UNICODE_VERSION: 16.0.0
  JIT available: true
  match: "TWIG 3.31" major="3" minor="31"
  ERR "(abc" opts=0x0: Compilation failed: missing closing parenthesis at offset 4
  ERR "abc)" opts=0x0: Compilation failed: unmatched closing parenthesis at offset 3
  ERR "[a-" opts=0x0: Compilation failed: missing terminating ] for character class at offset 3
  ERR "a{2,1}" opts=0x0: Compilation failed: numbers out of order in {} quantifier at offset 5
  ERR "(?<n>a)(?<n>b)" opts=0x0: Compilation failed: two named subpatterns have the same name (PCRE2_DUPNAMES not set) at offset 12
  ERR "\\" opts=0x0: Compilation failed: \ at end of pattern at offset 1
  ERR "x**" opts=0x0: Compilation failed: quantifier does not follow a repeatable item at offset 2
  ERR "(?P<1a>x)" opts=0x0: Compilation failed: subpattern name must start with a non-digit at offset 4
  ERR "\\p{Foo}" opts=0x80000: Compilation failed: unknown property after \P or \p at offset 7
  ERR "(?<=a+)b" opts=0x0: Compilation failed: length of lookbehind assertion is not limited at offset 0
  ERR "<0xFF byte, printed lossily as U+FFFD>" opts=0x80000: Compilation failed: UTF-8 error: illegal byte (0xfe or 0xff) at offset 0
  ERR "[[:foo:]]" opts=0x0: Compilation failed: unknown POSIX class name at offset 8
  ```

- PHP 8.5.13 oracle on the same patterns (`...\rust-eco\scripts\pcre_errors.php`, delimiter `~`): identical texts and offsets for every
  case **except** `[[:foo:]]`, which PHP reports `at offset 3`. `preg_last_error_msg()` is `Internal error` for compile failures.
  - The `\` case cannot be expressed through PHP delimiters, because `\~` escapes the delimiter.
  - 10.46 ships `pcre2_compile_class.c`, which is absent from PHP's 10.44 `pcre2lib` (file listings). This is consistent with a rewrite of
    class compilation between the versions.

### 6.4 Prototype: building PHP's own PCRE2 10.44 (VERIFIED)

Location: `...\scratchpad\tmp-pcre2\php-pcre2-1044\` (standalone package with an empty `[workspace]`).

- `build.rs`:
  - copies `PHPSRC/ext/pcre/pcre2lib` into `OUT_DIR` (php-src stays untouched);
  - writes a standalone `config.h`: PHP's file minus its first 8 lines (`#include <php_compat.h>` and the `config.w32.h`/`php_config.h`
    selection), prefixed with `HAVE_PCRE_JIT_SUPPORT 1`, `HAVE_MEMMOVE 1`, `HAVE_STDLIB_H 1`, `HAVE_STRING_H 1`;
  - compiles the same 28 files as `PHPSRC/ext/pcre/config.w32` `ADD_SOURCES`, with `PCRE2_CODE_UNIT_WIDTH=8`, `PCRE2_STATIC=1`, `HAVE_CONFIG_H=1`.
- PHP's `config.h` values thereby carried over: `SUPPORT_UNICODE`, `SUPPORT_PCRE2_8`, `SUPPORT_JIT` (through `HAVE_PCRE_JIT_SUPPORT`),
  `HEAP_LIMIT 20000000`, `PARENS_NEST_LIMIT 250`, `MATCH_LIMIT 10000000`, `MATCH_LIMIT_DEPTH MATCH_LIMIT`, `MAX_NAME_COUNT 10000`,
  `MAX_NAME_SIZE 128`, `NEWLINE_DEFAULT 2` (LF), `LINK_SIZE 2`, `MAX_VARLOOKBEHIND 255`.
- Build: 4.5 s from clean, including `cc`, with no warnings shown (`warnings(false)`).
- Output:

  ```
  runtime PCRE2 version: 10.44 2024-06-07 | unicode: 15.0.0 | jit: 1
  ERR "(abc": Compilation failed: missing closing parenthesis at offset 4
  ERR "[[:foo:]]": Compilation failed: unknown POSIX class name at offset 3
  ERR "\\p{Foo}": Compilation failed: unknown property after \P or \p at offset 7
  ```

  This matches the oracle exactly, including the `[[:foo:]]` offset.
- Remaining work for production:
  - ship a copy of the sources in the crate (crates.io packages cannot reference php-src paths);
  - generate `config.h` at build time;
  - add `cargo:rerun-if-changed` entries;
  - test on Linux (gcc/clang) and with JIT disabled targets.

  PHP links against Unicode 15.0.0 tables; no Rust crate provides those for PCRE.

### 6.5 Other experiments on this machine (VERIFIED)

| experiment | file | result |
|---|---|---|
| float digits vs PHP | `tmp-pcre2\src\bin\floatcmp.rs` + `rust-eco\scripts\float_samples.php` | see [1.4 Number formatting](#number-formatting-ryu-zmij-itoa) |
| encoding labels | `tmp-pcre2\src\bin\enccheck.rs` | `iso-8859-1` resolves to `windows-1252` in `encoding_rs` |
| feature probes | `tmp-pcre2\features-check\probe.rs`, `letchains.rs` | table in [5.1](#51-edition-2024-and-languagelibrary-features) |
| `docsrs` cfg / `doc_cfg` | `tmp-pcre2\examples\cfgcheck.rs`, `features-check\doccfg.rs`, `docauto.rs` | [5.6](#56-docsrs-metadata) |
| lints / MSRV / cargo lints | `tmp-pcre2\lints-check\` | [5.2](#52-rust-version--msrv-practice), [5.4](#54-lints-table-and-clippy) |
| virtual workspace resolver | `tmp-pcre2\vws-check\` | warning `virtual workspace defaulting to resolver = "1" despite one or more workspace members being on edition 2024 which implies resolver = "3"` |

### 6.6 Required setup summary

- Windows (this machine): nothing beyond the installed VS 2022 C++ workload. Recommended:
  - `cargo +stable` or a repository `rust-toolchain.toml`, because nightly is the default;
  - optionally `PCRE2_SYS_STATIC=1` if `pcre2-sys` is ever used.
- Linux CI: the `cc` crate uses system `cc`/`gcc` (present on GitHub ubuntu images, UNVERIFIED for the current image). Docker is
  available for the oracle job.
- PHP oracle: pin `php:8.5.13-cli` by digest. `docker run --rm -v <dir>:/s:ro php:8.5.13-cli php /s/script.php`. In Git Bash prefix
  `MSYS_NO_PATHCONV=1` and convert paths with `cygpath -w`, as used in this survey.

---

## 7. Open questions and UNVERIFIED items

1. **nextest with a libtest-mimic harness:** expected to work (documented by both projects) but not run here.
2. **IANA `2026f` (jiff-tzdb) vs PHP timezonedb `2026.6`:** the version letter and number correspond, but zone/link sets and PHP's
   compilation format may differ. Irrelevant if PHP's `timezonedb.h` is embedded as recommended.
3. **sljit license name** and whether PCRE2's "binary-distribution exception" wording matters. Read `PHPSRC/ext/pcre/pcre2lib/sljit/*.h` headers before publishing.
4. **PHP License clause 4 and crate names containing "php":** legal judgement, not verified. The conservative choice is to avoid the prefix.
5. **Debian/Ubuntu shipped rustc versions** (MSRV audience): not checked.
6. **GitHub-wide search for unpublished Rust Twig engines:** not done (API rate limit).
7. **Resolved during the survey:** `Division by zero`, the stack-limit error text and the `+1 month` overflow were checked against the oracle and php-src.
   Statements marked "from memory" elsewhere (e.g. hashbrown 0.15 removing `raw`) remain unverified.
8. **`ubuntu-latest` image contents** (gcc, Docker): standard, but not inspected for the 2026 image.
9. **docs.rs building C code for an in-tree PCRE2 sys crate:** expected to work (many `-sys` crates do), not tested.

---

## Appendix A: artefacts produced by this survey

All under `<scratch>\`:

- `rust-eco\data\crates\*.json`: crates.io crate and owner responses (all crates in sections 1 and 2).
- `rust-eco\data\search\search-*.json`: crates.io search responses (`twig`, `twig template`, `twig php`).
- `rust-eco\data\table-*.md`: generated tables used above.
- `rust-eco\data\float\samples.tsv`: 199,996 PHP float samples (`<hex bits>\t<var_export>\t<(string)>`).
- `rust-eco\scripts\crates_fetch.py`, `crates_summary.py`, `crates_search.py`, `crates_md.py`: crates.io tooling (1 req/s, generic UA).
- `rust-eco\scripts\pcre_errors.php`, `float_samples.php`: oracle scripts.
- `rust-eco\dl-minijinja\minijinja-3.1.0\`: minijinja tag 3.1.0 source.
- `rust-eco\dl-tera\tera-2.4.0\`: tera 2.4.0 crate source.
- `rust-eco\dl-jiff-tzdb\jiff-tzdb-1.0.0\`: jiff-tzdb crate source.
- `rust-eco\dl-rust-releases\RELEASES.md`: rust-lang/rust release notes (master, 2026-11-25).
- `rust-eco\dl-web\`: docs.rs metadata page, Cargo resolver/config reference pages, cargo-deny 0.22.1 `deny.template.toml` and `deny.toml`, crates-io-auth-action README.
- `rust-eco\dl-repos\`: shallow, blob-less clones (rust-web/twig, colin-kiegel/twig-rust, Nercury/twig-rs, Nercury/little-rs, MalteJanz/ludtwig, carthage-software/mago).
- `tmp-pcre2\`: throwaway cargo project with `src\main.rs` (pcre2 check), `src\bin\floatcmp.rs`, `src\bin\enccheck.rs`, and
  `php-pcre2-1044\` (PHP-bundled PCRE2 build), `features-check\`, `lints-check\`, `vws-check\`, `examples\cfgcheck.rs`, `build-vv.log`.
