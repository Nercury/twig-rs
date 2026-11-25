//! Raw bindings to the vendored PCRE2 10.44 (8-bit library), declared from `pcre2/pcre2.h`.
//!
//! `build.rs` renames the library's symbols from `pcre2_*_8` to `twig_pcre2_*`; the Rust names keep
//! PCRE2's spelling and `#[link_name]` points at the renamed symbols. Only what this crate uses is bound.

#![allow(non_camel_case_types, dead_code)]

use core::ffi::{c_int, c_void};

/// `PCRE2_SIZE` (`size_t`).
pub type PCRE2_SIZE = usize;
/// `PCRE2_UCHAR8`.
pub type PCRE2_UCHAR8 = u8;
/// `PCRE2_SPTR8`.
pub type PCRE2_SPTR8 = *const PCRE2_UCHAR8;

/// `PCRE2_UNSET` / `PCRE2_ZERO_TERMINATED`.
pub const PCRE2_UNSET: PCRE2_SIZE = !0;

macro_rules! opaque {
    ($($name:ident),* $(,)?) => {$(
        #[repr(C)]
        pub struct $name {
            _private: [u8; 0],
            _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
        }
    )*};
}

opaque!(
    pcre2_code_8,
    pcre2_general_context_8,
    pcre2_compile_context_8,
    pcre2_match_context_8,
    pcre2_match_data_8,
    pcre2_jit_stack_8,
);

/// `pcre2_jit_callback_8`.
pub type pcre2_jit_callback_8 = Option<unsafe extern "C" fn(*mut c_void) -> *mut pcre2_jit_stack_8>;

// Options for pcre2_compile(), pcre2_match(), pcre2_dfa_match().
pub const PCRE2_ANCHORED: u32 = 0x8000_0000;
pub const PCRE2_NO_UTF_CHECK: u32 = 0x4000_0000;
pub const PCRE2_ENDANCHORED: u32 = 0x2000_0000;

// Options for pcre2_compile() only.
pub const PCRE2_ALLOW_EMPTY_CLASS: u32 = 0x0000_0001;
pub const PCRE2_ALT_BSUX: u32 = 0x0000_0002;
pub const PCRE2_AUTO_CALLOUT: u32 = 0x0000_0004;
pub const PCRE2_CASELESS: u32 = 0x0000_0008;
pub const PCRE2_DOLLAR_ENDONLY: u32 = 0x0000_0010;
pub const PCRE2_DOTALL: u32 = 0x0000_0020;
pub const PCRE2_DUPNAMES: u32 = 0x0000_0040;
pub const PCRE2_EXTENDED: u32 = 0x0000_0080;
pub const PCRE2_FIRSTLINE: u32 = 0x0000_0100;
pub const PCRE2_MATCH_UNSET_BACKREF: u32 = 0x0000_0200;
pub const PCRE2_MULTILINE: u32 = 0x0000_0400;
pub const PCRE2_NEVER_UCP: u32 = 0x0000_0800;
pub const PCRE2_NEVER_UTF: u32 = 0x0000_1000;
pub const PCRE2_NO_AUTO_CAPTURE: u32 = 0x0000_2000;
pub const PCRE2_NO_AUTO_POSSESS: u32 = 0x0000_4000;
pub const PCRE2_NO_DOTSTAR_ANCHOR: u32 = 0x0000_8000;
pub const PCRE2_NO_START_OPTIMIZE: u32 = 0x0001_0000;
pub const PCRE2_UCP: u32 = 0x0002_0000;
pub const PCRE2_UNGREEDY: u32 = 0x0004_0000;
pub const PCRE2_UTF: u32 = 0x0008_0000;
pub const PCRE2_NEVER_BACKSLASH_C: u32 = 0x0010_0000;
pub const PCRE2_ALT_CIRCUMFLEX: u32 = 0x0020_0000;
pub const PCRE2_ALT_VERBNAMES: u32 = 0x0040_0000;
pub const PCRE2_USE_OFFSET_LIMIT: u32 = 0x0080_0000;
pub const PCRE2_EXTENDED_MORE: u32 = 0x0100_0000;
pub const PCRE2_LITERAL: u32 = 0x0200_0000;
pub const PCRE2_MATCH_INVALID_UTF: u32 = 0x0400_0000;

