//! The built-in escaping strategies on strings: the `switch` of `EscaperRuntime::escape()`
//! (TWIG/src/Runtime/EscaperRuntime.php) with the charset conversions through `iconv()`, which an [`IconvBackend`]
//! implements (an environment's is [`Environment::iconv_backend`](crate::runtime::Environment::iconv_backend)).

use std::fmt;

use twig_value::builtins::string::IconvBackend;
use twig_value::builtins::string::html::{
    self as php_html, ENT_QUOTES, ENT_SUBSTITUTE, HtmlCharset,
};
use twig_value::builtins::string::url::url_encode_bytes;
use twig_value::{Diagnostics, PhpError, PhpString};

use super::Strategy;

/// The message of the `RuntimeError` the `js`, `css`, `html_attr` and `html_attr_relaxed` strategies raise for a
/// string that is not valid UTF-8 (after the conversion from the charset).
pub const INVALID_UTF8_MESSAGE: &str = "The string to escape is not a valid UTF-8 string.";

/// Why an escaping strategy failed.
#[derive(Debug, Clone)]
pub enum EscapeError {
    /// Twig's `RuntimeError` with [`INVALID_UTF8_MESSAGE`].
    InvalidUtf8,
    /// A PHP exception: a warning or notice (of `iconv()` or `htmlspecialchars()`) that the diagnostics sink turned
    /// into an exception, or the `Error` of a charset `iconv()` does not support in twig-rs.
    Php(PhpError),
}

impl From<PhpError> for EscapeError {
    fn from(error: PhpError) -> Self {
        EscapeError::Php(error)
    }
}

