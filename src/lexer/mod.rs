use regex::{ Regex, Captures, quote };
use std::iter::{ Iterator };
use std::collections::{ VecDeque, HashMap };

use environment::{
    Environment,
    UnaryOperator,
    BinaryOperator
};
use token::Token;
use token::State;
use token::Value as TokenValue;
use error::{ Result, Error };

#[derive(Copy, Clone)]
pub struct Delimiters {
    pub start: &'static str,
    pub end: &'static str,
}

impl Delimiters {
    pub fn new(start: &'static str, end: &'static str) -> Delimiters {
        Delimiters {
            start: start,
            end: end,
        }
    }
}

struct Brackets;

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

pub struct Lexer {
    options: Options,
    whitespace: Regex,
    lex_var: Regex,
    lex_block: Regex,
    lex_raw_data: Regex,
    operator: Regex,
    lex_comment: Regex,
    lex_block_raw: Regex,
    lex_block_line: Regex,
    lex_tokens_start: Regex,
    interpolation_start: Regex,
    interpolation_end: Regex,
}

impl Lexer {
    pub fn default(env: &Environment) -> Lexer {
        let options = Options::default();

        Lexer {
            options: options,
            whitespace: {
                Regex::new(
                    r#"^\s+"#
                ).ok().expect("Failed to init whitespace")
            },
            lex_var: {
                Regex::new(
                    &format!(
                        r#"^\s*{}{}\s*|\s*{}"#,
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_variable.end),
                        &quote(&options.tag_variable.end)
                    )
                ).ok().expect("Failed to init lex_var")
            },
            lex_block: {
                Regex::new(
                    &format!(
                        r#"^\s*(?:{}{}\s*|\s*{})\n?"#,
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.end),
                        &quote(&options.tag_block.end)
                    )
                ).ok().expect("Failed to init lex_block")
            },
            lex_raw_data: {
                Regex::new(
                    &format!(
                        r#"(?s)({}{}|{})\s*(?:end%s)\s*(?:{}{}\s*|\s*{})"#,
                        &quote(&options.tag_block.start),
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.start),
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.end),
                        &quote(&options.tag_block.end)
                    )
                ).ok().expect("Failed to init lex_raw_data")
            },
            operator: Lexer::get_operator_regex(
                &env.unary_operators,
                &env.binary_operators
            ),
            lex_comment: {
                Regex::new(
                    &format!(
                        r#"(?s)(?:{}{}\s*|{})\n?"#,
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_comment.end),
                        &quote(&options.tag_comment.end)
                    )
                ).ok().expect("Failed to init lex_comment")
            },
            lex_block_raw: {
                Regex::new(
                    &format!(
                        r#"^(?s)\s*(raw|verbatim)\s*(?:{}{}\s*|\s*{})"#,
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.end),
                        &quote(&options.tag_block.end)
                    )
                ).ok().expect("Failed to init lex_block_raw")
            },
            lex_block_line: {
                Regex::new(
                    &format!(
                        r#"^(?s)\s*line\s+(\d+)\s*{}"#,
                        &quote(&options.tag_block.end)
                    )
                ).ok().expect("Failed to init lex_block_line")
            },
            lex_tokens_start: {
                Regex::new(
                    &format!(
                        r#"(?s)({}|{}|{})({})?"#,
                        &quote(&options.tag_variable.start),
                        &quote(&options.tag_block.start),
                        &quote(&options.tag_comment.start),
                        &quote(&options.whitespace_trim)
                    )
                ).ok().expect("Failed to init lex_tokens_start")
            },
            interpolation_start: {
                Regex::new(
                    &format!(
                        r#"^{}\s*"#,
                        &quote(&options.interpolation.start)
                    )
                ).ok().expect("Failed to init interpolation_start")
            },
            interpolation_end: {
                Regex::new(
                    &format!(
                        r#"^\s*{}"#,
                        &quote(&options.interpolation.end)
                    )
                ).ok().expect("Failed to init interpolation_end")
            },
        }
    }

    fn get_operator_regex(
        unary_operators: &HashMap<&'static str, UnaryOperator>,
        binary_operators: &HashMap<&'static str, BinaryOperator>
    ) -> Regex {
        let mut all: Vec<_> = Some("=").into_iter()
            .chain(
                unary_operators.keys()
                    .map(|&v| v)
            )
            .chain(
                binary_operators.keys()
                    .map(|&v| v)
            )
            .collect();

        all.sort_by(|a, b| b.len().cmp(&a.len()));

        let mut regex_items = Vec::new();

        for operator in all {
            let length = operator.len();

            assert!(length > 0);

            // an operator that ends with a character must be followed by
            // a whitespace or a parenthesis
            let mut r = match operator.chars().last() {
                Some(c) if c.is_alphabetic() => format!(
                    "({}){}",
                    quote(operator),
                    r#"[\s()]"#
                ),
                _ => format!(
                    "({})",
                    quote(operator)
                ),
            };

            r = r.replace(" ", "\\s+");

            regex_items.push(r);
        }

        let regex_string = format!(r#"^{}"#, &regex_items.connect("|"));

        match Regex::new(
            &regex_string,
        ) {
            Ok(regex) => regex,
            Err(e) => panic!("Failed to init operator_regex \n{}\n{:?}", regex_string, e),
        }
    }

    pub fn tokens<'r>(&'r self, code: &'r str) -> Iter
    {
        Iter::new(self, code)
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

impl<'code> Position<'code> {
    fn from_capture(options: &Options, code: &'code str, c: Captures<'code>) -> Position<'code> {
        let (all_start, all_end) = c.pos(0).expect("Expected full capture");
        let (first_start, first_end) = c.pos(1).expect("Expected at least one subcapture");
        let second = c.pos(2);

        Position {
            loc: all_start,
            len: first_end - first_start,
            all_len: all_end - all_start,
            value: match c.at(1).expect("Expected at least one subcapture") {
                s if s == options.tag_variable.start => TokenValue::VarStart,
                s if s == options.tag_block.start => TokenValue::BlockStart,
                s if s == options.tag_comment.start => TokenValue::CommentStart,
                _ => unreachable!("Unexpected capture!"),
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
        while position.loc < self.cursor {
            if self.position == positions_len {
                return;
            }
            position = self.positions[self.position].clone(); self.position += 1;
        }

        // push the template text first
        let loc = self.cursor;
        let text_content = &self.code[loc .. position.loc - loc];
        println!("   text_content {}", text_content);
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

            // `case $this->options['tag_comment'][0]:`
            TokenValue::CommentStart => self.lex_comment(),

            // `case $this->options['tag_block'][0]:`
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

            // `case $this->options['tag_variable'][0]:`

            _ => unreachable!("lex_data match position.value"),
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
                    unreachable!("captured lex_block but no capture data");
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
                    unreachable!("captured lex_var but no capture data");
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
                                _ => unreachable!("expected state at block or variable, but other state found"),
                            }
                        ),
                        var_line
                    );
                    return;
                }
            } else {
                unreachable!("captured whitespace but no capture data");
            }
        }

        // operators
        let loc = self.cursor;

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
        unimplemented!();
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
                None => unreachable!("Error should not be pushed without a line number"),
            })
        ));
    }

    fn push_state(&mut self, state: State) {
        println!("<- push state {:?}", state);
        self.states.push(state);
        self.state = state;
    }

    fn pop_state(&mut self) {
        match self.states.pop() {
            Some(state) => {
                println!("<- pop state {:?}", state);
                self.state = state;
            },
            None => panic!("Cannot pop state without a previous state"),
        }
    }

    fn move_cursor(&mut self, offset: usize) {
        let prev_loc = self.cursor;

        self.cursor += offset;
        self.line_num += self.code[prev_loc .. prev_loc + offset].lines().count();
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

#[cfg(test)]
mod test {
    use super::*;
    use token::*;
    use environment::Environment;

    #[test]
    fn name_label_for_tag() {
        let template = "{% § %}";
        let lexer = Lexer::default(&Environment::default());
        let mut stream = lexer.tokens(template);

        //expect(&mut stream, Value::Eof);
    }

    // fn expect<'r>(stream: &'r mut Iter<'r>, token_value: Value) {
    //     if let Err(unexpection) = stream.expect(token_value, None) {
    //         panic!("bad token: {:?}", unexpection);
    //     }
    // }
}