// Extra compile options (compile context).
pub const PCRE2_EXTRA_ALLOW_SURROGATE_ESCAPES: u32 = 0x0000_0001;
pub const PCRE2_EXTRA_BAD_ESCAPE_IS_LITERAL: u32 = 0x0000_0002;
pub const PCRE2_EXTRA_MATCH_WORD: u32 = 0x0000_0004;
pub const PCRE2_EXTRA_MATCH_LINE: u32 = 0x0000_0008;
pub const PCRE2_EXTRA_ESCAPED_CR_IS_LF: u32 = 0x0000_0010;
pub const PCRE2_EXTRA_ALT_BSUX: u32 = 0x0000_0020;
pub const PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK: u32 = 0x0000_0040;
pub const PCRE2_EXTRA_CASELESS_RESTRICT: u32 = 0x0000_0080;
pub const PCRE2_EXTRA_ASCII_BSD: u32 = 0x0000_0100;
pub const PCRE2_EXTRA_ASCII_BSS: u32 = 0x0000_0200;
pub const PCRE2_EXTRA_ASCII_BSW: u32 = 0x0000_0400;
pub const PCRE2_EXTRA_ASCII_POSIX: u32 = 0x0000_0800;
pub const PCRE2_EXTRA_ASCII_DIGIT: u32 = 0x0000_1000;

// pcre2_jit_compile() options.
pub const PCRE2_JIT_COMPLETE: u32 = 0x0000_0001;
pub const PCRE2_JIT_PARTIAL_SOFT: u32 = 0x0000_0002;
pub const PCRE2_JIT_PARTIAL_HARD: u32 = 0x0000_0004;
pub const PCRE2_JIT_INVALID_UTF: u32 = 0x0000_0100;

// Match-time options.
pub const PCRE2_NOTBOL: u32 = 0x0000_0001;
pub const PCRE2_NOTEOL: u32 = 0x0000_0002;
pub const PCRE2_NOTEMPTY: u32 = 0x0000_0004;
pub const PCRE2_NOTEMPTY_ATSTART: u32 = 0x0000_0008;
pub const PCRE2_PARTIAL_SOFT: u32 = 0x0000_0010;
pub const PCRE2_PARTIAL_HARD: u32 = 0x0000_0020;
pub const PCRE2_NO_JIT: u32 = 0x0000_2000;
pub const PCRE2_COPY_MATCHED_SUBJECT: u32 = 0x0000_4000;
pub const PCRE2_DISABLE_RECURSELOOP_CHECK: u32 = 0x0004_0000;

// Compile error codes used by this crate.
pub const PCRE2_ERROR_BACKSLASH_C_CALLER_DISABLED: c_int = 183;

// Match-time error codes.
pub const PCRE2_ERROR_NOMATCH: c_int = -1;
pub const PCRE2_ERROR_PARTIAL: c_int = -2;
pub const PCRE2_ERROR_UTF8_ERR1: c_int = -3;
pub const PCRE2_ERROR_UTF8_ERR21: c_int = -23;
pub const PCRE2_ERROR_BADOFFSET: c_int = -33;
pub const PCRE2_ERROR_BADOPTION: c_int = -34;
pub const PCRE2_ERROR_BADUTFOFFSET: c_int = -36;
pub const PCRE2_ERROR_INTERNAL: c_int = -44;
pub const PCRE2_ERROR_JIT_BADOPTION: c_int = -45;
pub const PCRE2_ERROR_JIT_STACKLIMIT: c_int = -46;
pub const PCRE2_ERROR_MATCHLIMIT: c_int = -47;
pub const PCRE2_ERROR_NOMEMORY: c_int = -48;
pub const PCRE2_ERROR_NULL: c_int = -51;
pub const PCRE2_ERROR_DEPTHLIMIT: c_int = -53;
pub const PCRE2_ERROR_RECURSIONLIMIT: c_int = -53;
pub const PCRE2_ERROR_HEAPLIMIT: c_int = -63;

