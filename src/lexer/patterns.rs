//! The lexer's patterns. Twig: the `Lexer::REGEX_*` constants and `Lexer::initialize()` (TWIG/src/Lexer.php).
//!
//! Twig's patterns are PCRE without the `u` flag, so everything is byte based and `\s` is `[\t\n\x0B\f\r ]`. The
//! patterns without PCRE-only constructs are compiled with the `regex` crate (Unicode disabled, anchored ones
//! matched against the code starting at the cursor); the ones that need look-around are matched by hand.

use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::bytes::{Regex, RegexBuilder};

use super::options::LexerOptions;
use super::php::is_pcre_space;

fn compile(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .unicode(false)
        .size_limit(1 << 30)
        .build()
        // Every pattern is built from fixed syntax and fully escaped delimiter bytes.
        .unwrap_or_else(|error| panic!("invalid lexer pattern {pattern:?}: {error}"))
}

// PHP runs PCRE2 with the JIT and a JIT stack of at most 192 KiB (`PCRE_JIT_STACK_MAX_SIZE`,
// PHPSRC/ext/pcre/php_pcre.c). A repeated group pushes a backtracking frame per iteration, so `preg_match()` returns
// false (`PREG_JIT_STACKLIMIT_ERROR`, no warning) once a lexeme repeats a group too often, and the lexer moves on as
// if the pattern did not match. The limits below are the largest iteration counts that still match, measured on
// the oracle (x86-64, PCRE2 10.44); twig-rs reproduces the resulting behaviour (for example `Unexpected character
// "'"` for a string literal with 8191 escapes).

/// `REGEX_STRING` matches at most this many escapes (`(?:\\.[^"\\]*)*` iterations).
const STRING_MAX_ESCAPES: usize = 8190;
/// `REGEX_DQ_STRING_PART` matches at most this many escapes and lone `#` (`(?:(?:\\.|#(?!\{))[^#"\\]*)*`
/// iterations).
const DQ_STRING_PART_MAX_ITERATIONS: usize = 4914;
/// `REGEX_NUMBER` matches at most this many stack units: one per `(_[0-9]+)` iteration in any of the three `LNUM`
/// calls, two for a matched fraction, two for a matched exponent and one for an exponent attempt that failed after
/// the `e`/`E`.
const NUMBER_MAX_STACK_UNITS: usize = 4911;

/// `REGEX_NAME`: `/[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*/A`.
static NAME: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^[a-zA-Z_\x7F-\xFF][a-zA-Z0-9_\x7F-\xFF]*"));

/// `REGEX_NUMBER` with its `(?(DEFINE)...)` subroutines expanded:
/// `LNUM (?:\.LNUM)? (?:[eE][+-]?LNUM)?` with `LNUM = [0-9]+(_[0-9]+)*`, anchored.
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    compile(r"^[0-9]+(?:_[0-9]+)*(?:\.[0-9]+(?:_[0-9]+)*)?(?:[eE][+\-]?[0-9]+(?:_[0-9]+)*)?")
});

