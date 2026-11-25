use regex::Captures;
use std::collections::VecDeque;

use super::Lexer;
use crate::Expect;
use crate::error::TemplateResult;
use crate::error::{Received, TemplateError};
use crate::tokens::{ConstNumberRef, ConstRef, LexerOptions, TokenRef, TokenValueRef};
use std::fmt;

const PUNCTUATION: &str = "()[]{}?:.,|";

/// Iteration state.
#[derive(Debug, Copy, Clone)]
pub enum State {
    Data,
    Block,
    Var,
    String,
    Interpolation,
}

/// Block position.
///
/// At start, Twig runs regexp that finds all interesting block starts, like {{ or {%.
/// If nothing like that is found, no parsing occurs. Otherwise, it uses this position
/// to divide parsing in kind of "chunks".
#[derive(Debug, Copy, Clone)]
struct Position<'code> {
    loc: usize,
    all_len: usize,
    value: TokenValueRef<'code>,
    ws_trim: bool,
}

impl<'code> Position<'code> {
    fn from_capture(options: &LexerOptions, c: Captures<'code>) -> Position<'code> {
        let all = c.get_match();
        let first = c
            .get(1)
            .expect("twig bug: expected at least one subcapture when collecting positions");
        let second = c.get(2);

        Position {
            loc: all.start(),
            all_len: all.len(),
            value: match first.as_str() {
                s if s == options.tag_variable.start => TokenValueRef::VarStart,
                s if s == options.tag_block.start => TokenValueRef::BlockStart,
                s if s == options.tag_comment.start => TokenValueRef::CommentStart,
                _ => unreachable!("twig bug: unexpected capture when collecting positions"),
            },
            ws_trim: second.is_some(),
        }
    }
}

/// Twig has different brackets: (, {, [, etc.
/// The "interpolation" bracket is memorized as `IntStart` and `IntEnd` and looks
/// like "#{ blah }".
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
enum BracketSymbol {
    Char(char),
    IntStart,
    IntEnd,
}

impl fmt::Display for BracketSymbol {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            BracketSymbol::Char(c) => fmt::Display::fmt(&c.to_string(), f),
            BracketSymbol::IntStart => fmt::Display::fmt(r#"#{"#, f),
            BracketSymbol::IntEnd => fmt::Display::fmt(r#"}"#, f),
        }
    }
}

/// We memorize started brackets using this struct.
#[derive(Debug, Copy, Clone)]
struct Bracket {
    open: BracketSymbol,
    close: BracketSymbol,
    line_num: usize,
}

impl Bracket {
    /// When creating bracket from the starting bracket, immediately set the
    /// kind of bracket that is oposite to starting one, so we don't have to do
    /// it in the iterator.
    fn new(open_char: BracketSymbol, line_num: usize) -> Bracket {
        Bracket {
            open: open_char,
            close: match open_char {
                BracketSymbol::Char('(') => BracketSymbol::Char(')'),
                BracketSymbol::Char('[') => BracketSymbol::Char(']'),
                BracketSymbol::Char('{') => BracketSymbol::Char('}'),
                BracketSymbol::Char('"') => BracketSymbol::Char('"'),
                BracketSymbol::IntStart => BracketSymbol::IntEnd,
                _ => unreachable!("twig bug: unknown bracket {:?}", open_char),
            },
            line_num,
        }
    }

    fn from_char(open_char: char, line_num: usize) -> Bracket {
        Bracket::new(BracketSymbol::Char(open_char), line_num)
    }
}

pub struct TokenIter<'iteration, 'code> {
    lexer: &'iteration Lexer,

    code: &'code str,
    tokens: VecDeque<TemplateResult<TokenRef<'code>>>,
    position: usize,
    positions: Vec<Position<'code>>,

    cursor: usize,
    end: usize,
    finished: bool,
    is_error: bool,

    state: State,
    states: Vec<State>,

    brackets: Vec<Bracket>,

    current_var_block_line: Option<usize>,
    line_num: usize,
}

impl<'iteration, 'code> Iterator for TokenIter<'iteration, 'code> {
    type Item = TemplateResult<TokenRef<'code>>;

    fn next(&mut self) -> Option<TemplateResult<TokenRef<'code>>> {
        if self.finished {
            return None;
        }

        if self.tokens.is_empty() {
            self.collect_tokens();
        }

        self.tokens.pop_front()
    }
}

