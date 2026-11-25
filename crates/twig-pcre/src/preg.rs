//! PHP's `preg_*` front end: a port of the parts of `PHPSRC/ext/pcre/php_pcre.c` that `preg_match()` uses.
//!
//! [`Pcre`] holds what PHP keeps in its per-thread pcre globals: the compiled-pattern cache, the last error
//! code, the `pcre.*` settings and the PCRE2 compile/match contexts with the JIT stack. A [`Pcre`] behaves
//! like one PHP process (or one ZTS thread): state carried from call to call (cached patterns, a JIT that
//! was switched off after an allocation failure) carries over the same way.
//!
//! PHP's matching depends on one property of the subject `zend_string` that byte slices do not carry:
//! the `IS_STR_VALID_UTF8` flag. For a `/u` pattern PHP skips the UTF-8 check of a flagged subject and
//! calls `pcre2_jit_match()` directly, which (unlike `pcre2_match()`) ignores the `(*NOTEMPTY)` and
//! `(*NOTEMPTY_ATSTART)` pattern flags; that is the only observable difference. PHP sets the flag on the
//! empty string and the one-byte ASCII strings at startup (they are interned singletons), on integer and
//! float conversions, on concatenations of flagged strings, on mbstring results and on a subject after a
//! successful `/u` match. [`SubjectUtf8`] lets callers say how PHP would see the subject;
//! [`SubjectUtf8::Auto`] models the singletons only.

// All FFI lives in `crate::pcre2`; the PHP front end is safe Rust.
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;

use crate::ffi;
use crate::pcre2::{
    self, Code, CompileContext, JitStack, MatchContext, MatchData, compile_options as co,
    error_codes,
};

/// `PCRE_CACHE_SIZE`: number of compiled patterns PHP keeps before evicting the oldest eighth.
pub const PCRE_CACHE_SIZE: usize = 4096;
/// `PCRE_JIT_STACK_MIN_SIZE`: initial size of PHP's JIT stack.
pub const PCRE_JIT_STACK_MIN_SIZE: usize = 32 * 1024;
/// `PCRE_JIT_STACK_MAX_SIZE`: maximum size of PHP's JIT stack.
pub const PCRE_JIT_STACK_MAX_SIZE: usize = 192 * 1024;
/// Default of the `pcre.backtrack_limit` ini setting.
pub const DEFAULT_BACKTRACK_LIMIT: i64 = 1_000_000;
/// Default of the `pcre.recursion_limit` ini setting.
pub const DEFAULT_RECURSION_LIMIT: i64 = 100_000;
/// `PREG_OFFSET_CAPTURE`.
pub const PREG_OFFSET_CAPTURE: i64 = 1 << 8;
/// `PREG_UNMATCHED_AS_NULL`.
pub const PREG_UNMATCHED_AS_NULL: i64 = 1 << 9;

/// `PREG_JIT`: the cached pattern has JIT code.
const PREG_JIT: u32 = 1 << 3;
/// `PHP_PCRE_PREALLOC_MDATA_SIZE`.
const PHP_PCRE_PREALLOC_MDATA_SIZE: u32 = 32;

/// The value of `preg_last_error()` (`php_pcre_error_code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PregError {
    /// `PREG_NO_ERROR` (0).
    #[default]
    NoError,
    /// `PREG_INTERNAL_ERROR` (1): also set by every pattern that fails to compile.
    Internal,
    /// `PREG_BACKTRACK_LIMIT_ERROR` (2).
    BacktrackLimit,
    /// `PREG_RECURSION_LIMIT_ERROR` (3).
    RecursionLimit,
    /// `PREG_BAD_UTF8_ERROR` (4).
    BadUtf8,
    /// `PREG_BAD_UTF8_OFFSET_ERROR` (5).
    BadUtf8Offset,
    /// `PREG_JIT_STACKLIMIT_ERROR` (6).
    JitStackLimit,
}

impl PregError {
    /// The integer `preg_last_error()` returns.
    pub fn code(self) -> i64 {
        match self {
            PregError::NoError => 0,
            PregError::Internal => 1,
            PregError::BacktrackLimit => 2,
            PregError::RecursionLimit => 3,
            PregError::BadUtf8 => 4,
            PregError::BadUtf8Offset => 5,
            PregError::JitStackLimit => 6,
        }
    }

    /// The message `preg_last_error_msg()` returns.
    ///
    /// Port of `php_pcre_get_error_msg()` (PHPSRC/ext/pcre/php_pcre.c).
    pub fn message(self) -> &'static str {
        match self {
            PregError::NoError => "No error",
            PregError::Internal => "Internal error",
            PregError::BadUtf8 => "Malformed UTF-8 characters, possibly incorrectly encoded",
            PregError::BadUtf8Offset => {
                "The offset did not correspond to the beginning of a valid UTF-8 code point"
            }
            PregError::BacktrackLimit => "Backtrack limit exhausted",
            PregError::RecursionLimit => "Recursion limit exhausted",
            PregError::JitStackLimit => "JIT stack limit exhausted",
        }
    }

    /// The name of the PHP constant with this value (`PREG_NO_ERROR`, ...).
    pub fn constant_name(self) -> &'static str {
        match self {
            PregError::NoError => "PREG_NO_ERROR",
            PregError::Internal => "PREG_INTERNAL_ERROR",
            PregError::BacktrackLimit => "PREG_BACKTRACK_LIMIT_ERROR",
            PregError::RecursionLimit => "PREG_RECURSION_LIMIT_ERROR",
            PregError::BadUtf8 => "PREG_BAD_UTF8_ERROR",
            PregError::BadUtf8Offset => "PREG_BAD_UTF8_OFFSET_ERROR",
            PregError::JitStackLimit => "PREG_JIT_STACKLIMIT_ERROR",
        }
    }

    /// Maps a PCRE2 match error to the PHP error code.
    ///
    /// Port of `pcre_handle_exec_error()` (PHPSRC/ext/pcre/php_pcre.c).
    pub fn from_pcre2_error(pcre_code: i32) -> PregError {
        match pcre_code {
            error_codes::MATCHLIMIT => PregError::BacktrackLimit,
            error_codes::DEPTHLIMIT => PregError::RecursionLimit,
            error_codes::BADUTFOFFSET => PregError::BadUtf8Offset,
            error_codes::JIT_STACKLIMIT => PregError::JitStackLimit,
            c if (error_codes::UTF8_ERR21..=error_codes::UTF8_ERR1).contains(&c) => {
                PregError::BadUtf8
            }
            _ => PregError::Internal,
        }
    }
}

