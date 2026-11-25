//! Safe wrapper over the vendored PCRE2 10.44 (8-bit code units).
//!
//! This layer adds no PHP behaviour: it compiles patterns with explicit options, runs `pcre2_match()` /
//! `pcre2_jit_match()` with explicit match contexts and exposes the offset vector. The PHP front end
//! (delimiters, modifiers, limits, warnings) lives in [`crate::preg`].

use core::ffi::{c_int, c_void};
use core::ptr::{self, NonNull};
use std::fmt;

use crate::ffi;

/// `PCRE2_UNSET`: offset of a capture group that did not participate in the match.
pub const UNSET: usize = ffi::PCRE2_UNSET;

/// Options accepted by [`Code::compile`] (`pcre2_compile()` option bits).
pub mod compile_options {
    use crate::ffi;

    /// `PCRE2_ANCHORED`.
    pub const ANCHORED: u32 = ffi::PCRE2_ANCHORED;
    /// `PCRE2_NO_UTF_CHECK` (pattern is known to be valid UTF).
    pub const NO_UTF_CHECK: u32 = ffi::PCRE2_NO_UTF_CHECK;
    /// `PCRE2_ENDANCHORED`.
    pub const ENDANCHORED: u32 = ffi::PCRE2_ENDANCHORED;
    /// `PCRE2_ALLOW_EMPTY_CLASS`.
    pub const ALLOW_EMPTY_CLASS: u32 = ffi::PCRE2_ALLOW_EMPTY_CLASS;
    /// `PCRE2_ALT_BSUX`.
    pub const ALT_BSUX: u32 = ffi::PCRE2_ALT_BSUX;
    /// `PCRE2_AUTO_CALLOUT`.
    pub const AUTO_CALLOUT: u32 = ffi::PCRE2_AUTO_CALLOUT;
    /// `PCRE2_CASELESS` (PHP modifier `i`).
    pub const CASELESS: u32 = ffi::PCRE2_CASELESS;
    /// `PCRE2_DOLLAR_ENDONLY` (PHP modifier `D`).
    pub const DOLLAR_ENDONLY: u32 = ffi::PCRE2_DOLLAR_ENDONLY;
    /// `PCRE2_DOTALL` (PHP modifier `s`).
    pub const DOTALL: u32 = ffi::PCRE2_DOTALL;
    /// `PCRE2_DUPNAMES` (PHP modifier `J`).
    pub const DUPNAMES: u32 = ffi::PCRE2_DUPNAMES;
    /// `PCRE2_EXTENDED` (PHP modifier `x`).
    pub const EXTENDED: u32 = ffi::PCRE2_EXTENDED;
    /// `PCRE2_FIRSTLINE`.
    pub const FIRSTLINE: u32 = ffi::PCRE2_FIRSTLINE;
    /// `PCRE2_MATCH_UNSET_BACKREF`.
    pub const MATCH_UNSET_BACKREF: u32 = ffi::PCRE2_MATCH_UNSET_BACKREF;
    /// `PCRE2_MULTILINE` (PHP modifier `m`).
    pub const MULTILINE: u32 = ffi::PCRE2_MULTILINE;
    /// `PCRE2_NEVER_UCP`.
    pub const NEVER_UCP: u32 = ffi::PCRE2_NEVER_UCP;
    /// `PCRE2_NEVER_UTF`.
    pub const NEVER_UTF: u32 = ffi::PCRE2_NEVER_UTF;
    /// `PCRE2_NO_AUTO_CAPTURE` (PHP modifier `n`).
    pub const NO_AUTO_CAPTURE: u32 = ffi::PCRE2_NO_AUTO_CAPTURE;
    /// `PCRE2_NO_AUTO_POSSESS`.
    pub const NO_AUTO_POSSESS: u32 = ffi::PCRE2_NO_AUTO_POSSESS;
    /// `PCRE2_NO_DOTSTAR_ANCHOR`.
    pub const NO_DOTSTAR_ANCHOR: u32 = ffi::PCRE2_NO_DOTSTAR_ANCHOR;
    /// `PCRE2_NO_START_OPTIMIZE`.
    pub const NO_START_OPTIMIZE: u32 = ffi::PCRE2_NO_START_OPTIMIZE;
    /// `PCRE2_UCP` (part of PHP modifier `u`).
    pub const UCP: u32 = ffi::PCRE2_UCP;
    /// `PCRE2_UNGREEDY` (PHP modifier `U`).
    pub const UNGREEDY: u32 = ffi::PCRE2_UNGREEDY;
    /// `PCRE2_UTF` (part of PHP modifier `u`).
    pub const UTF: u32 = ffi::PCRE2_UTF;
    /// `PCRE2_NEVER_BACKSLASH_C` (part of PHP modifier `u`).
    pub const NEVER_BACKSLASH_C: u32 = ffi::PCRE2_NEVER_BACKSLASH_C;
    /// `PCRE2_ALT_CIRCUMFLEX`.
    pub const ALT_CIRCUMFLEX: u32 = ffi::PCRE2_ALT_CIRCUMFLEX;
    /// `PCRE2_ALT_VERBNAMES`.
    pub const ALT_VERBNAMES: u32 = ffi::PCRE2_ALT_VERBNAMES;
    /// `PCRE2_USE_OFFSET_LIMIT`.
    pub const USE_OFFSET_LIMIT: u32 = ffi::PCRE2_USE_OFFSET_LIMIT;
    /// `PCRE2_EXTENDED_MORE`.
    pub const EXTENDED_MORE: u32 = ffi::PCRE2_EXTENDED_MORE;
    /// `PCRE2_LITERAL`.
    pub const LITERAL: u32 = ffi::PCRE2_LITERAL;
    /// `PCRE2_MATCH_INVALID_UTF`.
    pub const MATCH_INVALID_UTF: u32 = ffi::PCRE2_MATCH_INVALID_UTF;
}