impl<'code, T> Expect<(usize, TokenValueRef<'code>)> for T
where
    T: Iterator<Item = TemplateResult<TokenRef<'code>>>,
{
    type Output = TemplateResult<TokenRef<'code>>;

    fn expect(&mut self, (line, expected): (usize, TokenValueRef<'code>)) -> Self::Output {
        let maybe_token = self.next();
        match (maybe_token, expected) {
            (None, _) => Err(TemplateError::ExpectedTokenTypeButReceived((
                expected.into(),
                Received::EndOfStream,
            ))
            .at(line)),
            (Some(Ok(token)), expected) => {
                if token.value == expected {
                    Ok(token)
                } else {
                    Err(TemplateError::ExpectedTokenTypeButReceived((
                        expected.into(),
                        Received::Token(token.value.into()),
                    ))
                    .at(token.line))
                }
            }
            (Some(error), _) => error,
        }
    }
}

/// Iterator over tokens.
impl<'iteration, 'code> TokenIter<'iteration, 'code> {
    /// Create the iterator.
    pub fn new<'caller>(lexer: &'caller Lexer, code: &'code str) -> TokenIter<'caller, 'code> {
        // find all token starts in one go
        let positions = lexer
            .matchers
            .lex_tokens_start
            .captures_iter(code)
            .map(|c| Position::from_capture(&lexer.options, c))
            .collect::<Vec<Position>>();

        let code_len = code.len();

        TokenIter {
            lexer,
            code,
            cursor: 0,
            current_var_block_line: None,
            line_num: 1,
            end: code_len,
            state: State::Data,
            states: Vec::new(),
            brackets: Vec::new(),
            position: 0,
            positions,
            tokens: VecDeque::new(),
            is_error: false,
            finished: false,
        }
    }

    /// When we run out of tokens, we call this function to buffer more.
    fn collect_tokens(&mut self) {
        loop {
            if self.is_error {
                self.finished = true;
                break;
            }

            if self.cursor == self.end {
                if let Some(bracket) = self.brackets.pop() {
                    self.push_error(
                        TemplateError::Unclosed(format!("{}", bracket.open)),
                        Some(bracket.line_num),
                    );
                    break;
                }

                self.finished = true;
                break;
            }

            if !self.tokens.is_empty() {
                break;
            }

            // dispatch to the lexing functions depending
            // on the current state
            match self.state {
                State::Data => self.lex_data(),
                State::Block => self.lex_block(),
                State::Var => self.lex_var(),
                State::String => self.lex_string(),
                State::Interpolation => self.lex_interpolation(),
            }
        }
    }

    fn lex_data(&mut self) {
        let positions_len = self.positions.len();

        // if no matches are left we return the rest of the template as simple text token
        if self.position == positions_len {
            let loc = self.cursor;

            self.push_token(TokenValueRef::Text(&self.code[loc..]));
            self.cursor = self.end;

            return;
        }

        // Find the first token after the current cursor
        let mut position = self.positions[self.position];
        self.position += 1;

        while position.loc < self.cursor {
            if self.position == positions_len {
                return;
            }
            position = self.positions[self.position];
            self.position += 1;
        }

        // push the template text first
        let loc = self.cursor;
        let text_content = &self.code[loc..position.loc];

        self.push_token(if position.ws_trim {
            TokenValueRef::Text(text_content.trim_end())
        } else {
            TokenValueRef::Text(text_content)
        });
        self.move_cursor(text_content.len() + position.all_len);

        match position.value {
            TokenValueRef::CommentStart => self.lex_comment(),
            TokenValueRef::BlockStart => {
                let loc = self.cursor;
                // raw data?
                if let Some(captures) = self
                    .lexer
                    .matchers
                    .lex_block_raw
                    .captures(&self.code[loc..])
                    && let Some(tag) = captures.get(1)
                {
                    self.move_cursor(captures.get_match().len());
                    self.lex_raw_data(tag.as_str());
                    return;
                }
                // {% line \d+ %}
                if let Some(captures) = self
                    .lexer
                    .matchers
                    .lex_block_line
                    .captures(&self.code[loc..])
                {
                    match captures.get(1) {
                        Some(line_num) => {
                            self.move_cursor(captures.get_match().len());
                            self.line_num = line_num.as_str().parse()
                                .expect("twig bug: expected regexp matched as digit to be parseable as line number");
                            return;
                        }
                        None => {
                            unreachable!("twig bug: captured lex_block_line but no capture data");
                        }
                    }
                }

                self.push_token(TokenValueRef::BlockStart);
                self.push_state(State::Block);
                self.current_var_block_line = Some(self.line_num);
            }
            TokenValueRef::VarStart => {
                self.push_token(TokenValueRef::VarStart);
                self.push_state(State::Var);
                self.current_var_block_line = Some(self.line_num);
            }
            _ => unreachable!("twig bug: lex_data match position.value"),
        }
    }

    fn lex_block(&mut self) {
        if self.brackets.is_empty() {
            let loc = self.cursor;

            if let Some(m) = self.lexer.matchers.lex_block.find(&self.code[loc..]) {
                self.push_token(TokenValueRef::BlockEnd);
                self.move_cursor(m.len());
                self.pop_state();

                return;
            }
        }

        self.lex_expression();
    }

    fn lex_var(&mut self) {
        if self.brackets.is_empty() {
            let loc = self.cursor;

            if let Some(m) = self.lexer.matchers.lex_var.find(&self.code[loc..]) {
                self.push_token(TokenValueRef::VarEnd);
                self.move_cursor(m.len());
                self.pop_state();

                return;
            }
        }

        self.lex_expression();
    }

    fn lex_expression(&mut self) {
        // whitespace
        let loc = self.cursor;
        if let Some(m) = self.lexer.matchers.whitespace.find(&self.code[loc..]) {
            self.move_cursor(m.len());
            if self.cursor >= self.end {
                let var_line = self.current_var_block_line;
                self.push_error(
                    TemplateError::Unclosed(
                        match self.state {
                            State::Block => "block",
                            State::Var => "variable",
                            _ => unreachable!("twig bug: expected state at block or variable, but other state found"),
                        }.into()
                    ),
                    var_line
                );
                return;
            }
        }

        // operators
        let loc = self.cursor;
        if let Some(m) = self.lexer.matchers.lex_operator.find(&self.code[loc..]) {
            let op_str = m.as_str().trim_end();

            self.push_token(TokenValueRef::Operator(op_str));
            self.move_cursor(m.len());

            return;
        }

        // names
        let loc = self.cursor;
        if let Some(m) = self.lexer.matchers.regex_name.find(&self.code[loc..]) {
            self.push_token(TokenValueRef::Name(m.as_str()));
            self.move_cursor(m.len());

            return;
        }

        // numbers
        let loc = self.cursor;
        if let Some(m) = self.lexer.matchers.regex_number.find(&self.code[loc..]) {
            let string = m.as_str();

            let all_chars_are_digits = string.chars().all(|c| c.is_ascii_digit());
            let twig_number = if all_chars_are_digits {
                let maybe_int = string.parse();
                match maybe_int {
                    Ok(int) => ConstNumberRef::Int(int),
                    _ => ConstNumberRef::Big(string),
                }
            } else {
                let maybe_float = string.parse::<f64>();
                match maybe_float {
                    Ok(float) => {
                        if float.is_finite() {
                            ConstNumberRef::Float(float)
                        } else {
                            ConstNumberRef::Big(string)
                        }
                    }
                    _ => ConstNumberRef::Big(string),
                }
            };

            self.push_token(TokenValueRef::Value(ConstRef::Num(twig_number)));
            self.move_cursor(m.len());

            return;
        }

        // punctuation
        let loc = self.cursor;
        if let Some(c) = self.code[loc..].chars().next()
            && PUNCTUATION.contains(c)
        {
            let line_num = self.line_num;

            // opening bracket
            if "([{".contains(c) {
                self.brackets.push(Bracket::from_char(c, line_num));
            } else if ")]}".contains(c) {
                match self.brackets.pop() {
                    Some(expect) => {
                        if expect.close != BracketSymbol::Char(c) {
                            self.push_error(
                                TemplateError::Unclosed(format!("{}", expect.open)),
                                Some(expect.line_num),
                            );
                            return;
                        }
                    }
                    None => {
                        self.push_error(
                            TemplateError::Unexpected(format!("{}", c)),
                            Some(line_num),
                        );
                        return;
                    }
                }
            }

            self.push_token(TokenValueRef::Punctuation(c));
            self.move_cursor(1);

            return;
        }

        // strings
        let loc = self.cursor;
        if let Some(m) = self.lexer.matchers.regex_string.find(&self.code[loc..]) {
            self.push_token(TokenValueRef::Value(ConstRef::Str(
                &self.code[loc + m.start() + 1..loc + m.end() - 1],
            )));
            self.move_cursor(m.len());

            return;
        }

        // opening double quoted string
        let loc = self.cursor;
        if self
            .lexer
            .matchers
            .regex_dq_string_delim
            .is_match(&self.code[loc..])
        {
            self.brackets.push(Bracket::from_char('"', self.line_num));
            self.push_state(State::String);
            self.move_cursor(1);

            return;
        }

        let next_char = &self.code[loc..loc + 1];
        let line_num = self.line_num;
        self.push_error(
            TemplateError::UnexpectedCharacter(next_char.to_string()),
            Some(line_num),
        );
    }

    fn lex_string(&mut self) {
        let loc = self.cursor;

        if let Some(m) = self
            .lexer
            .matchers
            .interpolation_start
            .find(&self.code[loc..])
        {
            self.brackets
                .push(Bracket::new(BracketSymbol::IntStart, self.line_num));
            self.push_token(TokenValueRef::InterpolationStart);
            self.move_cursor(m.len());
            self.push_state(State::Interpolation);

            return;
        }

        let (_, part_end) = self
            .lexer
            .matchers
            .match_regex_dq_string_part(&self.code[loc..]);
        if part_end > 0 {
            self.push_token(TokenValueRef::Value(ConstRef::Str(
                &self.code[loc..loc + part_end],
            )));
            self.move_cursor(part_end);

            return;
        }

        if self
            .lexer
            .matchers
            .regex_dq_string_delim
            .is_match(&self.code[loc..])
        {
            let last_bracket = self.brackets.pop();

            match last_bracket {
                Some(Bracket {
                    close: BracketSymbol::Char('"'),
                    ..
                }) => {
                    self.pop_state();
                    self.move_cursor(1);
                }
                Some(other_bracket) => {
                    self.push_error(
                        TemplateError::Unclosed(format!("{}", other_bracket.open)),
                        Some(other_bracket.line_num),
                    );
                }
                None => unreachable!("twig bug: expected bracket when lexng string end"),
            }
        }
    }

    fn lex_interpolation(&mut self) {
        let in_interpolation = matches!(
            self.brackets.last(),
            Some(bracket) if bracket.open == BracketSymbol::IntStart
        );

        if in_interpolation {
            let loc = self.cursor;
            if let Some(m) = self
                .lexer
                .matchers
                .interpolation_end
                .find(&self.code[loc..])
            {
                self.brackets.pop();
                self.push_token(TokenValueRef::InterpolationEnd);
                self.move_cursor(m.len());
                self.pop_state();

                return;
            }
        }

        self.lex_expression();
    }

    fn lex_comment(&mut self) {
        let loc = self.cursor;
        let maybe_found = self.lexer.matchers.lex_comment.find(&self.code[loc..]);

        match maybe_found {
            Some(m) => {
                self.move_cursor(m.end());
            }
            None => {
                let line_num = self.line_num;
                self.push_error(TemplateError::UnclosedComment, Some(line_num));
            }
        };
    }

    fn lex_raw_data(&mut self, tag: &'code str) {
        let loc = self.cursor;
        let maybe_captures = {
            match tag {
                "raw" => self.lexer.matchers.lex_raw_data.captures(&self.code[loc..]),
                "verbatim" => self
                    .lexer
                    .matchers
                    .lex_verbatim_data
                    .captures(&self.code[loc..]),
                _ => unreachable!("twig bug: expected raw or verbatim tag, but got {}", tag),
            }
        };

        match maybe_captures {
            Some(captures) => {
                let full = captures.get_match();

                match captures.get(1) {
                    Some(end_text) => {
                        let mut text = &self.code[loc..loc + full.start()];
                        self.move_cursor(full.len());

                        if end_text.as_str().contains('-') {
                            text = text.trim_end()
                        }

                        self.push_token(TokenValueRef::Text(text));
                    }
                    None => unreachable!("twig bug: captured lex_raw_data but no capture data"),
                }
            }
            None => {
                let line_num = self.line_num;
                self.push_error(
                    TemplateError::UnclosedBlock(tag.to_string()),
                    Some(line_num),
                );
            }
        };
    }

    fn push_token(&mut self, token_value: TokenValueRef<'code>) {
        // do not push empty text tokens
        if let TokenValueRef::Text(text) = token_value
            && text.is_empty()
        {
            return;
        }

        self.tokens.push_back(Ok(TokenRef {
            value: token_value,
            line: self.line_num,
        }));
    }

    fn push_error(&mut self, message: TemplateError, line_num: Option<usize>) {
        self.tokens.push_back(Err(message.at(match line_num {
            Some(line) => line,
            None => unreachable!("twig bug: error should not be pushed without a line number"),
        })));
        self.is_error = true;
    }

    fn push_state(&mut self, state: State) {
        self.states.push(self.state);
        self.state = state;
    }

    fn pop_state(&mut self) {
        match self.states.pop() {
            Some(state) => {
                self.state = state;
            }
            None => panic!("twig bug: cannot pop state without a previous state"),
        }
    }

    fn move_cursor(&mut self, offset: usize) {
        let prev_loc = self.cursor;

        self.cursor += offset;

        let mut lines = 0;
        for c in self.code[prev_loc..self.cursor].chars() {
            if c == '\n' {
                lines += 1;
            }
        }

        self.line_num += lines;
    }
}
