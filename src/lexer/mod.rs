//! The Twig 3.31.1 lexer: TWIG/src/Lexer.php, TWIG/src/Token.php and TWIG/src/TokenStream.php.
//!
//! Templates are byte strings and the lexer works on bytes like Twig's PCRE patterns (no `u` flag): names accept
//! every byte `>= 0x7F`, columns count bytes and `\s` is ASCII whitespace. Token offsets are byte offsets into the
//! original source; text, string literals, verbatim blocks and documentation have `"\r\n"`/`"\r"` normalised to
//! `"\n"`.
//!
//! ```
//! use twig::deprecation::IgnoreDeprecations;
//! use twig::lexer::{Lexer, TokenType};
//! use twig::Source;
//!
//! let stream = Lexer::default().tokenize(&Source::new("Hello {{ name }}!", "index"), &mut IgnoreDeprecations)?;
//! let types: Vec<TokenType> = stream.tokens().iter().map(|token| token.ty()).collect();
//! assert_eq!(
//!     types,
//!     [TokenType::Text, TokenType::VarStart, TokenType::Name, TokenType::VarEnd, TokenType::Text, TokenType::Eof]
//! );
//! # Ok::<(), twig::Error>(())
//! ```

mod escape;
mod operators;
mod options;
mod patterns;
mod php;
mod stream;
mod token;

pub use operators::DEFAULT_OPERATORS;
pub use options::LexerOptions;
pub use stream::TokenStream;
pub use token::{Token, TokenType, TokenValue, TokenValues};

use std::fmt;

use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::Error;
use crate::source::Source;
use operators::OperatorMatcher;
use patterns::{Patterns, TokenStart};

/// Turns template source code into a [`TokenStream`].
///
/// Twig: `Twig\Lexer` (TWIG/src/Lexer.php). The delimiter patterns and the operator pattern are built once, when
/// the lexer is created; Twig builds them on the first `tokenize()` call.
#[derive(Clone)]
pub struct Lexer {
    options: LexerOptions,
    patterns: Patterns,
    operators: OperatorMatcher,
}

impl Lexer {
    /// A lexer for the given options and operator tokens.
    ///
    /// `operators` are the operator tokens of the expression parsers in registry order, as Twig's
    /// `Lexer::getOperatorRegex()` collects them from `Environment::getExpressionParsers()`; see
    /// [`DEFAULT_OPERATORS`] for the default environment's list.
    ///
    /// Twig: `new Lexer($env, $options)`.
    ///
    /// # Panics
    ///
    /// If a delimiter is so long (hundreds of megabytes) that a compiled pattern exceeds the `regex` crate's size
    /// limit (raised to 1 GiB).
    pub fn new<I>(options: LexerOptions, operators: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<[u8]>,
    {
        Lexer {
            patterns: Patterns::new(&options),
            operators: OperatorMatcher::new(operators),
            options,
        }
    }

    /// The options the lexer was created with.
    pub fn options(&self) -> &LexerOptions {
        &self.options
    }

    /// Tokenizes a template. Deprecations raised on the way (Twig 3.12's escaped-character deprecation) are
    /// reported to `deprecations`.
    ///
    /// Twig: `Lexer::tokenize()`.
    ///
    /// # Errors
    ///
    /// A syntax error ([`ErrorKind::Syntax`](crate::ErrorKind::Syntax)) with Twig's message, line, source and, for
    /// unexpected characters and closing brackets, column: `Unclosed "%s".`, `Unexpected character "%s".`,
    /// `Unexpected "%s".`, `Unclosed comment.` or `Unexpected end of file: Unclosed "verbatim" block.`.
    pub fn tokenize(
        &self,
        source: &Source,
        deprecations: &mut dyn DeprecationSink,
    ) -> Result<TokenStream, Error> {
        let code = source.code();
        let mut lexing = Lexing {
            lexer: self,
            source,
            code,
            cursor: 0,
            lineno: 1,
            end: code.len(),
            tokens: Vec::new(),
            state: State::Data,
            states: Vec::new(),
            brackets: Vec::new(),
            next_position: 0,
            positions: self.patterns.tokens_start.find_all(code),
            current_var_block_line: 1,
            documentation: Vec::new(),
            deprecations,
        };
        lexing.run()?;
        Ok(TokenStream::new(lexing.tokens, source.clone()))
    }
}

/// `preg_match(Lexer::REGEX_NAME, $value)`: whether `value` starts with a name (the pattern is anchored at the start
/// only), i.e. its first byte is in `[a-zA-Z_\x7f-\xff]`. The parser uses it to accept operator tokens such as `and`
/// or `not in` as attribute names.
pub fn matches_name(value: &[u8]) -> bool {
    patterns::name_len(value).is_some()
}

/// `preg_match(Lexer::REGEX_NAME, $value, $matches) ? $matches[0] : null`: the name `value` starts with. The parser
/// compares it with the whole value to accept an operator token such as `and` (but not `not in`) as a variable name.
pub fn match_name(value: &[u8]) -> Option<&[u8]> {
    patterns::name_len(value).map(|length| &value[..length])
}

impl Default for Lexer {
    /// A lexer with Twig's default options and the default environment's operators.
    fn default() -> Self {
        Lexer::new(LexerOptions::default(), DEFAULT_OPERATORS)
    }
}

impl fmt::Debug for Lexer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lexer")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// Twig: `Lexer::STATE_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    Block,
    Var,
    String,
    Interpolation,
}