/// Extra options set on a [`CompileContext`] (`pcre2_set_compile_extra_options()` bits).
pub mod extra_options {
    use crate::ffi;

    /// `PCRE2_EXTRA_ALLOW_SURROGATE_ESCAPES`.
    pub const ALLOW_SURROGATE_ESCAPES: u32 = ffi::PCRE2_EXTRA_ALLOW_SURROGATE_ESCAPES;
    /// `PCRE2_EXTRA_BAD_ESCAPE_IS_LITERAL`.
    pub const BAD_ESCAPE_IS_LITERAL: u32 = ffi::PCRE2_EXTRA_BAD_ESCAPE_IS_LITERAL;
    /// `PCRE2_EXTRA_MATCH_WORD`.
    pub const MATCH_WORD: u32 = ffi::PCRE2_EXTRA_MATCH_WORD;
    /// `PCRE2_EXTRA_MATCH_LINE`.
    pub const MATCH_LINE: u32 = ffi::PCRE2_EXTRA_MATCH_LINE;
    /// `PCRE2_EXTRA_ESCAPED_CR_IS_LF`.
    pub const ESCAPED_CR_IS_LF: u32 = ffi::PCRE2_EXTRA_ESCAPED_CR_IS_LF;
    /// `PCRE2_EXTRA_ALT_BSUX`.
    pub const ALT_BSUX: u32 = ffi::PCRE2_EXTRA_ALT_BSUX;
    /// `PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK` (PHP 8.5 does not set it).
    pub const ALLOW_LOOKAROUND_BSK: u32 = ffi::PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK;
    /// `PCRE2_EXTRA_CASELESS_RESTRICT` (PHP modifier `r`).
    pub const CASELESS_RESTRICT: u32 = ffi::PCRE2_EXTRA_CASELESS_RESTRICT;
    /// `PCRE2_EXTRA_ASCII_BSD`.
    pub const ASCII_BSD: u32 = ffi::PCRE2_EXTRA_ASCII_BSD;
    /// `PCRE2_EXTRA_ASCII_BSS`.
    pub const ASCII_BSS: u32 = ffi::PCRE2_EXTRA_ASCII_BSS;
    /// `PCRE2_EXTRA_ASCII_BSW`.
    pub const ASCII_BSW: u32 = ffi::PCRE2_EXTRA_ASCII_BSW;
    /// `PCRE2_EXTRA_ASCII_POSIX`.
    pub const ASCII_POSIX: u32 = ffi::PCRE2_EXTRA_ASCII_POSIX;
    /// `PCRE2_EXTRA_ASCII_DIGIT`.
    pub const ASCII_DIGIT: u32 = ffi::PCRE2_EXTRA_ASCII_DIGIT;
}

/// Options accepted by [`Code::match_at`] and [`Code::jit_match`] (`pcre2_match()` option bits).
pub mod match_options {
    use crate::ffi;

    /// `PCRE2_ANCHORED` (not supported by the JIT fast path).
    pub const ANCHORED: u32 = ffi::PCRE2_ANCHORED;
    /// `PCRE2_NO_UTF_CHECK`.
    pub const NO_UTF_CHECK: u32 = ffi::PCRE2_NO_UTF_CHECK;
    /// `PCRE2_ENDANCHORED`.
    pub const ENDANCHORED: u32 = ffi::PCRE2_ENDANCHORED;
    /// `PCRE2_NOTBOL`.
    pub const NOTBOL: u32 = ffi::PCRE2_NOTBOL;
    /// `PCRE2_NOTEOL`.
    pub const NOTEOL: u32 = ffi::PCRE2_NOTEOL;
    /// `PCRE2_NOTEMPTY`.
    pub const NOTEMPTY: u32 = ffi::PCRE2_NOTEMPTY;
    /// `PCRE2_NOTEMPTY_ATSTART`.
    pub const NOTEMPTY_ATSTART: u32 = ffi::PCRE2_NOTEMPTY_ATSTART;
    /// `PCRE2_NO_JIT`.
    pub const NO_JIT: u32 = ffi::PCRE2_NO_JIT;
    /// `PCRE2_DISABLE_RECURSELOOP_CHECK`.
    pub const DISABLE_RECURSELOOP_CHECK: u32 = ffi::PCRE2_DISABLE_RECURSELOOP_CHECK;
}

/// Error codes returned by matching (`PCRE2_ERROR_*`, negative), as far as this crate interprets them.
pub mod error_codes {
    use core::ffi::c_int;

    use crate::ffi;