impl fmt::Display for PregError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// Severity of a [`Diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticLevel {
    /// `E_WARNING`.
    Warning,
    /// `E_NOTICE`.
    Notice,
}

/// A PHP warning or notice raised by a `preg_*` function through `php_error_docref()`. The function
/// continues after raising it (in PHP an error handler that throws only takes effect once the function
/// returns).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    /// `E_WARNING` or `E_NOTICE`.
    pub level: DiagnosticLevel,
    /// The PHP function that raised it, e.g. `"preg_match"`.
    pub function: &'static str,
    /// The message without the `"<function>(): "` prefix. Bytes, because messages such as
    /// `Unknown modifier '%c'` embed a byte of the pattern.
    pub message: Vec<u8>,
}

impl Diagnostic {
    fn warning(function: &'static str, message: impl Into<Vec<u8>>) -> Self {
        Diagnostic {
            level: DiagnosticLevel::Warning,
            function,
            message: message.into(),
        }
    }

    /// The text PHP passes to error handlers: `"<function>(): <message>"` (`php_error_docref()` with
    /// `html_errors=0`, as in the CLI).
    pub fn text(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.function.len() + 4 + self.message.len());
        out.extend_from_slice(self.function.as_bytes());
        out.extend_from_slice(b"(): ");
        out.extend_from_slice(&self.message);
        out
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.text()))
    }
}

/// A PHP `ValueError` thrown by `preg_match()` for an invalid argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValueError {
    /// The verbatim exception message, e.g. `preg_match(): Argument #4 ($flags) must be a PREG_* constant`.
    pub message: String,
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ValueError {}

/// Whether PHP would see the subject string as flagged valid UTF-8 (`IS_STR_VALID_UTF8`, see the module
/// documentation). Only `/u` patterns with `(*NOTEMPTY)` or `(*NOTEMPTY_ATSTART)` can tell the difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SubjectUtf8 {
    /// Flagged when empty or a single ASCII byte, like PHP's interned singleton strings (the empty string
    /// and `ZSTR_CHAR()`), which PHP produces for such strings in almost every code path.
    #[default]
    Auto,
    /// The subject carries the flag (it should then be valid UTF-8; an invalid subject is checked anyway).
    Flagged,
    /// The subject does not carry the flag.
    Unflagged,
}

impl SubjectUtf8 {
    fn flagged(self, subject: &[u8]) -> bool {
        match self {
            SubjectUtf8::Auto => subject.is_empty() || (subject.len() == 1 && subject[0] < 0x80),
            SubjectUtf8::Flagged => true,
            SubjectUtf8::Unflagged => false,
        }
    }
}

/// A compiled pattern as PHP caches it (`pcre_cache_entry`).
#[derive(Debug)]
pub struct CompiledPattern {
    re: Code,
    /// `PREG_JIT` when JIT code exists.
    preg_options: u32,
    /// The PCRE2 options derived from the modifiers (not those the pattern sets itself, like `(*UTF)`).
    compile_options: u32,
    capture_count: u32,
    name_count: u32,
}

impl CompiledPattern {
    /// The compiled PCRE2 pattern.
    pub fn code(&self) -> &Code {
        &self.re
    }

    /// The PCRE2 compile options PHP derived from the modifiers.
    pub fn compile_options(&self) -> u32 {
        self.compile_options
    }

    /// Whether PHP would run this pattern with its JIT code.
    pub fn is_jit(&self) -> bool {
        self.preg_options & PREG_JIT != 0
    }

    /// Number of capture groups.
    pub fn capture_count(&self) -> u32 {
        self.capture_count
    }

    /// Number of named groups.
    pub fn name_count(&self) -> u32 {
        self.name_count
    }
}

/// A key of the `$matches` array filled by `preg_match()`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MatchKey {
    /// An integer key (group number).
    Index(i64),
    /// A string key (group name, or `"MARK"`).
    Name(Vec<u8>),
}

/// A value of the `$matches` array filled by `preg_match()`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MatchValue {
    /// A string: the captured text, or `""` for an unset group without `PREG_UNMATCHED_AS_NULL`.
    Str(Vec<u8>),
    /// `null`: an unset group with `PREG_UNMATCHED_AS_NULL`.
    Null,
    /// `[text, offset]` with `PREG_OFFSET_CAPTURE`: `None` stands for `null`; unset groups have offset -1.
    Pair(Option<Vec<u8>>, i64),
}

/// The `$matches` array: entries in PHP's array order.
pub type MatchArray = Vec<(MatchKey, MatchValue)>;

/// Result of [`Pcre::preg_match_captures`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PregMatch {
    /// The return value: `Some(0 | 1)`, or `None` for `false`.
    pub result: Option<i64>,
    /// The new value of `$matches`; `None` when PHP leaves the variable untouched (the pattern did not
    /// compile).
    pub matches: Option<MatchArray>,
}

/// Insertion-ordered key/value list with the PHP hash operations `populate_subpat_array()` uses.
struct PhpArray {
    entries: MatchArray,
    next_index: i64,
}

impl PhpArray {
    fn new() -> Self {
        PhpArray {
            entries: Vec::new(),
            next_index: 0,
        }
    }

