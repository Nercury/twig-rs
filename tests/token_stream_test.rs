//! Port of Twig 3.31.1's `tests/TokenStreamTest.php` and `tests/SourceTest.php` (TWIG/tests/), plus checks of the
//! remaining `TokenStream` methods.

use twig::Source;
use twig::lexer::{Token, TokenStream, TokenType, TokenValue};

fn tokens() -> Vec<Token> {
    let mut tokens: Vec<Token> = (1..=7)
        .map(|i| Token::new(TokenType::Text, i64::from(i), 1))
        .collect();
    tokens.push(Token::new(TokenType::Eof, 0i64, 1));
    tokens
}

#[test]
fn test_next() {
    let mut stream = TokenStream::new(tokens(), Source::new("", ""));
    let mut repr = Vec::new();
    while !stream.is_eof() {
        let token = stream.next().unwrap();
        repr.push(token.value().to_string());
    }
    assert_eq!(
        repr.join(", "),
        "1, 2, 3, 4, 5, 6, 7",
        "->next() advances the pointer and returns the current token"
    );
}

#[test]
fn test_end_of_template_next() {
    let mut stream = TokenStream::new(
        vec![Token::new(TokenType::BlockStart, 1i64, 1)],
        Source::new("", ""),
    );
    let error = loop {
        if stream.is_eof() {
            panic!("the stream has no EOF token");
        }
        if let Err(error) = stream.next() {
            break error;
        }
    };
    assert!(
        error.to_string().contains("Unexpected end of template"),
        "{error}"
    );
}

#[test]
fn test_end_of_template_look() {
    let mut stream = TokenStream::new(
        vec![Token::new(TokenType::BlockStart, 1i64, 1)],
        Source::new("", ""),
    );
    let error = loop {
        if stream.is_eof() {
            panic!("the stream has no EOF token");
        }
        if let Err(error) = stream.look(1) {
            break error;
        }
        stream.next().unwrap();
    };
    assert!(
        error.to_string().contains("Unexpected end of template"),
        "{error}"
    );
}

#[test]
fn test_get_column() {
    let source = Source::new("foo\nbarbaz\nqux", "index");

    // first line: column is the 1-based offset
    assert_eq!(source.column(0), 1);
    assert_eq!(source.column(2), 3);

    // a "\n" closes the line; the next character starts a new line at column 1
    assert_eq!(source.column(4), 1);
    assert_eq!(source.column(7), 4);

    // "\r\n" and "\r" also close the line
    assert_eq!(Source::new("foo\r\nbar", "index").column(5), 1);
    assert_eq!(Source::new("foo\rbar", "index").column(4), 1);

    // an unknown offset is a `None` offset at the call site (Twig: getColumn(-1) is null)
}

#[test]
fn source_accessors() {
    let source = Source::with_path("{{ x }}", "index", "/path/to/index");
    assert_eq!(source.code(), b"{{ x }}");
    assert_eq!(source.name(), "index");
    assert_eq!(source.path(), "/path/to/index");
    assert_eq!(Source::new("", "index").path(), "");
}

#[test]
fn next_if_and_test() {
    let mut stream = TokenStream::new(
        vec![
            Token::new(TokenType::Name, "for", 1),
            Token::new(TokenType::Punctuation, ",", 1),
            Token::new(TokenType::Eof, "", 2),
        ],
        Source::new("", "index"),
    );
    assert!(stream.test(TokenType::Name));
    assert!(stream.test_value(TokenType::Name, "for"));
    assert!(stream.test_value(TokenType::Name, &["if", "for"]));
    assert_eq!(stream.next_if_value(TokenType::Name, "if").unwrap(), None);
    assert_eq!(
        stream
            .next_if(TokenType::Name)
            .unwrap()
            .map(|t| t.value().clone()),
        Some(TokenValue::from("for"))
    );
    assert_eq!(stream.look(1).unwrap().ty(), TokenType::Eof);
    assert!(
        stream
            .next_if_value(TokenType::Punctuation, ",")
            .unwrap()
            .is_some()
    );
    assert!(stream.is_eof());
    // nextIf() on the last token passes the test and then fails to advance, like Twig.
    let error = stream.next_if(TokenType::Eof).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Unexpected end of template in \"index\" at line 2."
    );
    // Like Twig, the failed next() moved past the last token: getCurrent() has no token any more.
    assert!(stream.try_current().is_none());
    assert_eq!(stream.current().ty(), TokenType::Eof);
}

