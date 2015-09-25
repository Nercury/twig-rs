pub mod delimiters;
pub mod options;
pub mod iter;
pub mod matchers;

use CompiledEnvironment;

use lexer::matchers::Matchers;
use lexer::options::Options;
use lexer::iter::Iter;

/// Parses template file and converts it to a stream of tokens.
pub struct Lexer {
    options: Options,
    matchers: Matchers,
}

impl Lexer {
    /// Initialize default lexer with default options.
    pub fn default(env: &CompiledEnvironment) -> Lexer {
        let options = Options::default();

        Lexer {
            options: options,
            matchers: Matchers::new(
                &options,
                &env.unary_operators,
                &env.binary_operators
            ),
        }
    }

    /// Convert provided template into a token stream.
    pub fn tokens<'r, 'code>(&'r self, code: &'code str) -> Iter<'r, 'code>
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
    use CompiledEnvironment;

    #[test]
    fn name_label_for_tag() {
        let template = "{% § %}";
        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(template);

        _s = expect(_s, Value::BlockStart);
        _s = expect(_s, Value::Name("§"));
    }

    #[test]
    fn test_name_label_for_function() {
        let template = "{{ §() }}";
        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(template);

        _s = expect(_s, Value::VarStart);
        _s = expect(_s, Value::Name("§"));
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

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        // foo\nbar\n
        _s = expect_with_line(_s, Value::Text("foo\nbar\n"), 1);
        // \n (after {% line %})
        _s = expect_with_line(_s, Value::Text("\n"), 10);
        // {{
        _s = expect_with_line(_s, Value::VarStart, 11);
        // baz
        _s = expect_with_line(_s, Value::Name("baz"), 12);
    }

    #[test]
    fn test_long_comments() {
        let template = [
            "{# ",
            &*repeat("*").take(100000).collect::<String>(),
            " #}",
        ].concat();

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        expect_end(_s);
    }

    #[test]
    fn test_raw() {
        let template = [
            "{% raw %}aaa{% endraw %}",
        ].concat();

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        expect(_s, Value::Text("aaa"));
    }

    #[test]
    fn test_raw_trim() {
        let template = [
            "{% raw %}aaa  {%- endraw %}",
        ].concat();

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        expect(_s, Value::Text("aaa"));
    }

    #[test]
    fn test_verbatim() {
        let template = [
            "{% verbatim %}bbb{% endverbatim %}",
        ].concat();

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        expect(_s, Value::Text("bbb"));
    }

    #[test]
    fn test_long_raw() {
        let text = &*repeat("*").take(100000).collect::<String>();

        let template = [
            "{% raw %}",
            text,
            "{% endraw %}",
        ].concat();

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        expect(_s, Value::Text(text));
    }

    #[test]
    fn test_long_var() {
        let text = &*repeat("x").take(100000).collect::<String>();

        let template = [
            "{{ ",
            text,
            " }}",
        ].concat();

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s = expect(_s, Value::VarStart);
        _s = expect(_s, Value::Name(text));
    }

    #[test]
    fn test_long_block() {
        let text = &*repeat("x").take(100000).collect::<String>();

        let template = [
            "{% ",
            text,
            " %}",
        ].concat();

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s = expect(_s, Value::BlockStart);
        _s = expect(_s, Value::Name(text));
    }

    #[test]
    fn test_big_numbers() {
        let template = "{{ 922337203685477580700 }}";

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s.next();
        _s = expect(_s, Value::Number(TwigNumber::Big("922337203685477580700")));
    }

    #[test]
    fn test_int_numbers() {
        let template = "{{ 922337203685477 }}";

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s.next();
        _s = expect(_s, Value::Number(TwigNumber::Int(922337203685477)));
    }

    #[test]
    fn test_float_numbers() {
        let template = "{{ 92233720368547.33 }}";

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s.next();
        _s = expect(_s, Value::Number(TwigNumber::Float(92233720368547.33)));
    }