    /// `zend_hash_update()`: replace in place or append.
    fn update(&mut self, key: MatchKey, value: MatchValue) {
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// `zend_hash_add()`: append unless the key exists.
    fn add(&mut self, key: MatchKey, value: MatchValue) {
        if !self.entries.iter().any(|(k, _)| *k == key) {
            self.entries.push((key, value));
        }
    }

    /// `zend_hash_next_index_insert_new()`.
    fn push(&mut self, value: MatchValue) {
        self.entries.push((MatchKey::Index(self.next_index), value));
        self.next_index += 1;
    }
}

/// PHP's per-thread PCRE state (`zend_pcre_globals` plus the `ZEND_TLS` contexts of `php_pcre.c`), with
/// the `pcre.*` ini settings at their defaults: backtrack limit 1000000, recursion limit 100000, JIT on.
pub struct Pcre {
    /// `PCRE_G(pcre_cache)`, keyed by the full regex string.
    cache: HashMap<Vec<u8>, Arc<CompiledPattern>>,
    /// Insertion order of `cache`, oldest first (the order `pcre_clean_cache()` walks).
    cache_order: VecDeque<Vec<u8>>,
    /// `PCRE_G(error_code)`.
    error_code: PregError,
    /// `PCRE_G(jit)` (`pcre.jit`); always false without JIT support.
    jit: bool,
    /// Whether the library has JIT support (`HAVE_PCRE_JIT_SUPPORT`).
    jit_supported: bool,
    backtrack_limit: i64,
    recursion_limit: i64,
    cctx: CompileContext,
    mctx: MatchContext,
    /// PHP's `jit_stack` while it is not assigned to `mctx` (`pcre.jit=0`).
    spare_jit_stack: Option<JitStack>,
    /// PHP's preallocated `mdata` (32 pairs).
    mdata: MatchData,
}

impl fmt::Debug for Pcre {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pcre")
            .field("cached_patterns", &self.cache.len())
            .field("error_code", &self.error_code)
            .field("jit", &self.jit)
            .field("backtrack_limit", &self.backtrack_limit)
            .field("recursion_limit", &self.recursion_limit)
            .finish_non_exhaustive()
    }
}

impl Default for Pcre {
    fn default() -> Self {
        Self::new()
    }
}

impl Pcre {
    /// A fresh state, as after PHP's module startup.
    ///
    /// Port of `PHP_GINIT_FUNCTION(pcre)`, `php_pcre_init_pcre2()` and the ini handlers run at startup
    /// (`OnUpdateBacktrackLimit`, `OnUpdateRecursionLimit`, `OnUpdateJit`). PHP refuses to start when it
    /// cannot allocate the JIT stack; this port runs without JIT instead.
    pub fn new() -> Self {
        let jit_supported = pcre2::Config::get().jit;
        let jit_stack = if jit_supported {
            JitStack::new(PCRE_JIT_STACK_MIN_SIZE, PCRE_JIT_STACK_MAX_SIZE)
        } else {
            None
        };
        let mut pcre = Pcre {
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            error_code: PregError::NoError,
            jit: jit_supported && jit_stack.is_some(),
            jit_supported: jit_supported && jit_stack.is_some(),
            backtrack_limit: DEFAULT_BACKTRACK_LIMIT,
            recursion_limit: DEFAULT_RECURSION_LIMIT,
            cctx: CompileContext::new(),
            mctx: MatchContext::new(),
            spare_jit_stack: jit_stack,
            mdata: MatchData::new(PHP_PCRE_PREALLOC_MDATA_SIZE),
        };
        pcre.set_backtrack_limit(DEFAULT_BACKTRACK_LIMIT);
        pcre.set_recursion_limit(DEFAULT_RECURSION_LIMIT);
        let jit = pcre.jit;
        pcre.set_jit(jit);
        pcre
    }

    /// `pcre.backtrack_limit`.
    pub fn backtrack_limit(&self) -> i64 {
        self.backtrack_limit
    }

    /// Sets `pcre.backtrack_limit` (the match context's match limit).
    ///
    /// Port of `OnUpdateBacktrackLimit()`: the value is converted with C's `(uint32_t)` cast.
    pub fn set_backtrack_limit(&mut self, limit: i64) {
        self.backtrack_limit = limit;
        self.mctx.set_match_limit(limit as u32);
    }

    /// `pcre.recursion_limit`.
    pub fn recursion_limit(&self) -> i64 {
        self.recursion_limit
    }

    /// Sets `pcre.recursion_limit` (the match context's depth limit).
    ///
    /// Port of `OnUpdateRecursionLimit()`: the value is converted with C's `(uint32_t)` cast.
    pub fn set_recursion_limit(&mut self, limit: i64) {
        self.recursion_limit = limit;
        self.mctx.set_depth_limit(limit as u32);
    }

    /// `pcre.jit`: whether newly compiled patterns get JIT code. Always false without JIT support.
    pub fn jit(&self) -> bool {
        self.jit
    }

    /// Whether the linked PCRE2 has JIT support (and the JIT stack could be allocated).
    pub fn jit_supported(&self) -> bool {
        self.jit_supported
    }

    /// Sets `pcre.jit`. Patterns already in the cache keep the JIT code they were compiled with; while the
    /// setting is off, the JIT stack is detached from the match context, so such patterns run on the
    /// JIT's small machine-stack area, as in PHP. Ignored without JIT support.
    ///
    /// Port of `OnUpdateJit()`.
    pub fn set_jit(&mut self, on: bool) {
        if !self.jit_supported {
            return;
        }
        self.jit = on;
        if on {
            if let Some(stack) = self.spare_jit_stack.take() {
                self.spare_jit_stack = self.mctx.assign_jit_stack(Some(stack));
            }
        } else if self.mctx.has_jit_stack() {
            self.spare_jit_stack = self.mctx.assign_jit_stack(None);
        }
    }

    /// `preg_last_error()`.
    pub fn last_error(&self) -> PregError {
        self.error_code
    }

