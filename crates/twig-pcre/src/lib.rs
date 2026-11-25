//! PCRE2 10.44, built exactly as PHP 8.5.13 bundles it, with PHP's `preg_match()` front end, for Twig's
//! `matches` operator.
//!
//! Layers:
//!
//! - [`pcre2`]: a safe wrapper over the vendored C library (`pcre2/`, a copy of
//!   `PHPSRC/ext/pcre/pcre2lib` compiled with PHP's own `config.h`: Unicode on, newline LF, link size 2,
//!   JIT on). It knows nothing about PHP.
//! - [`preg`]: a port of `PHPSRC/ext/pcre/php_pcre.c`: delimiter and modifier parsing, the warning texts,
//!   the compiled-pattern cache, PHP's match and depth limits (`pcre.backtrack_limit=1000000`,
//!   `pcre.recursion_limit=100000`), the JIT stack (32 KiB to 192 KiB), `preg_match()` with and without
//!   `$matches`, `preg_last_error()`, `preg_last_error_msg()` and `preg_quote()`. A [`preg::Pcre`] is the
//!   state one PHP thread keeps between calls.
//! - Twig helpers: [`matches()`] (`CoreExtension::matches()`) and [`validate_constant_regexp`] /
//!   [`null_constant_regexp_error`] (`MatchesBinary`'s compile-time check), which return the exact
//!   messages Twig builds.
//!
//! The free functions use a per-thread [`preg::Pcre`], mirroring PHP's per-thread pcre globals; use
//! [`with_thread_state`] or an explicit [`preg::Pcre`] for anything else.
//!
//! ```
//! assert_eq!(twig_pcre::matches(b"/^\\d+$/", b"123"), Ok(1));
//! let err = twig_pcre::matches(b"/(/", b"x").unwrap_err();
//! assert_eq!(
//!     err.message(b"/(/"),
//!     b"Regexp \"/(/\" passed to \"matches\" is not valid: Compilation failed: missing closing parenthesis at offset 1"
//! );
//! assert_eq!(
//!     twig_pcre::validate_constant_regexp(b"abc").unwrap_err(),
//!     b"Regexp \"abc\" passed to \"matches\" is not valid: Delimiter must not be alphanumeric, backslash, or NUL byte."
//! );
//! ```
//!
//! # Fidelity
//!
//! Results, warnings, `preg_last_error()` codes and Twig messages are checked against PHP 8.5.13 and Twig
//! 3.31.1 with oracle vectors (`tests/vectors/`), including the exact points where the backtrack limit, the
//! recursion limit and the JIT stack limit are reached, both with PHP's default `pcre.jit=1` and with
//! `pcre.jit=0` (PCRE2's interpreter). Known differences, all outside normal use:
//!
//! - PHP's `IS_STR_VALID_UTF8` string flag is not visible in a byte slice; see [`SubjectUtf8`]. It only
//!   changes results for `/u` patterns containing `(*NOTEMPTY)` or `(*NOTEMPTY_ATSTART)`.
//! - A pattern that switches UTF mode on by itself (`(*UTF)` without the `u` modifier) matched against
//!   invalid UTF-8 is undefined behaviour in PHP (no UTF check, JIT reads invalid UTF-8); here it fails
//!   with PCRE2's UTF-8 error (`preg_last_error()` = `PREG_BAD_UTF8_ERROR`).
//! - Allocation failures: PHP refuses to start without a JIT stack and returns `false` with
//!   `PREG_INTERNAL_ERROR` when `memory_limit` stops it allocating match data; here the JIT is switched off
//!   and allocation failures of contexts and match data panic.
//! - Builds without the `jit` feature (or for an architecture sljit does not support) behave like PHP with
//!   `pcre.jit=0`, which reaches the backtrack/recursion limits at other points than the default
//!   JIT-enabled configuration and honours `(*NOTEMPTY)` where the JIT fast path ignores it.
//!
//! # Stack use
//!
//! `pcre2_match()` keeps its backtracking frames on the heap and the JIT runs on its own stack, but
//! `pcre2_compile()` and `pcre2_jit_compile()` recurse on the machine stack, bounded by
//! `PARENS_NEST_LIMIT` (250). The deepest valid pattern needs between 256 KiB and 384 KiB of stack
//! (measured on Windows x64); `tests/threads.rs` checks that 512 KiB is enough.
//!
//! The vendored C sources are unchanged; `pcre2_printint.c` (not compiled by PHP) is left out.
//! `pcre2/LICENCE` holds the PCRE2 (BSD-3-Clause) and sljit (BSD-2-Clause) licences.

