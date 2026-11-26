//! `htmlspecialchars()` (reference/php/stdlib.md §5.8): Twig's `html` escaping strategy and
//! autoescaping (`htmlspecialchars($s, ENT_QUOTES | ENT_SUBSTITUTE, $charset)`).
//!
//! Ports `php_escape_html_entities_ex()` with `all = 0` (PHPSRC/ext/standard/html.c) for every
//! flag combination and all fourteen charsets of `html.c`: UTF-8 with the UTR #36 "maximal
//! subpart" error strategy, the single-byte charsets (ISO-8859-1/5/15, Windows-1251/1252, CP866,
//! KOI8-R, MacRoman) and the multi-byte BIG5, BIG5-HKSCS, GB2312, Shift_JIS and EUC-JP with their
//! own lead/trail byte checks. `double_encode = false` recognises numeric entities and the named
//! entities of the document type (HTML 4.01, XHTML, XML 1.0, HTML 5).

use super::html_entities;
use super::{bool_param, long_param, str_param};
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::Value;

/// `ENT_COMPAT`: convert double quotes only.
pub const ENT_COMPAT: i64 = 2;
/// `ENT_QUOTES`: convert double and single quotes.
pub const ENT_QUOTES: i64 = 3;
/// `ENT_NOQUOTES`: leave both quote kinds alone.
pub const ENT_NOQUOTES: i64 = 0;
/// `ENT_IGNORE`: drop invalid code unit sequences.
pub const ENT_IGNORE: i64 = 4;
/// `ENT_SUBSTITUTE`: replace invalid code unit sequences with U+FFFD (`&#xFFFD;` outside UTF-8).
pub const ENT_SUBSTITUTE: i64 = 8;
/// `ENT_HTML401` document type (the default).
pub const ENT_HTML401: i64 = 0;
/// `ENT_XML1` document type.
pub const ENT_XML1: i64 = 16;
/// `ENT_XHTML` document type.
pub const ENT_XHTML: i64 = 32;
/// `ENT_HTML5` document type.
pub const ENT_HTML5: i64 = 48;
/// `ENT_DISALLOWED`: replace code points that are invalid in the document type.
pub const ENT_DISALLOWED: i64 = 128;

const QUOTE_SINGLE: i32 = 1;
const QUOTE_DOUBLE: i32 = 2;
const IGNORE_ERRORS: i32 = 4;
const SUBSTITUTE_ERRORS: i32 = 8;
const DOC_TYPE_MASK: i32 = 16 | 32;
const DOC_HTML401: i32 = 0;
const DOC_XML1: i32 = 16;
const DOC_XHTML: i32 = 32;
const DOC_HTML5: i32 = 16 | 32;
const SUBSTITUTE_DISALLOWED_CHARS: i32 = 128;

/// The charsets of `html.c` (`enum entity_charset`, PHPSRC/ext/standard/html_tables.h), in their
/// upstream order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HtmlCharset {
    /// `cs_utf_8`
    Utf8,
    /// `cs_8859_1`
    Iso8859_1,
    /// `cs_cp1252`
    Cp1252,
    /// `cs_8859_15`
    Iso8859_15,
    /// `cs_cp1251`
    Cp1251,
    /// `cs_8859_5`
    Iso8859_5,
    /// `cs_cp866`
    Cp866,
    /// `cs_macroman`
    MacRoman,
    /// `cs_koi8r`
    Koi8R,
    /// `cs_big5`
    Big5,
    /// `cs_gb2312`
    Gb2312,
    /// `cs_big5hkscs`
    Big5Hkscs,
    /// `cs_sjis`
    Sjis,
    /// `cs_eucjp`
    EucJp,
}

impl HtmlCharset {
    /// `CHARSET_UNICODE_COMPAT()`: UTF-8 and ISO-8859-1, whose code units are Unicode code points.
    fn unicode_compat(self) -> bool {
        self <= HtmlCharset::Iso8859_1
    }
}

