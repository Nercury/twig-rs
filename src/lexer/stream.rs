//! Token streams. Twig: `Twig\TokenStream` (TWIG/src/TokenStream.php).

use std::fmt::{self, Write as _};

use super::php;
use super::token::{Token, TokenType, TokenValue, TokenValues};
use crate::error::Error;
use crate::source::Source;

/// A stream of tokens with a cursor, as produced by [`Lexer::tokenize`](super::Lexer::tokenize).
///
/// Twig: `Twig\TokenStream` (TWIG/src/TokenStream.php). A stream produced by the lexer always ends with an
/// `EOF_TYPE` token.
///
/// Like Twig, a failed [`next`](Self::next) leaves the position past the last token. [`try_current`](Self::try_current)
/// then returns `None` (Twig's `getCurrent()` fails with a `TypeError` in that state, which `Parser::parse()` can run
/// into); the other methods use the last token.
#[derive(Debug, Clone)]
pub struct TokenStream {
    tokens: Vec<Token>,
    current: usize,
    source: Source,
}

impl TokenStream {
    /// A stream over `tokens`. Twig: `new TokenStream($tokens, $source)`.
    ///
    /// An empty token list gets an `EOF_TYPE` token at line 1 (Twig would fail on first access instead).
    pub fn new(mut tokens: Vec<Token>, source: Source) -> Self {
        if tokens.is_empty() {
            tokens.push(Token::new(TokenType::Eof, TokenValue::empty(), 1));
        }
        TokenStream {
            tokens,
            current: 0,
            source,
        }
    }

    /// Inserts tokens at the current position. Twig: `TokenStream::injectTokens()`.
    pub fn inject_tokens(&mut self, tokens: impl IntoIterator<Item = Token>) {
        let at = self.current.min(self.tokens.len());
        self.tokens.splice(at..at, tokens);
    }