// pcre2_pattern_info() requests.
pub const PCRE2_INFO_ALLOPTIONS: u32 = 0;
pub const PCRE2_INFO_ARGOPTIONS: u32 = 1;
pub const PCRE2_INFO_CAPTURECOUNT: u32 = 4;
pub const PCRE2_INFO_JITSIZE: u32 = 10;
pub const PCRE2_INFO_NAMECOUNT: u32 = 17;
pub const PCRE2_INFO_NAMEENTRYSIZE: u32 = 18;
pub const PCRE2_INFO_NAMETABLE: u32 = 19;
pub const PCRE2_INFO_EXTRAOPTIONS: u32 = 26;

// pcre2_config() requests.
pub const PCRE2_CONFIG_BSR: u32 = 0;
pub const PCRE2_CONFIG_JIT: u32 = 1;
pub const PCRE2_CONFIG_JITTARGET: u32 = 2;
pub const PCRE2_CONFIG_LINKSIZE: u32 = 3;
pub const PCRE2_CONFIG_MATCHLIMIT: u32 = 4;
pub const PCRE2_CONFIG_NEWLINE: u32 = 5;
pub const PCRE2_CONFIG_PARENSLIMIT: u32 = 6;
pub const PCRE2_CONFIG_DEPTHLIMIT: u32 = 7;
pub const PCRE2_CONFIG_UNICODE: u32 = 9;
pub const PCRE2_CONFIG_UNICODE_VERSION: u32 = 10;
pub const PCRE2_CONFIG_VERSION: u32 = 11;
pub const PCRE2_CONFIG_HEAPLIMIT: u32 = 12;
pub const PCRE2_CONFIG_NEVER_BACKSLASH_C: u32 = 13;