#[test]
fn inject_tokens_at_the_current_position() {
    let mut stream = TokenStream::new(
        vec![
            Token::new(TokenType::Name, "a", 1),
            Token::new(TokenType::Eof, "", 1),
        ],
        Source::new("", "index"),
    );
    stream.next().unwrap();
    stream.inject_tokens([
        Token::new(TokenType::Name, "b", 1),
        Token::new(TokenType::Name, "c", 1),
    ]);
    let values: Vec<String> = stream
        .tokens()
        .iter()
        .map(|t| t.value().to_string())
        .collect();
    assert_eq!(values, ["a", "b", "c", ""]);
    assert!(stream.test_value(TokenType::Name, "b"));
}

#[test]
fn expect_reports_synthesized_tokens_without_column() {
    let mut stream = TokenStream::new(
        vec![
            Token::new(TokenType::Name, "extends", 3),
            Token::new(TokenType::Eof, "", 3),
        ],
        Source::new("", "index"),
    );
    let error = stream.expect(TokenType::String, None, None).unwrap_err();
    assert_eq!(error.template_column(), None);
    assert_eq!(
        error.to_string(),
        "Unexpected token \"name\" of value \"extends\" (\"string\" expected) in \"index\" at line 3."
    );
}

#[test]
fn displays_like_twig() {
    let stream = TokenStream::new(
        vec![
            Token::new(TokenType::VarStart, "", 1),
            Token::new(TokenType::Number, 1.5, 1),
            Token::new(TokenType::Eof, "", 1),
        ],
        Source::new("", "index"),
    );
    assert_eq!(
        stream.to_string(),
        "VAR_START_TYPE()\nNUMBER_TYPE(1.5)\nEOF_TYPE()"
    );
}

#[test]
fn empty_streams_get_an_eof_token() {
    let stream = TokenStream::new(Vec::new(), Source::new("", "index"));
    assert!(stream.is_eof());
}

#[test]
fn matches_name_like_preg_match_regex_name() {
    use twig::lexer::matches_name;
    assert!(matches_name(b"and"));
    assert!(matches_name(b"not in"));
    assert!(matches_name(b"b-and"));
    assert!(matches_name(b"_x"));
    assert!(matches_name(b"\x7f"));
    assert!(matches_name("é".as_bytes()));
    assert!(!matches_name(b"=="));
    assert!(!matches_name(b"1a"));
    assert!(!matches_name(b""));
}

#[test]
fn match_name_returns_the_leading_name() {
    use twig::lexer::match_name;
    assert_eq!(match_name(b"and"), Some(&b"and"[..]));
    assert_eq!(match_name(b"not in"), Some(&b"not"[..]));
    assert_eq!(match_name(b"b-and"), Some(&b"b"[..]));
    assert_eq!(match_name("é x".as_bytes()), Some("é".as_bytes()));
    assert_eq!(match_name(b"..."), None);
}

#[test]
fn look_past_the_end_fails() {
    let stream = TokenStream::new(
        vec![
            Token::new(TokenType::Name, "a", 1),
            Token::new(TokenType::Eof, "", 2),
        ],
        Source::new("", "index"),
    );
    assert_eq!(stream.look(0).unwrap().ty(), TokenType::Name);
    assert_eq!(stream.look(1).unwrap().ty(), TokenType::Eof);
    let error = stream.look(2).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Unexpected end of template in \"index\" at line 2."
    );
    assert_eq!(stream.look(usize::MAX).unwrap_err().template_line(), 2);
}