/// `REGEX_STRING`: `/"([^#"\\]*(?:\\.[^#"\\]*)*)"|'([^'\\]*(?:\\.[^'\\]*)*)'/As`.
static STRING: LazyLock<Regex> =
    LazyLock::new(|| compile(r#"^(?s:"[^#"\\]*(?:\\.[^#"\\]*)*"|'[^'\\]*(?:\\.[^'\\]*)*')"#));

/// The whitespace pattern of `Lexer::lexExpression()`: `/\s+/A`.
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s+"));

/// `REGEX_RAW_INLINE_COMMENT`: `/#[^\r\n]*/A` (the public `REGEX_INLINE_COMMENT` is not used by the lexer).
static INLINE_COMMENT: LazyLock<Regex> = LazyLock::new(|| compile(r"^#[^\r\n]*"));

/// The length of the `REGEX_NAME` match at the start of `rest`.
pub(crate) fn name_len(rest: &[u8]) -> Option<usize> {
    NAME.find(rest).map(|m| m.end())
}

/// The length of the `/\s+/A` match at the start of `rest`.
pub(crate) fn whitespace_len(rest: &[u8]) -> Option<usize> {
    WHITESPACE.find(rest).map(|m| m.end())
}

/// The length of the `REGEX_RAW_INLINE_COMMENT` match at the start of `rest`.
pub(crate) fn inline_comment_len(rest: &[u8]) -> Option<usize> {
    INLINE_COMMENT.find(rest).map(|m| m.end())
}

/// The length of the `REGEX_STRING` match at the start of `rest`, `None` when PCRE does not match it (including
/// its JIT stack limit).
pub(crate) fn string_len(rest: &[u8]) -> Option<usize> {
    let end = STRING.find(rest)?.end();
    let content = &rest[1..end - 1];
    let mut escapes = 0;
    let mut i = 0;
    while i < content.len() {
        if content[i] == b'\\' {
            escapes += 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    (escapes <= STRING_MAX_ESCAPES).then_some(end)
}

/// The length of the `REGEX_NUMBER` match at the start of `rest`, `None` when PCRE does not match it (including its
/// JIT stack limit).
pub(crate) fn number_len(rest: &[u8]) -> Option<usize> {
    let end = NUMBER.find(rest)?.end();
    let number = &rest[..end];
    let has_exponent = number.iter().any(|&b| b == b'e' || b == b'E');
    let failed_exponent_attempt = !has_exponent && matches!(rest.get(end), Some(b'e' | b'E'));
    let units = number.iter().filter(|&&b| b == b'_').count()
        + 2 * usize::from(number.contains(&b'.'))
        + 2 * usize::from(has_exponent)
        + usize::from(failed_exponent_attempt);
    (units <= NUMBER_MAX_STACK_UNITS).then_some(end)
}

/// `REGEX_DQ_STRING_PART`: `/[^#"\\]*(?:(?:\\.|#(?!\{))[^#"\\]*)*/As`, matched by hand because of the look-ahead.
/// Returns the length of the (possibly empty) match at `start`, or 0 when PCRE fails on its JIT stack limit (which
/// the lexer treats like an empty match).
pub(crate) fn dq_string_part_len(code: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut iterations = 0;
    loop {
        while i < code.len() && !matches!(code[i], b'#' | b'"' | b'\\') {
            i += 1;
        }
        match code.get(i) {
            // `\\.` with the `s` flag: a backslash and any byte.
            Some(b'\\') if i + 1 < code.len() => i += 2,
            // `#(?!\{)`
            Some(b'#') if code.get(i + 1) != Some(&b'{') => i += 1,
            _ => break,
        }
        iterations += 1;
    }
    if iterations > DQ_STRING_PART_MAX_ITERATIONS {
        0
    } else {
        i - start
    }
}

/// The patterns `Lexer::initialize()` builds from the options.
#[derive(Debug, Clone)]
pub(crate) struct Patterns {
    /// `lex_var`: `\s*(?:-}}\s*|~}}[ws]*|}})`, anchored.
    pub(crate) lex_var: Regex,
    /// `lex_block`: `\s*(?:-%}\s*\n?|~%}[ws]*|%}(?:\r\n?|\n)?)`, anchored.
    pub(crate) lex_block: Regex,
    /// `lex_raw_data`: `{%(-|~)?\s*endverbatim\s*(?:-%}\s*|~%}[ws]*|%})`, searched.
    pub(crate) lex_raw_data: Regex,
    /// `lex_comment`: `(?:-#}\s*\n?|~#}[ws]*|#}(?:\r\n?|\n)?)`, searched.
    pub(crate) lex_comment: Regex,
    /// `lex_documentation_comment`: `(?:-##}\s*\n?|~##}[ws]*|-#}\s*\n?|~#}[ws]*|#}(?:\r\n?|\n)?)`, searched.
    pub(crate) lex_documentation_comment: Regex,
    /// `lex_block_raw`: `\s*verbatim\s*(?:-%}\s*|~%}[ws]*|%})`, anchored.
    pub(crate) lex_block_raw: Regex,
    /// `lex_block_line`: `\s*line\s+(\d+)\s*%}`, anchored (no extended mode).
    pub(crate) lex_block_line: Regex,
    /// `interpolation_start`: `#{\s*`, anchored (no extended mode).
    pub(crate) interpolation_start: Regex,
    /// `interpolation_end`: `\s*}`, anchored (no extended mode).
    pub(crate) interpolation_end: Regex,
    /// `lex_tokens_start`.
    pub(crate) tokens_start: TokensStart,
    /// False when both interpolation delimiters are empty (see [`LexerOptions`]).
    pub(crate) interpolation_enabled: bool,
    /// The documentation comment opener: `tag_comment[0]` followed by `#`.
    pub(crate) documentation_open: Vec<u8>,
}

/// Appends `bytes` to a `regex` pattern as literals (every byte escaped).
fn push_literal(pattern: &mut String, bytes: &[u8]) {
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            pattern.push(char::from(byte));
        } else {
            let _ = write!(pattern, "\\x{byte:02X}");
        }
    }
}

/// What PCRE's extended mode (`x` flag) ignores in a pattern: the `isspace()` characters of the default tables
/// plus NEL (`0x85`), which PCRE2 built with Unicode support also skips in non-UTF patterns
/// (PHPSRC/ext/pcre/pcre2lib/pcre2_compile.c). `preg_quote()` does not escape them, so Twig's delimiters lose them in
/// the `x`-flagged patterns.
fn strip_extended_whitespace(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .copied()
        .filter(|&b| !is_pcre_space(b) && b != 0x85)
        .collect()
}

/// `preg_quote($bytes, '#')` inserted into an `x`-flagged pattern.
fn quote_x(bytes: &[u8]) -> String {
    let mut pattern = String::new();
    push_literal(&mut pattern, &strip_extended_whitespace(bytes));
    pattern
}

/// `preg_quote($bytes, '#')` inserted into a pattern without the `x` flag.
fn quote(bytes: &[u8]) -> String {
    let mut pattern = String::new();
    push_literal(&mut pattern, bytes);
    pattern
}

/// `[<whitespace_line_chars>]*`, or nothing for an empty set (a class that matches nothing, repeated).
fn line_chars_repeat(chars: &[u8]) -> String {
    if chars.is_empty() {
        return String::new();
    }
    let mut pattern = String::from("[");
    for &byte in chars {
        let _ = write!(pattern, "\\x{byte:02X}");
    }
    pattern.push_str("]*");
    pattern
}

impl Patterns {
    /// Port of `Lexer::initialize()`.
    pub(crate) fn new(options: &LexerOptions) -> Self {
        let trim = &options.whitespace_trim;
        let line_trim = &options.whitespace_line_trim;
        let line_chars = line_chars_repeat(&options.whitespace_line_chars);
        let concat = |a: &[u8], b: &[u8]| [a, b].concat();

        let [_, var_close] = &options.tag_variable;
        let [block_open, block_close] = &options.tag_block;
        let [_, comment_close] = &options.tag_comment;
        let [interpolation_open, interpolation_close] = &options.interpolation;

        let lex_var = format!(
            r"^\s*(?:{}\s*|{}{line_chars}|{})",
            quote_x(&concat(trim, var_close)),
            quote_x(&concat(line_trim, var_close)),
            quote_x(var_close),
        );
        let lex_block = format!(
            r"^\s*(?:{}\s*\n?|{}{line_chars}|{}(?:\r\n?|\n)?)",
            quote_x(&concat(trim, block_close)),
            quote_x(&concat(line_trim, block_close)),
            quote_x(block_close),
        );
        let lex_raw_data = format!(
            r"{}({}|{})?\s*endverbatim\s*(?:{}\s*|{}{line_chars}|{})",
            quote_x(block_open),
            quote_x(trim),
            quote_x(line_trim),
            quote_x(&concat(trim, block_close)),
            quote_x(&concat(line_trim, block_close)),
            quote_x(block_close),
        );
        let lex_comment = format!(
            r"(?:{}\s*\n?|{}{line_chars}|{}(?:\r\n?|\n)?)",
            quote_x(&concat(trim, comment_close)),
            quote_x(&concat(line_trim, comment_close)),
            quote_x(comment_close),
        );
        let lex_documentation_comment = format!(
            r"(?:{}\s*\n?|{}{line_chars}|{}\s*\n?|{}{line_chars}|{}(?:\r\n?|\n)?)",
            quote_x(&[trim.as_slice(), b"#", comment_close].concat()),
            quote_x(&[line_trim.as_slice(), b"#", comment_close].concat()),
            quote_x(&concat(trim, comment_close)),
            quote_x(&concat(line_trim, comment_close)),
            quote_x(comment_close),
        );
        let lex_block_raw = format!(
            r"^\s*verbatim\s*(?:{}\s*|{}{line_chars}|{})",
            quote_x(&concat(trim, block_close)),
            quote_x(&concat(line_trim, block_close)),
            quote_x(block_close),
        );
        let lex_block_line = format!(r"^\s*line\s+([0-9]+)\s*{}", quote(block_close));
        let interpolation_start = format!(r"^{}\s*", quote(interpolation_open));
        let interpolation_end = format!(r"^\s*{}", quote(interpolation_close));

        Patterns {
            lex_var: compile(&lex_var),
            lex_block: compile(&lex_block),
            lex_raw_data: compile(&lex_raw_data),
            lex_comment: compile(&lex_comment),
            lex_documentation_comment: compile(&lex_documentation_comment),
            lex_block_raw: compile(&lex_block_raw),
            lex_block_line: compile(&lex_block_line),
            interpolation_start: compile(&interpolation_start),
            interpolation_end: compile(&interpolation_end),
            tokens_start: TokensStart::new(options),
            interpolation_enabled: !(interpolation_open.is_empty()
                && interpolation_close.is_empty()),
            documentation_open: [options.tag_comment[0].as_slice(), b"#"].concat(),
        }
    }
}

/// One match of `lex_tokens_start`: the whole match is `start..end`, group 1 (the opening delimiter) is
/// `start..delimiter_end` and group 2 (the trim modifier, empty when it did not participate, as in
/// `preg_match_all()`'s output) is `delimiter_end..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TokenStart {
    pub(crate) start: usize,
    pub(crate) delimiter_end: usize,
    pub(crate) end: usize,
}

/// `lex_tokens_start`: `({{|{%|{##(?!})|{#)(-|~)?` with the `x` flag, matched by hand because of the look-ahead
/// that keeps the empty comment `{##}` a regular comment.
#[derive(Debug, Clone)]
pub(crate) struct TokensStart {
    /// The group 1 alternatives in pattern order: variable, block, documentation comment, comment openers.
    alternatives: [Vec<u8>; 4],
    /// The negative look-ahead after the documentation comment opener, when the comment closer starts with `#`.
    documentation_lookahead: Option<Vec<u8>>,
    /// The group 2 alternatives in pattern order: `whitespace_trim`, `whitespace_line_trim`.
    trims: [Vec<u8>; 2],
    /// Bytes a match can start with, used to skip ahead when no alternative is empty.
    first_bytes: [bool; 256],
    /// Whether an alternative is empty, so that a match can start anywhere.
    may_match_empty: bool,
}

impl TokensStart {
    fn new(options: &LexerOptions) -> Self {
        let comment_close = &options.tag_comment[1];
        let alternatives = [
            strip_extended_whitespace(&options.tag_variable[0]),
            strip_extended_whitespace(&options.tag_block[0]),
            strip_extended_whitespace(&[options.tag_comment[0].as_slice(), b"#"].concat()),
            strip_extended_whitespace(&options.tag_comment[0]),
        ];
        let mut first_bytes = [false; 256];
        for alternative in &alternatives {
            if let Some(&byte) = alternative.first() {
                first_bytes[usize::from(byte)] = true;
            }
        }
        TokensStart {
            may_match_empty: alternatives.iter().any(Vec::is_empty),
            alternatives,
            documentation_lookahead: comment_close
                .strip_prefix(b"#")
                .map(strip_extended_whitespace),
            trims: [
                strip_extended_whitespace(&options.whitespace_trim),
                strip_extended_whitespace(&options.whitespace_line_trim),
            ],
            first_bytes,
        }
    }

    /// The leftmost match starting at or after `from`.
    fn search(&self, code: &[u8], from: usize) -> Option<TokenStart> {
        if self.may_match_empty {
            return (from..=code.len()).find_map(|start| self.match_at(code, start, false));
        }
        let mut from = from;
        while let Some(skip) = code
            .get(from..)?
            .iter()
            .position(|&b| self.first_bytes[usize::from(b)])
        {
            let start = from + skip;
            if let Some(found) = self.match_at(code, start, false) {
                return Some(found);
            }
            from = start + 1;
        }
        None
    }

    /// The anchored match at `start`, as PCRE's backtracking finds it: group 1 alternatives in order, then group 2
    /// (greedy `?`: its alternatives in order, then nothing). With `not_empty`, empty matches are rejected
    /// (`PCRE2_NOTEMPTY_ATSTART`).
    fn match_at(&self, code: &[u8], start: usize, not_empty: bool) -> Option<TokenStart> {
        let rest = &code[start..];
        for (index, alternative) in self.alternatives.iter().enumerate() {
            if !rest.starts_with(alternative) {
                continue;
            }
            let delimiter_end = start + alternative.len();
            if index == 2
                && let Some(lookahead) = &self.documentation_lookahead
                && code[delimiter_end..].starts_with(lookahead)
            {
                continue;
            }
            let after = &code[delimiter_end..];
            let trims = self
                .trims
                .iter()
                .filter(|trim| after.starts_with(trim))
                .map(Vec::len);
            for end in trims
                .chain(std::iter::once(0))
                .map(|len| delimiter_end + len)
            {
                if !(not_empty && end == start) {
                    return Some(TokenStart {
                        start,
                        delimiter_end,
                        end,
                    });
                }
            }
        }
        None
    }

    /// Every match, like `preg_match_all()`: non-overlapping, left to right, with Perl's handling of empty
    /// matches (retry at the same offset rejecting empty matches, else advance one byte).
    pub(crate) fn find_all(&self, code: &[u8]) -> Vec<TokenStart> {
        let mut matches = Vec::new();
        let mut offset = 0;
        while let Some(found) = self.search(code, offset) {
            matches.push(found);
            if found.end > found.start {
                offset = found.end;
                continue;
            }
            match self.match_at(code, found.end, true) {
                Some(retry) => {
                    matches.push(retry);
                    offset = retry.end;
                }
                None if found.end < code.len() => offset = found.end + 1,
                None => break,
            }
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_double_quoted_string_parts() {
        assert_eq!(dq_string_part_len(b"abc\"", 0), 3);
        assert_eq!(dq_string_part_len(b"a # b\"", 0), 5);
        assert_eq!(dq_string_part_len(b"a #{b}\"", 0), 2);
        assert_eq!(dq_string_part_len(b"a\\\"b\"", 0), 4);
        assert_eq!(dq_string_part_len(b"a\\", 0), 1);
        assert_eq!(dq_string_part_len(b"\"", 0), 0);
    }

    #[test]
    fn finds_token_starts_like_preg_match_all() {
        let starts = TokensStart::new(&LexerOptions::default());
        let found = starts.find_all(b"a{{- b }}{%~ c %}{## d #}{##}{# e #}");
        let groups: Vec<(usize, usize, usize)> = found
            .iter()
            .map(|m| (m.start, m.delimiter_end, m.end))
            .collect();
        assert_eq!(
            groups,
            [
                (1, 3, 4),
                (9, 11, 12),
                (17, 20, 20),
                (25, 27, 27),
                (29, 31, 31)
            ]
        );
    }

    #[test]
    fn handles_empty_matches_like_perl() {
        let options = LexerOptions {
            tag_variable: [Vec::new(), b"}}".to_vec()],
            ..LexerOptions::default()
        };
        let starts = TokensStart::new(&options);
        let found = starts.find_all(b"a{%");
        let groups: Vec<(usize, usize, usize)> = found
            .iter()
            .map(|m| (m.start, m.delimiter_end, m.end))
            .collect();
        // Empty match at 0, retry fails, empty match at 1, retry finds "{%", then an empty match at the end.
        assert_eq!(groups, [(0, 0, 0), (1, 1, 1), (1, 3, 3), (3, 3, 3)]);
    }
}
