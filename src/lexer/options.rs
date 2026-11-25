//! Lexer options. Twig: the `$options` argument of `Lexer::__construct()` (TWIG/src/Lexer.php).

/// The delimiters and whitespace-control characters of the template syntax.
///
/// Twig: the `$options` array of `Lexer::__construct()`; [`Default`] gives Twig's defaults. Values are byte strings
/// inserted literally (quoted) into the lexer's patterns, as Twig does with `preg_quote()`, including the quirks
/// that follow from Twig's patterns: whitespace (and the byte `0x85`) inside a delimiter is ignored where Twig's
/// pattern uses PCRE's extended mode, and is significant elsewhere (`{% line %}` and interpolation delimiters).
///
/// Differences from Twig, all limited to option values Twig cannot handle sensibly:
/// - `whitespace_line_chars` is the literal set of characters (default `" \t\0\x0B"` as bytes), not the body of
///   a PCRE character class (Twig's default is the 9-character PHP string `' \t\0\x0B'`, whose escapes PCRE
///   interprets). Bodies without escapes or class syntax behave the same; an empty set matches nothing, where Twig's
///   empty class `[]` is a PCRE syntax error that disables the closing delimiters.
/// - `whitespace_trim` and `whitespace_line_trim` are always treated literally; Twig inserts them unquoted into the
///   `endverbatim` pattern, so values containing PCRE metacharacters break Twig's `{% verbatim %}` handling.
/// - When both interpolation delimiters are empty, interpolation is disabled (Twig loops forever on any
///   double-quoted string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexerOptions {
    /// `tag_comment`, default `['{#', '#}']`.
    pub tag_comment: [Vec<u8>; 2],
    /// `tag_block`, default `['{%', '%}']`.
    pub tag_block: [Vec<u8>; 2],
    /// `tag_variable`, default `['{{', '}}']`.
    pub tag_variable: [Vec<u8>; 2],
    /// `whitespace_trim`, default `'-'`.
    pub whitespace_trim: Vec<u8>,
    /// `whitespace_line_trim`, default `'~'`.
    pub whitespace_line_trim: Vec<u8>,
    /// `whitespace_line_chars`, default `" \t\0\x0B"` (space, tab, NUL, vertical tab).
    pub whitespace_line_chars: Vec<u8>,
    /// `interpolation`, default `['#{', '}']`.
    pub interpolation: [Vec<u8>; 2],
}

impl Default for LexerOptions {
    fn default() -> Self {
        LexerOptions {
            tag_comment: [b"{#".to_vec(), b"#}".to_vec()],
            tag_block: [b"{%".to_vec(), b"%}".to_vec()],
            tag_variable: [b"{{".to_vec(), b"}}".to_vec()],
            whitespace_trim: b"-".to_vec(),
            whitespace_line_trim: b"~".to_vec(),
            whitespace_line_chars: b" \t\0\x0B".to_vec(),
            interpolation: [b"#{".to_vec(), b"}".to_vec()],
        }
    }
}
