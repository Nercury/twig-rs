//! Builds the vendored PCRE2 10.44 (`pcre2/`, a verbatim copy of `PHPSRC/ext/pcre/pcre2lib`) the way
//! PHP 8.5.13 builds its bundled copy.
//!
//! What PHP's build contributes and how it is reproduced here:
//!
//! - `PHPSRC/ext/pcre/config0.m4` / `config.w32`: the same 28 translation units, `-DHAVE_CONFIG_H`,
//!   `-DHAVE_MEMMOVE`, `PCRE2_CODE_UNIT_WIDTH=8`, `PCRE2_STATIC`, `HAVE_BUNDLED_PCRE` and, with the default
//!   `--with-pcre-jit`, `HAVE_PCRE_JIT_SUPPORT` (which makes `pcre2lib/config.h` define `SUPPORT_JIT`).
//! - `pcre2lib/config.h` is used unchanged. It includes `<php_compat.h>` and `<php_config.h>`, which in a
//!   PHP build come from PHP's `main/` directory; this script generates small stand-ins for both in
//!   `OUT_DIR/include`. Like `PHPSRC/main/php_compat.h`, the stand-in renames the library's external
//!   symbols to a private prefix (`twig_pcre2_*` for the API, `_twig_pcre2_*` for library internals), so
//!   another PCRE2 copy linked into the same binary cannot clash with this one. Symbol names have no
//!   effect on behaviour.
//! - `config.h` itself carries PHP's limits: Unicode on, `NEWLINE_DEFAULT 2` (LF), `LINK_SIZE 2`,
//!   `PARENS_NEST_LIMIT 250`, `MATCH_LIMIT 10000000`, `MATCH_LIMIT_DEPTH MATCH_LIMIT`,
//!   `HEAP_LIMIT 20000000`, `MAX_NAME_SIZE 128`, `MAX_NAME_COUNT 10000`, `MAX_VARLOOKBEHIND 255`.
//!   (PHP sets the run-time match and depth limits on its match context; see `src/preg.rs`.)
//! - The JIT (sljit, included by `pcre2_jit_compile.c`) is compiled when the `jit` feature is on (the
//!   default) and sljit supports the target architecture.
//! - PHP's `php_config.h` defines none of PCRE2's own feature macros (`HAVE_BUILTIN_MUL_OVERFLOW`,
//!   `HAVE_ATTRIBUTE_UNINITIALIZED`, ...), so they stay undefined here too; its system feature macros
//!   (`_GNU_SOURCE`, ...) only change which libc declarations are visible and are not reproduced.
//! - PHP builds with `-O2`; the C code is always optimized the same way here, whatever the Cargo profile.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

/// `pcrelib_sources` of `PHPSRC/ext/pcre/config0.m4` (the same list as `ADD_SOURCES` in `config.w32`).
const SOURCES: [&str; 28] = [
    "pcre2_auto_possess.c",
    "pcre2_chartables.c",
    "pcre2_chkdint.c",
    "pcre2_compile.c",
    "pcre2_config.c",
    "pcre2_context.c",
    "pcre2_convert.c",
    "pcre2_dfa_match.c",
    "pcre2_error.c",
    "pcre2_extuni.c",
    "pcre2_find_bracket.c",
    "pcre2_jit_compile.c",
    "pcre2_maketables.c",
    "pcre2_match_data.c",
    "pcre2_match.c",
    "pcre2_newline.c",
    "pcre2_ord2utf.c",
    "pcre2_pattern_info.c",
    "pcre2_script_run.c",
    "pcre2_serialize.c",
    "pcre2_string_utils.c",
    "pcre2_study.c",
    "pcre2_substitute.c",
    "pcre2_substring.c",
    "pcre2_tables.c",
    "pcre2_ucd.c",
    "pcre2_valid_utf.c",
    "pcre2_xclass.c",
];