/// `charset_map` (PHPSRC/ext/standard/html_tables.h): names matched case-insensitively.
const CHARSET_MAP: [(&str, HtmlCharset); 33] = [
    ("ISO-8859-1", HtmlCharset::Iso8859_1),
    ("ISO8859-1", HtmlCharset::Iso8859_1),
    ("ISO-8859-15", HtmlCharset::Iso8859_15),
    ("ISO8859-15", HtmlCharset::Iso8859_15),
    ("utf-8", HtmlCharset::Utf8),
    ("cp1252", HtmlCharset::Cp1252),
    ("Windows-1252", HtmlCharset::Cp1252),
    ("1252", HtmlCharset::Cp1252),
    ("BIG5", HtmlCharset::Big5),
    ("950", HtmlCharset::Big5),
    ("GB2312", HtmlCharset::Gb2312),
    ("936", HtmlCharset::Gb2312),
    ("BIG5-HKSCS", HtmlCharset::Big5Hkscs),
    ("Shift_JIS", HtmlCharset::Sjis),
    ("SJIS", HtmlCharset::Sjis),
    ("932", HtmlCharset::Sjis),
    ("SJIS-win", HtmlCharset::Sjis),
    ("CP932", HtmlCharset::Sjis),
    ("EUCJP", HtmlCharset::EucJp),
    ("EUC-JP", HtmlCharset::EucJp),
    ("eucJP-win", HtmlCharset::EucJp),
    ("KOI8-R", HtmlCharset::Koi8R),
    ("koi8-ru", HtmlCharset::Koi8R),
    ("koi8r", HtmlCharset::Koi8R),
    ("cp1251", HtmlCharset::Cp1251),
    ("Windows-1251", HtmlCharset::Cp1251),
    ("win-1251", HtmlCharset::Cp1251),
    ("iso8859-5", HtmlCharset::Iso8859_5),
    ("iso-8859-5", HtmlCharset::Iso8859_5),
    ("cp866", HtmlCharset::Cp866),
    ("866", HtmlCharset::Cp866),
    ("ibm866", HtmlCharset::Cp866),
    ("MacRoman", HtmlCharset::MacRoman),
];

/// The charset of a name of `charset_map` (case-insensitive, the whole name), if any.
pub fn charset_by_name(name: &[u8]) -> Option<HtmlCharset> {
    CHARSET_MAP
        .iter()
        .find(|(n, _)| n.as_bytes().eq_ignore_ascii_case(name))
        .map(|&(_, cs)| cs)
}