    #[test]
    fn test_string_with_escaped_delimiter() {
        let templates = [
            (r#"{{ 'foo \' bar' }}"#, r#"foo \' bar"#),
            (r#"{{ "foo \" bar" }}"#, r#"foo \" bar"#),
        ];

        let lexer = Lexer::default(&CompiledEnvironment::default());

        for &(template, expected) in &templates {
            let mut _s = lexer.tokens(&template);
            _s = expect(_s, Value::VarStart);
            _s = expect(_s, Value::String(TwigString::new(expected)));
        }
    }

    #[test]
    fn test_string_with_interpolation() {
        let template = r#"foo {{ "bar #{ baz + 1 }" }}"#;

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s = expect(_s, Value::Text("foo "));
        _s = expect(_s, Value::VarStart);
        _s = expect(_s, Value::String(TwigString::new("bar ")));
        _s = expect(_s, Value::InterpolationStart);
        _s = expect(_s, Value::Name("baz"));
        _s = expect(_s, Value::Operator("+"));
        _s = expect(_s, Value::Number(TwigNumber::Int(1)));
        _s = expect(_s, Value::InterpolationEnd);
        _s = expect(_s, Value::VarEnd);
    }

    #[test]
    fn test_string_with_escaped_interpolation() {
        let template = r#"{{ "bar \#{baz+1}" }}"#;

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s = expect(_s, Value::VarStart);
        _s = expect(_s, Value::String(TwigString::new(r#"bar \#{baz+1}"#)));
        _s = expect(_s, Value::VarEnd);
    }

    #[test]
    fn test_string_with_hash() {
        let template = r#"{{ "bar # baz" }}"#;

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s = expect(_s, Value::VarStart);
        _s = expect(_s, Value::String(TwigString::new("bar # baz")));
        _s = expect(_s, Value::VarEnd);
    }

    #[test]
    fn test_string_with_unterminated_interpolation() {
        let template = r#"{{ "bar #{x" }}"#;

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        expect_error(_s, r#"Unclosed """ at line 1"#);
    }

    #[test]
    fn test_string_with_nested_interpolations() {
        let template = r#"{{ "bar #{ "foo#{bar}" }" }}"#;

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s = expect(_s, Value::VarStart);
        _s = expect(_s, Value::String(TwigString::new(r#"bar "#)));
        _s = expect(_s, Value::InterpolationStart);
        _s = expect(_s, Value::String(TwigString::new(r#"foo"#)));
        _s = expect(_s, Value::InterpolationStart);
        _s = expect(_s, Value::Name("bar"));
        _s = expect(_s, Value::InterpolationEnd);
        _s = expect(_s, Value::InterpolationEnd);
        _s = expect(_s, Value::VarEnd);
    }

    #[test]
    fn test_string_with_nested_interpolations_in_block() {
        let template = r#"{% foo "bar #{ "foo#{bar}" }" %}"#;

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s = expect(_s, Value::BlockStart);
        _s = expect(_s, Value::Name("foo"));
        _s = expect(_s, Value::String(TwigString::new(r#"bar "#)));
        _s = expect(_s, Value::InterpolationStart);
        _s = expect(_s, Value::String(TwigString::new(r#"foo"#)));
        _s = expect(_s, Value::InterpolationStart);
        _s = expect(_s, Value::Name("bar"));
        _s = expect(_s, Value::InterpolationEnd);
        _s = expect(_s, Value::InterpolationEnd);
        _s = expect(_s, Value::BlockEnd);
    }

    #[test]
    fn test_operator_ending_with_a_letter_at_the_end_of_a_line() {
        let template = "{{ 1 and\n0}}";

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        _s = expect(_s, Value::VarStart);
        _s = expect(_s, Value::Number(TwigNumber::Int(1)));
        _s = expect(_s, Value::Operator("and"));
    }

    #[test]
    fn test_unterminated_variable() {
        let template = "

{{

bar


";

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        expect_error(_s, "Unclosed \"variable\" at line 3");
    }

    #[test]
    fn test_unterminated_block() {
        let template = "

{%

bar


";

        let lexer = Lexer::default(&CompiledEnvironment::default());
        let mut _s = lexer.tokens(&template);

        expect_error(_s, "Unclosed \"block\" at line 3");
    }

    fn count_token(template: &'static str, token_value: Value) -> u32 {
        let lexer = Lexer::default(&CompiledEnvironment::default());
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

    /// Runs iterator until it returns error and then checks if error string matches.
    fn expect_error<'i, 'c>(mut stream: Iter<'i, 'c>, text: &'i str) {
        let mut next = stream.next();
        loop {
            match next {
                None => panic!("expected error, but reached the end of token stream"),
                Some(Err(ref e)) => {
                    assert_eq!(e.get_message(), text);
                    return;
                },
                Some(Ok(_)) => next = stream.next(),
            };
        }
    }

    /// Runs iterator and expects that it is at the end.
    fn expect_end<'i, 'c>(mut stream: Iter<'i, 'c>) {
        match stream.next() {
            Some(other) => panic!("expected the stream to be at the end, but got {:?}", other),
            _ => (),
        }
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