    /// `PCRE2_ERROR_NOMATCH`.
    pub const NOMATCH: c_int = ffi::PCRE2_ERROR_NOMATCH;
    /// `PCRE2_ERROR_PARTIAL`.
    pub const PARTIAL: c_int = ffi::PCRE2_ERROR_PARTIAL;
    /// `PCRE2_ERROR_UTF8_ERR1` (first of the UTF-8 validity errors).
    pub const UTF8_ERR1: c_int = ffi::PCRE2_ERROR_UTF8_ERR1;
    /// `PCRE2_ERROR_UTF8_ERR21` (last of the UTF-8 validity errors).
    pub const UTF8_ERR21: c_int = ffi::PCRE2_ERROR_UTF8_ERR21;
    /// `PCRE2_ERROR_BADOFFSET`.
    pub const BADOFFSET: c_int = ffi::PCRE2_ERROR_BADOFFSET;
    /// `PCRE2_ERROR_BADOPTION`.
    pub const BADOPTION: c_int = ffi::PCRE2_ERROR_BADOPTION;
    /// `PCRE2_ERROR_BADUTFOFFSET`.
    pub const BADUTFOFFSET: c_int = ffi::PCRE2_ERROR_BADUTFOFFSET;
    /// `PCRE2_ERROR_INTERNAL`.
    pub const INTERNAL: c_int = ffi::PCRE2_ERROR_INTERNAL;
    /// `PCRE2_ERROR_JIT_BADOPTION`.
    pub const JIT_BADOPTION: c_int = ffi::PCRE2_ERROR_JIT_BADOPTION;
    /// `PCRE2_ERROR_JIT_STACKLIMIT`.
    pub const JIT_STACKLIMIT: c_int = ffi::PCRE2_ERROR_JIT_STACKLIMIT;
    /// `PCRE2_ERROR_MATCHLIMIT`.
    pub const MATCHLIMIT: c_int = ffi::PCRE2_ERROR_MATCHLIMIT;
    /// `PCRE2_ERROR_NOMEMORY`.
    pub const NOMEMORY: c_int = ffi::PCRE2_ERROR_NOMEMORY;
    /// `PCRE2_ERROR_DEPTHLIMIT` (alias `PCRE2_ERROR_RECURSIONLIMIT`).
    pub const DEPTHLIMIT: c_int = ffi::PCRE2_ERROR_DEPTHLIMIT;
    /// `PCRE2_ERROR_HEAPLIMIT`.
    pub const HEAPLIMIT: c_int = ffi::PCRE2_ERROR_HEAPLIMIT;
    /// `PCRE2_ERROR_BACKSLASH_C_CALLER_DISABLED` (compile error 183).
    pub const BACKSLASH_C_CALLER_DISABLED: c_int = ffi::PCRE2_ERROR_BACKSLASH_C_CALLER_DISABLED;
}

/// Returns PCRE2's message for an error code, as `pcre2_get_error_message()` writes it into a buffer of
/// `buffer_size` code units (a longer message is cut to `buffer_size - 1` units, like the C function).
///
/// A code PCRE2 has no text for (the C function returns `PCRE2_ERROR_BADDATA`) and a zero `buffer_size`
/// give an empty message.
pub fn error_message(code: c_int, buffer_size: usize) -> Vec<u8> {
    if buffer_size == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u8; buffer_size];
    // SAFETY: the buffer is valid for `buffer_size` code units and PCRE2 writes at most that many,
    // always NUL-terminating what it wrote.
    let rc = unsafe { ffi::pcre2_get_error_message_8(code, buffer.as_mut_ptr(), buffer.len()) };
    let len = if rc >= 0 {
        rc as usize
    } else {
        buffer.iter().position(|&b| b == 0).unwrap_or(0)
    };
    buffer.truncate(len);
    buffer
}

/// Reads a numeric `pcre2_config()` value.
fn config_u32(what: u32) -> u32 {
    let mut value: u32 = 0;
    // SAFETY: every numeric request this module makes writes one uint32_t.
    unsafe { ffi::pcre2_config_8(what, (&raw mut value).cast::<c_void>()) };
    value
}

/// Reads a string `pcre2_config()` value.
fn config_string(what: u32) -> String {
    // SAFETY: a NULL `where` asks for the required buffer length in code units (including the NUL).
    let len = unsafe { ffi::pcre2_config_8(what, ptr::null_mut()) };
    if len <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u8; len as usize];
    // SAFETY: the buffer has the length PCRE2 asked for.
    let written = unsafe { ffi::pcre2_config_8(what, buffer.as_mut_ptr().cast::<c_void>()) };
    buffer.truncate(written.max(1) as usize - 1);
    String::from_utf8_lossy(&buffer).into_owned()
}

/// Build-time configuration of the linked PCRE2 library (`pcre2_config()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// `PCRE2_CONFIG_VERSION`, e.g. `"10.44 2024-06-07"` (PHP's `PCRE_VERSION`).
    pub version: String,
    /// `PCRE2_CONFIG_UNICODE_VERSION`, e.g. `"15.0.0"`.
    pub unicode_version: String,
    /// `PCRE2_CONFIG_UNICODE`: Unicode support compiled in.
    pub unicode: bool,
    /// `PCRE2_CONFIG_JIT`: JIT compiled in.
    pub jit: bool,
    /// `PCRE2_CONFIG_JITTARGET`, e.g. `"x86 64bit (little endian + unaligned)"`; empty without JIT.
    pub jit_target: String,
    /// `PCRE2_CONFIG_NEWLINE` (2 = LF).
    pub newline: u32,
    /// `PCRE2_CONFIG_BSR` (1 = Unicode).
    pub bsr: u32,
    /// `PCRE2_CONFIG_LINKSIZE`.
    pub link_size: u32,
    /// `PCRE2_CONFIG_PARENSLIMIT`.
    pub parens_limit: u32,
    /// `PCRE2_CONFIG_MATCHLIMIT` (library default, before PHP's `pcre.backtrack_limit`).
    pub match_limit: u32,
    /// `PCRE2_CONFIG_DEPTHLIMIT` (library default, before PHP's `pcre.recursion_limit`).
    pub depth_limit: u32,
    /// `PCRE2_CONFIG_HEAPLIMIT` in KiB.
    pub heap_limit: u32,
    /// `PCRE2_CONFIG_NEVER_BACKSLASH_C`.
    pub never_backslash_c: bool,
}

