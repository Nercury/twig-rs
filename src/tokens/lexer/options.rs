use super::delimiters::Delimiters;

/// Lexer options.
#[derive(Copy, Clone)]
pub struct LexerOptions {
    pub tag_comment: Delimiters,
    pub tag_block: Delimiters,
    pub tag_variable: Delimiters,
    pub whitespace_trim: &'static str,
    pub interpolation: Delimiters,
}

impl LexerOptions {
    pub fn default() -> LexerOptions {
        LexerOptions {
            tag_comment: Delimiters::new("{#", "#}"),
            tag_block: Delimiters::new("{%", "%}"),
            tag_variable: Delimiters::new("{{", "}}"),
            whitespace_trim: "-",
            interpolation: Delimiters::new("#{", "}"),
        }
    }
}
