//! The lexer-level facts of `reference/twig/syntax.md`, one test per section: the verified probes of the §9
//! appendix whose outcome is decided by the lexer or by `TokenStream::expect()`, the §2.16 whitespace-control table
//! and the §2.17 error catalogue. Messages are copied from the reference (verified on the Twig 3.31.1 oracle); the
//! same templates are also covered by the oracle vectors of `tests/vectors/lexer.json`.

use twig::Source;
use twig::deprecation::IgnoreDeprecations;
use twig::lexer::{Lexer, TokenStream, TokenType};

fn tokenize(template: &[u8]) -> Result<TokenStream, twig::Error> {
    Lexer::default().tokenize(&Source::new(template, "index"), &mut IgnoreDeprecations)
}

/// The final message of the lexer error for `template`.
fn lexer_error(template: &[u8]) -> String {
    match tokenize(template) {
        Ok(stream) => panic!("{template:?} tokenized without error:\n{stream}"),
        Err(error) => error.to_string(),
    }
}

/// The values of the TEXT tokens of `template`.
fn texts(template: &[u8]) -> Vec<Vec<u8>> {
    let stream = tokenize(template)
        .unwrap_or_else(|error| panic!("{template:?} failed to tokenize: {error}"));
    stream
        .tokens()
        .iter()
        .filter(|token| token.ty() == TokenType::Text)
        .map(|token| token.value().as_bytes().expect("text").to_vec())
        .collect()
}

/// The message of the `TokenStream::expect()` call the parser makes after consuming `skip` tokens.
fn expect_error(
    template: &str,
    skip: usize,
    ty: TokenType,
    value: Option<&str>,
    message: Option<&str>,
) -> String {
    let mut stream = tokenize(template.as_bytes())
        .unwrap_or_else(|error| panic!("{template:?} failed to tokenize: {error}"));
    for _ in 0..skip {
        stream.next().unwrap();
    }
    match stream.expect(ty, value, message) {
        Ok(token) => panic!("{template:?}: expect() returned {token}"),
        Err(error) => error.to_string(),
    }
}

/// §9 appendix: probes that fail in the lexer.
#[test]
fn appendix_lexer_errors() {
    let cases: &[(&str, &str)] = &[
        ("{{ \"#{\" }}", "Unclosed \"\"\" in \"index\" at line 1."),
        (
            "{% verbatim %}abc",
            "Unexpected end of file: Unclosed \"verbatim\" block in \"index\" at line 1.",
        ),
        ("line1\n{# a ", "Unclosed comment in \"index\" at line 2."),
        ("{{ x ", "Unclosed \"variable\" in \"index\" at line 1."),
        ("\n{% if x\n", "Unclosed \"block\" in \"index\" at line 2."),
        (
            "{{ x }",
            "Unexpected \"}\" in \"index\" at line 1 column 6.",
        ),
        (
            "{{ $x }}",
            "Unexpected character \"$\" in \"index\" at line 1 column 4.",
        ),
        ("{{ (1 }}", "Unclosed \"(\" in \"index\" at line 1."),
    ];
    for &(template, expected) in cases {
        assert_eq!(lexer_error(template.as_bytes()), expected, "{template:?}");
    }
}

/// A template, the number of tokens the parser consumes before its `expect()` call, the call's type, value and message
/// arguments, and the final error message.
type ExpectCase = (
    &'static str,
    usize,
    TokenType,
    Option<&'static str>,
    Option<&'static str>,
    &'static str,
);

