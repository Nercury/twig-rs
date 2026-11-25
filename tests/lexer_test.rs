//! Port of Twig 3.31.1's `tests/LexerTest.php` (TWIG/tests/LexerTest.php). Every PHPUnit test has a counterpart
//! with the same name in snake case; the two tests that go through the parser keep their lexer-level part (the
//! `TokenStream::expect()` call that raises the error).

use twig::deprecation::{Deprecation, IgnoreDeprecations};
use twig::lexer::{
    DEFAULT_OPERATORS, Lexer, LexerOptions, Token, TokenStream, TokenType, TokenValue,
};
use twig::{Error, Source};

fn tokenize(template: &str) -> TokenStream {
    tokenize_bytes(template.as_bytes())
}

fn tokenize_bytes(template: &[u8]) -> TokenStream {
    Lexer::default()
        .tokenize(&Source::new(template, "index"), &mut IgnoreDeprecations)
        .unwrap_or_else(|error| panic!("{template:?} failed to tokenize: {error}"))
}

fn tokenize_error(template: &str) -> Error {
    match Lexer::default().tokenize(&Source::new(template, "index"), &mut IgnoreDeprecations) {
        Ok(stream) => panic!("{template:?} tokenized without error:\n{stream}"),
        Err(error) => error,
    }
}

fn expect(stream: &mut TokenStream, ty: TokenType) -> Token {
    stream
        .expect(ty, None, None)
        .unwrap_or_else(|error| panic!("{error}"))
}

fn expect_value(stream: &mut TokenStream, ty: TokenType, value: &str) -> Token {
    stream
        .expect(ty, Some(value), None)
        .unwrap_or_else(|error| panic!("{error}"))
}

fn bytes(token: &Token) -> &[u8] {
    token.value().as_bytes().expect("a string token")
}

fn count_token(template: &str, ty: TokenType, value: Option<&str>) -> usize {
    let mut stream = tokenize(template);
    let mut count = 0;
    while !stream.is_eof() {
        let token = stream.next().unwrap();
        if token.test(ty)
            && value.is_none_or(|value| token.value().as_bytes() == Some(value.as_bytes()))
        {
            count += 1;
        }
    }
    count
}

#[test]
fn test_name_label_for_tag() {
    let mut stream = tokenize("{% § %}");
    expect(&mut stream, TokenType::BlockStart);
    assert_eq!(bytes(&expect(&mut stream, TokenType::Name)), "§".as_bytes());
}

#[test]
fn test_name_label_for_function() {
    let mut stream = tokenize("{{ §() }}");
    expect(&mut stream, TokenType::VarStart);
    assert_eq!(bytes(&expect(&mut stream, TokenType::Name)), "§".as_bytes());
}

#[test]
fn test_brackets_nesting() {
    let template = r#"{{ {"a":{"b":"c"}} }}"#;
    assert_eq!(count_token(template, TokenType::Punctuation, Some("{")), 2);
    assert_eq!(count_token(template, TokenType::Punctuation, Some("}")), 2);
}

#[test]
fn test_line_directive() {
    let template = "foo\nbar\n{% line 10 %}\n{{\nbaz\n}}\n";
    let mut stream = tokenize(template);
    // foo\nbar\n
    assert_eq!(expect(&mut stream, TokenType::Text).line(), 1);
    // \n (after {% line %})
    assert_eq!(expect(&mut stream, TokenType::Text).line(), 10);
    // {{
    assert_eq!(expect(&mut stream, TokenType::VarStart).line(), 11);
    // baz
    assert_eq!(expect(&mut stream, TokenType::Name).line(), 12);
}

#[test]
fn test_line_directive_inline() {
    let template = "foo\nbar{% line 10 %}{{\nbaz\n}}\n";
    let mut stream = tokenize(template);
    // foo\nbar
    assert_eq!(expect(&mut stream, TokenType::Text).line(), 1);
    // {{
    assert_eq!(expect(&mut stream, TokenType::VarStart).line(), 10);
    // baz
    assert_eq!(expect(&mut stream, TokenType::Name).line(), 11);
}

#[test]
fn test_long_comments() {
    tokenize(&format!("{{# {} #}}", "*".repeat(100_000)));
}

#[test]
fn test_long_verbatim() {
    tokenize(&format!(
        "{{% verbatim %}}{}{{% endverbatim %}}",
        "*".repeat(100_000)
    ));
}

