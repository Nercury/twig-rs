//! Tests of the escaping strategies and the file-extension strategy guesser (`twig::escaper`): ports of the string
//! tables of `TWIG/tests/Runtime/EscaperRuntimeTest.php` and of `TWIG/tests/FileExtensionEscapingStrategyTest.php`,
//! and oracle vectors (`tests/vectors/escaper/strategies.json`: `EscaperRuntime::escape()` for single bytes,
//! characters, invalid UTF-8 and random strings with every strategy and many charsets; `guess()` for template names).
//! The charset conversions go through the build's default iconv backend (`twig::runtime::default_iconv_backend()`:
//! glibc's with the `glibc-iconv` feature, the clean-room one otherwise).
//!
//! The `Markup`, safe-class and custom-escaper cases of `EscaperRuntimeTest.php` belong to the `escape` filter's
//! runtime, which is built on these functions.
//!
//! Regenerate the vectors (from the repository root; Git Bash on Windows needs MSYS_NO_PATHCONV=1):
//!
//!     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/escaper/strategies.gen.php > tests/vectors/escaper/strategies.json

mod common;

use common::Json;
use twig::escaper::{
    self, EscapeError, Strategy, escape, escape_css, escape_html, escape_html_attr, escape_js,
    escape_url, guess, implied_strategies, invalid_strategy_message,
};
use twig::runtime::{BasicIconv, default_iconv_backend};
use twig_value::{Diagnostic, Ignore};

/// `(new EscaperRuntime())->escape($string, $strategy)` for a string, with the default `UTF-8` charset.
fn esc(string: &str, strategy: &str) -> String {
    let strategy = Strategy::from_name(strategy.as_bytes()).expect("a built-in strategy");
    let escaped = escape(
        string.as_bytes(),
        strategy,
        b"UTF-8",
        &BasicIconv,
        &mut Ignore,
    )
    .expect("no error")
    .expect("not false");
    String::from_utf8(escaped.as_bytes().to_vec()).expect("UTF-8 output")
}

/// `codepointToUtf8()` of the upstream test.
fn codepoint_to_utf8(codepoint: u32) -> String {
    char::from_u32(codepoint)
        .map(String::from)
        .unwrap_or_default()
}

const HTML_SPECIAL_CHARS: &[(&str, &str)] = &[
    ("'", "&#039;"),
    ("\"", "&quot;"),
    ("<", "&lt;"),
    (">", "&gt;"),
    ("&", "&amp;"),
];

const HTML_ATTR_SPECIAL_CHARS: &[(&str, &str)] = &[
    ("'", "&#x27;"),
    // Characters beyond ASCII value 255 to unicode escape
    ("Ā", "&#x0100;"),
    ("😀", "&#x1F600;"),
    // Immune chars excluded
    (",", ","),
    (".", "."),
    ("-", "-"),
    ("_", "_"),
    // Basic alnums excluded
    ("a", "a"),
    ("A", "A"),
    ("z", "z"),
    ("Z", "Z"),
    ("0", "0"),
    ("9", "9"),
    // Basic control characters and null
    ("\r", "&#x0D;"),
    ("\n", "&#x0A;"),
    ("\t", "&#x09;"),
    ("\0", "&#xFFFD;"),
    // Encode chars as named entities where possible
    ("<", "&lt;"),
    (">", "&gt;"),
    ("&", "&amp;"),
    ("\"", "&quot;"),
    // Encode spaces for quoteless attribute protection
    (" ", "&#x20;"),
];

const JS_SPECIAL_CHARS: &[(&str, &str)] = &[
    // HTML special chars - escape without exception to hex
    ("<", "\\u003C"),
    (">", "\\u003E"),
    ("'", "\\u0027"),
    ("\"", "\\u0022"),
    ("&", "\\u0026"),
    ("/", "\\/"),
    // Characters beyond ASCII value 255 to unicode escape
    ("Ā", "\\u0100"),
    ("😀", "\\uD83D\\uDE00"),
    // Immune chars excluded
    (",", ","),
    (".", "."),
    ("_", "_"),
    // Basic alnums excluded
    ("a", "a"),
    ("A", "A"),
    ("z", "z"),
    ("Z", "Z"),
    ("0", "0"),
    ("9", "9"),
    // Basic control characters and null
    ("\r", "\\r"),
    ("\n", "\\n"),
    ("\x08", "\\b"),
    ("\t", "\\t"),
    ("\x0C", "\\f"),
    ("\0", "\\u0000"),
    // Encode spaces for quoteless attribute protection
    (" ", "\\u0020"),
];