impl Config {
    /// Queries the linked library.
    pub fn get() -> Self {
        let jit = config_u32(ffi::PCRE2_CONFIG_JIT) != 0;
        Config {
            version: config_string(ffi::PCRE2_CONFIG_VERSION),
            unicode_version: config_string(ffi::PCRE2_CONFIG_UNICODE_VERSION),
            unicode: config_u32(ffi::PCRE2_CONFIG_UNICODE) != 0,
            jit,
            jit_target: if jit {
                config_string(ffi::PCRE2_CONFIG_JITTARGET)
            } else {
                String::new()
            },
            newline: config_u32(ffi::PCRE2_CONFIG_NEWLINE),
            bsr: config_u32(ffi::PCRE2_CONFIG_BSR),
            link_size: config_u32(ffi::PCRE2_CONFIG_LINKSIZE),
            parens_limit: config_u32(ffi::PCRE2_CONFIG_PARENSLIMIT),
            match_limit: config_u32(ffi::PCRE2_CONFIG_MATCHLIMIT),
            depth_limit: config_u32(ffi::PCRE2_CONFIG_DEPTHLIMIT),
            heap_limit: config_u32(ffi::PCRE2_CONFIG_HEAPLIMIT),
            never_backslash_c: config_u32(ffi::PCRE2_CONFIG_NEVER_BACKSLASH_C) != 0,
        }
    }
}

/// A failed `pcre2_compile()`: the error code and the pattern offset PCRE2 reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompileError {
    /// PCRE2 compile error number (100 and up), or a negative UTF error code.
    pub code: c_int,
    /// Offset in the pattern, in code units, where PCRE2 stopped.
    pub offset: usize,
}

impl CompileError {
    /// PCRE2's message for the error (`pcre2_get_error_message()`).
    pub fn message(&self) -> String {
        String::from_utf8_lossy(&error_message(self.code, 256)).into_owned()
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at offset {}", self.message(), self.offset)
    }
}

impl std::error::Error for CompileError {}

/// A compile context (`pcre2_compile_context`): character tables and extra options for compilation.
pub struct CompileContext {
    ptr: NonNull<ffi::pcre2_compile_context_8>,
}

// SAFETY: the context is exclusively owned; PCRE2 contexts carry no thread affinity.
unsafe impl Send for CompileContext {}

impl CompileContext {
    /// `pcre2_compile_context_create(NULL)`: library defaults (newline LF, BSR Unicode, parens limit 250,
    /// default character tables).
    ///
    /// # Panics
    ///
    /// Panics when PCRE2 cannot allocate the context.
    pub fn new() -> Self {
        // SAFETY: a NULL general context selects malloc/free.
        let ptr = unsafe { ffi::pcre2_compile_context_create_8(ptr::null_mut()) };
        CompileContext {
            ptr: NonNull::new(ptr).expect("pcre2_compile_context_create failed"),
        }
    }

    /// `pcre2_set_compile_extra_options()`.
    pub fn set_extra_options(&mut self, extra_options: u32) {
        // SAFETY: valid context pointer.
        unsafe { ffi::pcre2_set_compile_extra_options_8(self.ptr.as_ptr(), extra_options) };
    }

    /// `pcre2_set_character_tables(ctx, NULL)`: use PCRE2's built-in (C locale) tables.
    pub fn use_default_tables(&mut self) {
        // SAFETY: valid context pointer; NULL selects the built-in tables.
        unsafe { ffi::pcre2_set_character_tables_8(self.ptr.as_ptr(), ptr::null()) };
    }
}

impl Default for CompileContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CompileContext {
    fn drop(&mut self) {
        // SAFETY: the pointer came from pcre2_compile_context_create and is freed once.
        unsafe { ffi::pcre2_compile_context_free_8(self.ptr.as_ptr()) };
    }
}

impl fmt::Debug for CompileContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompileContext").finish_non_exhaustive()
    }
}

/// A JIT stack (`pcre2_jit_stack`), used by JIT matching through a [`MatchContext`].
pub struct JitStack {
    ptr: NonNull<ffi::pcre2_jit_stack_8>,
}

// SAFETY: exclusively owned; only used through a MatchContext, which is not Sync.
unsafe impl Send for JitStack {}

impl JitStack {
    /// `pcre2_jit_stack_create(start_size, max_size, NULL)`. Returns `None` when the library has no JIT or
    /// the stack cannot be allocated.
    pub fn new(start_size: usize, max_size: usize) -> Option<Self> {
        // SAFETY: plain sizes; a NULL general context selects malloc/free.
        let ptr = unsafe { ffi::pcre2_jit_stack_create_8(start_size, max_size, ptr::null_mut()) };
        NonNull::new(ptr).map(|ptr| JitStack { ptr })
    }
}

impl Drop for JitStack {
    fn drop(&mut self) {
        // SAFETY: the pointer came from pcre2_jit_stack_create and is freed once.
        unsafe { ffi::pcre2_jit_stack_free_8(self.ptr.as_ptr()) };
    }
}

impl fmt::Debug for JitStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JitStack").finish_non_exhaustive()
    }
}

/// A match context (`pcre2_match_context`): match/depth/heap limits and the JIT stack.
pub struct MatchContext {
    ptr: NonNull<ffi::pcre2_match_context_8>,
    jit_stack: Option<JitStack>,
}