unsafe extern "C" {
    #[link_name = "twig_pcre2_config"]
    pub fn pcre2_config_8(what: u32, where_: *mut c_void) -> c_int;

    #[link_name = "twig_pcre2_compile_context_create"]
    pub fn pcre2_compile_context_create_8(
        gcontext: *mut pcre2_general_context_8,
    ) -> *mut pcre2_compile_context_8;
    #[link_name = "twig_pcre2_compile_context_free"]
    pub fn pcre2_compile_context_free_8(ccontext: *mut pcre2_compile_context_8);
    #[link_name = "twig_pcre2_set_character_tables"]
    pub fn pcre2_set_character_tables_8(
        ccontext: *mut pcre2_compile_context_8,
        tables: *const u8,
    ) -> c_int;
    #[link_name = "twig_pcre2_set_compile_extra_options"]
    pub fn pcre2_set_compile_extra_options_8(
        ccontext: *mut pcre2_compile_context_8,
        extra_options: u32,
    ) -> c_int;

    #[link_name = "twig_pcre2_compile"]
    pub fn pcre2_compile_8(
        pattern: PCRE2_SPTR8,
        length: PCRE2_SIZE,
        options: u32,
        errorcode: *mut c_int,
        erroroffset: *mut PCRE2_SIZE,
        ccontext: *mut pcre2_compile_context_8,
    ) -> *mut pcre2_code_8;
    #[link_name = "twig_pcre2_code_free"]
    pub fn pcre2_code_free_8(code: *mut pcre2_code_8);
    #[link_name = "twig_pcre2_get_error_message"]
    pub fn pcre2_get_error_message_8(
        errorcode: c_int,
        buffer: *mut PCRE2_UCHAR8,
        bufflen: PCRE2_SIZE,
    ) -> c_int;
    #[link_name = "twig_pcre2_pattern_info"]
    pub fn pcre2_pattern_info_8(code: *const pcre2_code_8, what: u32, where_: *mut c_void)
    -> c_int;
    #[link_name = "twig_pcre2_jit_compile"]
    pub fn pcre2_jit_compile_8(code: *mut pcre2_code_8, options: u32) -> c_int;

    #[link_name = "twig_pcre2_match_context_create"]
    pub fn pcre2_match_context_create_8(
        gcontext: *mut pcre2_general_context_8,
    ) -> *mut pcre2_match_context_8;
    #[link_name = "twig_pcre2_match_context_free"]
    pub fn pcre2_match_context_free_8(mcontext: *mut pcre2_match_context_8);
    #[link_name = "twig_pcre2_set_match_limit"]
    pub fn pcre2_set_match_limit_8(mcontext: *mut pcre2_match_context_8, value: u32) -> c_int;
    #[link_name = "twig_pcre2_set_depth_limit"]
    pub fn pcre2_set_depth_limit_8(mcontext: *mut pcre2_match_context_8, value: u32) -> c_int;
    #[link_name = "twig_pcre2_set_heap_limit"]
    pub fn pcre2_set_heap_limit_8(mcontext: *mut pcre2_match_context_8, value: u32) -> c_int;

    #[link_name = "twig_pcre2_jit_stack_create"]
    pub fn pcre2_jit_stack_create_8(
        startsize: PCRE2_SIZE,
        maxsize: PCRE2_SIZE,
        gcontext: *mut pcre2_general_context_8,
    ) -> *mut pcre2_jit_stack_8;
    #[link_name = "twig_pcre2_jit_stack_free"]
    pub fn pcre2_jit_stack_free_8(jit_stack: *mut pcre2_jit_stack_8);
    #[link_name = "twig_pcre2_jit_stack_assign"]
    pub fn pcre2_jit_stack_assign_8(
        mcontext: *mut pcre2_match_context_8,
        callback: pcre2_jit_callback_8,
        callback_data: *mut c_void,
    );

    #[link_name = "twig_pcre2_match_data_create"]
    pub fn pcre2_match_data_create_8(
        ovecsize: u32,
        gcontext: *mut pcre2_general_context_8,
    ) -> *mut pcre2_match_data_8;
    #[link_name = "twig_pcre2_match_data_create_from_pattern"]
    pub fn pcre2_match_data_create_from_pattern_8(
        code: *const pcre2_code_8,
        gcontext: *mut pcre2_general_context_8,
    ) -> *mut pcre2_match_data_8;
    #[link_name = "twig_pcre2_match_data_free"]
    pub fn pcre2_match_data_free_8(match_data: *mut pcre2_match_data_8);
    #[link_name = "twig_pcre2_get_ovector_pointer"]
    pub fn pcre2_get_ovector_pointer_8(match_data: *mut pcre2_match_data_8) -> *mut PCRE2_SIZE;
    #[link_name = "twig_pcre2_get_ovector_count"]
    pub fn pcre2_get_ovector_count_8(match_data: *mut pcre2_match_data_8) -> u32;
    #[link_name = "twig_pcre2_get_mark"]
    pub fn pcre2_get_mark_8(match_data: *mut pcre2_match_data_8) -> PCRE2_SPTR8;
    #[link_name = "twig_pcre2_get_startchar"]
    pub fn pcre2_get_startchar_8(match_data: *mut pcre2_match_data_8) -> PCRE2_SIZE;

    #[link_name = "twig_pcre2_match"]
    pub fn pcre2_match_8(
        code: *const pcre2_code_8,
        subject: PCRE2_SPTR8,
        length: PCRE2_SIZE,
        startoffset: PCRE2_SIZE,
        options: u32,
        match_data: *mut pcre2_match_data_8,
        mcontext: *mut pcre2_match_context_8,
    ) -> c_int;
    #[link_name = "twig_pcre2_jit_match"]
    pub fn pcre2_jit_match_8(
        code: *const pcre2_code_8,
        subject: PCRE2_SPTR8,
        length: PCRE2_SIZE,
        startoffset: PCRE2_SIZE,
        options: u32,
        match_data: *mut pcre2_match_data_8,
        mcontext: *mut pcre2_match_context_8,
    ) -> c_int;
}