const URL_SPECIAL_CHARS: &[(&str, &str)] = &[
    // HTML special chars - escape without exception to percent encoding
    ("<", "%3C"),
    (">", "%3E"),
    ("'", "%27"),
    ("\"", "%22"),
    ("&", "%26"),
    // Characters beyond ASCII value 255 to hex sequence
    ("Ā", "%C4%80"),
    // Punctuation and unreserved check
    (",", "%2C"),
    (".", "."),
    ("_", "_"),
    ("-", "-"),
    (":", "%3A"),
    (";", "%3B"),
    ("!", "%21"),
    // Basic alnums excluded
    ("a", "a"),
    ("A", "A"),
    ("z", "z"),
    ("Z", "Z"),
    ("0", "0"),
    ("9", "9"),
    // Basic control characters and null
    ("\r", "%0D"),
    ("\n", "%0A"),
    ("\t", "%09"),
    ("\0", "%00"),
    // PHP quirks from the past
    (" ", "%20"),
    ("~", "~"),
    ("+", "%2B"),
];

const CSS_SPECIAL_CHARS: &[(&str, &str)] = &[
    // HTML special chars - escape without exception to hex
    ("<", "\\3C "),
    (">", "\\3E "),
    ("'", "\\27 "),
    ("\"", "\\22 "),
    ("&", "\\26 "),
    // Characters beyond ASCII value 255 to unicode escape
    ("Ā", "\\100 "),
    // Immune chars excluded
    (",", "\\2C "),
    (".", "\\2E "),
    ("_", "\\5F "),
    // Basic alnums excluded
    ("a", "a"),
    ("A", "A"),
    ("z", "z"),
    ("Z", "Z"),
    ("0", "0"),
    ("9", "9"),
    // Basic control characters and null
    ("\r", "\\D "),
    ("\n", "\\A "),
    ("\t", "\\9 "),
    ("\0", "\\0 "),
    // Encode spaces for quoteless attribute protection
    (" ", "\\20 "),
];

#[test]
fn html_escaping_converts_special_chars() {
    for (key, value) in HTML_SPECIAL_CHARS {
        assert_eq!(esc(key, "html"), *value, "Failed to escape: {key}");
    }
}

#[test]
fn html_attribute_escaping_converts_special_chars() {
    for (key, value) in HTML_ATTR_SPECIAL_CHARS {
        assert_eq!(esc(key, "html_attr"), *value, "Failed to escape: {key}");
    }
}

#[test]
fn html_attribute_relaxed_escaping_converts_special_chars() {
    for (key, value) in HTML_ATTR_SPECIAL_CHARS {
        assert_eq!(
            esc(key, "html_attr_relaxed"),
            *value,
            "Failed to escape: {key}"
        );
    }
}

/// Also `testJavascriptEscapingConvertsSpecialCharsWithInternalEncoding()`: twig-rs has no `mb_internal_encoding()`.
#[test]
fn javascript_escaping_converts_special_chars() {
    for (key, value) in JS_SPECIAL_CHARS {
        assert_eq!(esc(key, "js"), *value, "Failed to escape: {key}");
    }
}

#[test]
fn javascript_and_css_escaping_return_empty_and_digit_strings() {
    assert_eq!(esc("", "js"), "");
    assert_eq!(esc("123", "js"), "123");
    assert_eq!(esc("", "css"), "");
    assert_eq!(esc("123", "css"), "123");
}

#[test]
fn css_escaping_converts_special_chars() {
    for (key, value) in CSS_SPECIAL_CHARS {
        assert_eq!(esc(key, "css"), *value, "Failed to escape: {key}");
    }
}

#[test]
fn url_escaping_converts_special_chars() {
    for (key, value) in URL_SPECIAL_CHARS {
        assert_eq!(esc(key, "url"), *value, "Failed to escape: {key}");
    }
}

#[test]
fn unicode_codepoint_conversion_to_utf8() {
    let result: String = [0x20, 0x7E, 0x799]
        .into_iter()
        .map(codepoint_to_utf8)
        .collect();
    assert_eq!(result, " ~ޙ");
}

/// The OWASP range tests: alphanumerics and the immune characters are kept, every other character below 0xFF is
/// escaped.
fn check_owasp_ranges(strategy: &str, immune: &[&str]) {
    for chr in 0..0xFFu32 {
        let literal = codepoint_to_utf8(chr);
        let alnum = (0x30..=0x39).contains(&chr)
            || (0x41..=0x5A).contains(&chr)
            || (0x61..=0x7A).contains(&chr);
        if alnum || immune.contains(&literal.as_str()) {
            assert_eq!(esc(&literal, strategy), literal, "{literal:?}");
        } else {
            assert_ne!(
                esc(&literal, strategy),
                literal,
                "{literal:?} should be escaped!"
            );
        }
    }
}