/// Port of `determine_charset()` (PHPSRC/ext/standard/html.c) with `quiet = false`: `None` or an
/// empty name select `default_charset` (`UTF-8`); the name is read up to its first NUL byte (C
/// string); unknown names warn `htmlspecialchars(): Charset "%s" is not supported, assuming UTF-8`.
pub fn determine_charset(
    hint: Option<&[u8]>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<HtmlCharset> {
    let hint = hint.map(|h| match memchr::memchr(0, h) {
        Some(nul) => &h[..nul],
        None => h,
    });
    let Some(hint) = hint.filter(|h| !h.is_empty()) else {
        return Ok(HtmlCharset::Utf8);
    };
    if let Some(cs) = charset_by_name(hint) {
        return Ok(cs);
    }
    diag.warn(PhpString::concat(&[
        b"htmlspecialchars(): Charset \"",
        hint,
        b"\" is not supported, assuming UTF-8",
    ]))?;
    Ok(HtmlCharset::Utf8)
}

fn utf8_lead(c: u8) -> bool {
    c < 0x80 || (0xC2..=0xF4).contains(&c)
}

fn utf8_trail(c: u8) -> bool {
    (0x80..=0xBF).contains(&c)
}

fn gb2312_lead(c: u8) -> bool {
    c != 0x8E && c != 0x8F && c != 0xA0 && c != 0xFF
}

fn gb2312_trail(c: u8) -> bool {
    (0xA1..=0xFE).contains(&c)
}

fn sjis_lead(c: u8) -> bool {
    c != 0x80 && c != 0xA0 && c < 0xFD
}

fn sjis_trail(c: u8) -> bool {
    c >= 0x40 && c != 0x7F && c < 0xFD
}

/// Port of `get_next_char()` (PHPSRC/ext/standard/html.c): decodes the code unit sequence at
/// `pos` (`pos < s.len()`). `Ok((char, next))`, or `Err(next)` for an invalid sequence where
/// `next` is where decoding resumes (UTR #36 strategy 2 for UTF-8).
pub fn next_char(cs: HtmlCharset, s: &[u8], pos: usize) -> Result<(u32, usize), usize> {
    let avail = s.len() - pos;
    let c = s[pos];
    let b = |i: usize| s[pos + i];
    match cs {
        HtmlCharset::Utf8 => {
            if c < 0x80 {
                Ok((u32::from(c), pos + 1))
            } else if c < 0xC2 {
                Err(pos + 1)
            } else if c < 0xE0 {
                if avail < 2 {
                    return Err(pos + 1);
                }
                if !utf8_trail(b(1)) {
                    return Err(pos + if utf8_lead(b(1)) { 1 } else { 2 });
                }
                let cp = (u32::from(c & 0x1f) << 6) | u32::from(b(1) & 0x3f);
                if cp < 0x80 {
                    return Err(pos + 2);
                }
                Ok((cp, pos + 2))
            } else if c < 0xF0 {
                if avail < 3 || !utf8_trail(b(1)) || !utf8_trail(b(2)) {
                    return Err(if avail < 2 || utf8_lead(b(1)) {
                        pos + 1
                    } else if avail < 3 || utf8_lead(b(2)) {
                        pos + 2
                    } else {
                        pos + 3
                    });
                }
                let cp = (u32::from(c & 0x0f) << 12)
                    | (u32::from(b(1) & 0x3f) << 6)
                    | u32::from(b(2) & 0x3f);
                if cp < 0x800 || (0xD800..=0xDFFF).contains(&cp) {
                    return Err(pos + 3);
                }
                Ok((cp, pos + 3))
            } else if c < 0xF5 {
                if avail < 4 || !utf8_trail(b(1)) || !utf8_trail(b(2)) || !utf8_trail(b(3)) {
                    return Err(if avail < 2 || utf8_lead(b(1)) {
                        pos + 1
                    } else if avail < 3 || utf8_lead(b(2)) {
                        pos + 2
                    } else if avail < 4 || utf8_lead(b(3)) {
                        pos + 3
                    } else {
                        pos + 4
                    });
                }
                let cp = (u32::from(c & 0x07) << 18)
                    | (u32::from(b(1) & 0x3f) << 12)
                    | (u32::from(b(2) & 0x3f) << 6)
                    | u32::from(b(3) & 0x3f);
                if !(0x10000..=0x10FFFF).contains(&cp) {
                    return Err(pos + 4);
                }
                Ok((cp, pos + 4))
            } else {
                Err(pos + 1)
            }
        }
        HtmlCharset::Big5 | HtmlCharset::Big5Hkscs => {
            if (0x81..=0xFE).contains(&c) {
                if avail < 2 {
                    return Err(pos + 1);
                }
                let next = b(1);
                if (0x40..=0x7E).contains(&next) || (0xA1..=0xFE).contains(&next) {
                    Ok(((u32::from(c) << 8) | u32::from(next), pos + 2))
                } else if cs == HtmlCharset::Big5 || (next != 0x80 && next != 0xFF) {
                    Err(pos + 1)
                } else {
                    Err(pos + 2)
                }
            } else {
                Ok((u32::from(c), pos + 1))
            }
        }
        HtmlCharset::Gb2312 => {
            if (0xA1..=0xFE).contains(&c) {
                if avail < 2 {
                    return Err(pos + 1);
                }
                let next = b(1);
                if gb2312_trail(next) {
                    Ok(((u32::from(c) << 8) | u32::from(next), pos + 2))
                } else if gb2312_lead(next) {
                    Err(pos + 1)
                } else {
                    Err(pos + 2)
                }
            } else if gb2312_lead(c) {
                Ok((u32::from(c), pos + 1))
            } else {
                Err(pos + 1)
            }
        }
        HtmlCharset::Sjis => {
            if (0x81..=0x9F).contains(&c) || (0xE0..=0xFC).contains(&c) {
                if avail < 2 {
                    return Err(pos + 1);
                }
                let next = b(1);
                if sjis_trail(next) {
                    Ok(((u32::from(c) << 8) | u32::from(next), pos + 2))
                } else if sjis_lead(next) {
                    Err(pos + 1)
                } else {
                    Err(pos + 2)
                }
            } else if c < 0x80 || (0xA1..=0xDF).contains(&c) {
                Ok((u32::from(c), pos + 1))
            } else {
                Err(pos + 1)
            }
        }
        HtmlCharset::EucJp => {
            let bad_follow = |x: u8| if x != 0xA0 && x != 0xFF { 1 } else { 2 };
            if (0xA1..=0xFE).contains(&c) {
                if avail < 2 {
                    return Err(pos + 1);
                }
                let next = b(1);
                if (0xA1..=0xFE).contains(&next) {
                    Ok(((u32::from(c) << 8) | u32::from(next), pos + 2))
                } else {
                    Err(pos + bad_follow(next))
                }
            } else if c == 0x8E {
                if avail < 2 {
                    return Err(pos + 1);
                }
                let next = b(1);
                if (0xA1..=0xDF).contains(&next) {
                    Ok(((u32::from(c) << 8) | u32::from(next), pos + 2))
                } else {
                    Err(pos + bad_follow(next))
                }
            } else if c == 0x8F {
                let in_range = |x: u8| (0xA1..=0xFE).contains(&x);
                if avail < 3 || !in_range(b(1)) || !in_range(b(2)) {
                    Err(if avail < 2 || (b(1) != 0xA0 && b(1) != 0xFF) {
                        pos + 1
                    } else if avail < 3 || (b(2) != 0xA0 && b(2) != 0xFF) {
                        pos + 2
                    } else {
                        pos + 3
                    })
                } else {
                    Ok((
                        (u32::from(c) << 16) | (u32::from(b(1)) << 8) | u32::from(b(2)),
                        pos + 3,
                    ))
                }
            } else if c != 0xA0 && c != 0xFF {
                Ok((u32::from(c), pos + 1))
            } else {
                Err(pos + 1)
            }
        }
        // single-byte charsets
        _ => Ok((u32::from(c), pos + 1)),
    }
}

/// Port of `unicode_cp_is_allowed()` (PHPSRC/ext/standard/html.c).
fn unicode_cp_is_allowed(cp: u32, doctype: i32) -> bool {
    match doctype {
        DOC_HTML401 => {
            (0x20..=0x7E).contains(&cp)
                || cp == 0x0A
                || cp == 0x09
                || cp == 0x0D
                || (0xA0..=0xD7FF).contains(&cp)
                || (0xE000..=0x10FFFF).contains(&cp)
        }
        DOC_HTML5 => {
            (0x20..=0x7E).contains(&cp)
                || ((0x09..=0x0D).contains(&cp) && cp != 0x0B)
                || (0xA0..=0xD7FF).contains(&cp)
                || ((0xE000..=0x10FFFF).contains(&cp)
                    && (cp & 0xFFFF) < 0xFFFE
                    && !(0xFDD0..=0xFDEF).contains(&cp))
        }
        _ => {
            (0x20..=0xD7FF).contains(&cp)
                || cp == 0x0A
                || cp == 0x09
                || cp == 0x0D
                || ((0xE000..=0x10FFFF).contains(&cp) && cp != 0xFFFE && cp != 0xFFFF)
        }
    }
}

/// Port of `numeric_entity_is_allowed()` (PHPSRC/ext/standard/html.c).
fn numeric_entity_is_allowed(cp: u32, doctype: i32) -> bool {
    match doctype {
        DOC_HTML401 => cp <= 0x10FFFF,
        DOC_HTML5 => {
            (0x20..=0x7E).contains(&cp)
                || ((0x09..=0x0C).contains(&cp) && cp != 0x0B)
                || ((0xA0..=0x10FFFF).contains(&cp)
                    && (cp & 0xFFFF) < 0xFFFE
                    && !(0xFDD0..=0xFDEF).contains(&cp))
        }
        _ => unicode_cp_is_allowed(cp, doctype),
    }
}

/// Port of `process_numeric_entity()` (PHPSRC/ext/standard/html.c): `s[pos..]` follows `&#`.
/// Returns the code point and the position of the terminating `;`. The digits are read like
/// glibc `strtol()` (hexadecimal also skips a `0x`/`0X` prefix that is followed by a hex digit;
/// overflow saturates).
fn numeric_entity(s: &[u8], pos: usize) -> Option<(u32, usize)> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut p = pos;
    let hex = matches!(at(p), b'x' | b'X');
    if hex {
        p += 1;
    }
    let is_digit = |c: u8| {
        if hex {
            c.is_ascii_hexdigit()
        } else {
            c.is_ascii_digit()
        }
    };
    if !is_digit(at(p)) {
        return None;
    }
    if hex && at(p) == b'0' && matches!(at(p + 1), b'x' | b'X') && at(p + 2).is_ascii_hexdigit() {
        p += 2;
    }
    let radix: u64 = if hex { 16 } else { 10 };
    let mut value: u64 = 0;
    while is_digit(at(p)) {
        let d = u64::from((at(p) as char).to_digit(radix as u32).expect("digit"));
        value = value.saturating_mul(radix).saturating_add(d);
        p += 1;
    }
    if at(p) != b';' || value > 0x10FFFF {
        return None;
    }
    Some((value as u32, p))
}

