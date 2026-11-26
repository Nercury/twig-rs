# Product plan (roadmap phase 6)

Turns the engine into a library people can pick up. The parity rounds ran dry with fuzz round 6 (roadmap phase 5 is
done); nothing here may change template behaviour.

## API ergonomics

Open: a derive macro for `Object`, once someone needs one (the guide's hand-written `Object` and `ObjectBuilder`
cover host objects). Changes here also update the guide in the crate documentation (`src/lib.rs`), the examples and
the README.

## Quality

- Benchmarks (`criterion`, root `benches/`): parsing+compiling, rendering loops/inheritance/macros/escaping, compared
  over time; a one-off comparison against PHP Twig through the oracle for the README.
- Profiling pass on the VM and `twig-value` hot paths, with the fixture suite and fuzz vectors as the safety net: a
  first pass is done (`research/benchmarks/README.md`: what changed, before/after numbers, the remaining hot spots).
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

0.2.0 is prepared (versions, crates.io and docs.rs metadata, per-crate licence files and READMEs, `CHANGELOG.md`, the
README's installation section); `cargo package` builds the seven published crates, each far below crates.io's 10 MB
limit. Open:

- Publishing needs the owner's go. Before it: check on crates.io that the owner holds the `twig` name (the 2015
  versions may be there) and that the `twig-*` names are free. Then `cargo publish` in dependency order (twig-unicode,
  twig-timelib, twig-pcre, twig-value, twig-iconv, twig-date, twig), date the CHANGELOG entry, tag `v0.2.0`, and
  update the README's status bullet, which still says that nothing is published. Until then the installation section
  names a version that is not on crates.io.
- twig-iconv's publication (see the licensing section) gates twig's `glibc-iconv` feature: crates.io refuses
  dependencies that are not published, optional ones included, so without twig-iconv the feature and its optional
  dependency have to be removed before twig is published.
