# Product plan (roadmap phase 6)

Turns the engine into a library people can pick up. Starts when the parity rounds run dry; nothing here may change
template behaviour.

## API ergonomics

- **Top-level re-exports**: `twig::{Environment, EnvironmentOptions, Error, Result, Source, Value, Array}` and the
  loaders (`twig::loader::{ArrayLoader, FilesystemLoader, ChainLoader}`), so a first program needs one `use`.
- **Context building**: `From` conversions into `Value` for Rust scalars, strings, `Option`, slices/`Vec`,
  `HashMap`/`BTreeMap`/`IndexMap` with string or integer keys (PHP key normalisation applies), and a `context!`
  macro (`context! { name => "World", items => vec![1, 2] }` → `Array`). `u64`/`u128` above `i64::MAX` become floats,
  like PHP's `json_decode`.
- **serde** (optional feature `serde` on `twig-value`, re-exported by `twig`): `to_value(&impl Serialize)` producing PHP
  arrays (structs and maps → string-keyed arrays, sequences → lists, unit/`None` → null, enums like serde_json's
  external tagging). `render` accepts anything convertible into a context `Array`.
- **Simple callables**: `TwigFilter::from_fn` / `TwigFunction::from_fn` / `TwigTest::from_fn` taking a closure and a
  parameter list, building the `PhpCallable` + runtime pair; options via a builder (`is_safe`, `needs_context`, …).
  The full `Call` API stays for advanced cases.
- **Rendering conveniences**: `Environment::render_string(source, context)` (a `template_from_string` equivalent
  without registering a name), and `display` into any `std::io::Write`.
- **Host objects**: documented `Object` implementation guide with a worked example; an `ObjectBuilder` for simple
  property/method bags. A derive macro is out of scope until someone needs it.

## Documentation and examples

- Crate docs: overview, compatibility statement (target versions, link to deviations), quick start, data passing,
  autoescaping, extending (filters/functions/tests, extensions, token parsers), loaders, sandbox, errors and
  diagnostics policy, thread safety.
- `examples/`: `hello` (ArrayLoader), `filesystem` (FilesystemLoader with namespaces), `custom_filter`,
  `extension` (filters + a token parser), `sandbox`, `serde_context`. Each compiled in CI.
- README: status, install, quick start, compatibility, layout, contributing pointer (oracle + fuzzing workflow).

## Quality

- Benchmarks (`criterion`, root `benches/`): parsing+compiling, rendering loops/inheritance/macros/escaping, compared
  over time; a one-off comparison against PHP Twig through the oracle for the README.
- Profiling pass on the VM and `twig-value` hot paths, with the fixture suite and fuzz vectors as the safety net.
- CI: Linux, Windows and macOS (stable) plus MSRV 1.88 on Linux; `cargo-deny` (licences, advisories); docs build;
  fuzz regression vectors run as normal tests (they need no Docker).

## Release

- `NOTICE` listing every third-party source and licence: Twig (BSD-3-Clause; ported logic and vendored fixtures), PHP
  (PHP License 3.01) and the Zend Engine (Zend Engine License 2.00) for ported php-src logic, timelib (MIT), PCRE2
  (BSD-3-Clause) and sljit (BSD-2-Clause), Unicode data (Unicode License v3), the IANA time zone database (public
  domain), musl/ARM optimized-routines (MIT), glibc-measured iconv tables (data), xxHash via `xxhash-rust`.
- Per-crate licence expressions reviewed against what each crate contains.
- CHANGELOG, versions (0.2.0 for every crate), crates.io metadata, docs.rs settings. Publishing needs the owner's go.
