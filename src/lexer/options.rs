use lexer::delimiters::Delimiters;

/// Twig options.
#[derive(Copy, Clone)]
pub struct Options {
    pub tag_comment: Delimiters,
    pub tag_block: Delimiters,
    pub tag_variable: Delimiters,
    pub whitespace_trim: &'static str,
    pub interpolation: Delimiters,
}

impl Options {
    pub fn default() -> Options {
        Options {
            tag_comment: Delimiters::new("{#", "#}"),
            tag_block: Delimiters::new("{%", "%}"),
            tag_variable: Delimiters::new("{{", "}}"),
            whitespace_trim: "-",
            interpolation: Delimiters::new("#{", "}"),
        }
    }
}