    /// `preg_last_error_msg()`.
    pub fn last_error_msg(&self) -> &'static str {
        self.error_code.message()
    }

    /// Number of patterns in the compiled-pattern cache.
    pub fn cached_patterns(&self) -> usize {
        self.cache.len()
    }

    fn handle_exec_error(&mut self, pcre_code: i32) {
        self.error_code = PregError::from_pcre2_error(pcre_code);
    }

    /// Parses a PHP regex (delimiters, pattern, modifiers), compiles it or fetches it from the cache.
    /// Returns `None` after emitting a warning when the regex is invalid; `function` names the PHP function
    /// for the warning prefix.
    ///
    /// Port of `pcre_get_compiled_regex_cache_ex()` (PHPSRC/ext/pcre/php_pcre.c) with `locale_aware` and no
    /// `setlocale()` call (PHP's built-in character tables).
    pub fn compile(
        &mut self,
        regex: &[u8],
        function: &'static str,
        diagnostics: &mut dyn FnMut(Diagnostic),
    ) -> Option<Arc<CompiledPattern>> {
        if let Some(entry) = self.cache.get(regex) {
            return Some(Arc::clone(entry));
        }

        let end = regex.len();
        let mut p = 0;

        // Parse through the leading whitespace, and display a warning if we get to the end without
        // encountering a delimiter. (`isspace()` of the C/C.UTF-8 locale: ASCII whitespace only.)
        while p < end && is_c_space(regex[p]) {
            p += 1;
        }
        if p >= end {
            diagnostics(Diagnostic::warning(function, "Empty regular expression"));
            self.handle_exec_error(error_codes::INTERNAL);
            return None;
        }

        // Get the delimiter and display a warning if it is alphanumeric or a backslash.
        let mut delimiter = regex[p];
        p += 1;
        if delimiter.is_ascii_alphanumeric() || delimiter == b'\\' || delimiter == 0 {
            diagnostics(Diagnostic::warning(
                function,
                "Delimiter must not be alphanumeric, backslash, or NUL byte",
            ));
            self.handle_exec_error(error_codes::INTERNAL);
            return None;
        }

        let start_delimiter = delimiter;
        const BRACKETS: &[u8; 14] = b"([{< )]}> )]}>";
        if let Some(i) = BRACKETS.iter().position(|&c| c == delimiter) {
            delimiter = BRACKETS[i + 5];
        }
        let end_delimiter = delimiter;

        let mut pp = p;
        if start_delimiter == end_delimiter {
            // We need to iterate through the pattern, searching for the ending delimiter, but skipping the
            // backslashed delimiters. If the ending delimiter is not found, display a warning.
            while pp < end {
                if regex[pp] == b'\\' && pp + 1 < end {
                    pp += 1;
                } else if regex[pp] == delimiter {
                    break;
                }
                pp += 1;
            }
        } else {
            // We iterate through the pattern, searching for the matching ending delimiter. For each matching
            // starting delimiter, we increment nesting level, and decrement it for each matching ending
            // delimiter. If we reach the end of the pattern without matching, display a warning.
            let mut brackets = 1i32;
            while pp < end {
                if regex[pp] == b'\\' && pp + 1 < end {
                    pp += 1;
                } else if regex[pp] == end_delimiter && {
                    brackets -= 1;
                    brackets <= 0
                } {
                    break;
                } else if regex[pp] == start_delimiter {
                    brackets += 1;
                }
                pp += 1;
            }
        }

        if pp >= end {
            let mut message = if start_delimiter == end_delimiter {
                b"No ending delimiter '".to_vec()
            } else {
                b"No ending matching delimiter '".to_vec()
            };
            message.push(delimiter);
            message.extend_from_slice(b"' found");
            diagnostics(Diagnostic::warning(function, message));
            self.handle_exec_error(error_codes::INTERNAL);
            return None;
        }

        // Make a copy of the actual pattern.
        let pattern = &regex[p..pp];

        // Move on to the options.
        pp += 1;

        // Parse through the options, setting appropriate flags. Display a warning if we encounter an
        // unknown modifier.
        let mut coptions: u32 = 0;
        let mut eoptions: u32 = 0;
        while pp < end {
            let c = regex[pp];
            pp += 1;
            match c {
                // Perl compatible options
                b'i' => coptions |= co::CASELESS,
                b'm' => coptions |= co::MULTILINE,
                b'n' => coptions |= co::NO_AUTO_CAPTURE,
                b's' => coptions |= co::DOTALL,
                b'x' => coptions |= co::EXTENDED,
                // PCRE specific options
                b'A' => coptions |= co::ANCHORED,
                b'D' => coptions |= co::DOLLAR_ENDONLY,
                b'r' => eoptions |= ffi::PCRE2_EXTRA_CASELESS_RESTRICT,
                b'S' | b'X' => {}
                b'U' => coptions |= co::UNGREEDY,
                // \d, \w etc. match Unicode characters only with UCP; \C is unsafe in UTF mode.
                b'u' => coptions |= co::UTF | co::UCP | co::NEVER_BACKSLASH_C,
                b'J' => coptions |= co::DUPNAMES,
                b' ' | b'\n' | b'\r' => {}
                // 'e' (legacy eval) and everything else
                _ => {
                    if c != 0 {
                        let mut message = b"Unknown modifier '".to_vec();
                        message.push(c);
                        message.push(b'\'');
                        diagnostics(Diagnostic::warning(function, message));
                    } else {
                        diagnostics(Diagnostic::warning(
                            function,
                            "NUL byte is not a valid modifier",
                        ));
                    }
                    self.handle_exec_error(error_codes::INTERNAL);
                    return None;
                }
            }
        }

        self.cctx.use_default_tables();
        self.cctx.set_extra_options(eoptions);

        // Compile pattern and display a warning if compilation failed.
        let mut re = match Code::compile(pattern, coptions, Some(&self.cctx)) {
            Ok(re) => re,
            Err(err) => {
                let err_msg = if err.code == error_codes::BACKSLASH_C_CALLER_DISABLED {
                    b"using \\C is incompatible with the 'u' modifier".to_vec()
                } else {
                    // PCRE2_UCHAR error[128]
                    pcre2::error_message(err.code, 128)
                };
                let mut message = b"Compilation failed: ".to_vec();
                message.extend_from_slice(&err_msg);
                message.extend_from_slice(format!(" at offset {}", err.offset).as_bytes());
                diagnostics(Diagnostic::warning(function, message));
                self.handle_exec_error(error_codes::INTERNAL);
                return None;
            }
        };

        let mut poptions = 0;
        if self.jit {
            // Enable PCRE JIT compiler
            let rc = re.jit_compile(ffi::PCRE2_JIT_COMPLETE);
            if rc >= 0 {
                if re.jit_size() > 0 {
                    poptions |= PREG_JIT;
                }
            } else if rc == error_codes::NOMEMORY {
                diagnostics(Diagnostic::warning(
                    function,
                    "Allocation of JIT memory failed, PCRE JIT will be disabled. \
                     This is likely caused by security restrictions. \
                     Either grant PHP permission to allocate executable memory, or set pcre.jit=0",
                ));
                self.jit = false;
            } else {
                let mut message = b"JIT compilation failed: ".to_vec();
                message.extend_from_slice(&pcre2::error_message(rc, 128));
                diagnostics(Diagnostic::warning(function, message));
                self.handle_exec_error(error_codes::INTERNAL);
            }
        }

        // If we reached cache limit, clean out the items from the head of the list; these are supposedly
        // the oldest ones (but not necessarily the least used ones). No entry is in use here (matching
        // never re-enters), so `pcre_clean_cache()` removes the PCRE_CACHE_SIZE / 8 oldest entries.
        if self.cache.len() == PCRE_CACHE_SIZE {
            for _ in 0..PCRE_CACHE_SIZE / 8 {
                if let Some(key) = self.cache_order.pop_front() {
                    self.cache.remove(&key);
                }
            }
        }

        let entry = Arc::new(CompiledPattern {
            capture_count: re.capture_count(),
            name_count: re.name_count(),
            re,
            preg_options: poptions,
            compile_options: coptions,
        });
        self.cache.insert(regex.to_vec(), Arc::clone(&entry));
        self.cache_order.push_back(regex.to_vec());
        Some(entry)
    }

    /// `preg_match($pattern, $subject)`: `Some(1)` or `Some(0)`, or `None` for `false` (after a warning,
    /// or a match error recorded in [`Pcre::last_error`]). The subject is seen as [`SubjectUtf8::Auto`].
    ///
    /// Port of the two-argument `ZEND_FRAMELESS_FUNCTION(preg_match, 2)` and `php_pcre_match_impl()`
    /// without `$matches` (PHPSRC/ext/pcre/php_pcre.c). Without `$matches`, an empty match is followed by
    /// PHP's `PCRE2_NOTEMPTY_ATSTART | PCRE2_ANCHORED` retry, whose errors make the call return `false`.
    pub fn preg_match(
        &mut self,
        pattern: &[u8],
        subject: &[u8],
        diagnostics: &mut dyn FnMut(Diagnostic),
    ) -> Option<i64> {
        self.preg_match_ex(pattern, subject, SubjectUtf8::Auto, diagnostics)
    }

    /// [`Pcre::preg_match`] with an explicit view of the subject's `IS_STR_VALID_UTF8` flag.
    pub fn preg_match_ex(
        &mut self,
        pattern: &[u8],
        subject: &[u8],
        subject_utf8: SubjectUtf8,
        diagnostics: &mut dyn FnMut(Diagnostic),
    ) -> Option<i64> {
        let pce = self.compile(pattern, "preg_match", diagnostics)?;
        let flagged = subject_utf8.flagged(subject);
        self.match_impl(&pce, subject, flagged, None, 0, 0, diagnostics)
            .unwrap_or_else(|_| unreachable!("flags are 0"))
    }

    /// `preg_match($pattern, $subject, $matches, $flags, $offset)`.
    ///
    /// Port of `php_do_pcre_match()` and `php_pcre_match_impl()` with `$matches`
    /// (PHPSRC/ext/pcre/php_pcre.c). `flags` takes `PREG_OFFSET_CAPTURE` and `PREG_UNMATCHED_AS_NULL`;
    /// any other bit in the low byte is the PHP `ValueError`. (PHP has already set `$matches` to an empty
    /// array when it throws that one; the `$offset` `ValueError` leaves `$matches` untouched.)
    pub fn preg_match_captures(
        &mut self,
        pattern: &[u8],
        subject: &[u8],
        flags: i64,
        offset: i64,
        subject_utf8: SubjectUtf8,
        diagnostics: &mut dyn FnMut(Diagnostic),
    ) -> Result<PregMatch, ValueError> {
        let Some(pce) = self.compile(pattern, "preg_match", diagnostics) else {
            return Ok(PregMatch {
                result: None,
                matches: None,
            });
        };
        if offset == i64::MIN {
            return Err(ValueError {
                message: format!(
                    "preg_match(): Argument #5 ($offset) must be greater than {}",
                    i64::MIN
                ),
            });
        }
        let mut matches = PhpArray::new();
        let flagged = subject_utf8.flagged(subject);
        let result = self.match_impl(
            &pce,
            subject,
            flagged,
            Some(&mut matches),
            flags,
            offset,
            diagnostics,
        )?;
        Ok(PregMatch {
            result,
            matches: Some(matches.entries),
        })
    }

    /// Port of `php_pcre_match_impl()` for `global = false` (PHPSRC/ext/pcre/php_pcre.c).
    #[allow(clippy::too_many_arguments)]
    fn match_impl(
        &mut self,
        pce: &CompiledPattern,
        subject: &[u8],
        subject_flagged_utf8: bool,
        subpats: Option<&mut PhpArray>,
        flags: i64,
        start_offset: i64,
        diagnostics: &mut dyn FnMut(Diagnostic),
    ) -> Result<Option<i64>, ValueError> {
        let subject_len = subject.len();

        let (offset_capture, unmatched_as_null) = if flags != 0 {
            if flags & 0xff != 0 {
                // subpats_order must be 0 for preg_match()
                return Err(ValueError {
                    message: "preg_match(): Argument #4 ($flags) must be a PREG_* constant".into(),
                });
            }
            (
                flags & PREG_OFFSET_CAPTURE != 0,
                flags & PREG_UNMATCHED_AS_NULL != 0,
            )
        } else {
            (false, false)
        };

        // Negative offset counts from the end of the string.
        let start_offset2: usize = if start_offset < 0 {
            let back = start_offset.unsigned_abs();
            if back <= subject_len as u64 {
                subject_len - back as usize
            } else {
                0
            }
        } else {
            usize::try_from(start_offset).unwrap_or(usize::MAX)
        };

        if start_offset2 > subject_len {
            self.handle_exec_error(error_codes::BADOFFSET);
            return Ok(None);
        }

        let num_subpats = pce.capture_count as usize + 1;

        // Build a mapping from subpattern numbers to their names, only if there are named subpatterns.
        let subpat_names: Option<Vec<Option<Vec<u8>>>> = if subpats.is_some() && pce.name_count > 0
        {
            let mut names = vec![None; num_subpats];
            for (number, name) in pce.re.names() {
                if let Some(slot) = names.get_mut(number as usize) {
                    *slot = Some(name);
                }
            }
            Some(names)
        } else {
            None
        };

        let mut matched: i64 = 0;
        self.error_code = PregError::NoError;

        let mut own_mdata;
        let match_data: &mut MatchData = if num_subpats <= PHP_PCRE_PREALLOC_MDATA_SIZE as usize {
            &mut self.mdata
        } else {
            own_mdata = MatchData::for_code(&pce.re);
            &mut own_mdata
        };

        let options = if pce.compile_options & co::UTF != 0
            && !is_known_valid_utf8(subject_flagged_utf8, subject, start_offset2)
        {
            0
        } else {
            ffi::PCRE2_NO_UTF_CHECK
        };

        // Execute the regular expression.
        let mut count = if pce.preg_options & PREG_JIT != 0 && options != 0 {
            pce.re.jit_match(
                subject,
                start_offset2,
                ffi::PCRE2_NO_UTF_CHECK,
                match_data,
                Some(&self.mctx),
            )
        } else {
            pce.re.match_at(
                subject,
                start_offset2,
                options,
                match_data,
                Some(&self.mctx),
            )
        };

        let mut exec_error = None;
        // `while (1)` of php_pcre_match_impl(); without `global` every path leaves after one round.
        if count >= 0 {
            // Check for too many substrings condition.
            if count == 0 {
                diagnostics(Diagnostic {
                    level: DiagnosticLevel::Notice,
                    function: "preg_match",
                    message: b"Matched, but too many substrings".to_vec(),
                });
                count = num_subpats as i32;
            }
            matched += 1;

            if let Some(subpats) = subpats {
                let offsets = match_data.ovector();
                if offsets[1] < offsets[0] {
                    diagnostics(Diagnostic::warning(
                        "preg_match",
                        "Get subpatterns list failed",
                    ));
                    return Ok(None);
                }
                // PHP copies the mark with `ZVAL_STRING(&val, (char *)mark)`, i.e. up to its first NUL byte.
                let mark = match_data
                    .mark()
                    .map(|m| m.split(|&b| b == 0).next().unwrap_or_default().to_vec());
                populate_subpat_array(
                    subpats,
                    subject,
                    offsets,
                    subpat_names.as_deref(),
                    num_subpats,
                    count as usize,
                    mark,
                    offset_capture,
                    unmatched_as_null,
                );
            } else {
                // Advance to the next piece.
                let offsets = match_data.ovector();
                let next = offsets[1];
                // If we have matched an empty string, mimic what Perl's /g options does: set
                // PCRE2_NOTEMPTY_ATSTART and try the match again at the same point.
                if next == offsets[0] {
                    count = pce.re.match_at(
                        subject,
                        next,
                        ffi::PCRE2_NO_UTF_CHECK | ffi::PCRE2_NOTEMPTY_ATSTART | ffi::PCRE2_ANCHORED,
                        match_data,
                        Some(&self.mctx),
                    );
                    if count < 0 && count != error_codes::NOMATCH {
                        exec_error = Some(count);
                    }
                    // A match or PCRE2_ERROR_NOMATCH: nothing more to do without `global` (PHP advances
                    // the offset for the next global round, which never comes).
                }
            }
        } else if count != error_codes::NOMATCH {
            exec_error = Some(count);
        }
        if let Some(code) = exec_error {
            self.handle_exec_error(code);
        }

        if self.error_code == PregError::NoError {
            Ok(Some(matched))
        } else {
            Ok(None)
        }
    }

    /// Twig's `matches` operator at run time.
    ///
    /// Twig: `CoreExtension::matches()` (TWIG/src/Extension/CoreExtension.php): `preg_match($regexp, $str ??
    /// '')` under an error handler that throws on the first warning. The caller passes `b""` for a null
    /// subject.
    pub fn twig_matches(&mut self, regexp: &[u8], subject: &[u8]) -> Result<i64, MatchesError> {
        self.twig_matches_ex(regexp, subject, SubjectUtf8::Auto)
    }

    /// [`Pcre::twig_matches`] with an explicit view of the subject's `IS_STR_VALID_UTF8` flag: PHP flags
    /// the string conversions of integers and floats, so pass [`SubjectUtf8::Flagged`] for such subjects
    /// (`{{ 12 matches '/(*NOTEMPTY)x*/u' }}` is 1 in PHP, while the literal `'ab'` gives 0).
    pub fn twig_matches_ex(
        &mut self,
        regexp: &[u8],
        subject: &[u8],
        subject_utf8: SubjectUtf8,
    ) -> Result<i64, MatchesError> {
        let mut first = None;
        let result = self.preg_match_ex(regexp, subject, subject_utf8, &mut |d| {
            first.get_or_insert(d);
        });
        if let Some(diagnostic) = first {
            return Err(MatchesError::Invalid(diagnostic));
        }
        result.ok_or(MatchesError::Failed(self.error_code))
    }

    /// Twig's compile-time check of a constant `matches` regexp. Returns the `SyntaxError` message on
    /// failure.
    ///
    /// Twig: `MatchesBinary::__construct()` (TWIG/src/Node/Expression/Binary/MatchesBinary.php):
    /// `preg_match($regexp, '')` under an error handler that throws
    /// `SyntaxError(sprintf('Regexp "%s" passed to "matches" is not valid: %s.', $regexp, substr($m, 14)))`
    /// on the first warning. A match error (no warning) is ignored, as in Twig.
    pub fn twig_validate_constant(&mut self, regexp: &[u8]) -> Result<(), Vec<u8>> {
        let mut first = None;
        self.preg_match(regexp, b"", &mut |d| {
            first.get_or_insert(d);
        });
        match first {
            None => Ok(()),
            Some(diagnostic) => {
                let mut message = b"Regexp \"".to_vec();
                message.extend_from_slice(regexp);
                message.extend_from_slice(b"\" passed to \"matches\" is not valid: ");
                message.extend_from_slice(php_substr(&diagnostic.text(), 14));
                message.push(b'.');
                Err(message)
            }
        }
    }
}

