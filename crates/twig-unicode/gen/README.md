# twig-unicode table generator

Converts PHP mbstring's generated Unicode data, `PHPSRC/ext/mbstring/unicode_data.h`, into
`crates/twig-unicode/src/tables.rs`. The arrays are copied value for value and in the same order,
so the lookups ported from `PHPSRC/ext/mbstring/php_unicode.c` (`prop_lookup()` binary search,
`mph_lookup()` minimal perfect hash) read the Rust tables exactly as PHP reads the C ones.

php-src itself generates `unicode_data.h` with `PHPSRC/ext/mbstring/ucgendat/ucgendat.php` from the
Unicode Character Database. php-src 8.5.13 ships the Unicode 17.0 version. The generator checks the
header's shape (37 properties, sorted and disjoint ranges, hash table sizes, special-casing indexes)
and stops if a php-src upgrade changed it.

## Regenerating

Run it by hand when the PHP target changes (see `reference/target.md`), from the repository root:

```bash
git clone --depth 1 --branch php-8.5.13 https://github.com/php/php-src.git /tmp/php-src
cargo +stable run --manifest-path crates/twig-unicode/gen/Cargo.toml -- \
    /tmp/php-src/ext/mbstring/unicode_data.h crates/twig-unicode/src/tables.rs
```

Then regenerate the oracle vectors in `crates/twig-unicode/tests/vectors/` and run
`cargo +stable test -p twig-unicode`.

This directory is its own Cargo workspace (empty `[workspace]` table), so it is not built with
twig-rs and adds nothing to the published crate.

## Licences

- The generated data derives from the Unicode Character Database, Copyright (c) Unicode, Inc.,
  Unicode License v3 (`Unicode-3.0`).
- Table layout and lookup scheme: php-src (PHP License 3.01). `ucgendat.php` is based on OpenLDAP's
  `ucgendat.c` and on ucdata-2.5 (Copyright 2001 Computing Research Labs, New Mexico State
  University, MIT-style notice).
- No libmbfl (LGPL) code or data is used.
- This tool: BSD-3-Clause, like twig-rs.