#[test]
fn javascript_escaping_escapes_owasp_recommended_ranges() {
    check_owasp_ranges("js", &[",", ".", "_"]);
}

#[test]
fn html_attribute_escaping_escapes_owasp_recommended_ranges() {
    check_owasp_ranges("html_attr", &[",", ".", "-", "_"]);
}

#[test]
fn html_attribute_relaxed_escaping_escapes_owasp_recommended_ranges() {
    check_owasp_ranges(
        "html_attr_relaxed",
        &[",", ".", "-", "_", ":", "@", "[", "]"],
    );
}

#[test]
fn css_escaping_escapes_owasp_recommended_ranges() {
    check_owasp_ranges("css", &[]);
}

/// `testUnknownCustomEscaper()`: an unknown strategy is a `RuntimeError` listing the valid ones.
#[test]
fn unknown_strategies() {
    assert_eq!(Strategy::from_name(b"bar"), None);
    assert_eq!(Strategy::from_name(b"HTML"), None);
    assert_eq!(
        invalid_strategy_message(b"bar", []),
        "Invalid escaping strategy \"bar\" (valid ones: \"html\", \"js\", \"url\", \"css\", \"html_attr\", \"html_attr_relaxed\")."
    );
    for name in escaper::BUILTIN_STRATEGIES {
        assert_eq!(Strategy::from_name(name.as_bytes()).unwrap().name(), name);
    }
}

#[test]
fn implied_strategies_match_escaper_runtime() {
    assert_eq!(
        implied_strategies(b"html_attr"),
        ["html_attr_relaxed", "html"]
    );
    assert_eq!(implied_strategies(b"html_attr_relaxed"), ["html"]);
    assert!(implied_strategies(b"html").is_empty());
}

#[test]
fn invalid_utf8_is_a_runtime_error() {
    for escape_fn in [escape_js, escape_css] {
        let error = escape_fn(b"a\xFF", b"UTF-8", &BasicIconv, &mut Ignore).unwrap_err();
        assert!(matches!(error, EscapeError::InvalidUtf8));
        assert_eq!(
            error.to_string(),
            "The string to escape is not a valid UTF-8 string."
        );
    }
    assert!(matches!(
        escape_html_attr(b"\xC3", b"UTF-8", true, &BasicIconv, &mut Ignore),
        Err(EscapeError::InvalidUtf8)
    ));
    // `html` substitutes invalid sequences, `url` encodes bytes
    assert_eq!(
        escape_html(b"a\xFF", b"UTF-8", &BasicIconv, &mut Ignore)
            .unwrap()
            .unwrap()
            .as_bytes(),
        "a\u{FFFD}".as_bytes()
    );
    assert_eq!(escape_url(b"\xFF").as_bytes(), b"%FF");
}

// ---- port of TWIG/tests/FileExtensionEscapingStrategyTest.php -----------------------------------------------------

#[test]
fn guesses_strategies_from_file_names() {
    let cases: &[(Option<&str>, &str)] = &[
        // default
        (Some("html"), "foo.html"),
        (Some("html"), "foo.html.twig"),
        (Some("html"), "foo"),
        (Some("html"), "foo.bar.twig"),
        (Some("html"), "foo.txt/foo"),
        (Some("html"), "foo.txt/foo.js/"),
        // css
        (Some("css"), "foo.css"),
        (Some("css"), "foo.css.twig"),
        (Some("css"), "foo.twig.css"),
        (Some("css"), "foo.js.css"),
        (Some("css"), "foo.js.css.twig"),
        // js
        (Some("js"), "foo.js"),
        (Some("js"), "foo.js.twig"),
        (Some("js"), "foo.txt/foo.js"),
        (Some("js"), "foo.txt.twig/foo.js"),
        // txt
        (None, "foo.txt"),
        (None, "foo.txt.twig"),
    ];
    for (strategy, name) in cases {
        assert_eq!(guess(name), *strategy, "{name}");
    }
}

// ---- oracle vectors -------------------------------------------------------------------------------------------------