/// The `SyntaxError` message of Twig's compile-time check when the constant regexp is `null`
/// (`{{ x matches null }}`), before `Error::updateRepr()` adds the template name and line.
///
/// Twig: `MatchesBinary::__construct()` (TWIG/src/Node/Expression/Binary/MatchesBinary.php) calls
/// `preg_match(null, '')`. Before the function body runs, PHP's parameter parsing raises the `E_DEPRECATED`
/// "preg_match(): Passing null to parameter #1 ($pattern) of type string is deprecated"
/// (`zend_null_arg_deprecated()`, PHPSRC/Zend/zend_API.c); Twig's handler turns it into
/// `SyntaxError(sprintf('Regexp "%s" passed to "matches" is not valid: %s.', null, substr($m, 14)))`, and
/// because that exception is pending the parameter parsing fails and `preg_match()` returns without
/// touching the PCRE state (no compilation, `preg_last_error()` unchanged). Other non-string constants
/// (`true`, numbers) are converted with PHP's string conversion and checked with
/// [`Pcre::twig_validate_constant`] like strings.
pub fn twig_null_constant_error() -> Vec<u8> {
    b"Regexp \"\" passed to \"matches\" is not valid: \
      Passing null to parameter #1 ($pattern) of type string is deprecated."
        .to_vec()
}

