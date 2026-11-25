//! The PHP builtin functions Twig calls at runtime (reference/php/stdlib.md), in coercive
//! (`strict_types=0`) mode: argument parsing with [`param`](crate::param), PHP's diagnostics and
//! exceptions, results as [`Value`](crate::Value)s.
//!
//! * [`format`](mod@format): number and string formatting (`sprintf`, `number_format`, `round`);
//! * [`string`](mod@string): string functions (`strtr`, `trim`, `nl2br`, `strip_tags`, ...);
//! * [`array`](mod@array): array functions (`range`, sorting, `min`/`max`, ...).

pub mod array;
pub mod format;
pub mod string;