/// Whether `name` (bytes between `&` and `;`) is a named entity of the document type's inverse
/// map (`unescape_inverse_map(1, flags)` + the XHTML `&apos;` hack).
fn named_entity_known(name: &[u8], doctype: i32) -> bool {
    let Ok(name) = std::str::from_utf8(name) else {
        return false;
    };
    match doctype {
        DOC_HTML401 => html_entities::HTML4.binary_search(&name).is_ok(),
        DOC_XHTML => html_entities::HTML4.binary_search(&name).is_ok() || name == "apos",
        DOC_HTML5 => html_entities::HTML5.binary_search(&name).is_ok(),
        // DOC_XML1: `ent_ht_be_apos`
        _ => {
            debug_assert_eq!(doctype, DOC_XML1);
            matches!(name, "gt" | "amp" | "lt" | "apos" | "quot")
        }
    }
}

/// Port of `php_escape_html_entities_ex()` with `all = 0` (PHPSRC/ext/standard/html.c) once the
/// charset is known: the body of `htmlspecialchars()`.
///
/// `&`, `<`, `>` and, depending on `flags`, `"` (`&quot;`) and `'` (`&#039;` for HTML 4.01,
/// `&apos;` otherwise) are replaced. Invalid sequences are dropped with `ENT_IGNORE`, replaced with
/// `ENT_SUBSTITUTE` (U+FFFD bytes for UTF-8, `&#xFFFD;` otherwise), and otherwise make the whole
/// result `""`. Without `double_encode`, valid numeric and named entities are copied unchanged.
/// Only the low 32 bits of `flags` count (`(int) flags`).
pub fn escape_html(s: &[u8], flags: i64, charset: HtmlCharset, double_encode: bool) -> PhpString {
    let flags = flags as i32;
    let doctype = flags & DOC_TYPE_MASK;
    if charset == HtmlCharset::Utf8
        && double_encode
        && flags & SUBSTITUTE_DISALLOWED_CHARS == 0
        && std::str::from_utf8(s).is_ok()
    {
        return escape_html_utf8(s, flags, doctype);
    }
    escape_html_chars(s, flags, doctype, charset, double_encode)
}