/// Charsets of the vectors whose `iconv()` conversions the build's default backend does not convert (it throws
/// `iconv(): conversion of charset "X" is not supported by twig-rs`, reference/deviations.md): none with the
/// `glibc-iconv` feature; without it, every charset but the Unicode ones, ASCII and ISO-8859-1.
#[cfg(feature = "glibc-iconv")]
const UNSUPPORTED_ICONV_CHARSETS: &[&str] = &[];
#[cfg(not(feature = "glibc-iconv"))]
const UNSUPPORTED_ICONV_CHARSETS: &[&str] = &[
    "CP1252",
    "windows-1252",
    "ISO-8859-15",
    "WINDOWS-1251",
    "KOI8-R",
    "koi8-ru",
    "MACROMAN",
    "CP866",
    "ISO-8859-5",
    "ISO-8859-2",
    "BIG5",
    "SJIS",
    "EUC-JP",
    "GB2312",
];

/// How many cases of the vectors the build's default backend cannot convert (see
/// [`UNSUPPORTED_ICONV_CHARSETS`]).
#[cfg(feature = "glibc-iconv")]
const UNSUPPORTED_CASES: usize = 0;
#[cfg(not(feature = "glibc-iconv"))]
const UNSUPPORTED_CASES: usize = 8603;

#[test]
fn strategy_vectors() {
    let vectors = Json::load_vectors("escaper/strategies.json");
    let iconv = default_iconv_backend();
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut unsupported = 0;
    for case in vectors.field("escape").as_array() {
        let string = case.field("s").as_bytes();
        let strategy_name = case.field("strategy").as_str().unwrap();
        let charset = case.field("charset").as_str().unwrap();
        let strategy = Strategy::from_name(strategy_name.as_bytes()).unwrap();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = escape(
            &string,
            strategy,
            charset.as_bytes(),
            &*iconv,
            &mut diagnostics,
        );
        if let Err(EscapeError::Php(error)) = &result
            && UNSUPPORTED_ICONV_CHARSETS.contains(&charset)
            && error.to_string().contains("is not supported by twig-rs")
        {
            unsupported += 1;
            continue;
        }
        checked += 1;
        let actual = match &result {
            Ok(Some(out)) => format!("out {:?}", String::from_utf8_lossy(out.as_bytes())),
            Ok(None) => "false".to_owned(),
            Err(EscapeError::InvalidUtf8) => {
                format!(
                    "Twig\\Error\\RuntimeError: {}",
                    escaper::INVALID_UTF8_MESSAGE
                )
            }
            Err(EscapeError::Php(error)) => format!("PHP error {error}"),
        };
        let expected = match (case.get("out"), case.get("error")) {
            (Some(Json::Bool(false)), _) => "false".to_owned(),
            (Some(out), _) => format!("out {:?}", String::from_utf8_lossy(&out.as_bytes())),
            (None, Some(error)) => format!(
                "{}: {}",
                error.field("class").as_str().unwrap(),
                error.field("message").as_str().unwrap()
            ),
            (None, None) => panic!("malformed case {case:?}"),
        };
        let expected_diagnostics: Vec<String> = case
            .field("diagnostics")
            .as_array()
            .iter()
            .map(Json::as_lossy_string)
            .collect();
        let actual_diagnostics: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
        if actual != expected || actual_diagnostics != expected_diagnostics {
            failures.push(format!(
                "{:?} {strategy_name} {charset}: expected {expected} {expected_diagnostics:?}, got {actual} {actual_diagnostics:?}",
                String::from_utf8_lossy(&string)
            ));
        }
    }
    assert_eq!(
        unsupported, UNSUPPORTED_CASES,
        "cases that need iconv charsets the build does not convert"
    );
    assert_eq!(checked + unsupported, 31008, "the vectors have 31008 cases");
    // the first failures of each charset and strategy
    let mut shown: Vec<String> = Vec::new();
    let mut per_kind: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for failure in &failures {
        let kind: String = failure
            .rsplit_once(": expected")
            .map_or("", |(head, _)| head)
            .rsplitn(3, ' ')
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let count = per_kind.entry(kind).or_default();
        *count += 1;
        if *count <= 3 {
            shown.push(failure.clone());
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} cases failed ({per_kind:?}):\n{}",
        failures.len(),
        shown.join("\n")
    );
}

#[test]
fn guess_vectors() {
    let vectors = Json::load_vectors("escaper/strategies.json");
    let mut checked = 0;
    for case in vectors.field("guess").as_array() {
        let name = case.field("name").as_str().unwrap();
        let expected = case.field("strategy").as_str();
        assert_eq!(guess(name), expected, "{name:?}");
        checked += 1;
    }
    assert!(checked > 40);
}
