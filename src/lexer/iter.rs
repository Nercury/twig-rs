use regex::{ Regex, Captures, quote };
use std::collections::{ VecDeque, HashMap };

use super::Lexer;
use error::{ Result, Error };
use token::{ Token, TwigNumber };
use token::State;
use token::Value as TokenValue;
use lexer::options::Options;
use std::u64;

const PUNCTUATION: &'static str = "()[]{}?:.,|";

#[derive(Debug, Copy, Clone)]
struct Position<'code> {
    loc: usize,
    len: usize,
    all_len: usize,
    value: TokenValue<'code>,
    ws_trim: bool,
}

struct Brackets;

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

    brackets: Vec<Brackets>,

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

    /// When we run out of tokens, we call this function to buffer more.
    /// > Compatible with `tokenize`.
    fn collect_tokens(&mut self) {
        println!("> collect_tokens");
        if self.cursor < self.end {
            // dispatch to the lexing functions depending
            // on the current state
            match self.state {
                State::Data => self.lex_data(),
                State::Block => self.lex_block(),
                State::Var => self.lex_var(),
                State::String => self.lex_string(),
                State::Interpolation => self.lex_interpolation(),
            }
        } else {
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
                    }
                }
                // {% line \d+ %}
                if let Some(captures) = self.lexer.lex_block_line.captures(&self.code[loc ..]) {
                    println!("      lex_block_line");
                    unimplemented!();
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
                let number: f64 = match string.parse() {
                    Ok(number) => number,
                    _ => unreachable!("twig bug: expected that anything matched by regex_number can be parsed as 64-bit float"),
                };
                let int = number as u64;
                let twig_number = if string.chars().all(|c| c.is_digit(10)) && int <= u64::MAX {
                    TwigNumber::Int(int)
                } else {
                    TwigNumber::Float(number)
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

                // opening bracket
                if "([{".contains(c) {
                    unimplemented!();
                } else if ")]}".contains(c) {
                    unimplemented!();
                }

                self.push_token(TokenValue::Punctuation(c));
                self.move_cursor(1);

                return;
            }
        }

        unimplemented!();
    }

    fn lex_string(&mut self) {
        println!(">> lex_string");
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
                self.push_error("Unclosed comment", Some(loc));
            }
        };
    }

    fn lex_raw_data(&mut self, tag: &'code str) {
        println!("   > lex_raw_data");
        let pos = self.cursor;
        let code_at_cursor = &self.code[pos..];

        unimplemented!();
        //if !self.lexer.lex_block_raw
    }

    fn push_token(&mut self, token_value: TokenValue<'code>) {
        println!("<- push_token {:?}", token_value);

        // do not push empty text tokens
        if let TokenValue::Text(ref text) = token_value {
            if text.len() == 0 {
                return;
            }
        }

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
        let mut line_start_offset = pos;
        while line_start_offset > 0 {
            if self.code.chars().take(line_start_offset).last().unwrap() == '\n' {
                break;
            }

            line_start_offset -= 1;
        }

        let mut line_end_offset = pos;
        while line_end_offset < self.code.len() {
            if self.code.chars().skip(line_end_offset - 1).next().unwrap() == '\n' {
                break;
            }

            line_end_offset += 1;
        }

        let line_cursor = pos - line_start_offset;
        let mut line = &self.code[line_start_offset .. line_end_offset];
        if line.len() > 0 {
            line = &line[0..line.len()-1];
        }

        println!("{}", line);
        println!("{}^", "-".to_string().chars().cycle().take(line_cursor).map(|c| c.to_string()).collect::<Vec<_>>().concat());
    }

    fn move_cursor(&mut self, offset: usize) {
        let prev_loc = self.cursor;

        self.cursor += offset;
        self.line_num += self.code[prev_loc .. self.cursor].lines().count() - 1;

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
