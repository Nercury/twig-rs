pub mod delimiters;
pub mod options;
pub mod iter;

use regex::{ Regex, Captures, quote };
use std::iter::{ Iterator };
use std::collections::{ VecDeque, HashMap };

use environment::{
    Environment,
    UnaryOperator,
    BinaryOperator
};
use error::{ Result, Error };

use self::delimiters::Delimiters;
use self::options::Options;
use self::iter::Iter;

pub struct Lexer {
    options: Options,
    whitespace: Regex,
    regex_name: Regex,
    regex_number: Regex,
    regex_string: Regex,
    lex_var: Regex,
    lex_block: Regex,
    lex_raw_data: Regex,
    lex_verbatim_data: Regex,
    lex_operator: Regex,
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
                    r#"\A\s+"#
                ).ok().expect("Failed to init whitespace")
            },
            regex_name: {
                Regex::new(
                    r#"\A[a-zA-Z_\x7F-\xFF][a-zA-Z0-9_\x7F-\xFF]*"#
                ).ok().expect("Failed to init regex_name")
            },
            regex_number: {
                Regex::new(
                    r#"\A[0-9]+(?:\.[0-9]+)?"#
                ).ok().expect("Failed to init regex_number")
            },
            regex_string: {
                Regex::new(
                    r#"\A(?s:"([^#"\\]*(?:\\.[^#"\\]*)*)"|'([^'\\]*(?:\\.[^'\\]*)*)')"#
                ).ok().expect("Failed to init regex_string")
            },
            lex_var: {
                Regex::new(
                    &format!(
                        r#"\A(?:\s*{}{}\s*|\s*{})"#,
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_variable.end),
                        &quote(&options.tag_variable.end)
                    )
                ).ok().expect("Failed to init lex_var")
            },
            lex_block: {
                Regex::new(
                    &format!(
                        r#"\A\s*(?:{}{}\s*|\s*{})\n?"#,
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.end),
                        &quote(&options.tag_block.end)
                    )
                ).ok().expect("Failed to init lex_block")
            },
            lex_raw_data: {
                Regex::new(
                    &format!(
                        r#"(?s)({}{}|{})\s*(?:endraw)\s*(?:{}{}\s*|\s*{})"#,
                        &quote(&options.tag_block.start),
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.start),
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.end),
                        &quote(&options.tag_block.end)
                    )
                ).ok().expect("Failed to init lex_raw_data")
            },
            lex_verbatim_data: {
                Regex::new(
                    &format!(
                        r#"(?s)({}{}|{})\s*(?:endverbatim)\s*(?:{}{}\s*|\s*{})"#,
                        &quote(&options.tag_block.start),
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.start),
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.end),
                        &quote(&options.tag_block.end)
                    )
                ).ok().expect("Failed to init lex_verbatim_data")
            },
            lex_operator: Lexer::get_operator_regex(
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
                        r#"\A(?s)\s*(raw|verbatim)\s*(?:{}{}\s*|\s*{})"#,
                        &quote(&options.whitespace_trim),
                        &quote(&options.tag_block.end),
                        &quote(&options.tag_block.end)
                    )
                ).ok().expect("Failed to init lex_block_raw")
            },
            lex_block_line: {
                Regex::new(
                    &format!(
                        r#"\A(?s)\s*line\s+(\d+)\s*{}"#,
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
                        r#"\A{}\s*"#,
                        &quote(&options.interpolation.start)
                    )
                ).ok().expect("Failed to init interpolation_start")
            },
            interpolation_end: {
                Regex::new(
                    &format!(
                        r#"\A\s*{}"#,
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
                    "{}{}",
                    quote(operator),
                    r#"[\s()]"#
                ),
                _ => format!(
                    "{}",
                    quote(operator)
                ),
            };

            r = r.replace(" ", "\\s+");

            regex_items.push(r);
        }

        let regex_string = format!("\\A(?:{})", &regex_items.connect("|"));

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

#[cfg(test)]
mod test {
    use super::*;
    use token::*;
    use error::Result;
    use lexer::iter::Iter;
    use std::iter::repeat;
    use environment::Environment;

    #[test]
    fn name_label_for_tag() {
        let template = "{% § %}";
        let lexer = Lexer::default(&Environment::default());
        let mut stream = lexer.tokens(template);

        stream = expect(stream, Value::BlockStart);
        stream = expect(stream, Value::Name("§"));
    }