/// Every external function of the PCRE2 10.44 API (the `PCRE2_SUFFIX` function names of `pcre2.h`).
/// `PHPSRC/main/php_compat.h` renames the same functions (most of them) to `php_pcre2_*`.
const API_FUNCTIONS: [&str; 76] = [
    "callout_enumerate",
    "code_copy",
    "code_copy_with_tables",
    "code_free",
    "compile",
    "compile_context_copy",
    "compile_context_create",
    "compile_context_free",
    "config",
    "convert_context_copy",
    "convert_context_create",
    "convert_context_free",
    "converted_pattern_free",
    "dfa_match",
    "general_context_copy",
    "general_context_create",
    "general_context_free",
    "get_error_message",
    "get_mark",
    "get_match_data_heapframes_size",
    "get_match_data_size",
    "get_ovector_pointer",
    "get_ovector_count",
    "get_startchar",
    "jit_compile",
    "jit_match",
    "jit_free_unused_memory",
    "jit_stack_assign",
    "jit_stack_create",
    "jit_stack_free",
    "maketables",
    "maketables_free",
    "match",
    "match_context_copy",
    "match_context_create",
    "match_context_free",
    "match_data_create",
    "match_data_create_from_pattern",
    "match_data_free",
    "pattern_convert",
    "pattern_info",
    "serialize_decode",
    "serialize_encode",
    "serialize_free",
    "serialize_get_number_of_codes",
    "set_bsr",
    "set_callout",
    "set_character_tables",
    "set_compile_extra_options",
    "set_compile_recursion_guard",
    "set_depth_limit",
    "set_glob_escape",
    "set_glob_separator",
    "set_heap_limit",
    "set_match_limit",
    "set_max_varlookbehind",
    "set_max_pattern_length",
    "set_max_pattern_compiled_length",
    "set_newline",
    "set_parens_nest_limit",
    "set_offset_limit",
    "set_substitute_callout",
    "substitute",
    "substring_copy_byname",
    "substring_copy_bynumber",
    "substring_free",
    "substring_get_byname",
    "substring_get_bynumber",
    "substring_length_byname",
    "substring_length_bynumber",
    "substring_list_get",
    "substring_list_free",
    "substring_nametable_scan",
    "substring_number_from_name",
    "set_recursion_limit",
    "set_recursion_memory_management",
];

/// Architectures sljit's `SLJIT_CONFIG_AUTO` detection supports (`pcre2/sljit/sljitConfigCPU.h`).
const JIT_ARCHS: [&str; 12] = [
    "x86",
    "x86_64",
    "arm",
    "aarch64",
    "powerpc",
    "powerpc64",
    "mips",
    "mips64",
    "riscv32",
    "riscv64",
    "loongarch64",
    "s390x",
];

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let pcre2_dir = manifest_dir.join("pcre2");
    let include_dir = out_dir.join("include");
    fs::create_dir_all(&include_dir).expect("create OUT_DIR/include");

    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let jit = env::var_os("CARGO_FEATURE_JIT").is_some() && JIT_ARCHS.contains(&arch.as_str());

    let internal_header =
        fs::read_to_string(pcre2_dir.join("pcre2_internal.h")).expect("read pcre2_internal.h");
    fs::write(include_dir.join("php_config.h"), php_config_h(jit)).expect("write php_config.h");
    fs::write(
        include_dir.join("php_compat.h"),
        php_compat_h(&internal_header),
    )
    .expect("write php_compat.h");

    let mut build = cc::Build::new();
    build
        .include(&include_dir)
        .include(&pcre2_dir)
        .define("HAVE_CONFIG_H", None)
        .define("HAVE_MEMMOVE", None)
        .opt_level(2)
        .warnings(false)
        .extra_warnings(false)
        .flag_if_supported("-Wno-implicit-fallthrough");
    for source in SOURCES {
        build.file(pcre2_dir.join(source));
    }
    build.compile("twig_pcre2");

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=pcre2");
}