/// §9 appendix: probes that fail in a `TokenStream::expect()` call of the parser, reproduced at the token where the
/// parser makes the call.
#[test]
fn appendix_expect_errors() {
    use TokenType::{BlockEnd, Punctuation, VarEnd};
    let cases: &[ExpectCase] = &[
        (
            "{{ 1__2 }}",
            2,
            VarEnd,
            None,
            None,
            "Unexpected token \"name\" of value \"__2\" (\"end of print statement\" expected) in \"index\" at line 1 column 5.",
        ),
        (
            "{{ x",
            2,
            VarEnd,
            None,
            None,
            "Unexpected token \"end of template\" (\"end of print statement\" expected) in \"index\" at line 1 column 5.",
        ),
        (
            "{{ 'a' 'b' }}",
            2,
            VarEnd,
            None,
            None,
            "Unexpected token \"string\" of value \"b\" (\"end of print statement\" expected) in \"index\" at line 1 column 8.",
        ),
        (
            "{% line 5 %}{{ x }}\n{{ y",
            6,
            VarEnd,
            None,
            None,
            "Unexpected token \"end of template\" (\"end of print statement\" expected) in \"index\" at line 6 column 5.",
        ),
        (
            "{{ true ?.5 : 1 }}",
            4,
            VarEnd,
            None,
            None,
            "Unexpected token \"punctuation\" of value \":\" (\"end of print statement\" expected) in \"index\" at line 1 column 13.",
        ),
        (
            "{{ 1...5 }}",
            2,
            VarEnd,
            None,
            None,
            "Unexpected token \"operator\" of value \"...\" (\"end of print statement\" expected) in \"index\" at line 1 column 5.",
        ),
        (
            "{{ 1 0 }}",
            2,
            VarEnd,
            None,
            None,
            "Unexpected token \"number\" (\"end of print statement\" expected) in \"index\" at line 1 column 6.",
        ),
        (
            "{{ 1 1.5 }}",
            2,
            VarEnd,
            None,
            None,
            "Unexpected token \"number\" of value \"1.5\" (\"end of print statement\" expected) in \"index\" at line 1 column 6.",
        ),
        (
            "{{ {\"a\"} }}",
            3,
            Punctuation,
            Some(":"),
            Some("A mapping key must be followed by a colon (:)"),
            "A mapping key must be followed by a colon (:). Unexpected token \"punctuation\" of value \"}\" (\"punctuation\" expected with value \":\") in \"index\" at line 1 column 8.",
        ),
        (
            "{{ [1, 2 3] }}",
            5,
            Punctuation,
            Some(","),
            Some("A sequence element must be followed by a comma"),
            "A sequence element must be followed by a comma. Unexpected token \"number\" of value \"3\" (\"punctuation\" expected with value \",\") in \"index\" at line 1 column 10.",
        ),
        (
            "{% for x in y if x %}{% endfor %}",
            5,
            BlockEnd,
            None,
            None,
            "Unexpected token \"name\" of value \"if\" (\"end of statement block\" expected) in \"index\" at line 1 column 15.",
        ),
        (
            "{% include 'b' with {} ignore missing %}",
            6,
            BlockEnd,
            None,
            None,
            "Unexpected token \"name\" of value \"ignore\" (\"end of statement block\" expected) in \"index\" at line 1 column 24.",
        ),
        (
            "{% types a ? : 'x' %}",
            3,
            Punctuation,
            Some(":"),
            Some("A type name must be followed by a colon (:)"),
            "A type name must be followed by a colon (:). Unexpected token \"operator\" of value \"? :\" (\"punctuation\" expected with value \":\") in \"index\" at line 1 column 12.",
        ),
        // §1.2: a line of 0 suppresses " at line N" and the column.
        (
            "{% line 0 %}{{ x",
            2,
            VarEnd,
            None,
            None,
            "Unexpected token \"end of template\" (\"end of print statement\" expected) in \"index\".",
        ),
    ];
    for &(template, skip, ty, value, message, expected) in cases {
        assert_eq!(
            expect_error(template, skip, ty, value, message),
            expected,
            "{template:?}"
        );
    }
}

/// §3: `TokenStream::next()` past the EOF token, which is what an unclosed `if` produces.
#[test]
fn unexpected_end_of_template() {
    let mut stream = tokenize(b"{% if x %}\n\nabc").unwrap();
    let error = loop {
        if let Err(error) = stream.next() {
            break error;
        }
    };
    assert_eq!(
        error.to_string(),
        "Unexpected end of template in \"index\" at line 3."
    );
}

