use regex::{ Captures };
use std::collections::{ VecDeque };

use super::Lexer;
use { Result, Error };
use token::{ Token };
use value::{ TwigNumberRef, TwigValueRef };
use token::Value as TokenValue;
use lexer::options::Options;
use std::fmt;
use Expect;
use error::{ ErrorMessage, Received };

const PUNCTUATION: &'static str = "()[]{}?:.,|";

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
    len: usize,
    all_len: usize,
    value: TokenValue<'code>,
    ws_trim: bool,
}

impl<'code> Position<'code> {
    fn from_capture(options: &Options, c: Captures<'code>) -> Position<'code> {
        let (all_start, all_end) = c.pos(0).expect("twig bug: expected full capture when collecting positions");
        let (first_start, first_end) = c.pos(1).expect("twig bug: expected at least one subcapture (start, end) when collecting positions");
        let second = c.pos(2);

        Position {
            loc: all_start,
            len: first_end - first_start,
            all_len: all_end - all_start,
            value: match c.at(1).expect("twig bug: expected at least one subcapture (text) when collecting positions") {
                s if s == options.tag_variable.start => TokenValue::VarStart,
                s if s == options.tag_block.start => TokenValue::BlockStart,
                s if s == options.tag_comment.start => TokenValue::CommentStart,
                _ => unreachable!("twig bug: unexpected capture when collecting positions"),
            },
            ws_trim: match second {
                Some(_) => true,
                _ => false,
            },
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
            line_num: line_num,
        }
    }

    fn from_char(open_char: char, line_num: usize) -> Bracket {
        Bracket::new(BracketSymbol::Char(open_char), line_num)
    }
}

pub struct TokenIter<'iteration, 'code> {
    lexer: &'iteration Lexer,

    code: &'code str,
    tokens: VecDeque<Result<Token<'code>>>,
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
    type Item = Result<Token<'code>>;

    fn next(&mut self) -> Option<Result<Token<'code>>> {

        if self.finished {
            return None;
        }

        if self.tokens.len() == 0 {
            self.collect_tokens();
        }

        self.tokens.pop_front()
    }
}

impl<'code, T> Expect<TokenValue<'code>> for T where T: Iterator<Item=Result<Token<'code>>> {
    type Output = Result<Token<'code>>;

    fn expect(&mut self, expected: TokenValue<'code>) -> Self::Output {
        let maybe_token = self.next();
        match (maybe_token, expected) {
            (None, _) => return Err(
                Error::new(
                    ErrorMessage::ExpectedTokenButReceived(
                        (expected.into(), Received::EndOfStream)
                    )
                )
            ),
            (Some(Ok(token)), expected) => if token.value == expected {
                Ok(token)
            } else {
                return Err(
                    Error::new_at(
                        ErrorMessage::ExpectedTokenButReceived(
                            (expected.into(), Received::Token(token.value.into()))
                        ),
                        token.line
                    )
                );
            },
            (Some(error), _) => error,
        }
    }
}

/// Iterator over tokens.
impl<'iteration, 'code> TokenIter<'iteration, 'code> {

    /// Create the iterator.
    pub fn new<'caller>(lexer: &'caller Lexer, code: &'code str) -> TokenIter<'caller, 'code> {
        // find all token starts in one go
        let positions = lexer.matchers.lex_tokens_start.captures_iter(code)
            .filter_map(|c| match c.is_empty() {
                true => None,
                false => Some(Position::from_capture(&lexer.options, c)),
            })
            .collect::<Vec<Position>>();

        let code_len = code.len();

        let iter = TokenIter {
            lexer: lexer,
            code: code,
            cursor: 0,
            current_var_block_line: None,
            line_num: 1,
            end: code_len,
            state: State::Data,
            states: Vec::new(),
            brackets: Vec::new(),
            position: 0,
            positions: positions,
            tokens: VecDeque::new(),
            is_error: false,
            finished: false,
        };

