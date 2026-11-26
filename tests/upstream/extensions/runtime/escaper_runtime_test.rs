//! Port of `TWIG/tests/Runtime/EscaperRuntimeTest.php`, through `twig::extension::runtime::EscaperRuntime` (the
//! strategy functions underneath are also covered by `tests/escaper.rs`).
//!
//! `testJavascriptEscapingConvertsSpecialCharsWithInternalEncoding` sets `mb_internal_encoding('ISO-8859-1')` first;
//! twig-rs has no internal encoding setting (the `js` strategy never depended on it), so the port checks the same
//! table. `testUnknownCustomEscaper` also compares the message (oracle: cases `unknown-escaper*` of
//! `tests/vectors/upstream/extensions/messages.gen.json`).

use std::sync::Arc;

use twig::ErrorKind;
use twig::extension::runtime::EscaperRuntime;
use twig::runtime::Markup;
use twig::value::{Ignore, PhpString, Value, convert, ops};

use crate::classes::{EXTENSION_SAFE_HTML_INTERFACE, EXTENSION_TEST_CLASS, ExtensionTestClass};
use crate::support::{check_cases, export};

/// All character encodings supported by htmlspecialchars().
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
    ("\0", "&#xFFFD;"), // should use Unicode replacement char
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

/// `(new EscaperRuntime())->escape($value, $strategy, $charset, $autoescape)`.
fn escape_with(
    runtime: &EscaperRuntime,
    value: Value,
    strategy: &str,
    charset: Option<&str>,
    autoescape: bool,
) -> Result<Value, String> {
    runtime
        .escape(
            value,
            strategy.as_bytes(),
            charset.map(str::as_bytes),
            autoescape,
            &mut Ignore,
        )
        .map_err(|e| format!("{e:?}"))
}

/// `(new EscaperRuntime())->escape($string, $strategy)` as a string.
fn escape(string: &str, strategy: &str) -> Result<String, String> {
    let escaped = escape_with(
        &EscaperRuntime::default(),
        Value::from(string),
        strategy,
        None,
        false,
    )?;
    match escaped {
        Value::String(s) => Ok(s.to_string_lossy().into_owned()),
        other => Err(format!("not a string: {}", export(&other))),
    }
}

/// `foreach ($chars as $key => $value) { $this->assertEquals($value, escape($key, $strategy), 'Failed to escape: '.$key); }`
fn check_table(test: &str, table: &[(&str, &str)], strategy: &str) {
    check_cases(
        test,
        table
            .iter()
            .map(|(key, value)| (format!("{key:?}"), (*key, *value))),
        |(key, value)| match escape(key, strategy)? {
            escaped if escaped == *value => Ok(()),
            escaped => Err(format!(
                "Failed to escape: {key:?} (got {escaped:?}, expected {value:?})"
            )),
        },
    );
}

#[test]
fn test_html_escaping_converts_special_chars() {
    check_table(
        "testHtmlEscapingConvertsSpecialChars",
        HTML_SPECIAL_CHARS,
        "html",
    );
}

#[test]
fn test_html_attribute_escaping_converts_special_chars() {
    check_table(
        "testHtmlAttributeEscapingConvertsSpecialChars",
        HTML_ATTR_SPECIAL_CHARS,
        "html_attr",
    );
}

#[test]
fn test_html_attribute_relaxed_escaping_converts_special_chars() {
    check_table(
        "testHtmlAttributeRelaxedEscapingConvertsSpecialChars",
        HTML_ATTR_SPECIAL_CHARS,
        "html_attr_relaxed",
    );
}

#[test]
fn test_javascript_escaping_converts_special_chars() {
    check_table(
        "testJavascriptEscapingConvertsSpecialChars",
        JS_SPECIAL_CHARS,
        "js",
    );
}

#[test]
fn test_javascript_escaping_converts_special_chars_with_internal_encoding() {
    check_table(
        "testJavascriptEscapingConvertsSpecialCharsWithInternalEncoding",
        JS_SPECIAL_CHARS,
        "js",
    );
}

#[test]
fn test_javascript_escaping_returns_string_if_zero_length() {
    assert_eq!(escape("", "js").unwrap(), "");
}