/// Stand-in for the PHP build's `php_config.h` (`config.w32.h` on Windows): only the settings that
/// `pcre2lib` reads.
fn php_config_h(jit: bool) -> String {
    let mut out = String::from(
        "/* Generated by twig-pcre's build.rs: the settings of PHP 8.5.13's php_config.h / config.w32.h that\n   \
         ext/pcre/pcre2lib reads (PHPSRC/ext/pcre/config0.m4, config.w32). */\n\
         #ifndef TWIG_PCRE_PHP_CONFIG_H\n\
         #define TWIG_PCRE_PHP_CONFIG_H\n\
         #define HAVE_BUNDLED_PCRE 1\n\
         #define PCRE2_CODE_UNIT_WIDTH 8\n\
         #define PCRE2_STATIC 1\n",
    );
    if jit {
        out.push_str("#define HAVE_PCRE_JIT_SUPPORT 1\n");
    }
    out.push_str("#endif\n");
    out
}

/// Stand-in for `PHPSRC/main/php_compat.h`: renames the bundled library's external symbols.
///
/// The API functions are renamed exactly the way `php_compat.h` does it (`pcre2_compile_8` is the name
/// `pcre2.h` derives for `pcre2_compile`). The library-internal functions and tables get the same
/// treatment: `pcre2_internal.h` maps each internal name `_pcre2_xxx` to `PCRE2_SUFFIX(_pcre2_xxx_)`,
/// i.e. `_pcre2_xxx_8`, and that expansion is renamed to `_twig_pcre2_xxx`.
fn php_compat_h(internal_header: &str) -> String {
    let mut out = String::from(
        "/* Generated by twig-pcre's build.rs: like PHPSRC/main/php_compat.h, gives the bundled PCRE2's external\n   \
         symbols a private prefix so that no other PCRE2 copy in the same binary can clash with them. */\n\
         #ifndef TWIG_PCRE_PHP_COMPAT_H\n\
         #define TWIG_PCRE_PHP_COMPAT_H\n\
         #include <php_config.h>\n",
    );
    for name in API_FUNCTIONS {
        writeln!(out, "#define pcre2_{name}_8 twig_pcre2_{name}").expect("write to String");
    }
    // Internal names with a width suffix: `#define _pcre2_xxx PCRE2_SUFFIX(_pcre2_xxx_)`.
    let mut suffixed = Vec::new();
    for line in internal_header.lines() {
        let Some(rest) = line.strip_prefix("#define _pcre2_") else {
            continue;
        };
        let Some((_, arg)) = rest.split_once("PCRE2_SUFFIX(") else {
            continue;
        };
        let Some(arg) = arg.strip_suffix(')') else {
            continue;
        };
        let base = arg.strip_suffix('_').unwrap_or(arg);
        writeln!(out, "#define {arg}8 _twig{base}").expect("write to String");
        suffixed.push(base.to_owned());
    }
    assert!(
        suffixed.len() >= 40,
        "pcre2_internal.h: expected the PCRE2_SUFFIX name list, found {}",
        suffixed.len()
    );
    // Internal names without a width suffix (shared UTF-8 tables, ckd_smul): `extern ... PRIV(xxx)...`,
    // where `PRIV(xxx)` expands to `_pcre2_xxx`.
    let mut plain = Vec::new();
    for line in internal_header.lines().filter(|l| l.starts_with("extern")) {
        let Some((_, rest)) = line.split_once("PRIV(") else {
            continue;
        };
        let Some((name, _)) = rest.split_once(')') else {
            continue;
        };
        let full = format!("_pcre2_{name}");
        if !suffixed.contains(&full) && !plain.contains(&full) {
            writeln!(out, "#define {full} _twig{full}").expect("write to String");
            plain.push(full);
        }
    }
    assert!(
        !plain.is_empty(),
        "pcre2_internal.h: expected unsuffixed PRIV() declarations"
    );
    out.push_str(
        "#endif
",
    );
    out
}
