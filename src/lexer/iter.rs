use regex::{ Regex, Captures, quote };
use std::collections::{ VecDeque, HashMap };

use super::Lexer;
use error::{ Result, Error };
use token::{ Token, TwigNumber, TwigString };
use token::State;
use token::Value as TokenValue;
use lexer::options::Options;
use std::u64;
use std::fmt;

const PUNCTUATION: &'static str = "()[]{}?:.,|";

enum MatchMode {
    Normal,
    Escape,
    MaybeInterpolation(usize),
}

/// If matches strign contents up to #{, return pos as (start, end).
///
/// This is /[^#"\\]*(?:(?:\\.|#(?!\{))[^#"\\]*)*/As regular expression written
/// manually.
fn match_regex_dq_string_part(code: &str) -> (usize, usize) {
    let mut index = 0;
    let mut mode = MatchMode::Normal;

    for c in code.chars() {
        match mode {
            MatchMode::Normal => {
                match c {
                    '\\' => mode = MatchMode::Escape,
                    '#' => mode = MatchMode::MaybeInterpolation(index),
                    '"' => return (0, index),
                    _ => (),
                };
            },
            MatchMode::Escape => mode = MatchMode::Normal,
            MatchMode::MaybeInterpolation(started_at) => {
                match c {
                    '{' => return (0, started_at),
                    _ => mode = MatchMode::Normal,
                };
            }
        };

        index += 1;
    }

    (0, index)
}

#[cfg(test)]
mod test_match_regex_dq_string_part {
    use super::match_regex_dq_string_part;

    #[test]
    fn should_match_full_str_with_first_esc_char() {
        assert_eq!((0, 2), match_regex_dq_string_part("##"))
    }

    #[test]
    fn should_match_empty_str() {
        assert_eq!((0, 0), match_regex_dq_string_part(""))
    }