    #[test]
    fn test_name_label_for_function() {
        let template = "{{ §() }}";
        let lexer = Lexer::default(&Environment::default());
        let mut stream = lexer.tokens(template);

        stream = expect(stream, Value::VarStart);
        stream = expect(stream, Value::Name("§"));
    }

    #[test]
    fn test_brackets_nesting() {
        let template = r#"{{ {"a":{"b":"c"}} }}"#;

        assert_eq!(2, count_token(template, Value::Punctuation('{')));
        assert_eq!(2, count_token(template, Value::Punctuation('}')));
    }

    #[test]
    fn test_line_directive() {
        let template = [
            "foo",
            "bar",
            "{% line 10 %}",
            "{{",
            "baz",
            "}}",
        ].connect("\n");

        let lexer = Lexer::default(&Environment::default());
        let mut stream = lexer.tokens(&template);

        // foo\nbar\n
        stream = expect_with_line(stream, Value::Text("foo\nbar\n"), 1);
        // \n (after {% line %})
        stream = expect_with_line(stream, Value::Text("\n"), 10);
        // {{
        stream = expect_with_line(stream, Value::VarStart, 11);
        // baz
        // TODO: in twig tests the value on right is 12, but our iterator works in different way
        stream = expect_with_line(stream, Value::Name("baz"), 11);
    }

    #[test]
    fn test_long_comments() {
        let template = [
            "{# ",
            &*repeat("*").take(100000).collect::<String>(),
            " #}",
        ].concat();

        let lexer = Lexer::default(&Environment::default());
        let mut stream = lexer.tokens(&template);

        expect(stream, Value::Eof);
    }

    #[test]
    fn test_raw() {
        let template = [
            "{% raw %}aaa{% endraw %}",
        ].concat();

        let lexer = Lexer::default(&Environment::default());
        let mut stream = lexer.tokens(&template);

        expect(stream, Value::Text("aaa"));
    }

    #[test]
    fn test_raw_trim() {
        let template = [
            "{% raw %}aaa  {%- endraw %}",
        ].concat();

        let lexer = Lexer::default(&Environment::default());
        let mut stream = lexer.tokens(&template);

        expect(stream, Value::Text("aaa"));
    }

    #[test]
    fn test_verbatim() {
        let template = [
            "{% verbatim %}bbb{% endverbatim %}",
        ].concat();

        let lexer = Lexer::default(&Environment::default());
        let mut stream = lexer.tokens(&template);

        expect(stream, Value::Text("bbb"));
    }

    #[test]
    fn test_long_raw() {
        let text = &*repeat("*").take(100000).collect::<String>();

        let template = [
            "{% raw %}",
            text,
            "{% endraw %}",
        ].concat();

        let lexer = Lexer::default(&Environment::default());
        let mut stream = lexer.tokens(&template);

        expect(stream, Value::Text(text));
    }

    fn count_token(template: &'static str, token_value: Value) -> u32 {
        let lexer = Lexer::default(&Environment::default());
        let mut count = 0;

        for maybe_token in lexer.tokens(template) {
            if let Ok(token) = maybe_token {
                if token.value == token_value {
                    count += 1;
                }
            }
        }

        count
    }

    fn expect_with_line<'i, 'c>(mut stream: Iter<'i, 'c>, token_value: Value, line_num: usize) -> Iter<'i, 'c> {
        let maybe_token = stream.next();
        let token = assert_token_value(maybe_token, token_value);
        assert_eq!(token.line_num, line_num);
        stream
    }

    fn expect<'i, 'c>(mut stream: Iter<'i, 'c>, token_value: Value) -> Iter<'i, 'c> {
        let maybe_token = stream.next();
        assert_token_value(maybe_token, token_value);
        stream
    }

    fn assert_token_value<'c>(maybe_token: Option<Result<Token<'c>>>, token_value: Value<'c>) -> Token<'c> {
        match maybe_token {
            Some(Ok(token)) => if token.value != token_value {
                panic!("expected token {:?} but received {:?}", token_value, token.value);
            } else {
                token
            },
            Some(Err(e)) => panic!("expected token {:?} but received error {:?}", token_value, e),
            None => panic!("expected token {:?} but received end of stream", token_value),
        }
    }
}