/// Error of Twig's `matches` operator at run time ([`Pcre::twig_matches`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MatchesError {
    /// `preg_match()` raised a warning; Twig's error handler turns the first one into
    /// `RuntimeError(sprintf('Regexp "%s" passed to "matches" is not valid', $regexp).substr($m, 12))`.
    Invalid(Diagnostic),
    /// `preg_match()` returned `false` without a warning; Twig throws
    /// `RuntimeError(sprintf('Regexp "%s" passed to "matches" failed: %s.', $regexp, preg_last_error_msg()))`.
    Failed(PregError),
}

impl MatchesError {
    /// The `RuntimeError` message Twig builds for `regexp`, before `Error::updateRepr()` adds the template
    /// name and line. Note that the "not valid" form has no final period.
    pub fn message(&self, regexp: &[u8]) -> Vec<u8> {
        let mut message = b"Regexp \"".to_vec();
        message.extend_from_slice(regexp);
        match self {
            MatchesError::Invalid(diagnostic) => {
                message.extend_from_slice(b"\" passed to \"matches\" is not valid");
                message.extend_from_slice(php_substr(&diagnostic.text(), 12));
            }
            MatchesError::Failed(error) => {
                message.extend_from_slice(b"\" passed to \"matches\" failed: ");
                message.extend_from_slice(error.message().as_bytes());
                message.push(b'.');
            }
        }
        message
    }
}

