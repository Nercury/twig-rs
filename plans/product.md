# Product plan (roadmap phase 6)

Turns the engine into a library people can pick up. Starts when the parity rounds run dry; nothing here may change
template behaviour.

## API ergonomics

Each item also updates the guide in the crate documentation (`src/lib.rs`), the examples that use the old form and
the README.

- **Contexts from any data**: the render methods (`Environment::{render, render_bytes, display}`,
  `TemplateWrapper::{render, render_bytes, display, render_block, render_block_bytes, display_block}` and `Sandbox`'s)
  take `context: impl Into<Array>` instead of `Array`, so that a `BTreeMap`, a `HashMap` or a `[(K, V); N]` is a
  context without `Array::from`.
- **Rendering conveniences**: `Environment::render_string(source, context)` (a `template_from_string` equivalent
  without registering a name), and `display` into any `std::io::Write` (today it takes a `vm::Sink`, so a writer is
  wrapped in `vm::IoSink`).
- **Host objects**: documented `Object` implementation guide with a worked example; an `ObjectBuilder` for simple
  property/method bags. A derive macro is out of scope until someone needs it. (`examples/sandbox.rs` implements
  `Object` by hand for a class with two methods.)

## Quality

- Benchmarks (`criterion`, root `benches/`): parsing+compiling, rendering loops/inheritance/macros/escaping, compared
  over time; a one-off comparison against PHP Twig through the oracle for the README.
- Profiling pass on the VM and `twig-value` hot paths, with the fixture suite and fuzz vectors as the safety net.
- No hosted CI for now (owner's decision): the local check list in `reference/conventions.md` (stable, MSRV 1.88,
  all features, docs, `cargo deny check`) replaces it.

## Licensing of the default build

The default build is permissively licensed. glibc's exact `iconv()` (code that follows glibc's LGPL sources) is the
opt-in crate `twig-iconv` (LGPL-2.1-or-later), which twig's `glibc-iconv` feature makes the default iconv backend of
environments (`EnvironmentOptions::iconv_backend` sets one explicitly). Without it, environments use twig-value's
clean-room converter (`charset_basic`: the Unicode charsets, ASCII and ISO-8859-1; other charsets throw twig-rs's
"not supported" error, a deviation of the default build). Done; open:

- a legal review of the combination inside `twig-iconv`: its port of PHP's ext/iconv layer is PHP-3.01 code in an
  LGPL crate, and the FSF regards the PHP License as incompatible with the GPL family;
- whether `twig-iconv` is published: its vector tests compile twig-value's test support with `#[path]`, so they only
  run in the workspace;
- the clean-room converter could grow (the ISO-8859 family and the Windows codepages are the likely next charsets),
  shrinking the default build's deviation.

## Release

- `NOTICE` listing every third-party source and licence: Twig (BSD-3-Clause; ported logic and vendored fixtures), PHP
  (PHP License 3.01) and the Zend Engine (Zend Engine License 2.00) for ported php-src logic, timelib (MIT), PCRE2
  (BSD-3-Clause) and sljit (BSD-2-Clause), Unicode data (Unicode License v3), the IANA time zone database (public
  domain), musl/ARM optimized-routines (MIT), glibc's iconv (LGPL-2.1-or-later, only the optional `twig-iconv`) and
  its measured tables, xxHash via `xxhash-rust`.
- Per-crate licence expressions reviewed against what each crate contains.
- CHANGELOG, versions (0.2.0 for every crate), crates.io metadata, docs.rs settings (with the `serde` feature, so
  that `to_context` and `to_value` are documented). The README's installation section then names the crates.io
  version. Publishing needs the owner's go.
