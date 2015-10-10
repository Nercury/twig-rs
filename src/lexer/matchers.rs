use std::collections::HashSet;
use regex::{ Regex, quote };

use lexer::options::Options;

pub struct Matchers {
    pub whitespace: Regex,
    pub regex_name: Regex,
    pub regex_number: Regex,
    pub regex_string: Regex,
    pub regex_dq_string_delim: Regex,
    pub lex_var: Regex,
    pub lex_block: Regex,
    pub lex_raw_data: Regex,
    pub lex_verbatim_data: Regex,
    pub lex_operator: Regex,
    pub lex_comment: Regex,
    pub lex_block_raw: Regex,
    pub lex_block_line: Regex,
    pub lex_tokens_start: Regex,
    pub interpolation_start: Regex,
    pub interpolation_end: Regex,
}

impl Matchers {
    pub fn new(
        options: &Options,
        operators: &HashSet<&'static str>
    ) -> Matchers {
        Matchers {
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
            regex_dq_string_delim: {
                Regex::new(
                    r#"\A""#
                ).ok().expect("Failed to init regex_dq_string_delim")
            },
            // regex_dq_string_part - no negative forward lookup in rust regex lib
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
            lex_operator: Self::get_operator_regex(
                operators
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

    /// If matches strign contents up to #{, return pos as (start, end).
    ///
    /// This is /[^#"\\]*(?:(?:\\.|#(?!\{))[^#"\\]*)*/As regular expression written
    /// manually.
    pub fn match_regex_dq_string_part(&self, code: &str) -> (usize, usize) {
        enum MatchMode {
            Normal,
            Escape,
            MaybeInterpolation(usize),
        }

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

    #[allow(deprecated)]
    fn get_operator_regex(
        operators: &HashSet<&'static str>
    ) -> Regex {
        let mut all: Vec<_> = Some("=").into_iter()
            .chain(
                operators.iter()
                    .map(|v| *v)
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
}

#[cfg(test)]
mod test_match_regex_dq_string_part {
    use std::collections::HashSet;
    use lexer::options::Options;
    use super::Matchers;

    #[test]
    fn should_match_full_str_with_first_esc_char() {
        assert_eq!((0, 2), matchers().match_regex_dq_string_part("##"))
    }

    #[test]
    fn should_match_empty_str() {
        assert_eq!((0, 0), matchers().match_regex_dq_string_part(""))
    }

    #[test]
    fn should_match_up_to_str_end() {
        assert_eq!((0, 2), matchers().match_regex_dq_string_part(r#"##"foo"#))
    }

    #[test]
    fn should_skip_escaped_str_end() {
        assert_eq!((0, 7), matchers().match_regex_dq_string_part(r#"##\"foo"#))
    }

    #[test]
    fn should_match_up_to_interpolation_start() {
        assert_eq!((0, 3), matchers().match_regex_dq_string_part(r#"aa #{ foo"#))
    }

    #[test]
    fn should_skip_escaped_interpolation_start() {
        assert_eq!((0, 10), matchers().match_regex_dq_string_part(r#"aa \#{ foo"#))
    }

    fn matchers() -> Matchers {
        let options = Options::default();
        Matchers::new(
            &options,
            &HashSet::new()
        )
    }
}