// SAFETY: exclusively owned. The type is not Sync: JIT matching writes to the assigned JIT stack, so a
// context must not be used by two matches at once, which `&self` access from one thread cannot do
// (matching never calls back into Rust).
unsafe impl Send for MatchContext {}

impl MatchContext {
    /// `pcre2_match_context_create(NULL)`: library defaults (`MATCH_LIMIT`, `MATCH_LIMIT_DEPTH`,
    /// `HEAP_LIMIT` of `config.h`), no JIT stack (JIT matching then uses a 32 KiB machine-stack area).
    ///
    /// # Panics
    ///
    /// Panics when PCRE2 cannot allocate the context.
    pub fn new() -> Self {
        // SAFETY: a NULL general context selects malloc/free.
        let ptr = unsafe { ffi::pcre2_match_context_create_8(ptr::null_mut()) };
        MatchContext {
            ptr: NonNull::new(ptr).expect("pcre2_match_context_create failed"),
            jit_stack: None,
        }
    }

    /// `pcre2_set_match_limit()`.
    pub fn set_match_limit(&mut self, limit: u32) {
        // SAFETY: valid context pointer.
        unsafe { ffi::pcre2_set_match_limit_8(self.ptr.as_ptr(), limit) };
    }

    /// `pcre2_set_depth_limit()`.
    pub fn set_depth_limit(&mut self, limit: u32) {
        // SAFETY: valid context pointer.
        unsafe { ffi::pcre2_set_depth_limit_8(self.ptr.as_ptr(), limit) };
    }

    /// `pcre2_set_heap_limit()` (KiB).
    pub fn set_heap_limit(&mut self, limit_kib: u32) {
        // SAFETY: valid context pointer.
        unsafe { ffi::pcre2_set_heap_limit_8(self.ptr.as_ptr(), limit_kib) };
    }

    /// `pcre2_jit_stack_assign(ctx, NULL, stack)`; `None` unassigns (JIT matching then uses a 32 KiB area of
    /// the machine stack). Returns the previously assigned stack.
    pub fn assign_jit_stack(&mut self, stack: Option<JitStack>) -> Option<JitStack> {
        let data = stack
            .as_ref()
            .map_or(ptr::null_mut(), |s| s.ptr.as_ptr().cast::<c_void>());
        // SAFETY: valid context pointer; the stack (if any) is kept alive by `self.jit_stack` for as long
        // as it is assigned.
        unsafe { ffi::pcre2_jit_stack_assign_8(self.ptr.as_ptr(), None, data) };
        core::mem::replace(&mut self.jit_stack, stack)
    }

    /// Whether a JIT stack is assigned.
    pub fn has_jit_stack(&self) -> bool {
        self.jit_stack.is_some()
    }
}

impl Default for MatchContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MatchContext {
    fn drop(&mut self) {
        // SAFETY: the pointer came from pcre2_match_context_create and is freed once; the JIT stack field
        // is dropped afterwards.
        unsafe { ffi::pcre2_match_context_free_8(self.ptr.as_ptr()) };
    }
}

impl fmt::Debug for MatchContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatchContext")
            .field("jit_stack", &self.jit_stack.is_some())
            .finish_non_exhaustive()
    }
}

/// A match data block (`pcre2_match_data`): the offset vector filled in by a match.
pub struct MatchData {
    ptr: NonNull<ffi::pcre2_match_data_8>,
    /// Copy of the `(*MARK)` name of the last match, taken while its pattern was still borrowed.
    mark: Option<Vec<u8>>,
}

// SAFETY: exclusively owned; matches write to it only through `&mut self`.
unsafe impl Send for MatchData {}

impl MatchData {
    /// `pcre2_match_data_create(pairs, NULL)`: room for `pairs` offset pairs (at least 1).
    ///
    /// # Panics
    ///
    /// Panics when PCRE2 cannot allocate the block.
    pub fn new(pairs: u32) -> Self {
        // SAFETY: plain size; a NULL general context selects malloc/free.
        let ptr = unsafe { ffi::pcre2_match_data_create_8(pairs, ptr::null_mut()) };
        MatchData {
            ptr: NonNull::new(ptr).expect("pcre2_match_data_create failed"),
            mark: None,
        }
    }

    /// `pcre2_match_data_create_from_pattern(code, NULL)`: one pair per capture group plus one.
    ///
    /// # Panics
    ///
    /// Panics when PCRE2 cannot allocate the block.
    pub fn for_code(code: &Code) -> Self {
        // SAFETY: valid code pointer; a NULL general context selects malloc/free.
        let ptr = unsafe {
            ffi::pcre2_match_data_create_from_pattern_8(code.ptr.as_ptr(), ptr::null_mut())
        };
        MatchData {
            ptr: NonNull::new(ptr).expect("pcre2_match_data_create_from_pattern failed"),
            mark: None,
        }
    }

    /// Number of offset pairs (`pcre2_get_ovector_count()`).
    pub fn pair_count(&self) -> usize {
        // SAFETY: valid match data pointer.
        unsafe { ffi::pcre2_get_ovector_count_8(self.ptr.as_ptr()) as usize }
    }