/// The state of one `tokenize()` call (Twig keeps it in `Lexer` properties).
struct Lexing<'a> {
    lexer: &'a Lexer,
    source: &'a Source,
    code: &'a [u8],
    cursor: usize,
    lineno: i64,
    end: usize,
    tokens: Vec<Token>,
    state: State,
    states: Vec<State>,
    /// Open brackets: the opening text (`(`, `[`, `{`, `"` or the interpolation opener) and its line.
    brackets: Vec<(&'a [u8], i64)>,
    /// `$this->position + 1`: the index of the next candidate in `positions`.
    next_position: usize,
    positions: Vec<TokenStart>,
    current_var_block_line: i64,
    documentation: Vec<Vec<u8>>,
    deprecations: &'a mut dyn DeprecationSink,
}

impl Lexing<'_> {
    /// The main loop of `Lexer::tokenize()`.
    fn run(&mut self) -> Result<(), Error> {
        while self.cursor < self.end {
            match self.state {
                State::Data => self.lex_data()?,
                State::Block => self.lex_block()?,
                State::Var => self.lex_var()?,
                State::String => self.lex_string()?,
                State::Interpolation => self.lex_interpolation()?,
            }
        }

        self.push_token(TokenType::Eof, TokenValue::empty(), None, None);

        if let Some((expect, lineno)) = self.brackets.pop() {
            return Err(self.syntax_error(format!("Unclosed \"{}\".", php::lossy(expect)), lineno));
        }
        Ok(())
    }

    /// Twig: `Lexer::lexData()`.
    fn lex_data(&mut self) -> Result<(), Error> {
        let code = self.code;
        let lexer = self.lexer;
        let options = &lexer.options;

        // If no matches are left, the rest of the template is text.
        if self.next_position == self.positions.len() {
            let text = &code[self.cursor..];
            self.push_token(
                TokenType::Text,
                php::normalize_newlines(text).into_owned().into(),
                None,
                None,
            );
            self.move_cursor(text.len());
            return Ok(());
        }

        // Find the first token start after the current cursor.
        let mut position = self.positions[self.next_position];
        self.next_position += 1;
        while position.start < self.cursor {
            if self.next_position == self.positions.len() {
                return Ok(());
            }
            position = self.positions[self.next_position];
            self.next_position += 1;
        }

        // Push the template text first.
        let text_content = &code[self.cursor..position.start];
        let trim = &code[position.delimiter_end..position.end];
        let text = if trim == options.whitespace_trim.as_slice() {
            // whitespace_trim detected ({%-, {{- or {#-)
            php::rtrim(text_content, php::TRIM_DEFAULT)
        } else if trim == options.whitespace_line_trim.as_slice() {
            // whitespace_line_trim detected ({%~, {{~ or {#~): don't trim \r and \n
            php::rtrim(text_content, php::TRIM_LINE)
        } else {
            text_content
        };
        self.push_token(
            TokenType::Text,
            php::normalize_newlines(text).into_owned().into(),
            None,
            None,
        );
        self.move_cursor(text_content.len());

        let full_len = position.end - position.start;
        let delimiter = &code[position.start..position.delimiter_end];
        // A PHP `switch`: the cases are tried in order with the loose `==`.
        let is = |option: &[u8]| php::loose_equals_strings(delimiter, option);
        if is(&lexer.patterns.documentation_open) {
            self.move_cursor(full_len);
            self.lex_comment(true)?;
        } else if is(&options.tag_comment[0]) {
            self.move_cursor(full_len);
            self.lex_comment(false)?;
        } else if is(&options.tag_block[0]) {
            let lineno = self.lineno;
            let cursor = self.cursor;
            self.move_cursor(full_len);

            let patterns = &lexer.patterns;
            if let Some(raw) = patterns.lex_block_raw.find(&code[self.cursor..]) {
                // {% verbatim %}
                self.documentation.clear();
                self.move_cursor(raw.end());
                self.lex_raw_data()?;
            } else if let Some(line) = patterns.lex_block_line.captures(&code[self.cursor..]) {
                // {% line \d+ %}
                self.documentation.clear();
                self.move_cursor(line.get_match().end());
                self.lineno =
                    php::int_from_digits(line.get(1).map_or(&b""[..], |digits| digits.as_bytes()));
            } else {
                self.push_token(
                    TokenType::BlockStart,
                    TokenValue::empty(),
                    Some(cursor),
                    Some(lineno),
                );
                self.push_state(State::Block);
                self.current_var_block_line = self.lineno;
            }
        } else if is(&options.tag_variable[0]) {
            let lineno = self.lineno;
            let cursor = self.cursor;
            self.move_cursor(full_len);
            self.push_token(
                TokenType::VarStart,
                TokenValue::empty(),
                Some(cursor),
                Some(lineno),
            );
            self.push_state(State::Var);
            self.current_var_block_line = self.lineno;
        }
        Ok(())
    }

    /// Twig: `Lexer::lexBlock()`.
    fn lex_block(&mut self) -> Result<(), Error> {
        if self.brackets.is_empty()
            && let Some(close) = self
                .lexer
                .patterns
                .lex_block
                .find(&self.code[self.cursor..])
        {
            self.push_closing_token(TokenType::BlockEnd, close.end());
            self.pop_state();
            Ok(())
        } else {
            self.lex_expression()
        }
    }

    /// Twig: `Lexer::lexVar()`.
    fn lex_var(&mut self) -> Result<(), Error> {
        if self.brackets.is_empty()
            && let Some(close) = self.lexer.patterns.lex_var.find(&self.code[self.cursor..])
        {
            self.push_closing_token(TokenType::VarEnd, close.end());
            self.pop_state();
            Ok(())
        } else {
            self.lex_expression()
        }
    }

    /// Twig: `Lexer::lexExpression()`.
    fn lex_expression(&mut self) -> Result<(), Error> {
        let code = self.code;

        // whitespace
        if let Some(length) = patterns::whitespace_len(&code[self.cursor..]) {
            self.move_cursor(length);
            if self.cursor >= self.end {
                let what = if self.state == State::Block {
                    "block"
                } else {
                    "variable"
                };
                return Err(
                    self.syntax_error(format!("Unclosed \"{what}\"."), self.current_var_block_line)
                );
            }
        }

        let rest = &code[self.cursor..];
        if let Some(end) = self.lexer.operators.match_at(code, self.cursor) {
            // operators
            let matched = &code[self.cursor..end];
            let operator = collapse_whitespace(matched);
            if matches!(operator.as_slice(), b"{" | b"(" | b"[") {
                self.check_brackets(&operator)?;
            }
            self.push_token(TokenType::Operator, operator.into(), None, None);
            self.move_cursor(matched.len());
        } else if let Some(length) = patterns::name_len(rest) {
            // names
            self.push_token(TokenType::Name, rest[..length].into(), None, None);
            self.move_cursor(length);
        } else if let Some(length) = patterns::number_len(rest) {
            // numbers: `0 + str_replace('_', '', $match[0])`
            let digits: String = rest[..length]
                .iter()
                .filter(|&&b| b != b'_')
                .map(|&b| char::from(b))
                .collect();
            let value = match php::numeric_literal(&digits) {
                php::Number::Int(int) => TokenValue::Int(int),
                php::Number::Float(float) => TokenValue::Float(float),
            };
            self.push_token(TokenType::Number, value, None, None);
            self.move_cursor(length);
        } else if b"()[]{}?:.,|".contains(&rest[0]) {
            // punctuation
            let punctuation = &rest[..1];
            self.check_brackets(punctuation)?;
            self.push_token(TokenType::Punctuation, punctuation.into(), None, None);
            self.move_cursor(1);
        } else if let Some(length) = patterns::string_len(rest) {
            // strings
            let content = php::normalize_newlines(&rest[1..length - 1]);
            let value = self.stripcslashes(&content, rest[0]);
            self.push_token(TokenType::String, value.into(), None, None);
            self.move_cursor(length);
        } else if rest[0] == b'"' {
            // opening double quoted string
            self.brackets.push((b"\"", self.lineno));
            self.push_state(State::String);
            self.move_cursor(1);
        } else if let Some(length) = patterns::inline_comment_len(rest) {
            // inline comment
            if let Some(documentation) = rest[..length].strip_prefix(b"##") {
                self.add_documentation(documentation);
            }
            self.move_cursor(length);
        } else {
            // unlexable
            return Err(self.unexpected_character());
        }
        Ok(())
    }

    /// Twig: `Lexer::lexRawData()`.
    fn lex_raw_data(&mut self) -> Result<(), Error> {
        let code = self.code;
        let Some(end) = self
            .lexer
            .patterns
            .lex_raw_data
            .captures(&code[self.cursor..])
        else {
            return Err(self.syntax_error(
                "Unexpected end of file: Unclosed \"verbatim\" block.",
                self.lineno,
            ));
        };
        let whole = end.get_match();
        let offset = self.cursor;
        let text = &code[self.cursor..self.cursor + whole.start()];
        self.move_cursor(whole.end());

        let options = &self.lexer.options;
        let text = match end.get(1) {
            // whitespace_trim detected
            Some(trim) if trim.as_bytes() == options.whitespace_trim.as_slice() => {
                php::rtrim(text, php::TRIM_DEFAULT)
            }
            // whitespace_line_trim detected: don't trim \r and \n
            Some(_) => php::rtrim(text, php::TRIM_LINE),
            None => text,
        };
        // The token gets the line after the closing tag (Twig pushes it after moving the cursor).
        self.push_token(
            TokenType::Text,
            php::normalize_newlines(text).into_owned().into(),
            Some(offset),
            None,
        );
        Ok(())
    }

    /// Twig: `Lexer::lexComment()`.
    fn lex_comment(&mut self, is_documentation: bool) -> Result<(), Error> {
        let code = self.code;
        let patterns = &self.lexer.patterns;
        let regex = if is_documentation {
            &patterns.lex_documentation_comment
        } else {
            &patterns.lex_comment
        };
        let Some(close) = regex.find(&code[self.cursor..]) else {
            return Err(self.syntax_error("Unclosed comment.", self.lineno));
        };
        let comment = &code[self.cursor..self.cursor + close.start()];
        if is_documentation {
            // Support a symmetric "##}" closing marker.
            self.add_documentation(comment.strip_suffix(b"#").unwrap_or(comment));
        }
        self.move_cursor(close.end());
        Ok(())
    }

    /// Twig: `Lexer::lexString()`.
    fn lex_string(&mut self) -> Result<(), Error> {
        let code = self.code;
        let lexer = self.lexer;
        let patterns = &lexer.patterns;
        let rest = &code[self.cursor..];
        if patterns.interpolation_enabled
            && let Some(start) = patterns.interpolation_start.find(rest)
        {
            self.brackets
                .push((&lexer.options.interpolation[0], self.lineno));
            self.push_token(
                TokenType::InterpolationStart,
                TokenValue::empty(),
                None,
                None,
            );
            self.move_cursor(start.end());
            self.push_state(State::Interpolation);
            return Ok(());
        }
        let part = patterns::dq_string_part_len(code, self.cursor);
        if part > 0 {
            let content = php::normalize_newlines(&rest[..part]);
            let value = self.stripcslashes(&content, b'"');
            self.push_token(TokenType::String, value.into(), None, None);
            self.move_cursor(part);
        } else if rest[0] == b'"' {
            // Twig pops the bracket and then checks that the current character is `"` (always true here) before
            // reporting `Unclosed "%s".`.
            self.brackets.pop();
            self.pop_state();
            self.move_cursor(1);
        } else {
            // unlexable: only a trailing backslash at the end of the template gets here
            return Err(self.unexpected_character());
        }
        Ok(())
    }

    /// Twig: `Lexer::lexInterpolation()`.
    fn lex_interpolation(&mut self) -> Result<(), Error> {
        let interpolation_open = self.lexer.options.interpolation[0].as_slice();
        if self
            .brackets
            .last()
            .is_some_and(|&(expect, _)| expect == interpolation_open)
            && let Some(end) = self
                .lexer
                .patterns
                .interpolation_end
                .find(&self.code[self.cursor..])
        {
            self.brackets.pop();
            self.push_closing_token(TokenType::InterpolationEnd, end.end());
            self.pop_state();
            Ok(())
        } else {
            self.lex_expression()
        }
    }

    /// Twig: `Lexer::pushToken()`. Text tokens with an empty value are dropped; pending documentation is attached
    /// to the token unless it is a whitespace-only text token.
    fn push_token(
        &mut self,
        ty: TokenType,
        value: TokenValue,
        offset: Option<usize>,
        lineno: Option<i64>,
    ) {
        if ty == TokenType::Text && value.as_bytes().is_some_and(<[u8]>::is_empty) {
            return;
        }

        let mut documentation = None;
        if !self.documentation.is_empty()
            && (ty != TokenType::Text
                || value
                    .as_bytes()
                    .is_some_and(|text| !php::trim(text, php::TRIM_DEFAULT).is_empty()))
        {
            documentation = Some(self.documentation.join(&b'\n'));
            self.documentation.clear();
        }

        let token = Token::new(ty, value, lineno.unwrap_or(self.lineno))
            .with_offset(Some(offset.unwrap_or(self.cursor)))
            .with_documentation(documentation);
        self.tokens.push(token);
    }

    /// Twig: `Lexer::addDocumentation()`.
    fn add_documentation(&mut self, documentation: &[u8]) {
        let documentation = php::normalize_newlines(documentation);
        let documentation = php::trim(&documentation, php::TRIM_DEFAULT);
        if !documentation.is_empty() {
            self.documentation.push(documentation.to_vec());
        }
    }

    /// Twig: `Lexer::moveCursor()`. Every call is one chunk for line counting, exactly as in Twig.
    fn move_cursor(&mut self, length: usize) {
        let chunk = &self.code[self.cursor..self.cursor + length];
        self.cursor += length;
        // Twig's line number becomes a float on overflow, which makes the next token construction fail with a
        // PHP TypeError; twig-rs saturates instead.
        self.lineno = self.lineno.saturating_add(php::count_newlines(chunk));
    }

    /// Twig: `Lexer::pushClosingToken()`: the token points at the closing marker, after leading whitespace
    /// (`ltrim()`'s set, which excludes `\f` although the patterns' `\s*` consumes it).
    fn push_closing_token(&mut self, ty: TokenType, length: usize) {
        let matched = &self.code[self.cursor..self.cursor + length];
        let leading_whitespace = length - php::ltrim(matched, php::TRIM_DEFAULT).len();
        if leading_whitespace > 0 {
            self.move_cursor(leading_whitespace);
        }
        self.push_token(ty, TokenValue::empty(), None, None);
        self.move_cursor(length - leading_whitespace);
    }

    /// Twig: `Lexer::pushState()`.
    fn push_state(&mut self, state: State) {
        self.states.push(self.state);
        self.state = state;
    }

    /// Twig: `Lexer::popState()`. States are always pushed before they are popped, so Twig's
    /// `Cannot pop state without a previous state.` logic exception cannot happen.
    fn pop_state(&mut self) {
        debug_assert!(
            !self.states.is_empty(),
            "Cannot pop state without a previous state."
        );
        self.state = self.states.pop().unwrap_or(State::Data);
    }

    /// Twig: `Lexer::checkBrackets()`.
    fn check_brackets(&mut self, code: &[u8]) -> Result<(), Error> {
        let opening: Option<&'static [u8]> = match code {
            b"{" => Some(b"{"),
            b"(" => Some(b"("),
            b"[" => Some(b"["),
            _ => None,
        };
        if let Some(opening) = opening {
            self.brackets.push((opening, self.lineno));
        } else if matches!(code, b"}" | b")" | b"]") {
            let Some((expect, lineno)) = self.brackets.pop() else {
                let column = self.source.column(self.cursor);
                return Err(self
                    .syntax_error(format!("Unexpected \"{}\".", php::lossy(code)), self.lineno)
                    .with_column(Some(column)));
            };
            // str_replace(['{', '(', '['], ['}', ')', ']'], $expect)
            let closing: Vec<u8> = expect
                .iter()
                .map(|&b| match b {
                    b'{' => b'}',
                    b'(' => b')',
                    b'[' => b']',
                    other => other,
                })
                .collect();
            if code != closing.as_slice() {
                return Err(
                    self.syntax_error(format!("Unclosed \"{}\".", php::lossy(expect)), lineno)
                );
            }
        }
        Ok(())
    }

    /// Twig: `Lexer::stripcslashes()`, reporting the 3.12 deprecation for escaped characters without meaning.
    fn stripcslashes(&mut self, s: &[u8], quote: u8) -> Vec<u8> {
        let name = self.source.name();
        let lineno = self.lineno;
        let deprecations = &mut *self.deprecations;
        escape::stripcslashes(s, quote, |character, position| {
            deprecations.report(Deprecation::twig(
                "3.12",
                format!(
                    "Character \"{}\" should not be escaped; the \"\\\" character is ignored in Twig 3 but will not be \
                     in Twig 4. Please remove the extra \"\\\" character at position {position} in \"{name}\" at line \
                     {lineno}.",
                    php::lossy(&[character]),
                ),
            ));
        })
    }

    /// `Unexpected character "%s".` for the byte at the cursor, with its column.
    fn unexpected_character(&self) -> Error {
        let character = &self.code[self.cursor..=self.cursor];
        self.syntax_error(
            format!("Unexpected character \"{}\".", php::lossy(character)),
            self.lineno,
        )
        .with_column(Some(self.source.column(self.cursor)))
    }

    fn syntax_error(&self, message: impl Into<String>, lineno: i64) -> Error {
        Error::syntax(message)
            .with_line(lineno)
            .with_source(self.source.clone())
    }
}

/// `preg_replace('/\s+/', ' ', $match[0])`: whitespace runs collapse to one space.
fn collapse_whitespace(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut in_run = false;
    for &byte in text {
        if php::is_pcre_space(byte) {
            if !in_run {
                out.push(b' ');
                in_run = true;
            }
        } else {
            out.push(byte);
            in_run = false;
        }
    }
    out
}