/// The character loop of [`escape_html`].
fn escape_html_chars(
    s: &[u8],
    flags: i32,
    doctype: i32,
    charset: HtmlCharset,
    double_encode: bool,
) -> PhpString {
    let replacement: &[u8] = if charset == HtmlCharset::Utf8 {
        "\u{FFFD}".as_bytes()
    } else {
        b"&#xFFFD;"
    };
    let mut out: Vec<u8> = Vec::with_capacity(s.len() + s.len() / 8 + 16);
    let mut cursor = 0;
    while cursor < s.len() {
        let before = cursor;
        let this_char = match next_char(charset, s, cursor) {
            Ok((ch, next)) => {
                cursor = next;
                ch
            }
            Err(next) => {
                cursor = next;
                if flags & IGNORE_ERRORS != 0 {
                    continue;
                } else if flags & SUBSTITUTE_ERRORS != 0 {
                    out.extend_from_slice(replacement);
                    continue;
                } else {
                    return PhpString::empty();
                }
            }
        };
        let sequence = &s[before..cursor];
        if this_char != u32::from(b'&') {
            if (this_char == u32::from(b'\'') && flags & QUOTE_SINGLE == 0)
                || (this_char == u32::from(b'"') && flags & QUOTE_DOUBLE == 0)
            {
                out.extend_from_slice(sequence);
                continue;
            }
            let entity: Option<&[u8]> = match this_char {
                0x22 => Some(b"quot"),
                0x26 => Some(b"amp"),
                0x27 if doctype == DOC_HTML401 => Some(b"#039"),
                0x27 => Some(b"apos"),
                0x3C => Some(b"lt"),
                0x3E => Some(b"gt"),
                _ => None,
            };
            if let Some(entity) = entity {
                out.push(b'&');
                out.extend_from_slice(entity);
                out.push(b';');
                continue;
            }
            let mut disallowed = false;
            if flags & SUBSTITUTE_DISALLOWED_CHARS != 0 {
                disallowed = if charset.unicode_compat() {
                    !unicode_cp_is_allowed(this_char, doctype)
                } else {
                    // No mapping tables without `all`: only the ASCII range is checked.
                    this_char <= 0x7D && !unicode_cp_is_allowed(this_char, doctype)
                };
            }
            out.extend_from_slice(if disallowed { replacement } else { sequence });
        } else if double_encode {
            out.extend_from_slice(b"&amp;");
        } else {
            let at = |i: usize| s.get(i).copied().unwrap_or(0);
            let entity_end = if at(cursor) == b'#' {
                match numeric_entity(s, cursor + 1) {
                    Some((cp, semicolon))
                        if flags & SUBSTITUTE_DISALLOWED_CHARS == 0
                            || numeric_entity_is_allowed(cp, doctype) =>
                    {
                        Some(semicolon)
                    }
                    _ => None,
                }
            } else {
                let mut p = cursor;
                while at(p).is_ascii_alphanumeric() {
                    p += 1;
                }
                if at(p) == b';' && p > cursor && named_entity_known(&s[cursor..p], doctype) {
                    Some(p)
                } else {
                    None
                }
            };
            match entity_end {
                Some(semicolon) => {
                    out.push(b'&');
                    out.extend_from_slice(&s[cursor..semicolon]);
                    out.push(b';');
                    cursor = semicolon + 1;
                }
                None => out.extend_from_slice(b"&amp;"),
            }
        }
    }
    PhpString::from(out)
}