    /// The whole offset vector (`pcre2_get_ovector_pointer()`): `2 * pair_count()` offsets, start/end per
    /// group, [`UNSET`] for groups that did not participate. Only the first `2 * rc` entries are
    /// meaningful after a match that returned `rc`.
    pub fn ovector(&self) -> &[usize] {
        // SAFETY: PCRE2 guarantees the ovector holds 2 * ovector_count PCRE2_SIZE values for the lifetime
        // of the match data block.
        unsafe {
            let p = ffi::pcre2_get_ovector_pointer_8(self.ptr.as_ptr());
            core::slice::from_raw_parts(p, 2 * self.pair_count())
        }
    }

    /// The `(start, end)` offsets of group `i`, or `None` when it is unset or out of range.
    pub fn group(&self, i: usize) -> Option<(usize, usize)> {
        let ov = self.ovector();
        let (s, e) = (*ov.get(2 * i)?, *ov.get(2 * i + 1)?);
        (s != UNSET).then_some((s, e))
    }

    /// The `(*MARK)` name recorded by the last successful match, if any (`pcre2_get_mark()`). Like PHP, this
    /// wrapper only reads the mark after a successful match (PCRE2 leaves it undefined after most errors).
    pub fn mark(&self) -> Option<&[u8]> {
        self.mark.as_deref()
    }

    /// Copies the mark of the match that just returned `rc`. Must be called while that match's pattern is
    /// alive.
    fn record_mark(&mut self, rc: c_int) {
        if rc < 0 {
            self.mark = None;
            return;
        }
        // SAFETY: valid match data pointer, and the match succeeded, so PCRE2 has set the mark field. A
        // non-NULL mark points at a name inside the compiled pattern of that match (still borrowed by the
        // caller), preceded by its length code unit.
        self.mark = unsafe {
            let p = ffi::pcre2_get_mark_8(self.ptr.as_ptr());
            if p.is_null() {
                None
            } else {
                let len = usize::from(*p.sub(1));
                Some(core::slice::from_raw_parts(p, len).to_vec())
            }
        };
    }

    /// `pcre2_get_startchar()`: after a UTF error, the offset of the offending code unit.
    pub fn start_char(&self) -> usize {
        // SAFETY: valid match data pointer.
        unsafe { ffi::pcre2_get_startchar_8(self.ptr.as_ptr()) }
    }
}

impl Drop for MatchData {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a pcre2_match_data_create* call and is freed once.
        unsafe { ffi::pcre2_match_data_free_8(self.ptr.as_ptr()) };
    }
}

impl fmt::Debug for MatchData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatchData")
            .field("pairs", &self.pair_count())
            .finish()
    }
}

/// A compiled pattern (`pcre2_code`).
pub struct Code {
    ptr: NonNull<ffi::pcre2_code_8>,
}

// SAFETY: PCRE2 compiled patterns (including their JIT code) are read-only during matching and may be
// shared between threads; the only mutating operation, JIT compilation, takes `&mut self`.
unsafe impl Send for Code {}
// SAFETY: see above.
unsafe impl Sync for Code {}

impl Code {
    /// `pcre2_compile()` with the given options and compile context (`None`: library defaults).
    pub fn compile(
        pattern: &[u8],
        options: u32,
        context: Option<&CompileContext>,
    ) -> Result<Code, CompileError> {
        let mut errorcode: c_int = 0;
        let mut erroroffset: usize = 0;
        let ctx = context.map_or(ptr::null_mut(), |c| c.ptr.as_ptr());
        // PCRE2 treats a NULL pattern with length 0 as empty; pass a valid pointer anyway.
        let pat = if pattern.is_empty() {
            b"".as_ptr()
        } else {
            pattern.as_ptr()
        };
        // SAFETY: `pat` is valid for `pattern.len()` bytes (the length is explicit, NULs allowed); the
        // out-parameters are valid; the context pointer is NULL or valid.
        let code = unsafe {
            ffi::pcre2_compile_8(
                pat,
                pattern.len(),
                options,
                &raw mut errorcode,
                &raw mut erroroffset,
                ctx,
            )
        };
        match NonNull::new(code) {
            Some(ptr) => Ok(Code { ptr }),
            None => Err(CompileError {
                code: errorcode,
                offset: erroroffset,
            }),
        }
    }

    /// `pcre2_jit_compile()`; returns PCRE2's result code (0 on success, negative error).
    pub fn jit_compile(&mut self, options: u32) -> c_int {
        // SAFETY: valid code pointer, exclusively borrowed.
        unsafe { ffi::pcre2_jit_compile_8(self.ptr.as_ptr(), options) }
    }

    fn info_u32(&self, what: u32) -> u32 {
        let mut value: u32 = 0;
        // SAFETY: the requests used with this helper write one uint32_t.
        unsafe {
            ffi::pcre2_pattern_info_8(self.ptr.as_ptr(), what, (&raw mut value).cast::<c_void>())
        };
        value
    }

    /// `PCRE2_INFO_CAPTURECOUNT`: highest capture group number.
    pub fn capture_count(&self) -> u32 {
        self.info_u32(ffi::PCRE2_INFO_CAPTURECOUNT)
    }

    /// `PCRE2_INFO_NAMECOUNT`: number of named groups.
    pub fn name_count(&self) -> u32 {
        self.info_u32(ffi::PCRE2_INFO_NAMECOUNT)
    }

    /// `PCRE2_INFO_ARGOPTIONS`: the options passed to `pcre2_compile()`.
    pub fn arg_options(&self) -> u32 {
        self.info_u32(ffi::PCRE2_INFO_ARGOPTIONS)
    }

    /// `PCRE2_INFO_ALLOPTIONS`: compile options including those set by the pattern itself (`(*UTF)`, ...).
    pub fn all_options(&self) -> u32 {
        self.info_u32(ffi::PCRE2_INFO_ALLOPTIONS)
    }

