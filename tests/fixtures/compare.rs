//! The assertions of `doIntegrationTest()` (TWIG/src/Test/IntegrationTestCase.php, `reference/twig/test-suite.md`
//! §2.3-§2.5) and PHPUnit 9.6's `assertStringMatchesFormat()`.

use regex::bytes::Regex;

/// PHP's `trim()` default characters: `" \t\n\r\0\x0B"`.
const PHP_TRIM: &[u8] = b" \t\n\r\0\x0B";

/// What the harness trims rendered output and `--EXPECT--` text with: `trim($s, "\n ")`.
const OUTPUT_TRIM: &[u8] = b"\n ";

fn trim<'a>(s: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let start = s.iter().position(|b| !chars.contains(b)).unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|b| !chars.contains(b))
        .map_or(start, |i| i + 1);
    &s[start..end]
}

/// PHP's `trim($s)`.
pub fn php_trim(s: &[u8]) -> &[u8] {
    trim(s, PHP_TRIM)
}

/// `trim($s, "\n ")`.
pub fn trim_output(s: &[u8]) -> &[u8] {
    trim(s, OUTPUT_TRIM)
}

/// `sprintf('%s: %s', $e::class, $e->getMessage())`.
pub fn exception_string(class: &str, message: &str) -> String {
    format!("{class}: {message}")
}

/// An exception thrown by `$twig->load()` when the fixture expects one:
/// `assertSame(trim($exception), trim(sprintf('%s: %s', $e::class, $message)))`, then the message must end with
/// `.` or `?`.
///
/// # Errors
///
/// PHPUnit's failure message.
pub fn check_load_exception(expected: &[u8], class: &str, message: &str) -> Result<(), String> {
    let actual = exception_string(class, message);
    if php_trim(expected) != php_trim(actual.as_bytes()) {
        return Err(format!(
            "Failed asserting that two strings are identical.\n{}",
            diff(php_trim(expected), php_trim(actual.as_bytes()))
        ));
    }
    // `$last = substr($message, strlen($message) - 1)`
    match message.as_bytes().last() {
        Some(b'.' | b'?') => Ok(()),
        _ => Err(
            "Exception message must end with a dot or a question mark.\nFailed asserting that false is true."
                .to_owned(),
        ),
    }
}

/// An exception thrown while rendering when the fixture expects one:
/// `assertStringMatchesFormat(trim($exception), trim(sprintf('%s: %s', $e::class, $e->getMessage())))`.
///
/// # Errors
///
/// PHPUnit's failure message.
pub fn check_render_exception(expected: &[u8], class: &str, message: &str) -> Result<(), String> {
    let actual = exception_string(class, message);
    if string_matches_format(php_trim(expected), php_trim(actual.as_bytes())) {
        Ok(())
    } else {
        Err(format!(
            "Failed asserting that string matches format description.\n{}",
            diff(php_trim(expected), php_trim(actual.as_bytes()))
        ))
    }
}

/// No exception although the fixture expects one: `[$class] = explode(':', $exception)` and PHPUnit's exception
/// constraint fails.
pub fn missing_exception(expected: &[u8]) -> String {
    let class = expected.split(|&b| b == b':').next().unwrap_or_default();
    format!(
        "Failed asserting that exception of type \"{}\" is thrown.",
        String::from_utf8_lossy(class)
    )
}

/// The output comparison: `assertEquals(trim($expect, "\n "), $output, $message.' (in '.$file.')')`, a strict string
/// comparison; `output` is already trimmed like the harness does.
///
/// # Errors
///
/// PHPUnit's failure message.
pub fn check_output(expect: &[u8], output: &[u8], message: &str, file: &str) -> Result<(), String> {
    let expected = trim_output(expect);
    if expected == output {
        Ok(())
    } else {
        Err(format!(
            "{message} (in {file})\nFailed asserting that two strings are equal.\n{}",
            diff(expected, output)
        ))
    }
}

/// The deprecation comparison: `assertSame($deprecation, implode("\n", $deprecations))`.
///
/// # Errors
///
/// PHPUnit's failure message.
pub fn check_deprecations(expected: &[u8], deprecations: &[String]) -> Result<(), String> {
    let actual = deprecations.join("\n");
    if expected == actual.as_bytes() {
        Ok(())
    } else {
        Err(format!(
            "Failed asserting that two strings are identical.\n{}",
            diff(expected, actual.as_bytes())
        ))
    }
}

/// PHPUnit 9.6 `StringMatchesFormatDescription::matches()`: both strings get `"\r\n"` replaced by `"\n"`; the format
/// is `preg_quote()`d, then `strtr()` turns the placeholders `%%`, `%e`, `%s`, `%S`, `%a`, `%A`, `%w`, `%i`, `%d`,
/// `%x`, `%f`, `%c` into patterns, and the result is matched as `/^...$/s` (no `u` flag: bytes, ASCII classes; `$`
/// also matches before a final newline).
pub fn string_matches_format(format: &[u8], string: &[u8]) -> bool {
    let format = crlf_to_lf(format);
    let string = crlf_to_lf(string);
    let mut pattern = String::from("(?s-u)^");
    let mut i = 0;
    while i < format.len() {
        // preg_quote() never escapes `%` or the placeholder letters, so a placeholder in the quoted format is a
        // placeholder in the original one; strtr() replaces left to right without overlaps
        if format[i] == b'%'
            && let Some(replacement) = format.get(i + 1).and_then(|&c| placeholder(c))
        {
            pattern.push_str(replacement);
            i += 2;
            continue;
        }
        pattern.push_str(&format!("\\x{:02X}", format[i]));
        i += 1;
    }
    pattern.push_str("\\n?\\z");
    Regex::new(&pattern)
        .expect("an escaped format is a valid pattern")
        .is_match(&string)
}

/// The pattern of a format placeholder (`%e` is `\/`: `DIRECTORY_SEPARATOR` on the target's Linux).
fn placeholder(c: u8) -> Option<&'static str> {
    Some(match c {
        b'%' => "%",
        b'e' => "/",
        b's' => r"[^\r\n]+",
        b'S' => r"[^\r\n]*",
        b'a' => ".+",
        b'A' => ".*",
        b'w' => r"\s*",
        b'i' => r"[+-]?\d+",
        b'd' => r"\d+",
        b'x' => "[0-9a-fA-F]+",
        b'f' => r"[+-]?\.?\d+\.?\d*(?:[Ee][+-]?\d+)?",
        b'c' => ".",
        _ => return None,
    })
}

fn crlf_to_lf(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\r' && s.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// A compact expected/actual report.
pub fn diff(expected: &[u8], actual: &[u8]) -> String {
    format!(
        "--- Expected\n+++ Actual\n-{:?}\n+{:?}",
        String::from_utf8_lossy(expected),
        String::from_utf8_lossy(actual)
    )
}