impl fmt::Display for MatchesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatchesError::Invalid(diagnostic) => write!(f, "invalid regexp: {diagnostic}"),
            MatchesError::Failed(error) => write!(f, "match failed: {error}"),
        }
    }
}

impl std::error::Error for MatchesError {}

/// `preg_quote($str, $delimiter)`: backslash before every regex metacharacter
/// (`. \ + * ? [ ^ ] $ ( ) { } = ! > < | : - #`) and before the first byte of `delimiter`; a NUL byte
/// becomes `\000`. Twig uses it to build the patterns for dynamic filter, function and test names.
///
/// Port of `PHP_FUNCTION(preg_quote)` (PHPSRC/ext/pcre/php_pcre.c).
pub fn preg_quote(s: &[u8], delimiter: Option<&[u8]>) -> Vec<u8> {
    // `ZSTR_VAL(delim)[0]`: the NUL terminator for an empty delimiter, which the NUL case shadows.
    let delim_char = delimiter.and_then(|d| d.first().copied()).unwrap_or(0);
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        match c {
            b'.' | b'\\' | b'+' | b'*' | b'?' | b'[' | b'^' | b']' | b'$' | b'(' | b')' | b'{'
            | b'}' | b'=' | b'!' | b'>' | b'<' | b'|' | b':' | b'-' | b'#' => {
                out.push(b'\\');
                out.push(c);
            }
            0 => out.extend_from_slice(b"\\000"),
            _ => {
                if c == delim_char {
                    out.push(b'\\');
                }
                out.push(c);
            }
        }
    }
    out
}

/// Port of `is_known_valid_utf8()` (PHPSRC/ext/pcre/php_pcre.c); `flagged` is `ZSTR_IS_VALID_UTF8()`.
fn is_known_valid_utf8(flagged: bool, subject: &[u8], start_offset: usize) -> bool {
    if !flagged {
        // We don't know whether the string is valid UTF-8 or not.
        return false;
    }
    if start_offset == subject.len() {
        // Degenerate case: Offset points to end of string.
        return true;
    }
    // Check that the offset does not point to an UTF-8 continuation byte.
    (subject[start_offset] & 0xc0) != 0x80
}

/// `substr($s, $offset)` for a non-negative offset.
fn php_substr(s: &[u8], offset: usize) -> &[u8] {
    s.get(offset..).unwrap_or(b"")
}