/// [`escape_html`] of well-formed UTF-8 with `double_encode` and without `ENT_DISALLOWED`: every character decodes
/// (the UTF-8 decoder of `get_next_char()` accepts exactly the well-formed sequences), so only `&`, `<`, `>` and the
/// quotes `flags` select are replaced and every other byte is copied; the result is built in one allocation.
fn escape_html_utf8(s: &[u8], flags: i32, doctype: i32) -> PhpString {
    let apos: &'static [u8] = if doctype == DOC_HTML401 {
        b"&#039;"
    } else {
        b"&apos;"
    };
    let entity = |c: u8| -> Option<&'static [u8]> {
        match c {
            b'&' => Some(b"&amp;"),
            b'<' => Some(b"&lt;"),
            b'>' => Some(b"&gt;"),
            b'"' if flags & QUOTE_DOUBLE != 0 => Some(b"&quot;"),
            b'\'' if flags & QUOTE_SINGLE != 0 => Some(apos),
            _ => None,
        }
    };
    let len = s
        .iter()
        .map(|&c| entity(c).map_or(1, <[u8]>::len))
        .sum::<usize>();
    if len == s.len() {
        return PhpString::from_bytes(s);
    }
    PhpString::build(len, |buf| {
        let mut at = 0;
        let mut run = 0;
        for (i, &c) in s.iter().enumerate() {
            if let Some(entity) = entity(c) {
                buf[at..at + (i - run)].copy_from_slice(&s[run..i]);
                at += i - run;
                buf[at..at + entity.len()].copy_from_slice(entity);
                at += entity.len();
                run = i + 1;
            }
        }
        buf[at..].copy_from_slice(&s[run..]);
    })
}

