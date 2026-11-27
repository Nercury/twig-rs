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
- Profiling pass on the VM and `twig-value` hot paths, with the fixture suite and fuzz vectors as the safety net: two
  passes are done (`research/benchmarks/README.md`: what changed, before/after numbers, the remaining hot spots).
- No hosted CI for now (owner's decision): the local check list in `reference/conventions.md` (stable, MSRV 1.88,
  all features, docs, `cargo deny check`) replaces it.

## Licensing of the default build

The default build is permissively licensed. glibc's exact `iconv()` (code that follows glibc's LGPL sources) is the
opt-in crate `twig-iconv` (LGPL-2.1-or-later), which twig's `glibc-iconv` feature makes the default iconv backend of
environments (`EnvironmentOptions::iconv_backend` sets one explicitly). Without it, environments use twig-value's
clean-room converter (`charset_basic`: the Unicode charsets, ASCII, ISO-8859-1, every single-byte charset of glibc
(218 tables, EBCDIC and ISO 646 included), the CJK multi-byte charsets (SJIS, CP932, EUC-JP, EUC-JP-MS, GBK, GB18030,
EUC-CN, BIG5, BIG5-HKSCS, EUC-KR, UHC, JOHAB, EUC-TW) and ISO-2022-JP, -JP-2 and -KR, 1,093 of glibc's 1,180 names,
all learned black-box from the oracle and compared with the Unicode Consortium's and WHATWG's mapping files; other
charsets throw twig-rs's "not supported" error, a deviation of the default build). Done; open:

- a legal review of the combination inside `twig-iconv`: its port of PHP's ext/iconv layer is PHP-3.01 code in an
  LGPL crate, and the FSF regards the PHP License as incompatible with the GPL family;
- whether `twig-iconv` is published: its vector tests compile twig-value's test support with `#[path]`, so they only
  run in the workspace;
- the clean-room converter could grow further, shrinking the default build's deviation: ISO-2022-CN and
  ISO-2022-CN-EXT (GB 2312 and the CNS 11643 planes are there, ISO-IR-165 is not), UTF-7 and UTF-7-IMAP, JIS X 0213
  (EUC-JISX0213, SHIFT_JISX0213, ISO-2022-JP-3), the EBCDIC double-byte codepages (IBM930 to IBM1399, stateful),
  IBM932 and IBM943, and the charsets with combining prefixes (ISO_6937, T.61, ANSI_X3.110); each needs a model of
  its own, measured like `multibyte.gen.php` and `iso2022.rs` do.

## Release

0.2.0 is prepared (versions, crates.io and docs.rs metadata, per-crate licence files and READMEs, `CHANGELOG.md`, the
README's installation section); `cargo package` builds the seven published crates, each far below crates.io's 10 MB
limit. Open:

- Publishing needs the owner's go. Before it: check on crates.io that the owner holds the `twig` name (the 2015
  versions may be there) and that the `twig-*` names are free, and bring the revived code to `master`, the
  repository's default branch, which still has the 2015 code: crates.io resolves the relative links of twig's README
  against it, and the guide links to `reference/deviations.md` there. Then `cargo publish` in dependency order
  (twig-unicode, twig-timelib, twig-pcre, twig-value, twig-iconv, twig-date, twig), date the CHANGELOG entry, tag
  `v0.2.0`, and update the README's status bullet, which still says that nothing is published, and the git dependency
  lines of the README's installation section and of twig-iconv's crate documentation (the `revive` branch). Until
  then the installation section names a version that is not on crates.io.
- twig-iconv's publication (see the licensing section) gates twig's `glibc-iconv` feature: crates.io refuses
  dependencies that are not published, optional ones included, so without twig-iconv the feature and its optional
  dependency have to be removed before twig is published.