    /// `PCRE2_INFO_EXTRAOPTIONS`.
    pub fn extra_options(&self) -> u32 {
        self.info_u32(ffi::PCRE2_INFO_EXTRAOPTIONS)
    }

    /// `PCRE2_INFO_JITSIZE`: size of the JIT code, 0 when not JIT-compiled.
    pub fn jit_size(&self) -> usize {
        let mut value: usize = 0;
        // SAFETY: PCRE2_INFO_JITSIZE writes one size_t.
        unsafe {
            ffi::pcre2_pattern_info_8(
                self.ptr.as_ptr(),
                ffi::PCRE2_INFO_JITSIZE,
                (&raw mut value).cast::<c_void>(),
            )
        };
        value
    }

    /// The name table (`PCRE2_INFO_NAMETABLE`): `(group number, name)` in table order.
    pub fn names(&self) -> Vec<(u32, Vec<u8>)> {
        let count = self.name_count() as usize;
        if count == 0 {
            return Vec::new();
        }
        let entry_size = self.info_u32(ffi::PCRE2_INFO_NAMEENTRYSIZE) as usize;
        let mut table: *const u8 = ptr::null();
        // SAFETY: PCRE2_INFO_NAMETABLE writes one PCRE2_SPTR.
        unsafe {
            ffi::pcre2_pattern_info_8(
                self.ptr.as_ptr(),
                ffi::PCRE2_INFO_NAMETABLE,
                (&raw mut table).cast::<c_void>(),
            )
        };
        if table.is_null() {
            return Vec::new();
        }
        // SAFETY: the name table holds `count` entries of `entry_size` code units inside the compiled
        // pattern, which lives as long as `self`.
        let bytes = unsafe { core::slice::from_raw_parts(table, count * entry_size) };
        bytes
            .chunks(entry_size)
            .map(|e| {
                let number = u32::from(e[0]) << 8 | u32::from(e[1]);
                let name = &e[2..];
                let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                (number, name[..end].to_vec())
            })
            .collect()
    }

    /// `pcre2_match()`: returns PCRE2's result code (`> 0` match with that many pairs set, `0` match with
    /// too small an ovector, negative error, [`error_codes::NOMATCH`] for no match).
    pub fn match_at(
        &self,
        subject: &[u8],
        start: usize,
        options: u32,
        match_data: &mut MatchData,
        context: Option<&MatchContext>,
    ) -> c_int {
        let subj = if subject.is_empty() {
            b"".as_ptr()
        } else {
            subject.as_ptr()
        };
        let ctx = context.map_or(ptr::null_mut(), |c| c.ptr.as_ptr());
        // SAFETY: `subj` is valid for `subject.len()` bytes; pcre2_match() checks the offset, the options
        // and (unless NO_UTF_CHECK, see below) UTF validity itself. NO_UTF_CHECK with invalid UTF would be
        // undefined behaviour, so it is cleared here unless the subject really is valid.
        let options = self.sanitize_no_utf_check(subject, start, options);
        let rc = unsafe {
            ffi::pcre2_match_8(
                self.ptr.as_ptr(),
                subj,
                subject.len(),
                start,
                options,
                match_data.ptr.as_ptr(),
                ctx,
            )
        };
        match_data.record_mark(rc);
        rc
    }

    /// `pcre2_jit_match()`, the JIT fast path that skips `pcre2_match()`'s checks and option handling
    /// (notably it does not apply `(*NOTEMPTY)`/`(*NOTEMPTY_ATSTART)` from the pattern).
    ///
    /// The checks the fast path skips are made here instead, so that the call is always sound: an offset
    /// past the subject returns [`error_codes::BADOFFSET`], and for a UTF pattern an invalid subject (or
    /// an offset inside a character) is handed to [`Code::match_at`] without `NO_UTF_CHECK`, which reports
    /// PCRE2's precise UTF error. A pattern without JIT code returns [`error_codes::JIT_BADOPTION`].
    pub fn jit_match(
        &self,
        subject: &[u8],
        start: usize,
        options: u32,
        match_data: &mut MatchData,
        context: Option<&MatchContext>,
    ) -> c_int {
        if start > subject.len() {
            match_data.mark = None;
            return error_codes::BADOFFSET;
        }
        if self.all_options() & ffi::PCRE2_UTF != 0 && !valid_utf8_at(subject, start) {
            return self.match_at(
                subject,
                start,
                options & !ffi::PCRE2_NO_UTF_CHECK,
                match_data,
                context,
            );
        }
        let subj = if subject.is_empty() {
            b"".as_ptr()
        } else {
            subject.as_ptr()
        };
        let ctx = context.map_or(ptr::null_mut(), |c| c.ptr.as_ptr());
        // SAFETY: `subj` is valid for `subject.len()` bytes, `start <= len`, and a UTF pattern only gets a
        // valid UTF-8 subject with `start` on a character boundary. pcre2_jit_match() itself returns
        // JIT_BADOPTION when the pattern has no JIT code for the requested mode.
        let rc = unsafe {
            ffi::pcre2_jit_match_8(
                self.ptr.as_ptr(),
                subj,
                subject.len(),
                start,
                options,
                match_data.ptr.as_ptr(),
                ctx,
            )
        };
        match_data.record_mark(rc);
        rc
    }

