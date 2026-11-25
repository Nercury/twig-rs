use self::matchers::Matchers;
use crate::environment::LexingEnvironment;
use crate::tokens::{LexerOptions, TokenIter};
use std::collections::HashSet;

mod delimiters;
mod matchers;

pub mod iter;
pub mod options;

/// Parses template file and converts it to a stream of tokens.
pub struct Lexer {
    options: LexerOptions,
    matchers: Matchers,
}

impl Lexer {
    /// Creates a new lexer with specified options and operator list.
    pub fn new(options: LexerOptions, operators: &HashSet<&'static str>) -> Lexer {
        Lexer {
            options,
            matchers: Matchers::new(&options, operators),
        }
    }

    /// Initialize default lexer with default options.
    pub fn default(env: &LexingEnvironment) -> Lexer {
        Lexer::new(LexerOptions::default(), &env.operators)
    }

    /// Convert provided template into a token stream.
    pub fn tokens<'r, 'code>(&'r self, code: &'code str) -> TokenIter<'r, 'code> {
        TokenIter::new(self, code)
    }
}
