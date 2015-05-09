use regex::{ Regex, Captures, quote };
use std::iter::{ Iterator };
use std::collections::VecDeque;

use token::Token;
use token::State;
use token::Value as TokenValue;

pub struct Delimiters {
    pub start: String,
    pub end: String,
}

impl Delimiters {
    pub fn new(start: &str, end: &str) -> Delimiters {
        Delimiters {
            start: start.to_string(),
            end: end.to_string(),
        }
    }
}

pub struct Options {
    pub tag_comment: Delimiters,
    pub tag_block: Delimiters,
    pub tag_variable: Delimiters,
    pub whitespace_trim: String,
    pub interpolation: Delimiters,
}

impl Options {
    pub fn default() -> Options {
        Options {
            tag_comment: Delimiters::new("{#", "#}"),
            tag_block: Delimiters::new("{%", "%}"),
            tag_variable: Delimiters::new("{{", "}}"),
            whitespace_trim: "-".to_string(),
            interpolation: Delimiters::new("#{", "}"),
        }
    }
}

pub struct Lexer {
    options: Options,
    lex_var: Regex,
    lex_block: Regex,
    lex_raw_data: Regex,
    lex_comment: Regex,
    lex_block_raw: Regex,
    lex_block_line: Regex,
    lex_tokens_start: Regex,
    interpolation_start: Regex,
    interpolation_end: Regex,
}

impl Lexer {
    pub fn default() -> Lexer {
        let options = Options::default();

        let lex_var = Regex::new(
            &format!(
                r#"^\s*{}{}\s*|\s*{}"#,
                &quote(&options.whitespace_trim),
                &quote(&options.tag_variable.end),
                &quote(&options.tag_variable.end)
            )
        ).ok().expect("Failed to init lex_var");

        let lex_block = Regex::new(
            &format!(
                r#"^\s*(?:{}{}\s*|\s*{})\n?"#,
                &quote(&options.whitespace_trim),
                &quote(&options.tag_block.end),
                &quote(&options.tag_block.end)
            )
        ).ok().expect("Failed to init lex_block");

        let lex_raw_data = Regex::new(
            &format!(
                r#"(?s)({}{}|{})\s*(?:end%s)\s*(?:{}{}\s*|\s*{})"#,
                &quote(&options.tag_block.start),
                &quote(&options.whitespace_trim),
                &quote(&options.tag_block.start),
                &quote(&options.whitespace_trim),
                &quote(&options.tag_block.end),
                &quote(&options.tag_block.end)
            )
        ).ok().expect("Failed to init lex_raw_data");

        let lex_comment = Regex::new(
            &format!(
                r#"(?s)(?:{}{}\s*|{})\n?"#,
                &quote(&options.whitespace_trim),
                &quote(&options.tag_comment.end),
                &quote(&options.tag_comment.end)
            )
        ).ok().expect("Failed to init lex_comment");

        let lex_block_raw = Regex::new(
            &format!(
                r#"^(?s)\s*(raw|verbatim)\s*(?:{}{}\s*|\s*{})"#,
                &quote(&options.whitespace_trim),
                &quote(&options.tag_block.end),
                &quote(&options.tag_block.end)
            )
        ).ok().expect("Failed to init lex_block_raw");

        let lex_block_line = Regex::new(
            &format!(
                r#"^(?s)\s*line\s+(\d+)\s*{}"#,
                &quote(&options.tag_block.end)
            )
        ).ok().expect("Failed to init lex_block_line");

        let lex_tokens_start = Regex::new(
            &format!(
                r#"(?s)({}|{}|{})({})?"#,
                &quote(&options.tag_variable.start),
                &quote(&options.tag_block.start),
                &quote(&options.tag_comment.start),
                &quote(&options.whitespace_trim)
            )
        ).ok().expect("Failed to init lex_tokens_start");

        let interpolation_start = Regex::new(
            &format!(
                r#"^{}\s*"#,
                &quote(&options.interpolation.start)
            )
        ).ok().expect("Failed to init interpolation_start");

        let interpolation_end = Regex::new(
            &format!(
                r#"^\s*{}"#,
                &quote(&options.interpolation.end)
            )
        ).ok().expect("Failed to init interpolation_end");

        Lexer {
            options: options,
            lex_var: lex_var,
            lex_block: lex_block,
            lex_raw_data: lex_raw_data,
            lex_comment: lex_comment,
            lex_block_raw: lex_block_raw,
            lex_block_line: lex_block_line,
            lex_tokens_start: lex_tokens_start,
            interpolation_start: interpolation_start,
            interpolation_end: interpolation_end,
        }
    }

    pub fn tokenize<'r>(&'r self, code: &'r str) -> Iter
    {
        Iter::new(self, code)
    }
}

#[derive(Debug, Clone)]
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
                _ => panic!("Unexpected capture!"),
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
    tokens: VecDeque<Token<'code>>,
    position: usize,
    positions: Vec<Position<'code>>,

    cursor: usize,
    end: usize,
    finished: bool,

    state: State,

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
            line_num: 1,
            end: code_len,
            state: State::Data,
            position: 0,
            positions: positions,
            tokens: VecDeque::new(),
            finished: false,
        };

        iter
    }

    fn collect_tokens(&mut self) {
        println!("collect_tokens");
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
        println!("lex_data");
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
        println!("text_content {}", text_content);
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
                if let Some(captures) = self.lexer.lex_block_raw.captures(&self.code[loc ..]) {
                    println!("match raw");
                    if let Some((start, end)) = captures.pos(0) {
                        if let Some(tag) = captures.at(1) {
                            self.move_cursor(end - start);
                            self.lex_raw_data(tag);
                        }
                    }
                }
            },
            _ => (),
        }
    }

    fn lex_block(&mut self) {

    }

    fn lex_var(&mut self) {

    }

    fn lex_string(&mut self) {

    }

    fn lex_interpolation(&mut self) {

    }

    fn lex_comment(&mut self) {

    }

    fn lex_raw_data(&mut self, tag: &'code str) {
        let pos = self.cursor;
        let code_at_cursor = &self.code[pos..];

        //if !self.lexer.lex_block_raw
    }

    fn push_token(&mut self, token_value: TokenValue<'code>) {
        println!("push_token {:?}", token_value);

        // do not push empty text tokens
        if let TokenValue::Text(ref text) = token_value {
            if text.len() == 0 {
                println!("text.len() == 0, ignore");
                return;
            }
        }

        self.tokens.push_back(Token { value: token_value, line_num: self.line_num });
    }

    fn move_cursor(&mut self, offset: usize) {
        let prev_loc = self.cursor;

        self.cursor += offset;
        self.line_num += self.code[prev_loc .. prev_loc + offset].lines().count();
    }
}

impl<'iteration, 'code> Iterator for Iter<'iteration, 'code> {
    type Item = Token<'code>;

    fn next(&mut self) -> Option<Token<'code>> {
        println!("next");
        if self.tokens.len() == 0 {
            self.collect_tokens();
        }

        println!("pop");
        self.tokens.pop_front()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use token::*;

    #[test]
    fn name_label_for_tag() {
        let template = "{% § %}";
        let lexer = Lexer::default();
        let mut stream = lexer.tokenize(template);

        //expect(&mut stream, Value::Eof);
    }

    // fn expect<'r>(stream: &'r mut Iter<'r>, token_value: Value) {
    //     if let Err(unexpection) = stream.expect(token_value, None) {
    //         panic!("bad token: {:?}", unexpection);
    //     }
    // }
}