    #[test]
    fn should_match_up_to_str_end() {
        assert_eq!((0, 2), match_regex_dq_string_part(r#"##"foo"#))
    }

    #[test]
    fn should_skip_escaped_str_end() {
        assert_eq!((0, 7), match_regex_dq_string_part(r#"##\"foo"#))
    }

    #[test]
    fn should_match_up_to_interpolation_start() {
        assert_eq!((0, 3), match_regex_dq_string_part(r#"aa #{ foo"#))
    }

    #[test]
    fn should_skip_escaped_interpolation_start() {
        assert_eq!((0, 10), match_regex_dq_string_part(r#"aa \#{ foo"#))
    }
}

#[derive(Debug, Copy, Clone)]
struct Position<'code> {
    loc: usize,
    len: usize,
    all_len: usize,
    value: TokenValue<'code>,
    ws_trim: bool,
}

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

struct Bracket {
    open: BracketSymbol,
    close: BracketSymbol,
    line_num: usize,
}

impl Bracket {
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

impl<'code> Position<'code> {
    fn from_capture(options: &Options, code: &'code str, c: Captures<'code>) -> Position<'code> {
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

pub struct Iter<'iteration, 'code> {
    lexer: &'iteration Lexer,

    code: &'code str,
    tokens: VecDeque<Result<Token<'code>>>,
    position: usize,
    positions: Vec<Position<'code>>,

    cursor: usize,
    end: usize,
    finished: bool,

    state: State,
    states: Vec<State>,

    brackets: Vec<Bracket>,

    current_var_block_line: Option<usize>,
    line_num: usize,
}

/// Iterator over stuff.
///
/// ## Example
///
/// ```
/// let x = "a";
/// ```
impl<'iteration, 'code> Iter<'iteration, 'code> {
    /// Create the iterator.
    ///
    /// > Compatible with `tokenize`.
    pub fn new<'caller>(lexer: &'caller Lexer, code: &'code str) -> Iter<'caller, 'code> {
        // find all token starts in one go
        let positions = lexer.lex_tokens_start.captures_iter(code)
            .filter_map(|c| match c.is_empty() {
                true => None,
                false => Some(Position::from_capture(&lexer.options, code, c)),
            })
            .collect::<Vec<Position>>();

        let code_len = code.len();

        let mut iter = Iter {
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
            finished: false,
        };

        iter
    }

    pub fn get_line_num(&self) -> usize {
        self.line_num
    }

    /// When we run out of tokens, we call this function to buffer more.
    /// > Compatible with `tokenize`.
    fn collect_tokens(&mut self) {
        println!("> collect_tokens");
        loop {
            if self.cursor == self.end || self.tokens.len() > 0 {
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

        if self.cursor == self.end {
            if !self.finished {
                self.push_token(TokenValue::Eof);
                self.finished = true;
            }
        }
    }

    fn lex_data(&mut self) {
        println!(">> lex_data");
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
        println!("-- MOVE POSITION --");
        self.output_pos(position.loc);
        while position.loc < self.cursor {
            if self.position == positions_len {
                return;
            }
            position = self.positions[self.position].clone(); self.position += 1;
            println!("-- MOVE POSITION --");
            self.output_pos(position.loc);
        }

        // push the template text first
        let loc = self.cursor;
        let text_content = &self.code[loc .. position.loc];
        println!("Text is {:?}", text_content);
        self.push_token(
            if position.ws_trim {
                TokenValue::Text(text_content.trim_right())
            } else {
                TokenValue::Text(text_content)
            }
        );
        self.move_cursor(text_content.len() + position.all_len);

        println!("   match position.value {:?}", position.value);

        match position.value {
            TokenValue::CommentStart => self.lex_comment(),
            TokenValue::BlockStart => {
                let loc = self.cursor;
                // raw data?
                if let Some(captures) = self.lexer.lex_block_raw.captures(&self.code[loc ..]) {
                    println!("      lex_block_raw");
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
                if let Some(captures) = self.lexer.lex_block_line.captures(&self.code[loc ..]) {
                    println!("      lex_block_line");
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

                println!("      push block start");
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
        println!(">> lex_block");

        if 0 == self.brackets.len() {
            println!("      no brackets");
            let loc = self.cursor;

            if let Some(captures) = self.lexer.lex_block.captures(&self.code[loc ..]) {
                println!("      lex_block");
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
        println!(">> lex_var");

        if 0 == self.brackets.len() {
            println!("      no brackets");
            let loc = self.cursor;

            if let Some(captures) = self.lexer.lex_var.captures(&self.code[loc ..]) {
                println!("      lex_var");
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
        println!(">> lex_expression");

        // whitespace
        let loc = self.cursor;
        if let Some(captures) = self.lexer.whitespace.captures(&self.code[loc ..]) {
            println!("      expression whitespace");
            if let Some((start, end)) = captures.pos(0) {
                self.move_cursor(end - start);
                if self.cursor >= self.end {
                    let var_line = self.current_var_block_line;
                    self.push_error(
                        format!(
                            "Unclosed \"{}\"",
                            match self.state {
                                State::Block => "block",
                                State::Var => "variable",
                                _ => unreachable!("twig bug: expected state at block or variable, but other state found"),
                            }
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
        if let Some(captures) = self.lexer.lex_operator.captures(&self.code[loc ..]) {
            println!("      lex_operator {:?}", captures.at(1));
            if let Some((start, end)) = captures.pos(1) {
                self.push_token(TokenValue::Operator);
                self.move_cursor(end - start);

                return;
            } else {
                // Just skip, it is not op.
            }
        }

        // names
        let loc = self.cursor;
        if let Some(captures) = self.lexer.regex_name.captures(&self.code[loc ..]) {
            println!("      regex_name {:?}", captures.at(0));
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
        if let Some(captures) = self.lexer.regex_number.captures(&self.code[loc ..]) {
            println!("      regex_number {:?}", captures.at(0));
            if let Some((start, end)) = captures.pos(0) {
                let string = captures.at(0).unwrap(); // we checked that (0) exists above.

                let all_chars_are_digits = string.chars().all(|c| c.is_digit(10));
                let twig_number = if all_chars_are_digits {
                    let maybe_int = string.parse();
                    match maybe_int {
                        Ok(int) => TwigNumber::Int(int),
                        _ => TwigNumber::Big(string),
                    }
                } else {
                    let maybe_float = string.parse::<f64>();
                    match maybe_float {
                        Ok(float) => {
                            if float.is_finite() {
                                TwigNumber::Float(float)
                            } else {
                                TwigNumber::Big(string)
                            }
                        },
                        _ => TwigNumber::Big(string),
                    }
                };

                self.push_token(TokenValue::Number(twig_number));
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
                println!("      punctuation {:?}", c);

                let line_num = self.line_num;

                // opening bracket
                if "([{".contains(c) {
                    self.brackets.push(Bracket::from_char(c, line_num));
                } else if ")]}".contains(c) {
                    match self.brackets.pop() {
                        Some(expect) => {
                            if expect.close != BracketSymbol::Char(c) {
                                self.push_error(format!(r#"Unclosed "{}""#, expect.open), Some(expect.line_num));
                                return;
                            }
                        },
                        None => {
                            self.push_error(format!(r#"Unexpected "{}""#, c), Some(line_num));
                            return;
                        }
                    }
                }

                self.push_token(TokenValue::Punctuation(c));
                self.move_cursor(1);

                return;
            } else {
                println!("      not in punctuation {:?}", c);
            }
        }

        // strings
        let loc = self.cursor;
        if let Some(captures) = self.lexer.regex_string.captures(&self.code[loc ..]) {
            if let Some((start, end)) = captures.pos(0) {
                self.push_token(TokenValue::String(TwigString::new(
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
        if let Some(captures) = self.lexer.regex_dq_string_delim.captures(&self.code[loc ..]) {
            if let Some((start, end)) = captures.pos(0) {
                self.brackets.push(Bracket::from_char('"', self.line_num));
                self.push_state(State::String);
                self.move_cursor(1);

                return;
            } else {
                unreachable!("twig bug: captured regex_string but no capture data");
            }
        }

        println!("else");

        unimplemented!();
    }

    fn lex_string(&mut self) {
        println!(">> lex_string");

        let loc = self.cursor;
        if let Some(captures) = self.lexer.interpolation_start.captures(&self.code[loc ..]) {
            println!("      interpolation_start {:?}", captures.at(1));
            if let Some((start, end)) = captures.pos(0) {
                self.brackets.push(Bracket::new(BracketSymbol::IntStart, self.line_num));
                self.push_token(TokenValue::InterpolationStart);
                self.push_state(State::Interpolation);

                return;
            } else {
                unreachable!("twig bug: captured interpolation_start but no capture data");
            }
        }

        let (_, part_end) = match_regex_dq_string_part(&self.code[loc ..]);
        if part_end > 0 {
            self.push_token(TokenValue::String(TwigString::new(
                &self.code[loc .. loc + part_end]
            )));
            self.move_cursor(part_end);

            return;
        }
        unimplemented!();
    }

    fn lex_interpolation(&mut self) {
        println!(">> lex_interpolation");
        unimplemented!();
    }

    fn lex_comment(&mut self) {
        println!("   > lex_comment");

        let loc = self.cursor;
        let maybe_captures = self.lexer.lex_comment.captures(&self.code[loc ..]);

        match maybe_captures {
            Some(captures) => {
                if let Some((start, end)) = captures.pos(0) {
                    self.move_cursor(end);
                } else {
                    unreachable!("twig bug: captured lex_comment but no capture data");
                }
            },
            None => {
                let line_num = self.line_num;
                self.push_error("Unclosed comment", Some(line_num));
            }
        };
    }

    fn lex_raw_data(&mut self, tag: &'code str) {
        println!("   > lex_raw_data");
        let loc = self.cursor;
        let maybe_captures = {
            match tag {
                "raw" => self.lexer.lex_raw_data.captures(&self.code[loc ..]),
                "verbatim" => self.lexer.lex_verbatim_data.captures(&self.code[loc ..]),
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
                self.push_error(format!(r#"Unexpected end of file: Unclosed "{}" block"#, tag), Some(line_num));
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

        println!("<- push_token {:?}, line_num {:?}", token_value, self.line_num);

        self.tokens.push_back(Ok(Token { value: token_value, line_num: self.line_num }));
    }

    fn push_error<M: Into<String>>(&mut self, message: M, line_num: Option<usize>) {
        self.tokens.push_back(Err(
            Error::new(message, match line_num {
                Some(line) => line,
                None => unreachable!("twig bug: error should not be pushed without a line number"),
            })
        ));
    }

    fn push_state(&mut self, state: State) {
        println!("<- push state {:?}", state);
        self.states.push(self.state);
        self.state = state;
    }

    fn pop_state(&mut self) {
        match self.states.pop() {
            Some(state) => {
                println!("<- pop state {:?}", state);
                self.state = state;
            },
            None => panic!("twig bug: cannot pop state without a previous state"),
        }
    }

    fn output_pos(&self, pos: usize) {
        // let mut line_start_offset = pos;
        // while line_start_offset > 0 {
        //     if self.code.chars().take(line_start_offset).last().expect("not empty last") == '\n' {
        //         break;
        //     }
        //
        //     line_start_offset -= 1;
        // }
        //
        // let mut line_end_offset = pos;
        // while line_end_offset < self.code.len() {
        //     if line_end_offset > 0 && self.code.chars().skip(line_end_offset - 1).next().expect("not empty next") == '\n' {
        //         break;
        //     }
        //
        //     line_end_offset += 1;
        // }
        //
        // let line_cursor = pos - line_start_offset;
        // let mut line = &self.code[line_start_offset .. line_end_offset];
        // if line.len() > 0 {
        //     line = &line[0..line.len()-1];
        // }
        //
        // println!("{}", line);
        // println!("{}^", "-".to_string().chars().cycle().take(line_cursor).map(|c| c.to_string()).collect::<Vec<_>>().concat());
    }

    fn move_cursor(&mut self, offset: usize) {
        let prev_loc = self.cursor;

        self.cursor += offset;
        self.line_num += self.code[prev_loc .. self.cursor].lines().count() - 1;
        println!("line num is {}", self.line_num);

        println!("-- CURSOR --");
        self.output_pos(self.cursor);
    }
}

impl<'iteration, 'code> Iterator for Iter<'iteration, 'code> {
    type Item = Result<Token<'code>>;

    fn next(&mut self) -> Option<Result<Token<'code>>> {
        println!("<- next");
        if self.tokens.len() == 0 {
            self.collect_tokens();
        }

        println!("<- pop");
        self.tokens.pop_front()
    }
}