    /// Clears `NO_UTF_CHECK` when it would let PCRE2 read invalid UTF (undefined behaviour in C).
    fn sanitize_no_utf_check(&self, subject: &[u8], start: usize, options: u32) -> u32 {
        if options & ffi::PCRE2_NO_UTF_CHECK != 0
            && self.all_options() & ffi::PCRE2_UTF != 0
            && !valid_utf8_at(subject, start.min(subject.len()))
        {
            options & !ffi::PCRE2_NO_UTF_CHECK
        } else {
            options
        }
    }
}

/// Whether `subject` is valid UTF-8 (the same rules as PCRE2's `valid_utf()`: RFC 3629, no surrogates,
/// nothing above U+10FFFF, no overlongs) and `start` is at a character boundary.
fn valid_utf8_at(subject: &[u8], start: usize) -> bool {
    core::str::from_utf8(subject).is_ok()
        && (start == subject.len() || (subject[start] & 0xC0) != 0x80)
}

impl Drop for Code {
    fn drop(&mut self) {
        // SAFETY: the pointer came from pcre2_compile and is freed once (this also frees JIT code).
        unsafe { ffi::pcre2_code_free_8(self.ptr.as_ptr()) };
    }
}

impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Code")
            .field("capture_count", &self.capture_count())
            .field("all_options", &format_args!("{:#x}", self.all_options()))
            .field("jit_size", &self.jit_size())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_php_bundled_10_44() {
        let c = Config::get();
        assert_eq!(c.version, "10.44 2024-06-07");
        assert_eq!(c.unicode_version, "15.0.0");
        assert!(c.unicode);
        assert_eq!(c.newline, 2);
        assert_eq!(c.bsr, 1);
        assert_eq!(c.link_size, 2);
        assert_eq!(c.parens_limit, 250);
        assert_eq!(c.match_limit, 10_000_000);
        assert_eq!(c.depth_limit, 10_000_000);
        assert_eq!(c.heap_limit, 20_000_000);
        assert!(!c.never_backslash_c);
        // PHP 8.5.13's phpinfo(): "PCRE JIT Target => x86 64bit (little endian + unaligned)".
        if cfg!(all(feature = "jit", target_arch = "x86_64")) {
            assert!(c.jit);
            assert_eq!(c.jit_target, "x86 64bit (little endian + unaligned)");
        }
    }

    #[test]
    fn compile_errors_have_pcre2_messages_and_offsets() {
        let e = Code::compile(b"(abc", 0, None).unwrap_err();
        assert_eq!(e.to_string(), "missing closing parenthesis at offset 4");
        let e = Code::compile(b"[[:foo:]]", 0, None).unwrap_err();
        assert_eq!(e.to_string(), "unknown POSIX class name at offset 3");
        let e = Code::compile(b"\\p{Foo}", compile_options::UTF, None).unwrap_err();
        assert_eq!(
            e.to_string(),
            "unknown property after \\P or \\p at offset 7"
        );
    }

    #[test]
    fn match_with_offsets_and_captures() {
        let mut code = Code::compile(b"(?<major>\\d+)\\.(\\d+)(x)?", 0, None).unwrap();
        assert_eq!(code.capture_count(), 3);
        assert_eq!(code.names(), vec![(1, b"major".to_vec())]);
        let jit = code.jit_compile(ffi::PCRE2_JIT_COMPLETE);
        let mut md = MatchData::for_code(&code);
        let rc = code.match_at(b"TWIG 3.31", 0, 0, &mut md, None);
        assert_eq!(rc, 3);
        assert_eq!(md.group(0), Some((5, 9)));
        assert_eq!(md.group(1), Some((5, 6)));
        assert_eq!(md.group(2), Some((7, 9)));
        assert_eq!(md.group(3), None);
        if jit == 0 && Config::get().jit {
            assert!(code.jit_size() > 0);
            let rc = code.jit_match(b"x 1.2", 0, 0, &mut md, None);
            assert_eq!(rc, 3);
            assert_eq!(md.group(0), Some((2, 5)));
        }
        assert_eq!(
            code.match_at(b"none", 0, 0, &mut md, None),
            error_codes::NOMATCH
        );
        assert_eq!(
            code.match_at(b"1.2", 4, 0, &mut md, None),
            error_codes::BADOFFSET
        );
        assert_eq!(
            code.jit_match(b"1.2", 4, 0, &mut md, None),
            error_codes::BADOFFSET
        );
    }

    #[test]
    fn utf_checks_cannot_be_skipped_unsoundly() {
        let mut code = Code::compile(b"a", compile_options::UTF, None).unwrap();
        code.jit_compile(ffi::PCRE2_JIT_COMPLETE);
        let mut md = MatchData::new(1);
        let rc = code.match_at(b"\xFFa", 0, match_options::NO_UTF_CHECK, &mut md, None);
        assert!(
            (error_codes::UTF8_ERR21..=error_codes::UTF8_ERR1).contains(&rc),
            "{rc}"
        );
        let rc = code.jit_match(b"\xFFa", 0, 0, &mut md, None);
        assert!(
            (error_codes::UTF8_ERR21..=error_codes::UTF8_ERR1).contains(&rc),
            "{rc}"
        );
        assert_eq!(
            code.jit_match("éa".as_bytes(), 1, 0, &mut md, None),
            error_codes::BADUTFOFFSET
        );
    }

    #[test]
    fn error_message_buffer_semantics() {
        assert_eq!(
            error_message(error_codes::MATCHLIMIT, 128),
            b"match limit exceeded"
        );
        assert_eq!(error_message(error_codes::MATCHLIMIT, 6), b"match");
        assert_eq!(error_message(error_codes::MATCHLIMIT, 0), b"");
    }
}
