# Compatibility target

twig-rs reproduces one exact upstream configuration. Every behavioural question is answered against it.

| Component | Pinned version | Source |
|---|---|---|
| Twig | v3.31.1 (commit `6f6ec37…`, see `research/php-oracle/composer.lock`) | github.com/twigphp/Twig |
| PHP | 8.5.13 CLI, official image `php:8.5-cli@sha256:dbf32b33595283b79fd1a37ebc1614bdf1b83d3750daad9a57d67dbb9b96ec16` | php-src tag `php-8.5.13` |
| PCRE2 | 10.44 (bundled in php-src `ext/pcre/pcre2lib`, Unicode 15.0.0, JIT on) | php-src |
| timelib | 2022.17 (php-src `ext/date/lib`) | php-src |
| timezonedb | 2026.6 (598 zones, `ext/date/lib/timezonedb.h`) | php-src |
| mbstring Unicode tables | 17.0 (`ext/mbstring/unicode_data.h`) | php-src |
| xxHash (for `hash('xxh128')`) | 0.8.1 | php-src |

## PHP runtime settings of the oracle

The official image ships no `php.ini`, so PHP built-in defaults apply:
`precision=14`, `serialize_precision=-1`, `date.timezone=UTC`, `error_reporting=E_ALL`, `zend.assertions=1`,
64-bit `zend_long`, `LC_CTYPE=C.UTF-8` (other locale categories `C`), no intl, no xdebug, `PHP_SAPI=cli`
(this makes `dump()` output HTML-safe). The oracle only raises `memory_limit`.

## Twig defaults

`debug=false`, `charset='UTF-8'`, `strict_variables=false`, `autoescape='html'`, `cache=false`,
`auto_reload=debug`, `optimizations=-1`, `use_yield=false`. Default extensions in order: Core, Escaper,
YieldNotReady, Optimizer.

## Upgrading the target

1. Bump the versions in `research/php-oracle` (Dockerfile digest, composer constraints) and rebuild the oracle.
2. Diff Twig `src/` and `tests/Fixtures` between the old and new tags; port the changes and re-vendor
   `tests/twig/Fixtures`.
3. Regenerate `tests/twig/oracle-corpus.json` and every oracle vector file; review the diffs.
4. Update this file and the `reference/` documents whose facts changed.

## Path placeholders used in reference documents

- `TWIG/…` — twigphp/Twig at tag v3.31.1.
- `PHPSRC/…` — php/php-src at tag php-8.5.13.
- `<scratch>/…` — temporary files from the original survey sessions; not kept. Their key evidence is quoted inline.