    /// Moves to the next token and returns the previous current one.
    ///
    /// Twig: `TokenStream::next()`.
    ///
    /// # Errors
    ///
    /// `Unexpected end of template.` (at the line of the current token) when there is no next token; the position is
    /// then past the last token, as in Twig.
    #[allow(
        clippy::should_implement_trait,
        reason = "mirrors Twig's fallible TokenStream::next()"
    )]
    pub fn next(&mut self) -> Result<Token, Error> {
        let previous = self.current().clone();
        self.current += 1;
        if self.current >= self.tokens.len() {
            return Err(self.end_of_template(previous.line()));
        }
        Ok(previous)
    }

    /// Moves to the next token if the current one has type `ty`, returning it. Twig: `TokenStream::nextIf($type)`.
    ///
    /// # Errors
    ///
    /// The error of [`next`](Self::next) when the current token matches but is the last one.
    pub fn next_if(&mut self, ty: TokenType) -> Result<Option<Token>, Error> {
        if self.test(ty) {
            self.next().map(Some)
        } else {
            Ok(None)
        }
    }

    /// Moves to the next token if the current one has type `ty` and one of `values`, returning it.
    /// Twig: `TokenStream::nextIf($type, $values)`.
    ///
    /// # Errors
    ///
    /// The error of [`next`](Self::next) when the current token matches but is the last one.
    pub fn next_if_value<'a>(
        &mut self,
        ty: TokenType,
        values: impl Into<TokenValues<'a>>,
    ) -> Result<Option<Token>, Error> {
        if self.test_value(ty, values) {
            self.next().map(Some)
        } else {
            Ok(None)
        }
    }

    /// Returns the current token and moves to the next one if the current token has type `ty` (and value `value`,
    /// when given).
    ///
    /// Twig: `TokenStream::expect()`.
    ///
    /// # Errors
    ///
    /// When the current token does not match: `[<message>. ]Unexpected token "<type>"[ of value "<value>"] ("<type>"
    /// expected[ with value "<value>"]).` at the token's line and column (no column for a token without an offset);
    /// the value parts are left out when the value is falsy in PHP (`''`, `'0'`, `0`, `0.0`). When it matches but is
    /// the last token, the error of [`next`](Self::next).
    pub fn expect(
        &mut self,
        ty: TokenType,
        value: Option<&str>,
        message: Option<&str>,
    ) -> Result<Token, Error> {
        let token = self.current();
        let matches = match value {
            Some(value) => token.test_value(ty, value),
            None => token.test(ty),
        };
        if !matches {
            let mut text = String::new();
            if let Some(message) = message.filter(|m| php::is_truthy_str(m.as_bytes())) {
                text.push_str(message);
                text.push_str(". ");
            }
            text.push_str("Unexpected token \"");
            text.push_str(token.to_english());
            text.push('"');
            if token.value().is_truthy() {
                let _ = write!(text, " of value \"{}\"", token.value());
            }
            text.push_str(" (\"");
            text.push_str(ty.english());
            text.push_str("\" expected");
            if let Some(value) = value.filter(|v| php::is_truthy_str(v.as_bytes())) {
                let _ = write!(text, " with value \"{value}\"");
            }
            text.push_str(").");
            let column = token.offset().map(|offset| self.source.column(offset));
            return Err(Error::syntax(text)
                .with_line(token.line())
                .with_source(self.source.clone())
                .with_column(column));
        }
        let token = token.clone();
        self.next()?;
        Ok(token)
    }

    /// Looks at the token `number` positions ahead (Twig's default is 1).
    ///
    /// Twig: `TokenStream::look()`.
    ///
    /// # Errors
    ///
    /// `Unexpected end of template.` at the line of the token before that position when there is no such token. When
    /// even the token before is missing, Twig fails with a PHP `Error` (a method call on `null`); twig-rs then reports
    /// the line of the last token.
    pub fn look(&self, number: usize) -> Result<&Token, Error> {
        let position = self.current.saturating_add(number);
        match self.tokens.get(position) {
            Some(token) => Ok(token),
            None => {
                let index = position.saturating_sub(1).min(self.tokens.len() - 1);
                Err(self.end_of_template(self.tokens[index].line()))
            }
        }
    }

    /// Tests the type of the current token. Twig: `TokenStream::test($type)`.
    pub fn test(&self, ty: TokenType) -> bool {
        self.current().test(ty)
    }

    /// Tests the type and value of the current token. Twig: `TokenStream::test($type, $values)`.
    pub fn test_value<'a>(&self, ty: TokenType, values: impl Into<TokenValues<'a>>) -> bool {
        self.current().test_value(ty, values)
    }

    /// Whether the current token is the end of the template. Twig: `TokenStream::isEOF()`.
    pub fn is_eof(&self) -> bool {
        self.current().test(TokenType::Eof)
    }

    /// The current token, or the last token when a failed [`next`](Self::next) moved past it.
    /// Twig: `TokenStream::getCurrent()`.
    pub fn current(&self) -> &Token {
        self.try_current()
            .unwrap_or_else(|| &self.tokens[self.tokens.len() - 1])
    }

    /// The current token, `None` when a failed [`next`](Self::next) moved past the last one, where Twig's
    /// `getCurrent()` fails with `TypeError: Twig\TokenStream::getCurrent(): Return value must be of type
    /// Twig\Token, null returned` (verified on the oracle with `{% line 0 %}{% if true %}x`).
    pub fn try_current(&self) -> Option<&Token> {
        self.tokens.get(self.current)
    }

    /// Twig: `TokenStream::getSourceContext()`.
    pub fn source_context(&self) -> &Source {
        &self.source
    }

    /// All tokens of the stream, regardless of the position.
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Consumes the stream, returning all its tokens.
    pub fn into_tokens(self) -> Vec<Token> {
        self.tokens
    }

    fn end_of_template(&self, line: i64) -> Error {
        Error::syntax("Unexpected end of template.")
            .with_line(line)
            .with_source(self.source.clone())
    }
}

/// One token per line, as `TYPE_NAME(value)`. Twig: `TokenStream::__toString()`.
impl fmt::Display for TokenStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, token) in self.tokens.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{token}")?;
        }
        Ok(())
    }
}
