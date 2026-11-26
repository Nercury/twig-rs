# Code conventions

## Toolchain

- Build and test with `cargo +stable …` (the development machine defaults to nightly). Edition 2024,
  `rust-version = 1.88`.
- There is no hosted CI: every check runs locally, and all of them must pass before a commit is pushed:

  ```bash
  cargo +stable fmt --check
  cargo +stable clippy --workspace --all-targets --all-features -- -D warnings
  cargo +stable test --workspace --all-features
  RUSTDOCFLAGS=-Dwarnings cargo +stable doc --workspace --no-deps --all-features
  cargo +1.88 check --workspace --all-targets --all-features   # MSRV
  cargo deny check                                               # licences, advisories (cargo install cargo-deny)
  ```

  Without `--all-features` the default build must pass too (`cargo +stable test --workspace`). The test suite never
  needs Docker; only regenerating oracle vectors and fuzzing do.
- `unsafe` is forbidden workspace-wide except in `twig-pcre`, which wraps the PCRE2 C library.

## Crates and dependency direction

```
twig ──► twig-date ──► twig-value ──► twig-unicode
  │          └───────► twig-timelib
  ├────► twig-value
  ├────► twig-pcre
  ├────► twig-unicode
  └ ─ ─► twig-iconv (optional, feature `glibc-iconv`; LGPL-2.1-or-later) ──► twig-value

twig-test-support (dev only) ──► twig-value
twig-fuzz (dev only)         ──► twig, twig-value, twig-date, twig-test-support
```

- Nothing depends on `twig`. Low-level crates (`twig-unicode`, `twig-timelib`, `twig-pcre`) depend on nothing in the
  workspace. `twig-test-support` is a dev-dependency only (fixture DATA conversion, PHP test classes).
- The default build stays permissively licensed: code derived from LGPL sources (glibc's iconv) lives only in the
  optional `twig-iconv` crate. Never port LGPL code (glibc, libmbfl) into other crates; derive behaviour from
  specifications, PHP-licensed sources and black-box oracle observations instead.
- Crate names never contain "php" or "zend" (PHP License 3.01 clause 4, Zend Engine License). Module and function
  names may describe the PHP behaviour they reproduce.

## Porting upstream code

- A ported function's doc comment names what it mirrors, for example
  ``/// Port of `zend_compare()` (PHPSRC/Zend/zend_operators.c).`` or
  ``/// Twig: `CoreExtension::getAttribute()` (TWIG/src/Extension/CoreExtension.php).``
  Placeholders `TWIG` and `PHPSRC` are defined in `target.md`.
- Keep upstream's algorithm and evaluation order even when a "nicer" Rust version seems equivalent; equivalence must
  be shown by oracle vectors, not argued.
- Error, warning and deprecation texts are copied verbatim.

## Errors and diagnostics

- PHP exceptions (`TypeError`, `ValueError`, `DivisionByZeroError`, `ArgumentCountError`, `Error`, …) are returned as
  `Err` values that carry the PHP class and the verbatim message.
- PHP diagnostics that do not stop execution (`E_WARNING`, `E_NOTICE`, `E_DEPRECATED`, `E_USER_DEPRECATED`) are
  reported through an explicit diagnostics sink parameter, never printed and never dropped silently inside the
  library. The caller (ultimately the `Environment`) decides the policy.

## Tests and oracle vectors

- Unit tests live next to the code; cross-module tests in the crate's `tests/`.
- Oracle-generated expectations live in `crates/<crate>/tests/vectors/<topic>.json` (or `tests/vectors/` for the
  root crate). The oracle batch or PHP script that generates a vector file is committed next to it as
  `<topic>.gen.json` / `<topic>.gen.php`, and the test file's header says how to regenerate it
  (see `research/php-oracle/README.md`). Keep each vector file below ~5 MB.
- Upstream fixtures in `tests/twig/Fixtures` are never edited.

## Commits

One-line messages describing the change. No plans, worklogs or trailers in commit messages; plans live in `plans/`.