/// §9 appendix: probes whose lexer side succeeds; the TEXT tokens carry the whitespace handling.
#[test]
fn appendix_text_tokens() {
    let cases: &[(&[u8], &[&[u8]])] = &[
        (b"a  {{- 1 -}}  b", &[b"a", b"b"]),
        (b"a \n {{~ 1 ~}} \n b", &[b"a \n", b"\n b"]),
        (
            b"{% if true %}\nX{% endif %}\nY{{ 1 }}\nZ",
            &[b"X", b"Y", b"\nZ"],
        ),
        (b"{% if true %}\r\nX{% endif %}\r\nY", &[b"X", b"Y"]),
        (b"a\r\nb", &[b"a\nb"]),
        (
            b"{% verbatim %}\nx{{ y }}{% endverbatim %}\nY",
            &[b"\nx{{ y }}", b"\nY"],
        ),
        (
            b"A {%~ verbatim ~%}  x  {%~ endverbatim ~%}  \n B",
            &[b"A", b"x", b"\n B"],
        ),
        (b"\xef\xbb\xbf  {% extends 'b' %}", &[b"\xef\xbb\xbf  "]),
    ];
    for &(template, expected) in cases {
        assert_eq!(texts(template), expected, "{template:?}");
    }
}

/// §2.16: the whitespace-control table, row by row.
#[test]
fn whitespace_control_table() {
    let cases: &[(&[u8], &[&[u8]])] = &[
        // `{{-` `{%-` `{#-` `{##-`: PHP rtrim() of " \t\n\r\0\x0B" (NUL yes, \f no).
        (b"a \t\n\r\0\x0b{{- 1 }}", &[b"a"]),
        (b"a\x0c{{- 1 }}", &[b"a\x0c"]),
        (b"a \n{%- x %}", &[b"a"]),
        (b"a \n{#- c #}", &[b"a"]),
        (b"a \n{##- d ##}", &[b"a"]),
        // `{{~` `{%~` `{#~`: " \t\0\x0B" only (keeps \r and \n).
        (b"a\n \t\0\x0b{{~ 1 }}", &[b"a\n"]),
        (b"a \r\n{{~ 1 }}", &[b"a \n"]),
        (b"a\n \t{%~ x %}", &[b"a\n"]),
        (b"a\n \t{#~ c #}", &[b"a\n"]),
        // `-}}` `-%}` `-#}` `-##}`: PCRE \s* (\f yes, NUL no).
        (b"{{ 1 -}} \t\n\x0b\x0c\r b", &[b"b"]),
        (b"{{ 1 -}}\0b", &[b"\0b"]),
        (b"{% x -%}\n\n b", &[b"b"]),
        (b"{# c -#}\n\n b", &[b"b"]),
        (b"{## d -##}\n\n b", &[b"b"]),
        // `~}}` `~%}` `~#}` `~##}`: [ \t\0\x0B]* (keeps newlines).
        (b"{{ 1 ~}} \t\0\x0b\nb", &[b"\nb"]),
        (b"{% x ~%}  \n b", &[b"\n b"]),
        (b"{# c ~#}\t \nb", &[b"\nb"]),
        (b"{## d ~##}  b", &[b"b"]),
        // Plain `%}` and `#}`: exactly one following newline ("\r\n", "\r" or "\n").
        (b"{% x %}\n\nb", &[b"\nb"]),
        (b"{% x %}\r\n\r\nb", &[b"\nb"]),
        (b"{% x %}\r\rb", &[b"\nb"]),
        (b"{# c #}\n\nb", &[b"\nb"]),
        (b"{# c #}\r\nb", &[b"b"]),
        // Plain `}}`: nothing.
        (b"{{ x }}\nb", &[b"\nb"]),
        // Plain `%}` of `{% verbatim %}`, `{% endverbatim %}` and `{% line N %}`: nothing.
        (b"{% verbatim %}\nx{% endverbatim %}\nb", &[b"\nx", b"\nb"]),
        (b"{% line 5 %}\nb", &[b"\nb"]),
        // The asymmetry examples and the idioms of the section.
        (b"a\x0c{{- 1 -}}\x0cb", &[b"a\x0c", b"b"]),
        (b"a\0{{- 1 -}}\0b", &[b"a", b"\0b"]),
        (b"{%- if true -%}\n  a  \n{%- endif -%}", &[b"a"]),
        (b"a {#--#} b", &[b"a", b"b"]),
    ];
    for &(template, expected) in cases {
        assert_eq!(texts(template), expected, "{template:?}");
    }
}