#[test]
fn test_javascript_escaping_returns_string_if_contains_only_digits() {
    assert_eq!(escape("123", "js").unwrap(), "123");
}

#[test]
fn test_css_escaping_converts_special_chars() {
    check_table(
        "testCssEscapingConvertsSpecialChars",
        CSS_SPECIAL_CHARS,
        "css",
    );
}

#[test]
fn test_css_escaping_returns_string_if_zero_length() {
    assert_eq!(escape("", "css").unwrap(), "");
}

#[test]
fn test_css_escaping_returns_string_if_contains_only_digits() {
    assert_eq!(escape("123", "css").unwrap(), "123");
}

#[test]
fn test_url_escaping_converts_special_chars() {
    check_table(
        "testUrlEscapingConvertsSpecialChars",
        URL_SPECIAL_CHARS,
        "url",
    );
}

/// Convert a Unicode Codepoint to a literal UTF-8 character (`codepointToUtf8()` of the upstream test).
fn codepoint_to_utf8(codepoint: u32) -> String {
    char::from_u32(codepoint)
        .map(String::from)
        .expect("Codepoint requested outside of Unicode range.")
}

#[test]
fn test_unicode_codepoint_conversion_to_utf8() {
    let expected = " ~ޙ";
    let result: String = [0x20, 0x7E, 0x799]
        .into_iter()
        .map(codepoint_to_utf8)
        .collect();
    assert_eq!(expected, result);
}

/// The OWASP range tests: alphanumerics and the immune characters stay, everything else below 0xFF is escaped.
fn check_owasp_ranges(test: &str, strategy: &str, immune: &[&str]) {
    check_cases(
        test,
        (0u32..0xFF).map(|chr| (format!("{chr:#04x}"), chr)),
        |chr| {
            let literal = codepoint_to_utf8(*chr);
            let escaped = escape(&literal, strategy)?;
            let alnum = matches!(chr, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A);
            if alnum || immune.contains(&literal.as_str()) {
                if escaped == literal {
                    Ok(())
                } else {
                    Err(format!(
                        "{literal:?} should not be escaped, got {escaped:?}"
                    ))
                }
            } else if escaped != literal {
                Ok(())
            } else {
                Err(format!("{literal:?} should be escaped!"))
            }
        },
    );
}

#[test]
fn test_javascript_escaping_escapes_owasp_recommended_ranges() {
    check_owasp_ranges(
        "testJavascriptEscapingEscapesOwaspRecommendedRanges",
        "js",
        &[",", ".", "_"],
    );
}

#[test]
fn test_html_attribute_escaping_escapes_owasp_recommended_ranges() {
    check_owasp_ranges(
        "testHtmlAttributeEscapingEscapesOwaspRecommendedRanges",
        "html_attr",
        &[",", ".", "-", "_"],
    );
}

#[test]
fn test_html_attribute_relaxed_escaping_escapes_owasp_recommended_ranges() {
    check_owasp_ranges(
        "testHtmlAttributeRelaxedEscapingEscapesOwaspRecommendedRanges",
        "html_attr_relaxed",
        &[",", ".", "-", "_", ":", "@", "[", "]"],
    );
}

#[test]
fn test_css_escaping_escapes_owasp_recommended_ranges() {
    // CSS has no exceptions to escaping ranges
    check_owasp_ranges("testCssEscapingEscapesOwaspRecommendedRanges", "css", &[]);
}

#[test]
fn test_unknown_custom_escaper() {
    let error = EscaperRuntime::default()
        .escape(Value::from("foo"), b"bar", None, false, &mut Ignore)
        .unwrap_err();
    let error = error.twig_error().expect("a Twig error");
    assert_eq!(error.kind(), ErrorKind::Runtime, "{error}");
    // the message as the oracle prints it (the upstream test only checks the class), also with a custom escaper
    assert_eq!(
        error.to_string(),
        "Invalid escaping strategy \"bar\" (valid ones: \"html\", \"js\", \"url\", \"css\", \"html_attr\", \"html_attr_relaxed\")."
    );
    let runtime = EscaperRuntime::default();
    runtime.set_escaper("foo", escaper());
    let error = runtime
        .escape(Value::from("foo"), b"bar", None, false, &mut Ignore)
        .unwrap_err();
    assert_eq!(
        error.twig_error().map(ToString::to_string).as_deref(),
        Some(
            "Invalid escaping strategy \"bar\" (valid ones: \"html\", \"js\", \"url\", \"css\", \"html_attr\", \"html_attr_relaxed\", \"foo\")."
        )
    );
}