#[test]
fn test_long_var() {
    tokenize(&format!("{{{{ {} }}}}", "x".repeat(100_000)));
}

#[test]
fn test_long_block() {
    tokenize(&format!("{{% {} %}}", "x".repeat(100_000)));
}

#[test]
fn test_big_numbers() {
    let mut stream = tokenize("{{ 922337203685477580700 }}");
    stream.next().unwrap();
    let node = stream.next().unwrap();
    // assertEquals('922337203685477580700', $value): the value is the float 9.2233720368547758E+20.
    assert_eq!(
        node.value(),
        &TokenValue::Float(922_337_203_685_477_580_700.0)
    );
}

#[test]
fn test_string_with_escaped_delimiter() {
    let cases: &[(&str, &[u8])] = &[
        (r"{{ '\x6' }}", b"\x06"),
        (r"{{ '\065\x64' }}", b"\x35\x64"),
        // octal escape overflowing a single byte is constrained with % 256
        (r"{{ '\777' }}", b"\xff"),
        // octal escape exactly equal to 256 wraps around to a NUL byte
        (r"{{ '\400' }}", b"\x00"),
        (r"{{ 'App\\Test' }}", br"App\Test"),
        (r#"{{ "App\#{var}" }}"#, b"App#{var}"),
        (r"{{ 'foo \' bar' }}", b"foo ' bar"),
        (r#"{{ "foo \" bar" }}"#, b"foo \" bar"),
        (r"{{ '\f\n\r\t\v' }}", b"\x0c\n\r\t\x0b"),
        (r"{{ '\\f\\n\\r\\t\\v' }}", br"\f\n\r\t\v"),
        (r"{{ 'Ymd\\THis' }}", br"Ymd\THis"),
    ];
    for &(template, expected) in cases {
        let mut deprecations: Vec<Deprecation> = Vec::new();
        let mut stream = Lexer::default()
            .tokenize(&Source::new(template, "index"), &mut deprecations)
            .unwrap();
        expect(&mut stream, TokenType::VarStart);
        let token = expect(&mut stream, TokenType::String);
        assert_eq!(bytes(&token), expected, "{template}");
        assert!(deprecations.is_empty(), "{template}: {deprecations:?}");
    }
}

#[test]
fn test_string_with_escaped_delimiter_producing_deprecation() {
    let cases = [
        (
            r"{{ 'App\Test' }}",
            "AppTest",
            r#"Since twig/twig 3.12: Character "T" should not be escaped; the "\" character is ignored in Twig 3 but will not be in Twig 4. Please remove the extra "\" character at position 5 in "index" at line 1."#,
        ),
        (
            r#"{{ "foo \' bar" }}"#,
            "foo ' bar",
            r#"Since twig/twig 3.12: Character "'" should not be escaped; the "\" character is ignored in Twig 3 but will not be in Twig 4. Please remove the extra "\" character at position 6 in "index" at line 1."#,
        ),
        (
            r#"{{ 'foo \" bar' }}"#,
            "foo \" bar",
            r#"Since twig/twig 3.12: Character """ should not be escaped; the "\" character is ignored in Twig 3 but will not be in Twig 4. Please remove the extra "\" character at position 6 in "index" at line 1."#,
        ),
    ];
    for (template, expected, expected_deprecation) in cases {
        let mut deprecations: Vec<Deprecation> = Vec::new();
        let mut stream = Lexer::default()
            .tokenize(&Source::new(template, "index"), &mut deprecations)
            .unwrap();
        expect(&mut stream, TokenType::VarStart);
        expect_value(&mut stream, TokenType::String, expected);
        let messages: Vec<String> = deprecations.iter().map(ToString::to_string).collect();
        assert_eq!(messages, [expected_deprecation], "{template}");
    }
}

#[test]
fn test_string_with_interpolation() {
    let mut stream = tokenize(r#"foo {{ "bar #{ baz + 1 }" }}"#);
    expect_value(&mut stream, TokenType::Text, "foo ");
    expect(&mut stream, TokenType::VarStart);
    expect_value(&mut stream, TokenType::String, "bar ");
    expect(&mut stream, TokenType::InterpolationStart);
    expect_value(&mut stream, TokenType::Name, "baz");
    expect_value(&mut stream, TokenType::Operator, "+");
    expect_value(&mut stream, TokenType::Number, "1");
    expect(&mut stream, TokenType::InterpolationEnd);
    expect(&mut stream, TokenType::VarEnd);
}

#[test]
fn test_string_with_escaped_interpolation() {
    let mut stream = tokenize(r#"{{ "bar \#{baz+1}" }}"#);
    expect(&mut stream, TokenType::VarStart);
    expect_value(&mut stream, TokenType::String, "bar #{baz+1}");
    expect(&mut stream, TokenType::VarEnd);
}

#[test]
fn test_string_with_hash() {
    let mut stream = tokenize(r#"{{ "bar # baz" }}"#);
    expect(&mut stream, TokenType::VarStart);
    expect_value(&mut stream, TokenType::String, "bar # baz");
    expect(&mut stream, TokenType::VarEnd);
}

#[test]
fn test_string_with_unterminated_interpolation() {
    let error = tokenize_error(r#"{{ "bar #{x" }}"#);
    assert!(error.to_string().contains(r#"Unclosed """#), "{error}");
}

#[test]
fn test_string_with_nested_interpolations() {
    let mut stream = tokenize(r#"{{ "bar #{ "foo#{bar}" }" }}"#);
    expect(&mut stream, TokenType::VarStart);
    expect_value(&mut stream, TokenType::String, "bar ");
    expect(&mut stream, TokenType::InterpolationStart);
    expect_value(&mut stream, TokenType::String, "foo");
    expect(&mut stream, TokenType::InterpolationStart);
    expect_value(&mut stream, TokenType::Name, "bar");
    expect(&mut stream, TokenType::InterpolationEnd);
    expect(&mut stream, TokenType::InterpolationEnd);
    expect(&mut stream, TokenType::VarEnd);
}

#[test]
fn test_string_with_nested_interpolations_in_block() {
    let mut stream = tokenize(r#"{% foo "bar #{ "foo#{bar}" }" %}"#);
    expect(&mut stream, TokenType::BlockStart);
    expect_value(&mut stream, TokenType::Name, "foo");
    expect_value(&mut stream, TokenType::String, "bar ");
    expect(&mut stream, TokenType::InterpolationStart);
    expect_value(&mut stream, TokenType::String, "foo");
    expect(&mut stream, TokenType::InterpolationStart);
    expect_value(&mut stream, TokenType::Name, "bar");
    expect(&mut stream, TokenType::InterpolationEnd);
    expect(&mut stream, TokenType::InterpolationEnd);
    expect(&mut stream, TokenType::BlockEnd);
}

#[test]
fn test_operator_ending_with_a_letter_at_the_end_of_a_line() {
    let mut stream = tokenize("{{ 1 and\n0}}");
    expect(&mut stream, TokenType::VarStart);
    expect_value(&mut stream, TokenType::Number, "1");
    expect_value(&mut stream, TokenType::Operator, "and");
}

#[test]
fn test_filter_and_attribute_named_after_operator() {
    // Ensure that filters/attributes aren't mistaken for operators when their names conflict
    // (see https://github.com/twigphp/Twig/issues/4767)
    let template = "{{ 'foo'|and }}{{ 'bar' | and }}{{ foo.and }}{{ bar . and }}{{ foo and bar }}";
    let mut stream = tokenize(template);
    for value in ["foo", "bar"] {
        expect(&mut stream, TokenType::VarStart);
        expect_value(&mut stream, TokenType::String, value);
        expect_value(&mut stream, TokenType::Operator, "|");
        expect_value(&mut stream, TokenType::Name, "and");
        expect(&mut stream, TokenType::VarEnd);
    }
    for value in ["foo", "bar"] {
        expect(&mut stream, TokenType::VarStart);
        expect_value(&mut stream, TokenType::Name, value);
        expect_value(&mut stream, TokenType::Operator, ".");
        expect_value(&mut stream, TokenType::Name, "and");
        expect(&mut stream, TokenType::VarEnd);
    }
    expect(&mut stream, TokenType::VarStart);
    expect_value(&mut stream, TokenType::Name, "foo");
    expect_value(&mut stream, TokenType::Operator, "and");
    expect_value(&mut stream, TokenType::Name, "bar");
    expect(&mut stream, TokenType::VarEnd);
}

#[test]
fn test_overlapping_operator_tokens_and_aliases() {
    // `$env->getExpressionParsers()->add([unary 'custom' (alias 'shared'), unary 'custom not' (alias 'shared')])`:
    // the operator tokens as Lexer::getOperatorRegex() collects them (dumped from the oracle).
    let mut operators: Vec<&str> = DEFAULT_OPERATORS.to_vec();
    let after_prefix = operators.iter().position(|&op| op == "(").unwrap() + 1;
    operators.splice(
        after_prefix..after_prefix,
        ["custom", "shared", "custom not", "shared"],
    );
    let lexer = Lexer::new(LexerOptions::default(), &operators);

    let template =
        "{{ custom not 1 }}{{ custom 1 }}{{ shared 1 }}{{ not false }}{{ 1 not in [2] }}";
    let mut stream = lexer
        .tokenize(&Source::new(template, "index"), &mut IgnoreDeprecations)
        .unwrap();
    let mut found = Vec::new();
    while !stream.is_eof() {
        let token = stream.next().unwrap();
        // OPERATOR_TYPE tests also match punctuation tokens (backward compatibility), hence "]".
        if token.test(TokenType::Operator) {
            found.push(String::from_utf8(bytes(&token).to_vec()).unwrap());
        }
    }
    assert_eq!(
        found,
        ["custom not", "custom", "shared", "not", "not in", "[", "]"]
    );
}

#[test]
fn test_literal_is_not_an_operator() {
    // "literal" is the name of the LiteralExpressionParser but should not be treated as an operator token
    let mut stream = tokenize("{{ literal }}");
    expect(&mut stream, TokenType::VarStart);
    expect_value(&mut stream, TokenType::Name, "literal");
    expect(&mut stream, TokenType::VarEnd);
}

#[test]
fn test_unterminated_variable() {
    let error = tokenize_error("\n\n{{\n\nbar\n\n\n");
    assert!(
        error
            .to_string()
            .contains(r#"Unclosed "variable" in "index" at line 3"#),
        "{error}"
    );
}

#[test]
fn test_unterminated_block() {
    let error = tokenize_error("\n\n{%\n\nbar\n\n\n");
    assert!(
        error
            .to_string()
            .contains(r#"Unclosed "block" in "index" at line 3"#),
        "{error}"
    );
}

#[test]
fn test_overriding_syntax() {
    let template = "[# comment #]{# variable #}/# if true #/true/# endif #/";
    let options = LexerOptions {
        tag_comment: [b"[#".to_vec(), b"#]".to_vec()],
        tag_block: [b"/#".to_vec(), b"#/".to_vec()],
        tag_variable: [b"{#".to_vec(), b"#}".to_vec()],
        ..LexerOptions::default()
    };
    let lexer = Lexer::new(options, DEFAULT_OPERATORS);
    let mut stream = lexer
        .tokenize(&Source::new(template, "index"), &mut IgnoreDeprecations)
        .unwrap();
    expect(&mut stream, TokenType::VarStart);
    expect_value(&mut stream, TokenType::Name, "variable");
    expect(&mut stream, TokenType::VarEnd);
    expect(&mut stream, TokenType::BlockStart);
    expect_value(&mut stream, TokenType::Name, "if");
    expect_value(&mut stream, TokenType::Name, "true");
    expect(&mut stream, TokenType::BlockEnd);
    expect_value(&mut stream, TokenType::Text, "true");
    expect(&mut stream, TokenType::BlockStart);
    expect_value(&mut stream, TokenType::Name, "endif");
    expect(&mut stream, TokenType::BlockEnd);
}

#[test]
fn test_errors_at_the_end_of_the_stream() {
    // Twig checks that no PHP warning is emitted; here it is enough that tokenizing succeeds.
    for template in ["{{ =", "{{ .."] {
        tokenize(template);
    }
}

#[test]
fn test_strings() {
    let cases = [
        "日本では、春になると桜の花が咲きます。多くの人々は、公園や川の近くに集まり、お花見を楽しみます。桜の花びらが風に舞い、まるで雪のように見える瞬間は、とても美しいです。",
        "في العالم العربي، يُعتبر الخط العربي أحد أجمل أشكال الفن. يُستخدم الخط في تزيين المساجد والكتب والمخطوطات القديمة. يتميز الخط العربي بجماله وتناسقه، ويُعتبر رمزًا للثقافة الإسلامية.",
    ];
    for expected in cases {
        let mut stream = tokenize(&format!("{{{{ \"{expected}\" }}}}"));
        expect(&mut stream, TokenType::VarStart);
        expect_value(&mut stream, TokenType::String, expected);

        let mut stream = tokenize(&format!("{{{{ '{expected}' }}}}"));
        expect(&mut stream, TokenType::VarStart);
        expect_value(&mut stream, TokenType::String, expected);
    }
}

#[test]
fn test_inline_comment_with_hash_in_string() {
    let mut stream = tokenize(r#"{{ "me # this is NOT an inline comment" }}"#);
    expect(&mut stream, TokenType::VarStart);
    expect_value(
        &mut stream,
        TokenType::String,
        "me # this is NOT an inline comment",
    );
    expect(&mut stream, TokenType::VarEnd);
    assert!(stream.is_eof());
}

#[test]
fn test_inline_comment_for_variable() {
    let templates = [
        "{{\n            \"me\"\n            # this is an inline comment\n        }}",
        "{{\n            # this is an inline comment\n            \"me\"\n        }}",
        "{{\n            \"me\" # this is an inline comment\n        }}",
        "{{\n            # this is an inline comment\n            \"me\" # this is an inline comment\n            # this is an inline comment\n        }}",
    ];
    for template in templates {
        let mut stream = tokenize(template);
        expect(&mut stream, TokenType::VarStart);
        expect_value(&mut stream, TokenType::String, "me");
        expect(&mut stream, TokenType::VarEnd);
        assert!(stream.is_eof());
    }
}

#[test]
fn test_inline_comment_for_block() {
    let templates = [
        "{%\n            if true\n            # this is an inline comment\n        %}me{% endif %}",
        "{%\n            # this is an inline comment\n            if true\n        %}me{% endif %}",
        "{%\n            if true # this is an inline comment\n        %}me{% endif %}",
        "{%\n            # this is an inline comment\n            if true # this is an inline comment\n            # this is an inline comment\n        %}me{% endif %}",
    ];
    for template in templates {
        let mut stream = tokenize(template);
        expect(&mut stream, TokenType::BlockStart);
        expect_value(&mut stream, TokenType::Name, "if");
        expect_value(&mut stream, TokenType::Name, "true");
        expect(&mut stream, TokenType::BlockEnd);
        expect_value(&mut stream, TokenType::Text, "me");
        expect(&mut stream, TokenType::BlockStart);
        expect_value(&mut stream, TokenType::Name, "endif");
        expect(&mut stream, TokenType::BlockEnd);
        assert!(stream.is_eof());
    }
}

#[test]
fn test_inline_comment_for_comment() {
    let stream =
        tokenize("{#\n            Some regular comment # this is an inline comment\n        #}");
    assert!(stream.is_eof());
}

#[test]
fn test_unclosed_bracket_in_expression() {
    let cases = [
        ("{{ (1 + 3 }}", "("),
        (r#"{{ obj["a" }}"#, "["),
        ("{{ ({ a: 1) }}", "{"),
        ("{{ (([1]) + 3 }}", "("),
    ];
    for (template, bracket) in cases {
        let error = tokenize_error(template);
        let expected = format!(r#"Unclosed "{bracket}" in "index" at line 1."#);
        assert!(error.to_string().contains(&expected), "{template}: {error}");
    }
}

#[test]
fn test_unexpected_bracket_in_expression() {
    let cases = [
        ("{{ 1 + 3) }}", ")", 9),
        ("{{ obj] }}", "]", 7),
        ("{{ { a: 1 }}", "}", 12),
        ("{{ ([1] + 3)) }}", ")", 13),
    ];
    for (template, bracket, column) in cases {
        let error = tokenize_error(template);
        let expected = format!(r#"Unexpected "{bracket}" in "index" at line 1 column {column}."#);
        assert!(error.to_string().contains(&expected), "{template}: {error}");
    }
}

#[test]
fn test_tokens_carry_their_source_offset() {
    let mut stream = tokenize("Hello {{ name }}!");
    // [type, offset] pairs; offsets point at the start of each lexeme in the source
    let expected = [
        (TokenType::Text, 0),     // "Hello "
        (TokenType::VarStart, 6), // "{{"
        (TokenType::Name, 9),     // "name"
        (TokenType::VarEnd, 14),  // "}}"
        (TokenType::Text, 16),    // "!"
        (TokenType::Eof, 17),
    ];
    for (ty, offset) in expected {
        let token = stream.current().clone();
        assert!(token.test(ty), "Expected token \"{}\".", ty.english());
        assert_eq!(token.offset(), Some(offset));
        if !stream.is_eof() {
            stream.next().unwrap();
        }
    }
}

#[test]
fn test_offsets_allow_recovering_the_raw_expression_source() {
    let template = "Hello {{ name|upper ~ '!' }}";
    let mut stream = tokenize(template);
    expect(&mut stream, TokenType::Text);
    let start = expect(&mut stream, TokenType::VarStart).offset().unwrap();
    while !stream.test(TokenType::VarEnd) {
        stream.next().unwrap();
    }
    let end = stream.current().offset().unwrap();
    // slice the raw expression out of the original source, between "{{" and "}}"
    let raw = template[start + 2..end].trim();
    assert_eq!(raw, "name|upper ~ '!'");
}

#[test]
fn test_offsets_refer_to_the_original_source_when_line_endings_are_normalized() {
    let template = "Hello\r\n{{ name }}";
    let mut stream = tokenize(template);
    expect(&mut stream, TokenType::Text);
    let start = expect(&mut stream, TokenType::VarStart).offset().unwrap();
    assert_eq!(&template[start..start + 2], "{{");
    let name = expect(&mut stream, TokenType::Name);
    let name_offset = name.offset().unwrap();
    assert_eq!(&template[name_offset..name_offset + 4], "name");
    let end = expect(&mut stream, TokenType::VarEnd).offset().unwrap();
    assert_eq!(template[start + 2..end].trim(), "name");
}

#[test]
fn test_block_tag_delimiters_point_at_the_markers() {
    let template = "{% set x = 1 %}";
    let mut stream = tokenize(template);
    // the opening "{%" and the closing "%}" both point at the marker itself,
    // not at the whitespace the closing regex also consumes
    assert_eq!(expect(&mut stream, TokenType::BlockStart).offset(), Some(0));
    assert_eq!(&template[0..2], "{%");
    while !stream.test(TokenType::BlockEnd) {
        stream.next().unwrap();
    }
    let end = stream.current().offset().unwrap();
    assert_eq!(&template[end..end + 2], "%}");
}

#[test]
fn test_closing_delimiter_line_matches_the_marker_line() {
    // Twig parses the template; FromTokenParser fails on `$stream->expect(Token::NAME_TYPE, 'import')` at the
    // closing delimiter, which is what is reproduced here.
    let mut stream = tokenize("{% from 'forms.twig'\n  %}");
    for _ in 0..3 {
        stream.next().unwrap();
    }
    let error = stream
        .expect(TokenType::Name, Some("import"), None)
        .unwrap_err();
    assert_eq!(error.template_line(), 2);
    assert_eq!(error.template_column(), Some(3));
    assert!(
        error.to_string().ends_with("at line 2 column 3."),
        "{error}"
    );
}

#[test]
fn test_synthetic_tokens_have_no_offset() {
    assert_eq!(Token::new(TokenType::Name, "foo", 1).offset(), None);
}

#[test]
fn test_syntax_error_reports_the_column() {
    let error = tokenize_error("{{ 1 + 3) }}\n{{ ok }}");
    assert_eq!(error.template_line(), 1);
    assert_eq!(error.template_column(), Some(9));
    assert!(
        error.to_string().ends_with("at line 1 column 9."),
        "{error}"
    );
}

#[test]
fn test_syntax_error_column_uses_original_source_offsets() {
    // Twig parses the template; the parser fails on `$stream->expect(Token::VAR_END_TYPE)` after the expression
    // `1`, which is what is reproduced here.
    let mut stream = tokenize_bytes(b"x\r\n{{ 1__2 }}");
    for _ in 0..3 {
        stream.next().unwrap();
    }
    let error = stream.expect(TokenType::VarEnd, None, None).unwrap_err();
    assert_eq!(error.template_line(), 2);
    assert_eq!(error.template_column(), Some(5));
    assert!(
        error.to_string().ends_with("at line 2 column 5."),
        "{error}"
    );
}