/// §2.17: the lexer error catalogue, row by row.
#[test]
fn lexer_error_catalogue() {
    let cases: &[(&[u8], &str)] = &[
        // `Unclosed "%s".` for a bracket, string or interpolation still open at the end: the line where it opened.
        (b"{{ [1,\n2", "Unclosed \"[\" in \"index\" at line 1."),
        (b"{{ (\n1", "Unclosed \"(\" in \"index\" at line 1."),
        (b"{{ {a:\n1", "Unclosed \"{\" in \"index\" at line 1."),
        (b"\n{{ \"abc }}", "Unclosed \"\"\" in \"index\" at line 2."),
        (b"{{ \"#{ x", "Unclosed \"#{\" in \"index\" at line 1."),
        // `Unclosed "%s".` for "block"/"variable": whitespace runs to the end; the line of the opening tag.
        (b"\n{%\n\nx\n", "Unclosed \"block\" in \"index\" at line 2."),
        (b"{{\nx\n", "Unclosed \"variable\" in \"index\" at line 1."),
        (
            b"\n{% set a = \"#{ x ",
            "Unclosed \"variable\" in \"index\" at line 2.",
        ),
        // `Unexpected character "%s".`: the current line and the column of the byte.
        (
            b"{{ @ }}",
            "Unexpected character \"@\" in \"index\" at line 1 column 4.",
        ),
        (
            b"{{ ; }}",
            "Unexpected character \";\" in \"index\" at line 1 column 4.",
        ),
        (
            b"{{ ! }}",
            "Unexpected character \"!\" in \"index\" at line 1 column 4.",
        ),
        (
            b"{{ 'abc }}",
            "Unexpected character \"'\" in \"index\" at line 1 column 4.",
        ),
        (
            b"x\r\n{{ \xc3\xa9 + $ }}",
            "Unexpected character \"$\" in \"index\" at line 2 column 9.",
        ),
        (
            b"{{ \0 }}",
            "Unexpected character \"\0\" in \"index\" at line 1 column 4.",
        ),
        (
            b"{{ \"abc\\",
            "Unexpected character \"\\\" in \"index\" at line 1 column 8.",
        ),
        // `Unexpected end of file: Unclosed "verbatim" block.`: the line after the opening tag.
        (
            b"{%\nverbatim\n%}abc",
            "Unexpected end of file: Unclosed \"verbatim\" block in \"index\" at line 3.",
        ),
        // `Unclosed comment.`: the line of the opener.
        (b"a\n\n{# x\n", "Unclosed comment in \"index\" at line 3."),
        (b"{{ x }}{#}", "Unclosed comment in \"index\" at line 1."),
        // `Unexpected "%s".`: a closing bracket with an empty stack, with its column.
        (
            b"{{ ] }}",
            "Unexpected \"]\" in \"index\" at line 1 column 4.",
        ),
        (
            b"{{ 1 + 3) }}",
            "Unexpected \")\" in \"index\" at line 1 column 9.",
        ),
        // `Unclosed "%s".`: a closing bracket that does not match the innermost opening one, at its line.
        (b"{{ [1) }}", "Unclosed \"[\" in \"index\" at line 1."),
        (b"{{ (\n[1\n) }}", "Unclosed \"[\" in \"index\" at line 2."),
        (
            b"{{ \"#{ 1 )\" }}",
            "Unclosed \"#{\" in \"index\" at line 1.",
        ),
    ];
    for &(template, expected) in cases {
        assert_eq!(lexer_error(template), expected, "{template:?}");
    }
}