/// Port of `PHP_FUNCTION(htmlspecialchars)` / `php_html_entities(all = 0)`
/// (`htmlspecialchars(string $string, int $flags = ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401,
/// ?string $encoding = null, bool $double_encode = true): string`).
///
/// An empty string returns `""` before the charset is looked at (no warning).
pub fn htmlspecialchars(
    string: &Value,
    flags: Option<&Value>,
    encoding: Option<&Value>,
    double_encode: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let s = str_param(string, "htmlspecialchars", 1, "string", diag)?;
    let flags = match flags {
        Some(f) => long_param(f, "htmlspecialchars", 2, "flags", diag)?,
        None => ENT_QUOTES | ENT_SUBSTITUTE,
    };
    let hint = match encoding {
        Some(e) => param::parse_str_or_null(
            e,
            &Arg::new("htmlspecialchars", 3, "encoding").declared("?string"),
            diag,
        )?,
        None => None,
    };
    let double_encode = match double_encode {
        Some(d) => bool_param(d, "htmlspecialchars", 4, "double_encode", diag)?,
        None => true,
    };
    if s.is_empty() {
        return Ok(PhpString::empty());
    }
    let charset = determine_charset(hint.as_deref(), diag)?;
    Ok(escape_html(&s, flags, charset, double_encode))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc(s: &[u8]) -> Vec<u8> {
        escape_html(s, ENT_QUOTES | ENT_SUBSTITUTE, HtmlCharset::Utf8, true).to_vec()
    }

    /// The UTF-8 shortcut gives what the character loop gives, for every quote, error and doctype flag.
    #[test]
    fn utf8_shortcut_matches_the_character_loop() {
        let strings: [&[u8]; 9] = [
            b"",
            b"plain text",
            b"<a href=\"x\">it's & more</a>",
            "é ü 漢字 \u{10348} <>&\"'".as_bytes(),
            b"&amp; &#039; &lt",
            b"'''\"\"\"&&&<<<>>>",
            b"\x00\x01\x7f control",
            "\u{FFFD}\u{FEFF}\u{80}\u{9F}".as_bytes(),
            b"x",
        ];
        for quotes in [ENT_NOQUOTES, ENT_COMPAT, ENT_QUOTES, 1] {
            for errors in [0, ENT_IGNORE, ENT_SUBSTITUTE] {
                for doctype in [ENT_HTML401, ENT_XML1, ENT_XHTML, ENT_HTML5] {
                    let flags = (quotes | errors | doctype) as i32;
                    for s in strings {
                        assert_eq!(
                            escape_html_utf8(s, flags, flags & DOC_TYPE_MASK),
                            escape_html_chars(
                                s,
                                flags,
                                flags & DOC_TYPE_MASK,
                                HtmlCharset::Utf8,
                                true
                            ),
                            "{s:?} with flags {flags}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn basics() {
        assert_eq!(esc(b"<>&'\""), b"&lt;&gt;&amp;&#039;&quot;");
        assert_eq!(esc(b"a\xFFb\xC3"), "a\u{FFFD}b\u{FFFD}".as_bytes());
        assert_eq!(
            escape_html(
                b"a\x80b",
                ENT_QUOTES | ENT_SUBSTITUTE,
                HtmlCharset::Sjis,
                true
            ),
            "a&#xFFFD;b"
        );
        assert_eq!(
            escape_html(
                b"&#x0x41; &amp &copy; &apos;",
                ENT_QUOTES,
                HtmlCharset::Utf8,
                false
            ),
            "&#x0x41; &amp;amp &copy; &amp;apos;"
        );
        let mut entity_order = html_entities::HTML5.to_vec();
        entity_order.sort_unstable();
        assert_eq!(entity_order, html_entities::HTML5);
    }
}