/// `function escaper($string, $charset) { return $string.'**'.$charset; }`
fn escaper() -> twig::extension::runtime::EscaperCallback {
    Arc::new(|value, charset, diag| {
        let string = ops::concat(value, &Value::from("**"), diag)?;
        Ok(Value::String(ops::concat(
            &Value::String(string),
            &Value::String(charset.clone()),
            diag,
        )?))
    })
}

/// `[$expected, $string, $strategy, $charset]`.
type CustomEscaperCase = (&'static str, Value, &'static str, Option<&'static str>);

fn provide_custom_escaper_cases() -> Vec<(String, CustomEscaperCase)> {
    vec![
        (
            "foo**ISO-8859-1",
            Value::from("foo"),
            "foo",
            Some("ISO-8859-1"),
        ),
        ("**ISO-8859-1", Value::Null, "foo", Some("ISO-8859-1")),
        ("42**UTF-8", Value::Int(42), "foo", None),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

#[test]
fn test_custom_escaper() {
    check_cases(
        "testCustomEscaper",
        provide_custom_escaper_cases(),
        |(expected, string, strategy, charset)| {
            let escaper_runtime = EscaperRuntime::default();
            escaper_runtime.set_escaper("foo", escaper());
            let escaped = escape_with(&escaper_runtime, string.clone(), strategy, *charset, false)?;
            if escaped == Value::from(*expected) {
                Ok(())
            } else {
                Err(format!("expected {expected:?}, got {}", export(&escaped)))
            }
        },
    );
}

/// `(string) $value`.
fn string_of(value: &Value) -> String {
    convert::to_string(value, &mut Ignore)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|e| format!("<{e:?}>"))
}

/// `$escaper->escape($obj, $strategy, null, true)` as a string.
fn autoescape(runtime: &EscaperRuntime, value: Value, strategy: &str) -> Result<String, String> {
    let escaped = escape_with(runtime, value, strategy, None, true)?;
    match &escaped {
        Value::String(s) => Ok(s.to_string_lossy().into_owned()),
        other => Err(format!("not a string: {}", export(other))),
    }
}

/// `[$escapedHtml, $escapedJs, [$class => [$strategy]]]`.
type ObjectEscapingCase = (&'static str, &'static str, &'static str, &'static str);

fn provide_objects_for_escaping() -> Vec<(String, ObjectEscapingCase)> {
    vec![
        (
            "&lt;br /&gt;",
            "<br />",
            "\\Twig\\Tests\\Runtime\\Extension_TestClass",
            "js",
        ),
        (
            "<br />",
            "\\u003Cbr\\u0020\\/\\u003E",
            "\\Twig\\Tests\\Runtime\\Extension_TestClass",
            "html",
        ),
        (
            "&lt;br /&gt;",
            "<br />",
            "\\Twig\\Tests\\Runtime\\Extension_SafeHtmlInterface",
            "js",
        ),
        (
            "<br />",
            "<br />",
            "\\Twig\\Tests\\Runtime\\Extension_SafeHtmlInterface",
            "all",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

#[test]
fn test_object_escaping() {
    check_cases(
        "testObjectEscaping",
        provide_objects_for_escaping(),
        |(escaped_html, escaped_js, class, strategy)| {
            let obj = ExtensionTestClass::value();
            let escaper_runtime = EscaperRuntime::default();
            escaper_runtime.set_safe_classes([(*class, [*strategy])]);
            let html = autoescape(&escaper_runtime, obj.clone(), "html")?;
            let js = autoescape(&escaper_runtime, obj, "js")?;
            if html == *escaped_html && js == *escaped_js {
                Ok(())
            } else {
                Err(format!(
                    "html {html:?} (expected {escaped_html:?}), js {js:?} (expected {escaped_js:?})"
                ))
            }
        },
    );
}

fn provide_implied_strategies() -> Vec<(String, (bool, &'static str, &'static str))> {
    vec![
        (false, "html_attr", "html"),
        (false, "html_attr", "html_attr_relaxed"),
        (false, "html_attr_relaxed", "html"),
        (true, "html", "html_attr"),
        (true, "html", "html_attr_relaxed"),
        (true, "html_attr_relaxed", "html_attr"),
        (true, "html_attr", "js"),
    ]
    .into_iter()
    .map(|case| (format!("{} as {}", case.1, case.2), case))
    .collect()
}

#[test]
fn test_safe_classes_are_safe_for_implied_strategies() {
    check_cases(
        "testSafeClassesAreSafeForImpliedStrategies",
        provide_implied_strategies(),
        |(escaped, declared_strategy, strategy)| {
            let escaper_runtime = EscaperRuntime::default();
            escaper_runtime.add_safe_class(EXTENSION_TEST_CLASS, [*declared_strategy]);
            let output = autoescape(&escaper_runtime, ExtensionTestClass::value(), strategy)?;
            if *escaped == (output != "<br />") {
                Ok(())
            } else {
                Err(format!("got {output:?}"))
            }
        },
    );
}

#[test]
fn test_inherited_safe_classes_are_safe_for_implied_strategies() {
    let escaper_runtime = EscaperRuntime::default();
    escaper_runtime.add_safe_class(EXTENSION_SAFE_HTML_INTERFACE, ["html_attr"]);
    assert_eq!(
        autoescape(&escaper_runtime, ExtensionTestClass::value(), "html").unwrap(),
        "<br />"
    );
}

/// `new Markup('<br />', 'UTF-8'[, $safeStrategies])`.
fn markup(safe_strategies: Option<&[&str]>) -> Value {
    Value::Object(Markup::create(
        PhpString::from("<br />"),
        Some(PhpString::from("UTF-8")),
        safe_strategies.map(|s| s.iter().map(|s| PhpString::from(*s)).collect()),
    ))
}

/// `[$expected, $safeStrategies, $strategy]`.
type MarkupCase = (&'static str, Option<&'static [&'static str]>, &'static str);

fn provide_markup_safe_strategies() -> Vec<(String, MarkupCase)> {
    vec![
        ("no argument stays safe everywhere", ("<br />", None, "js")),
        ("all", ("<br />", Some(&["all"][..]), "js")),
        ("current strategy", ("<br />", Some(&["html"][..]), "html")),
        (
            "other strategy",
            ("\\u003Cbr\\u0020\\/\\u003E", Some(&["html"][..]), "js"),
        ),
        (
            "several strategies",
            ("<br />", Some(&["html", "js"][..]), "js"),
        ),
        (
            "implied strategy",
            ("<br />", Some(&["html_attr"][..]), "html"),
        ),
        ("no strategy", ("&lt;br /&gt;", Some(&[][..]), "html")),
    ]
    .into_iter()
    .map(|(name, case)| (name.to_owned(), case))
    .collect()
}

#[test]
fn test_markup_is_only_safe_for_its_own_strategies() {
    check_cases(
        "testMarkupIsOnlySafeForItsOwnStrategies",
        provide_markup_safe_strategies(),
        |(expected, safe_strategies, strategy)| {
            let escaped = escape_with(
                &EscaperRuntime::default(),
                markup(*safe_strategies),
                strategy,
                None,
                true,
            )?;
            match string_of(&escaped) {
                s if s == *expected => Ok(()),
                s => Err(format!("expected {expected:?}, got {s:?}")),
            }
        },
    );
}

#[test]
fn test_markup_strategies_take_precedence_over_safe_classes() {
    let escaper_runtime = EscaperRuntime::default();
    escaper_runtime.add_safe_class("Twig\\Markup", ["html"]);
    assert_eq!(
        autoescape(&escaper_runtime, markup(Some(&["js"])), "html").unwrap(),
        "&lt;br /&gt;"
    );
}

#[test]
fn test_markup_is_escaped_when_the_developer_calls_the_escape_filter() {
    let escaped = escape_with(
        &EscaperRuntime::default(),
        markup(Some(&["html"])),
        "html",
        None,
        false,
    )
    .unwrap();
    assert_eq!(escaped, Value::from("&lt;br /&gt;"));
}