impl fmt::Display for EscapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EscapeError::InvalidUtf8 => f.write_str(INVALID_UTF8_MESSAGE),
            EscapeError::Php(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EscapeError {}

/// The result of an escaping strategy: the escaped string, or `None` for PHP's `false` (a failed `iconv()`, after
/// its warning or notice).
pub type EscapeResult = Result<Option<PhpString>, EscapeError>;

/// `ENT_QUOTES | ENT_SUBSTITUTE`, the flags of the `html` strategy.
const HTML_FLAGS: i64 = ENT_QUOTES | ENT_SUBSTITUTE;

/// The charsets Twig passes to `htmlspecialchars()` itself (also matched after `strtoupper()`); other charsets go
/// through UTF-8 with `iconv()`. Twig: the static `$htmlspecialcharsCharsets` of `EscaperRuntime::escape()`.
const HTMLSPECIALCHARS_CHARSETS: [&[u8]; 32] = [
    b"ISO-8859-1",
    b"ISO8859-1",
    b"ISO-8859-15",
    b"ISO8859-15",
    b"utf-8",
    b"UTF-8",
    b"CP866",
    b"IBM866",
    b"866",
    b"CP1251",
    b"WINDOWS-1251",
    b"WIN-1251",
    b"1251",
    b"CP1252",
    b"WINDOWS-1252",
    b"1252",
    b"KOI8-R",
    b"KOI8-RU",
    b"KOI8R",
    b"BIG5",
    b"950",
    b"GB2312",
    b"936",
    b"BIG5-HKSCS",
    b"SHIFT_JIS",
    b"SJIS",
    b"932",
    b"EUC-JP",
    b"EUCJP",
    b"ISO8859-5",
    b"ISO-8859-5",
    b"MACROMAN",
];

/// Escapes a string with a built-in strategy: the part of `EscaperRuntime::escape()` that follows the type checks,
/// from `if ('' === $string) return '';` on. `charset` is the charset already resolved by the caller
/// (`$charset ?: $this->charset`); a charset other than UTF-8 is converted with `iconv` (PHP's `iconv()`);
/// diagnostics of `iconv()` and `htmlspecialchars()` go to `diag`.
///
/// # Errors
///
/// See [`EscapeError`].
pub fn escape(
    string: &[u8],
    strategy: Strategy,
    charset: &[u8],
    iconv: &dyn IconvBackend,
    diag: &mut dyn Diagnostics,
) -> EscapeResult {
    if string.is_empty() {
        return Ok(Some(PhpString::empty()));
    }
    match strategy {
        Strategy::Html => escape_html(string, charset, iconv, diag),
        Strategy::Js => escape_js(string, charset, iconv, diag),
        Strategy::Css => escape_css(string, charset, iconv, diag),
        Strategy::HtmlAttr => escape_html_attr(string, charset, false, iconv, diag),
        Strategy::HtmlAttrRelaxed => escape_html_attr(string, charset, true, iconv, diag),
        Strategy::Url => Ok(Some(escape_url(string))),
    }
}

/// The `html` strategy: `htmlspecialchars($string, ENT_QUOTES | ENT_SUBSTITUTE, $charset)` for `UTF-8` and the
/// charsets of Twig's list (matched as given, then upper-cased; `htmlspecialchars()` warns about the ones it does
/// not know, such as `1251`, and assumes UTF-8), else a round trip through UTF-8 with `iconv()` (`iconv`).
///
/// # Errors
///
/// A diagnostic turned into an exception, an unsupported `iconv()` charset.
pub fn escape_html(
    string: &[u8],
    charset: &[u8],
    iconv: &dyn IconvBackend,
    diag: &mut dyn Diagnostics,
) -> EscapeResult {
    if charset == b"UTF-8" {
        return Ok(Some(php_html::escape_html(
            string,
            HTML_FLAGS,
            HtmlCharset::Utf8,
            true,
        )));
    }
    if HTMLSPECIALCHARS_CHARSETS.contains(&charset)
        || HTMLSPECIALCHARS_CHARSETS.contains(&charset.to_ascii_uppercase().as_slice())
    {
        let html_charset = php_html::determine_charset(Some(charset), diag)?;
        return Ok(Some(php_html::escape_html(
            string,
            HTML_FLAGS,
            html_charset,
            true,
        )));
    }
    // `htmlspecialchars(false)` (a failed conversion) escapes `""`
    let converted = convert_encoding(string, b"UTF-8", charset, iconv, diag)?;
    let escaped = php_html::escape_html(
        converted.as_ref().map_or(&[][..], PhpString::as_bytes),
        HTML_FLAGS,
        HtmlCharset::Utf8,
        true,
    );
    Ok(iconv.iconv_bytes(b"UTF-8", charset, escaped.as_bytes(), diag)?)
}

/// The `js` strategy: every character but `a-zA-Z0-9,._` becomes a short escape (`\\`, `\/`, `\b`, `\f`, `\n`,
/// `\r`, `\t`) or `\uHHHH` (a UTF-16 surrogate pair outside the BMP); non-UTF-8 charsets are converted to UTF-8 and
/// back with `iconv()` (`iconv`).
///
/// # Errors
///
/// [`EscapeError::InvalidUtf8`] when the (converted) string is not valid UTF-8; a diagnostic turned into an
/// exception, an unsupported `iconv()` charset.
pub fn escape_js(
    string: &[u8],
    charset: &[u8],
    iconv: &dyn IconvBackend,
    diag: &mut dyn Diagnostics,
) -> EscapeResult {
    escape_utf8(string, charset, iconv, diag, |out, ch| {
        if ch.is_ascii_alphanumeric() || matches!(ch, ',' | '.' | '_') {
            out.push(ch as u8);
            return;
        }
        // A few characters have short escape sequences in JSON and JavaScript.
        let short: &[u8] = match ch {
            '\\' => b"\\\\",
            '/' => b"\\/",
            '\x08' => b"\\b",
            '\x0C' => b"\\f",
            '\n' => b"\\n",
            '\r' => b"\\r",
            '\t' => b"\\t",
            _ => b"",
        };
        if !short.is_empty() {
            out.extend_from_slice(short);
            return;
        }
        let codepoint = u32::from(ch);
        if codepoint < 0x10000 {
            out.extend_from_slice(b"\\u");
            push_hex(out, codepoint, 4);
            return;
        }
        // Split characters outside the BMP into surrogate pairs (RFC 2781, section 2.1)
        let u = codepoint - 0x10000;
        let high = 0xD800 | (u >> 10);
        let low = 0xDC00 | (u & 0x3FF);
        out.extend_from_slice(b"\\u");
        push_hex(out, high, 4);
        out.extend_from_slice(b"\\u");
        push_hex(out, low, 4);
    })
}

/// The `css` strategy: every character but `a-zA-Z0-9` becomes `\H ` (its code point in upper-case hex followed
/// by a space); non-UTF-8 charsets are converted as for `js`.
///
/// # Errors
///
/// As [`escape_js`].
pub fn escape_css(
    string: &[u8],
    charset: &[u8],
    iconv: &dyn IconvBackend,
    diag: &mut dyn Diagnostics,
) -> EscapeResult {
    escape_utf8(string, charset, iconv, diag, |out, ch| {
        if ch.is_ascii_alphanumeric() {
            out.push(ch as u8);
            return;
        }
        out.push(b'\\');
        push_hex(out, u32::from(ch), 1);
        out.push(b' ');
    })
}

/// The `html_attr` strategy (`relaxed = false`) and the `html_attr_relaxed` strategy (`relaxed = true`): every
/// character but `a-zA-Z0-9,.-_` (and `:@[]` when relaxed) becomes `&#xFFFD;` for control characters other than
/// tab, line feed and carriage return (by first byte: `0x00-0x1F`, `0x7F-0x9F`), `&quot;`, `&amp;`, `&lt;` or `&gt;`,
/// else `&#xHH;` for other ASCII characters and `&#xHHHH;` for the rest; non-UTF-8 charsets are converted as for
/// `js`.
///
/// # Errors
///
/// As [`escape_js`].
pub fn escape_html_attr(
    string: &[u8],
    charset: &[u8],
    relaxed: bool,
    iconv: &dyn IconvBackend,
    diag: &mut dyn Diagnostics,
) -> EscapeResult {
    escape_utf8(string, charset, iconv, diag, |out, ch| {
        let kept = ch.is_ascii_alphanumeric()
            || matches!(ch, ',' | '.' | '-' | '_')
            || (relaxed && matches!(ch, ':' | '@' | '[' | ']'));
        if kept {
            out.push(ch as u8);
            return;
        }
        let mut utf8 = [0; 4];
        let encoded = ch.encode_utf8(&mut utf8).as_bytes();
        let ord = encoded[0];
        // The following replaces characters undefined in HTML with the hex entity for the Unicode replacement
        // character.
        if (ord <= 0x1F && !matches!(ch, '\t' | '\n' | '\r')) || (0x7F..=0x9F).contains(&ord) {
            out.extend_from_slice(b"&#xFFFD;");
            return;
        }
        // Check if the current character to escape has a name entity we should replace it with while grabbing the
        // hex value of the character.
        if encoded.len() == 1 {
            match ord {
                34 => out.extend_from_slice(b"&quot;"),
                38 => out.extend_from_slice(b"&amp;"),
                60 => out.extend_from_slice(b"&lt;"),
                62 => out.extend_from_slice(b"&gt;"),
                _ => {
                    out.extend_from_slice(b"&#x");
                    push_hex(out, u32::from(ord), 2);
                    out.push(b';');
                }
            }
            return;
        }
        // Per OWASP recommendations, we'll use hex entities for any other characters where a named entity does not
        // exist.
        out.extend_from_slice(b"&#x");
        push_hex(out, u32::from(ch), 4);
        out.push(b';');
    })
}

/// Appends `value` in upper-case hexadecimal with at least `width` digits (`sprintf('%0<width>X', $value)`).
fn push_hex(out: &mut Vec<u8>, value: u32, width: usize) {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let needed = (32 - value.leading_zeros() as usize).div_ceil(4);
    for i in (0..needed.max(width)).rev() {
        out.push(DIGITS[((value >> (i * 4)) & 0xF) as usize]);
    }
}

/// The `url` strategy: `rawurlencode($string)` (RFC 3986; bytes, whatever the charset).
pub fn escape_url(string: &[u8]) -> PhpString {
    url_encode_bytes(string, true)
}

/// The common frame of the `js`, `css` and `html_attr` strategies: conversion to UTF-8 (unless the charset is
/// `UTF-8`), the `preg_match('//u', $string)` check, `preg_replace_callback()` with `replace` called for every
/// character, conversion back.
fn escape_utf8(
    string: &[u8],
    charset: &[u8],
    iconv: &dyn IconvBackend,
    diag: &mut dyn Diagnostics,
    mut replace: impl FnMut(&mut Vec<u8>, char),
) -> EscapeResult {
    let is_utf8 = charset == b"UTF-8";
    let converted = if is_utf8 {
        None
    } else {
        // a failed conversion (`false`) continues as `""`
        Some(convert_encoding(string, b"UTF-8", charset, iconv, diag)?.unwrap_or_default())
    };
    let input = converted.as_ref().map_or(string, PhpString::as_bytes);
    let Ok(text) = std::str::from_utf8(input) else {
        return Err(EscapeError::InvalidUtf8);
    };
    // the escaped bytes are collected in a buffer of the thread (taken out while in use) and copied into the string
    let mut out = SCRATCH.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()));
    out.clear();
    out.reserve(text.len());
    for ch in text.chars() {
        replace(&mut out, ch);
    }
    let result = if is_utf8 {
        Ok(Some(PhpString::from_bytes(&out)))
    } else {
        iconv
            .iconv_bytes(b"UTF-8", charset, &out, diag)
            .map_err(EscapeError::from)
    };
    if out.capacity() <= SCRATCH_CAPACITY {
        SCRATCH.with(|scratch| *scratch.borrow_mut() = out);
    }
    result
}