/// `isspace()` in the C and C.UTF-8 locales for one byte: space, `\t`, `\n`, `\v`, `\f`, `\r`.
fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// Port of `populate_subpat_array()` (PHPSRC/ext/pcre/php_pcre.c).
#[allow(clippy::too_many_arguments)]
fn populate_subpat_array(
    subpats: &mut PhpArray,
    subject: &[u8],
    offsets: &[usize],
    subpat_names: Option<&[Option<Vec<u8>>]>,
    num_subpats: usize,
    count: usize,
    mark: Option<Vec<u8>>,
    offset_capture: bool,
    unmatched_as_null: bool,
) {
    let value = |i: usize| -> MatchValue {
        let (start, end) = (offsets[2 * i], offsets[2 * i + 1]);
        if offset_capture {
            if start == pcre2::UNSET {
                MatchValue::Pair(
                    if unmatched_as_null {
                        None
                    } else {
                        Some(Vec::new())
                    },
                    -1,
                )
            } else {
                MatchValue::Pair(Some(subject[start..end].to_vec()), start as i64)
            }
        } else if start == pcre2::UNSET {
            if unmatched_as_null {
                MatchValue::Null
            } else {
                MatchValue::Str(Vec::new())
            }
        } else {
            MatchValue::Str(subject[start..end].to_vec())
        }
    };
    let unset_value = || -> MatchValue {
        // Only reached with unmatched_as_null.
        if offset_capture {
            MatchValue::Pair(None, -1)
        } else {
            MatchValue::Null
        }
    };
    let name_of = |i: usize| subpat_names.and_then(|names| names.get(i).cloned().flatten());

    for i in 0..count {
        let v = value(i);
        if let Some(name) = name_of(i) {
            // add_named(): with DUPNAMES keep the value of the group that actually matched.
            if offsets[2 * i] != pcre2::UNSET {
                subpats.update(MatchKey::Name(name), v.clone());
            } else {
                subpats.add(MatchKey::Name(name), v.clone());
            }
        }
        subpats.push(v);
    }
    if unmatched_as_null {
        for i in count..num_subpats {
            if let Some(name) = name_of(i) {
                subpats.add(MatchKey::Name(name), unset_value());
            }
            subpats.push(unset_value());
        }
    }
    // Add MARK, if available
    if let Some(mark) = mark {
        subpats
            .entries
            .push((MatchKey::Name(b"MARK".to_vec()), MatchValue::Str(mark)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(pcre: &mut Pcre, pattern: &[u8], subject: &[u8]) -> (Option<i64>, Vec<String>) {
        let mut diags = Vec::new();
        let r = pcre.preg_match(pattern, subject, &mut |d| diags.push(d.to_string()));
        (r, diags)
    }

    #[test]
    fn delimiter_and_modifier_warnings() {
        let mut p = Pcre::new();
        assert_eq!(
            run(&mut p, b"", b""),
            (None, vec!["preg_match(): Empty regular expression".into()])
        );
        assert_eq!(p.last_error(), PregError::Internal);
        assert_eq!(
            run(&mut p, b"abc", b""),
            (
                None,
                vec![
                    "preg_match(): Delimiter must not be alphanumeric, backslash, or NUL byte"
                        .into()
                ]
            )
        );
        assert_eq!(
            run(&mut p, b"/abc", b""),
            (
                None,
                vec!["preg_match(): No ending delimiter '/' found".into()]
            )
        );
        assert_eq!(
            run(&mut p, b"(abc", b""),
            (
                None,
                vec!["preg_match(): No ending matching delimiter ')' found".into()]
            )
        );
        assert_eq!(
            run(&mut p, b"/a/e", b""),
            (None, vec!["preg_match(): Unknown modifier 'e'".into()])
        );
        assert_eq!(
            run(&mut p, b"/a/\0", b""),
            (
                None,
                vec!["preg_match(): NUL byte is not a valid modifier".into()]
            )
        );
        assert_eq!(
            run(&mut p, b"/(/", b""),
            (
                None,
                vec![
                    "preg_match(): Compilation failed: missing closing parenthesis at offset 1"
                        .into()
                ]
            )
        );
        assert_eq!(
            run(&mut p, b"/\\C/u", b""),
            (
                None,
                vec!["preg_match(): Compilation failed: using \\C is incompatible with the 'u' modifier at offset 2"
                    .into()]
            )
        );
    }

    #[test]
    fn matching_and_last_error() {
        let mut p = Pcre::new();
        assert_eq!(run(&mut p, b"/b/", b"abc"), (Some(1), vec![]));
        assert_eq!(p.last_error(), PregError::NoError);
        assert_eq!(run(&mut p, b"(B)i", b"abc"), (Some(1), vec![]));
        assert_eq!(run(&mut p, b"/x/", b"abc"), (Some(0), vec![]));
        assert_eq!(run(&mut p, b"/./u", b"\xff"), (None, vec![]));
        assert_eq!(p.last_error(), PregError::BadUtf8);
        assert_eq!(
            p.last_error_msg(),
            "Malformed UTF-8 characters, possibly incorrectly encoded"
        );
        assert_eq!(
            run(&mut p, b"/(?:\\D+|<\\d+>)*[!?]/", b"foobar foobar foobar"),
            (None, vec![])
        );
        assert_eq!(p.last_error(), PregError::BacktrackLimit);
    }

    #[test]
    fn twig_messages() {
        let mut p = Pcre::new();
        let e = p.twig_matches(b"/(/", b"abc").unwrap_err();
        assert_eq!(
            e.message(b"/(/"),
            b"Regexp \"/(/\" passed to \"matches\" is not valid: Compilation failed: missing closing parenthesis at offset 1"
        );
        assert_eq!(
            p.twig_validate_constant(b"/(/").unwrap_err(),
            b"Regexp \"/(/\" passed to \"matches\" is not valid: Compilation failed: missing closing parenthesis at offset 1."
        );
        let e = p.twig_matches(b"/./u", b"\xff").unwrap_err();
        assert_eq!(
            e.message(b"/./u"),
            b"Regexp \"/./u\" passed to \"matches\" failed: Malformed UTF-8 characters, possibly incorrectly encoded."
        );
        assert_eq!(p.twig_matches(b"/a/", b"cat"), Ok(1));
        assert_eq!(p.twig_matches(b"/a/", b"dog"), Ok(0));
        assert_eq!(p.twig_validate_constant(b"/a/"), Ok(()));
    }

    #[test]
    fn captures_follow_php_array_rules() {
        let mut p = Pcre::new();
        let r = p
            .preg_match_captures(b"/(?<x>a)|(b)/", b"b", 0, 0, SubjectUtf8::Auto, &mut |_| {})
            .unwrap();
        assert_eq!(r.result, Some(1));
        assert_eq!(
            r.matches.unwrap(),
            vec![
                (MatchKey::Index(0), MatchValue::Str(b"b".to_vec())),
                (MatchKey::Name(b"x".to_vec()), MatchValue::Str(Vec::new())),
                (MatchKey::Index(1), MatchValue::Str(Vec::new())),
                (MatchKey::Index(2), MatchValue::Str(b"b".to_vec())),
            ]
        );
        let e = p
            .preg_match_captures(b"/a/", b"a", 1, 0, SubjectUtf8::Auto, &mut |_| {})
            .unwrap_err();
        assert_eq!(
            e.message,
            "preg_match(): Argument #4 ($flags) must be a PREG_* constant"
        );
    }
}