#![cfg_attr(docsrs, feature(doc_cfg))]

mod ffi;
pub mod pcre2;
pub mod preg;

use std::cell::RefCell;

pub use preg::{Diagnostic, DiagnosticLevel, MatchesError, Pcre, PregError, SubjectUtf8};

thread_local! {
    static THREAD_STATE: RefCell<Pcre> = RefCell::new(Pcre::new());
}

/// Runs `f` with this thread's PHP PCRE state (cache, last error, `pcre.*` settings).
///
/// # Panics
///
/// Panics when called re-entrantly from inside `f`.
pub fn with_thread_state<R>(f: impl FnOnce(&mut Pcre) -> R) -> R {
    THREAD_STATE.with_borrow_mut(f)
}

/// `preg_match($pattern, $subject)` with this thread's state: `Some(0 | 1)`, or `None` for `false`.
/// Warnings go to `diagnostics`; after `None` without a warning, [`preg_last_error`] tells why.
///
/// `diagnostics` runs while this thread's state is borrowed, so it must not call this crate's free
/// functions (they would panic, see [`with_thread_state`]).
pub fn preg_match(
    pattern: &[u8],
    subject: &[u8],
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Option<i64> {
    with_thread_state(|p| p.preg_match(pattern, subject, diagnostics))
}

/// `preg_last_error()` for this thread's state.
pub fn preg_last_error() -> PregError {
    with_thread_state(|p| p.last_error())
}

/// `preg_last_error_msg()` for this thread's state.
pub fn preg_last_error_msg() -> &'static str {
    preg_last_error().message()
}

/// Twig's `matches` operator at run time (Twig: `CoreExtension::matches()`), with this thread's state.
/// Returns PHP's `int` result (0 or 1), or the error whose [`MatchesError::message`] is the text of the
/// `RuntimeError` Twig throws. Pass `b""` for a null subject (`$str ?? ''`).
pub fn matches(regexp: &[u8], subject: &[u8]) -> Result<i64, MatchesError> {
    with_thread_state(|p| p.twig_matches(regexp, subject))
}

/// Twig's compile-time validation of a constant `matches` regexp (Twig: `MatchesBinary::__construct()`),
/// with this thread's state. On failure returns the message of the `SyntaxError` Twig throws (before
/// `Error::updateRepr()` adds the template name and line), e.g.
/// `Regexp "/(/" passed to "matches" is not valid: Compilation failed: missing closing parenthesis at offset 1.`
pub fn validate_constant_regexp(regexp: &[u8]) -> Result<(), Vec<u8>> {
    with_thread_state(|p| p.twig_validate_constant(regexp))
}

/// The `SyntaxError` message of Twig's compile-time check for a `null` constant regexp
/// (`{{ x matches null }}`), which PHP's null-argument deprecation produces before any regex parsing:
/// `Regexp "" passed to "matches" is not valid: Passing null to parameter #1 ($pattern) of type string is
/// deprecated.` The PCRE state is not touched. See [`preg::twig_null_constant_error`]; other non-string
/// constants go through PHP's string conversion and [`validate_constant_regexp`].
pub fn null_constant_regexp_error() -> Vec<u8> {
    preg::twig_null_constant_error()
}