thread_local! {
    /// The buffer [`escape_utf8`] collects escaped bytes in.
    static SCRATCH: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The largest buffer [`SCRATCH`] keeps.
const SCRATCH_CAPACITY: usize = 64 * 1024;

/// `iconv($from, $to, $string)`. Twig: `EscaperRuntime::convertEncoding()` (`iconv()` always exists here).
fn convert_encoding(
    string: &[u8],
    to: &[u8],
    from: &[u8],
    iconv: &dyn IconvBackend,
    diag: &mut dyn Diagnostics,
) -> Result<Option<PhpString>, PhpError> {
    iconv.iconv_bytes(from, to, string, diag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_digits_like_sprintf() {
        for value in [
            0,
            1,
            0xA,
            0xF,
            0x10,
            0x7F,
            0xFF,
            0x100,
            0xFFFD,
            0xFFFF,
            0x1F600,
            0x10FFFF,
            u32::MAX,
        ] {
            for (width, expected) in [
                (1, format!("{value:X}")),
                (2, format!("{value:02X}")),
                (4, format!("{value:04X}")),
            ] {
                let mut out = Vec::new();
                push_hex(&mut out, value, width);
                assert_eq!(out, expected.as_bytes(), "{value:#x} width {width}");
            }
        }
    }
}