        iter
    }

    /// When we run out of tokens, we call this function to buffer more.
    fn collect_tokens(&mut self) {
        loop {
            if self.is_error {
                self.finished = true;
                break;
            }

            if self.cursor == self.end {
                match self.brackets.pop() {
                    Some(bracket) => {
                        self.push_error(
                            ErrorMessage::Unclosed(format!("{}", bracket.open)),
                            Some(bracket.line_num)
                        );
                        break;
                    },
                    _ => (),
                };

                self.finished = true;
                break;
            }

            if self.tokens.len() > 0 {
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

            self.push_token(TokenValue::Text(&self.code[loc..]));
            self.cursor = self.end;

            return;
        }

        // Find the first token after the current cursor
        let mut position = self.positions[self.position].clone(); self.position += 1;

        while position.loc < self.cursor {
            if self.position == positions_len {
                return;
            }
            position = self.positions[self.position].clone(); self.position += 1;
        }

        // push the template text first
        let loc = self.cursor;
        let text_content = &self.code[loc .. position.loc];

        self.push_token(
            if position.ws_trim {
                TokenValue::Text(text_content.trim_right())
            } else {
                TokenValue::Text(text_content)
            }
        );
        self.move_cursor(text_content.len() + position.all_len);

        match position.value {
            TokenValue::CommentStart => self.lex_comment(),
            TokenValue::BlockStart => {
                let loc = self.cursor;
                // raw data?
                if let Some(captures) = self.lexer.matchers.lex_block_raw.captures(&self.code[loc ..]) {
                    if let Some((start, end)) = captures.pos(0) {
                        if let Some(tag) = captures.at(1) {
                            self.move_cursor(end - start);
                            self.lex_raw_data(tag);
                            return;
                        }
                    } else {
                        unreachable!("twig bug: captured lex_block_raw but no capture data");
                    }
                }
                // {% line \d+ %}
                if let Some(captures) = self.lexer.matchers.lex_block_line.captures(&self.code[loc ..]) {
                    let maybe_start_and_end = captures.pos(0);
                    let maybe_line_num = captures.at(1);

                    match (maybe_start_and_end, maybe_line_num) {
                        (Some((start, end)), Some(line_num)) => {
                            self.move_cursor(end - start);
                            self.line_num = line_num.parse()
                                .ok()
                                .expect("twig bug: expected regexp matched as digit to be parseable as line number");
                            return;
                        },
                        _ => {
                            unreachable!("twig bug: captured lex_block_line but no capture data");
                        }
                    }
                }

                self.push_token(TokenValue::BlockStart);
                self.push_state(State::Block);
                self.current_var_block_line = Some(self.line_num);
            },
            TokenValue::VarStart => {
                self.push_token(TokenValue::VarStart);
                self.push_state(State::Var);
                self.current_var_block_line = Some(self.line_num);
            },
            _ => unreachable!("twig bug: lex_data match position.value"),
        }
    }

    fn lex_block(&mut self) {

        if 0 == self.brackets.len() {

            let loc = self.cursor;

            if let Some(captures) = self.lexer.matchers.lex_block.captures(&self.code[loc ..]) {

                if let Some((start, end)) = captures.pos(0) {
                    self.push_token(TokenValue::BlockEnd);
                    self.move_cursor(end - start);
                    self.pop_state();

                    return;
                } else {
                    unreachable!("twig bug: captured lex_block but no capture data");
                }
            }
        }

        self.lex_expression();
    }

    fn lex_var(&mut self) {

        if 0 == self.brackets.len() {

            let loc = self.cursor;

            if let Some(captures) = self.lexer.matchers.lex_var.captures(&self.code[loc ..]) {

                if let Some((start, end)) = captures.pos(0) {
                    self.push_token(TokenValue::VarEnd);
                    self.move_cursor(end - start);
                    self.pop_state();

                    return;
                } else {
                    unreachable!("twig bug: captured lex_var but no capture data");
                }
            }
        }

        self.lex_expression();
    }

    fn lex_expression(&mut self) {

        // whitespace
        let loc = self.cursor;
        if let Some(captures) = self.lexer.matchers.whitespace.captures(&self.code[loc ..]) {
            if let Some((start, end)) = captures.pos(0) {
                self.move_cursor(end - start);
                if self.cursor >= self.end {
                    let var_line = self.current_var_block_line;
                    self.push_error(
                        ErrorMessage::Unclosed(
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
            } else {
                unreachable!("twig bug: captured whitespace but no capture data");
            }
        }

        // operators
        let loc = self.cursor;
        if let Some(captures) = self.lexer.matchers.lex_operator.captures(&self.code[loc ..]) {
            if let Some((start, end)) = captures.pos(0) {
                let op_str = self.code[loc + start .. loc + end].trim_right();

                self.push_token(TokenValue::Operator(op_str));
                self.move_cursor(end - start);

                return;
            } else {
                // Just skip, it is not op.
            }
        }

        // names
        let loc = self.cursor;
        if let Some(captures) = self.lexer.matchers.regex_name.captures(&self.code[loc ..]) {
            if let Some((start, end)) = captures.pos(0) {
                self.push_token(TokenValue::Name(&self.code[loc + start .. loc + end]));
                self.move_cursor(end - start);

                return;
            } else {
                unreachable!("twig bug: captured regex_name but no capture data");
            }
        }

        // numbers
        let loc = self.cursor;
        if let Some(captures) = self.lexer.matchers.regex_number.captures(&self.code[loc ..]) {
            if let Some((start, end)) = captures.pos(0) {
                let string = captures.at(0).unwrap(); // we checked that (0) exists above.

                let all_chars_are_digits = string.chars().all(|c| c.is_digit(10));
                let twig_number = if all_chars_are_digits {
                    let maybe_int = string.parse();
                    match maybe_int {
                        Ok(int) => TwigNumberRef::Int(int),
                        _ => TwigNumberRef::Big(string),
                    }
                } else {
                    let maybe_float = string.parse::<f64>();
                    match maybe_float {
                        Ok(float) => {
                            if float.is_finite() {
                                TwigNumberRef::Float(float)
                            } else {
                                TwigNumberRef::Big(string)
                            }
                        },
                        _ => TwigNumberRef::Big(string),
                    }
                };

                self.push_token(TokenValue::Value(TwigValueRef::Num(twig_number)));
                self.move_cursor(end - start);

                return;
            } else {
                unreachable!("twig bug: captured regex_number but no capture data");
            }
        }

        // punctuation
        let loc = self.cursor;
        if let Some(c) = self.code[loc..].chars().next() {
            if PUNCTUATION.contains(c) {

                let line_num = self.line_num;

                // opening bracket
                if "([{".contains(c) {
                    self.brackets.push(Bracket::from_char(c, line_num));
                } else if ")]}".contains(c) {
                    match self.brackets.pop() {
                        Some(expect) => {
                            if expect.close != BracketSymbol::Char(c) {
                                self.push_error(
                                    ErrorMessage::Unclosed(
                                        format!("{}", expect.open)
                                    ),
                                    Some(expect.line_num)
                                );
                                return;
                            }
                        },
                        None => {
                            self.push_error(
                                ErrorMessage::Unexpected(
                                    format!("{}", c)
                                ),
                                Some(line_num)
                            );
                            return;
                        }
                    }
                }

                self.push_token(TokenValue::Punctuation(c));
                self.move_cursor(1);

                return;
            }
        }

        // strings
        let loc = self.cursor;
        if let Some(captures) = self.lexer.matchers.regex_string.captures(&self.code[loc ..]) {
            if let Some((start, end)) = captures.pos(0) {
                self.push_token(TokenValue::Value(TwigValueRef::Str(
                    &self.code[loc + start + 1 .. loc + end - 1]
                )));
                self.move_cursor(end - start);

                return;
            } else {
                unreachable!("twig bug: captured regex_string but no capture data");
            }
        }

        // opening double quoted string
        let loc = self.cursor;
        if self.lexer.matchers.regex_dq_string_delim.is_match(&self.code[loc ..]) {
            self.brackets.push(Bracket::from_char('"', self.line_num));
            self.push_state(State::String);
            self.move_cursor(1);

            return;
        }

        let next_char = &self.code[loc .. loc + 1];
        let line_num = self.line_num;
        self.push_error(
            ErrorMessage::UnexpectedCharacter(
                format!("{}", next_char)
            ),
            Some(line_num)
        );
    }

    fn lex_string(&mut self) {

        let loc = self.cursor;

        if let Some(captures) = self.lexer.matchers.interpolation_start.captures(&self.code[loc ..]) {
            if let Some((start, end)) = captures.pos(0) {
                self.brackets.push(Bracket::new(BracketSymbol::IntStart, self.line_num));
                self.push_token(TokenValue::InterpolationStart);
                self.move_cursor(end - start);
                self.push_state(State::Interpolation);

                return;
            } else {
                unreachable!("twig bug: captured interpolation_start but no capture data");
            }
        }

        let (_, part_end) = self.lexer.matchers.match_regex_dq_string_part(&self.code[loc ..]);
        if part_end > 0 {
            self.push_token(TokenValue::Value(TwigValueRef::Str(
                &self.code[loc .. loc + part_end]
            )));
            self.move_cursor(part_end);

            return;
        }

        if self.lexer.matchers.regex_dq_string_delim.is_match(&self.code[loc ..]) {
            let last_bracket = self.brackets.pop();

            match last_bracket {
                Some(Bracket { close: BracketSymbol::Char('"'), .. }) => {
                    self.pop_state();
                    self.move_cursor(1);
                },
                Some(other_bracket) => {
                    self.push_error(
                        ErrorMessage::Unclosed(
                            format!("{}", other_bracket.open)
                        ),
                        Some(other_bracket.line_num)
                    );
                },
                None => unreachable!("twig bug: expected bracket when lexng string end"),
            }
        }
    }

    fn lex_interpolation(&mut self) {

        let in_interpolation = match self.brackets.last() {
            Some(bracket) if bracket.open == BracketSymbol::IntStart => true,
            _ => false,
        };

        if in_interpolation {
            let loc = self.cursor;
            if let Some(captures) = self.lexer.matchers.interpolation_end.captures(&self.code[loc ..]) {
                if let Some((start, end)) = captures.pos(0) {
                    self.brackets.pop();
                    self.push_token(TokenValue::InterpolationEnd);
                    self.move_cursor(end - start);
                    self.pop_state();

                    return;
                } else {
                    unreachable!("twig bug: captured interpolation_end but no capture data");
                }
            }
        }

        self.lex_expression();
    }

    fn lex_comment(&mut self) {

        let loc = self.cursor;
        let maybe_found = self.lexer.matchers.lex_comment.find(&self.code[loc ..]);

        match maybe_found {
            Some((_, end)) => {
                self.move_cursor(end);
            },
            None => {
                let line_num = self.line_num;
                self.push_error(ErrorMessage::UnclosedComment, Some(line_num));
            }
        };
    }

    fn lex_raw_data(&mut self, tag: &'code str) {
        let loc = self.cursor;
        let maybe_captures = {
            match tag {
                "raw" => self.lexer.matchers.lex_raw_data.captures(&self.code[loc ..]),
                "verbatim" => self.lexer.matchers.lex_verbatim_data.captures(&self.code[loc ..]),
                _ => unreachable!("twig bug: expected raw or verbatim tag, but got {}", tag),
            }
        };

        match maybe_captures {
            Some(captures) => {
                let maybe_full = captures.pos(0);
                let maybe_end = captures.at(1);

                match (maybe_full, maybe_end) {
                    (Some((start, end)), Some(end_text)) => {
                        let mut text = &self.code[loc..loc + start];
                        self.move_cursor(end - start);

                        if end_text.contains("-") {
                            text = text.trim_right()
                        }

                        self.push_token(TokenValue::Text(text));
                    },
                    _ => unreachable!("twig bug: captured lex_raw_data but no capture data"),
                }
            },
            None => {
                let line_num = self.line_num;
                self.push_error(
                    ErrorMessage::UnclosedBlock(
                        format!("{}", tag)
                    ),
                    Some(line_num)
                );
            }
        };
    }

    fn push_token(&mut self, token_value: TokenValue<'code>) {
        // do not push empty text tokens
        if let TokenValue::Text(ref text) = token_value {
            if text.len() == 0 {
                return;
            }
        }

        self.tokens.push_back(Ok(Token { value: token_value, line: self.line_num }));
    }

    fn push_error(&mut self, message: ErrorMessage, line_num: Option<usize>) {
        self.tokens.push_back(Err(
            Error::new_at(message, match line_num {
                Some(line) => line,
                None => unreachable!("twig bug: error should not be pushed without a line number"),
            })
        ));
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
            },
            None => panic!("twig bug: cannot pop state without a previous state"),
        }
    }

    fn move_cursor(&mut self, offset: usize) {
        let prev_loc = self.cursor;

        self.cursor += offset;

        let mut lines = 0;
        for c in self.code[prev_loc .. self.cursor].chars() {
            if c == '\n' {
                lines += 1;
            }
        }

        self.line_num += lines;
    }
}
