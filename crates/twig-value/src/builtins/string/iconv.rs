//! `iconv()` (reference/php/stdlib.md §7): Twig's `convert_encoding` filter and the non-UTF-8
//! paths of the escaper.
//!
//! PHP's `iconv()` (PHPSRC/ext/iconv/iconv.c) hands the work to glibc 2.41 in the oracle image.
//! This module reproduces that combination for a fixed set of charsets: the charset-name parsing
//! of `iconv_open()` (suffixes, `strip()`, aliases), the conversion through glibc's internal UCS-4
//! with each module's validity rules and error handling (`//IGNORE`, Unicode tag characters,
//! byte order marks), the round structure of glibc's conversion driver, and the loop of
//! `php_iconv_string()` around `iconv()` including the `ICONV_BROKEN_IGNORE` workaround PHP
//! compiles in for glibc. The behaviour was measured with the oracle and checked against glibc's
//! documented algorithm; no glibc code is used.
//!
//! # Supported charsets
//!
//! UTF-8 (glibc's: up to six bytes, code points up to `0x7FFFFFFF`), ASCII
//! (`ANSI_X3.4-1968`), ISO-8859-1 to -11 and -13 to -16, Windows-1250 to -1254, -1256 and -1257,
//! KOI8-R, KOI8-U, CP866, CP437, CP850, MacRoman (`MACINTOSH`), UCS-2 (`UCS-2`/`UCS-2LE` little
//! endian, `UCS-2BE`/`UNICODEBIG`), UCS-4 (`UCS-4` big endian, `UCS-4LE`), `UNICODE` (UCS-2 with
//! byte order mark), UTF-16, UTF-16LE, UTF-16BE, UTF-32, UTF-32LE and UTF-32BE, under every alias
//! glibc accepts.
//!
//! `//TRANSLIT` uses the transliteration table of the oracle's `C.UTF-8` locale (`LC_CTYPE`
//! `translit_neutral` and `translit_combining`, 6492 entries, `default_missing` `?`), read from
//! the compiled locale by `iconv_tables.gen.php`.
//!
//! # Not supported
//!
//! * Charsets glibc knows but this port has no tables for (the CJK and other multi-byte or
//!   stateful encodings, UTF-7, EBCDIC, `WCHAR_T`/`INTERNAL`, Windows-1255/1258 with their
//!   combining logic, ...): [`iconv`] throws an `Error` naming the charset instead of guessing.
//!   Unknown names behave like PHP (`Wrong encoding` warning, `false`).

use std::sync::OnceLock;

use super::iconv_tables as tables;
use super::str_param;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::string::PhpString;
use crate::value::Value;

/// `ICONV_CSNMAXLEN` (PHPSRC/ext/iconv/php_iconv.h).
const ICONV_CSNMAXLEN: usize = 64;

/// `GCONV_NCHAR_GOAL` (glibc iconv/gconv_open.c): characters per round of the first step.
const NCHAR_GOAL: usize = 8160;

/// The replacement candidates of transliteration entry `k` (`tables::TRANSLIT_TO` split at its
/// NUL terminators, indexed once).
fn translit_entry(k: usize) -> &'static str {
    static INDEX: OnceLock<Vec<&'static str>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut entries: Vec<&'static str> = tables::TRANSLIT_TO.split('\0').collect();
        entries.pop(); // after the last terminator
        debug_assert_eq!(entries.len(), tables::TRANSLIT_FROM.len());
        entries
    });
    index[k]
}

/// A single-byte charset of glibc: the code point of every byte (`0xFFFF` = invalid) and the
/// encoder's deviations from the inverse of that table.
pub(super) struct ByteTable {
    /// Code point per byte; `0xFFFF` marks bytes the decoder rejects.
    pub(super) decode: [u16; 256],
    /// Code points the encoder maps to a byte other than the inverse of `decode`.
    pub(super) extra: &'static [(u16, u8)],
    /// Code points some byte decodes to but the encoder rejects.
    pub(super) missing: &'static [u16],
}

impl ByteTable {
    fn encode(&self, cp: u32) -> Option<u8> {
        if cp >= 0xFFFF {
            return None;
        }
        let cp = cp as u16;
        if let Some(&(_, b)) = self.extra.iter().find(|&&(c, _)| c == cp) {
            return Some(b);
        }
        if self.missing.contains(&cp) {
            return None;
        }
        if usize::from(cp) < 256 && self.decode[usize::from(cp)] == cp {
            return Some(cp as u8);
        }
        self.decode.iter().position(|&c| c == cp).map(|b| b as u8)
    }
}

/// A charset this port converts from and to.
#[derive(Clone, Copy)]
enum Charset {
    Utf8,
    Ascii,
    Iso8859_1,
    Table(&'static ByteTable),
    /// `ISO-10646/UCS2/`: host (little-endian) byte order.
    Ucs2,
    /// `UNICODEBIG`: big-endian UCS-2 (glibc's "ucs2reverse" on little-endian hosts).
    Ucs2Be,
    /// `ISO-10646/UCS4/`: big endian.
    Ucs4,
    Ucs4Le,
    /// `UNICODE`: UCS-2 with a byte order mark.
    Unicode,
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
}

/// What a gconv module name resolves to.
enum Module {
    Supported(Charset),
    /// glibc's `INTERNAL` (`WCHAR_T`).
    Internal,
    /// A charset glibc converts but this port does not.
    Unsupported,
}

fn module_charset(module: &str) -> Module {
    use Charset::*;
    Module::Supported(match module {
        "ISO-10646/UTF8/" => Utf8,
        "ANSI_X3.4-1968//" => Ascii,
        "ISO-8859-1//" => Iso8859_1,
        "ISO-10646/UCS2/" => Ucs2,
        "UNICODEBIG//" => Ucs2Be,
        "ISO-10646/UCS4/" => Ucs4,
        "UCS-4LE//" => Ucs4Le,
        "UNICODE//" => Unicode,
        "UTF-16//" => Utf16,
        "UTF-16LE//" => Utf16Le,
        "UTF-16BE//" => Utf16Be,
        "UTF-32//" => Utf32,
        "UTF-32LE//" => Utf32Le,
        "UTF-32BE//" => Utf32Be,
        "ISO-8859-2//" => Table(&tables::ISO_8859_2),
        "ISO-8859-3//" => Table(&tables::ISO_8859_3),
        "ISO-8859-4//" => Table(&tables::ISO_8859_4),
        "ISO-8859-5//" => Table(&tables::ISO_8859_5),
        "ISO-8859-6//" => Table(&tables::ISO_8859_6),
        "ISO-8859-7//" => Table(&tables::ISO_8859_7),
        "ISO-8859-8//" => Table(&tables::ISO_8859_8),
        "ISO-8859-9//" => Table(&tables::ISO_8859_9),
        "ISO-8859-10//" => Table(&tables::ISO_8859_10),
        "ISO-8859-11//" => Table(&tables::ISO_8859_11),
        "ISO-8859-13//" => Table(&tables::ISO_8859_13),
        "ISO-8859-14//" => Table(&tables::ISO_8859_14),
        "ISO-8859-15//" => Table(&tables::ISO_8859_15),
        "ISO-8859-16//" => Table(&tables::ISO_8859_16),
        "CP1250//" => Table(&tables::CP1250),
        "CP1251//" => Table(&tables::CP1251),
        "CP1252//" => Table(&tables::CP1252),
        "CP1253//" => Table(&tables::CP1253),
        "CP1254//" => Table(&tables::CP1254),
        "CP1256//" => Table(&tables::CP1256),
        "CP1257//" => Table(&tables::CP1257),
        "KOI8-R//" => Table(&tables::KOI8_R),
        "KOI8-U//" => Table(&tables::KOI8_U),
        "IBM866//" => Table(&tables::IBM866),
        "MACINTOSH//" => Table(&tables::MACINTOSH),
        "IBM437//" => Table(&tables::IBM437),
        "IBM850//" => Table(&tables::IBM850),
        "INTERNAL" => return Module::Internal,
        _ => return Module::Unsupported,
    })
}

/// C `isspace()` (C/C.UTF-8 locale).
fn c_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// glibc `gconv_parse_code()` (iconv/gconv_charset.c): strips the `/TRANSLIT`, `/IGNORE`
/// (also `,`-separated, case-insensitive) and unknown suffixes of the error-handler part, returning
/// the remaining code and the `(translit, ignore)` flags.
fn parse_code(code: &[u8]) -> (Vec<u8>, bool, bool) {
    let mut code = code.to_vec();
    let (mut translit, mut ignore) = (false, false);
    loop {
        while let Some(&c) = code.last() {
            if c_isspace(c) || c == b',' || c == b'/' {
                code.pop();
            } else {
                break;
            }
        }
        if code.is_empty() {
            break;
        }
        let slashes = code.iter().filter(|&&c| c == b'/').count();
        let Some(suffix) = code.iter().rposition(|&c| c == b'/' || c == b',') else {
            break;
        };
        if slashes < 2 {
            break;
        }
        let s = &code[suffix..];
        if s.eq_ignore_ascii_case(b"/TRANSLIT") || s.eq_ignore_ascii_case(b",TRANSLIT") {
            translit = true;
        }
        if s.eq_ignore_ascii_case(b"/IGNORE") || s.eq_ignore_ascii_case(b",IGNORE") {
            ignore = true;
        }
        code.truncate(suffix);
    }
    (code, translit, ignore)
}

/// glibc `strip()` (iconv/gconv_charset.h): keeps alphanumerics and `_-.,:` (uppercased) and up to
/// two `/`, then pads to two `/`.
fn strip(code: &[u8]) -> String {
    let mut out = String::with_capacity(code.len() + 2);
    let mut slashes = 0;
    for &c in code {
        if c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b',' | b':') {
            out.push(char::from(c.to_ascii_uppercase()));
        } else if c == b'/' {
            slashes += 1;
            if slashes == 3 {
                break;
            }
            out.push('/');
        }
    }
    while slashes < 2 {
        out.push('/');
        slashes += 1;
    }
    out
}

/// The module a stripped name resolves to (the gconv cache), `"//"` meaning the locale's charset
/// (`UTF-8` under `C.UTF-8`).
fn resolve(stripped: &str) -> Option<&'static str> {
    let name = if stripped == "//" {
        "UTF-8//"
    } else {
        stripped
    };
    tables::NAMES
        .binary_search_by(|(alias, _)| alias.cmp(&name))
        .ok()
        .map(|i| tables::NAMES[i].1)
}

/// The C string of a PHP string argument (everything before the first NUL byte).
fn c_string(s: &[u8]) -> &[u8] {
    match memchr::memchr(0, s) {
        Some(nul) => &s[..nul],
        None => s,
    }
}

/// The status of a glibc conversion step (`__GCONV_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ok,
    EmptyInput,
    FullOutput,
    IllegalInput,
    IncompleteInput,
}

/// One decoded input unit.
enum Dec {
    Char(u32, usize),
    Incomplete,
    /// An invalid sequence: skipped length under `//IGNORE`, and whether the module keeps the
    /// `__GCONV_ILLEGAL_INPUT` result when it ignores it.
    Illegal {
        skip: usize,
        marks: bool,
    },
}

/// One encoded character.
enum Enc {
    Bytes,
    Full,
    /// A Unicode tag character (U+E0000..U+E007F) the charset cannot represent: dropped silently
    /// (`UNICODE_TAG_HANDLER`).
    TagSkip,
    /// `STANDARD_TO_LOOP_ERR_HANDLER(4)`: transliteration, `//IGNORE`, or an error.
    Unmappable,
    /// A surrogate code point rejected outside the standard handler.
    Surrogate {
        marks: bool,
    },
}

fn is_tag(cp: u32) -> bool {
    cp >> 7 == 0xE0000 >> 7
}

fn unmappable_or_tag(cp: u32) -> Enc {
    if is_tag(cp) {
        Enc::TagSkip
    } else {
        Enc::Unmappable
    }
}

/// glibc's UTF-8 decoder (`utf8_internal_loop`, iconv/gconv_simple.c): five- and six-byte forms
/// and code points above U+10FFFF are accepted, overlong forms and surrogates are not.
fn decode_utf8(s: &[u8]) -> Dec {
    let first = u32::from(s[0]);
    if first < 0x80 {
        return Dec::Char(first, 1);
    }
    let (cnt, mut ch) = if (0xC2..0xE0).contains(&first) {
        (2, first & 0x1f)
    } else if first & 0xF0 == 0xE0 {
        (3, first & 0x0f)
    } else if first & 0xF8 == 0xF0 {
        (4, first & 0x07)
    } else if first & 0xFC == 0xF8 {
        (5, first & 0x03)
    } else if first & 0xFE == 0xFC {
        (6, first & 0x01)
    } else {
        let mut i = 1;
        while i < s.len() && s[i] & 0xC0 == 0x80 && i < 5 {
            i += 1;
        }
        return Dec::Illegal {
            skip: i,
            marks: true,
        };
    };
    if cnt > s.len() {
        let mut i = 1;
        while i < s.len() && s[i] & 0xC0 == 0x80 {
            i += 1;
        }
        if i == s.len() {
            return Dec::Incomplete;
        }
        return Dec::Illegal {
            skip: i,
            marks: true,
        };
    }
    let mut i = 1;
    while i < cnt {
        let b = s[i];
        if b & 0xC0 != 0x80 {
            break;
        }
        ch = (ch << 6) | u32::from(b & 0x3f);
        i += 1;
    }
    if i < cnt || (cnt > 2 && ch >> (5 * cnt - 4) == 0) || (0xD800..=0xDFFF).contains(&ch) {
        return Dec::Illegal {
            skip: i,
            marks: true,
        };
    }
    Dec::Char(ch, cnt)
}

fn read16(s: &[u8], swap: bool) -> u32 {
    let v = u16::from_le_bytes([s[0], s[1]]);
    u32::from(if swap { v.swap_bytes() } else { v })
}

fn read32(s: &[u8], swap: bool) -> u32 {
    let v = u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    if swap { v.swap_bytes() } else { v }
}

/// The decoding step (charset to glibc's internal UCS-4).
struct Decoder {
    cs: Charset,
    swap: bool,
    /// `__invocation_counter` of the step.
    invocations: u64,
}

impl Decoder {
    fn new(cs: Charset) -> Decoder {
        let swap = matches!(cs, Charset::Utf16Be | Charset::Utf32Be);
        Decoder {
            cs,
            swap,
            invocations: 0,
        }
    }

    /// `MIN_NEEDED_FROM` of the module.
    fn min_needed(&self) -> usize {
        match self.cs {
            Charset::Ucs2 | Charset::Ucs2Be | Charset::Unicode => 2,
            Charset::Utf16 | Charset::Utf16Le | Charset::Utf16Be => 2,
            Charset::Ucs4 | Charset::Ucs4Le => 4,
            Charset::Utf32 | Charset::Utf32Le | Charset::Utf32Be => 4,
            _ => 1,
        }
    }

    /// The `PREPARE_LOOP` byte order mark detection of `UTF-16`, `UTF-32` and `UNICODE` on the
    /// first invocation; `Some(status)` returns early.
    fn prepare(&mut self, input: &[u8], pos: &mut usize) -> Option<Status> {
        if self.invocations != 0 {
            return None;
        }
        let (n, bom, bom_oe): (usize, &[u8], &[u8]) = match self.cs {
            Charset::Utf16 | Charset::Unicode => (2, b"\xFF\xFE", b"\xFE\xFF"),
            Charset::Utf32 => (4, b"\xFF\xFE\x00\x00", b"\x00\x00\xFE\xFF"),
            _ => return None,
        };
        let rest = &input[*pos..];
        if rest.len() < n {
            return Some(if rest.is_empty() {
                Status::EmptyInput
            } else {
                Status::IncompleteInput
            });
        }
        if &rest[..n] == bom {
            *pos += n;
        } else if &rest[..n] == bom_oe {
            self.swap = true;
            *pos += n;
        }
        None
    }

    fn decode(&self, s: &[u8]) -> Dec {
        let illegal = |skip: usize| Dec::Illegal { skip, marks: true };
        match self.cs {
            Charset::Utf8 => decode_utf8(s),
            Charset::Ascii => {
                if s[0] > 0x7F {
                    illegal(1)
                } else {
                    Dec::Char(u32::from(s[0]), 1)
                }
            }
            Charset::Iso8859_1 => Dec::Char(u32::from(s[0]), 1),
            Charset::Table(t) => match t.decode[usize::from(s[0])] {
                0xFFFF => illegal(1),
                cp => Dec::Char(u32::from(cp), 1),
            },
            Charset::Ucs2 | Charset::Unicode => {
                let u = read16(s, self.swap);
                if (0xD800..0xE000).contains(&u) {
                    illegal(2)
                } else {
                    Dec::Char(u, 2)
                }
            }
            Charset::Ucs2Be => {
                let u = read16(s, true);
                if (0xD800..0xE000).contains(&u) {
                    Dec::Illegal {
                        skip: 2,
                        marks: false,
                    }
                } else {
                    Dec::Char(u, 2)
                }
            }
            Charset::Utf16 | Charset::Utf16Le | Charset::Utf16Be => {
                let u1 = read16(s, self.swap);
                if !(0xD800..=0xDFFF).contains(&u1) {
                    return Dec::Char(u1, 2);
                }
                if u1 >= 0xDC00 {
                    return illegal(2);
                }
                if s.len() < 4 {
                    return Dec::Incomplete;
                }
                let u2 = read16(&s[2..], self.swap);
                if !(0xDC00..=0xDFFF).contains(&u2) {
                    return illegal(2);
                }
                Dec::Char(((u1 - 0xD7C0) << 10) + (u2 - 0xDC00), 4)
            }
            Charset::Utf32 | Charset::Utf32Le | Charset::Utf32Be => {
                let u = read32(s, self.swap);
                if u >= 0x110000 || (0xD800..0xE000).contains(&u) {
                    illegal(4)
                } else {
                    Dec::Char(u, 4)
                }
            }
            Charset::Ucs4 | Charset::Ucs4Le => unreachable!("UCS-4 uses its own loop"),
        }
    }

    /// One run of the module's loop function: decodes from `pos` until the input ends, an error
    /// stops it, or `max` characters were produced.
    fn run(
        &self,
        input: &[u8],
        pos: &mut usize,
        out: &mut Vec<u32>,
        max: usize,
        ignore: bool,
    ) -> Status {
        let end = input.len();
        if let Charset::Ucs4 | Charset::Ucs4Le = self.cs {
            // ucs4_internal_loop / ucs4le_internal_loop
            let big = matches!(self.cs, Charset::Ucs4);
            while *pos + 4 <= end && out.len() < max {
                let v = read32(&input[*pos..], big);
                if v > 0x7FFF_FFFF {
                    if ignore {
                        *pos += 4;
                        continue;
                    }
                    return Status::IllegalInput;
                }
                out.push(v);
                *pos += 4;
            }
            return if *pos == end {
                Status::EmptyInput
            } else if out.len() >= max {
                Status::FullOutput
            } else {
                Status::IncompleteInput
            };
        }
        let min = self.min_needed();
        let mut result = Status::EmptyInput;
        while *pos != end {
            if *pos + min > end {
                result = Status::IncompleteInput;
                break;
            }
            if out.len() >= max {
                result = Status::FullOutput;
                break;
            }
            match self.decode(&input[*pos..]) {
                Dec::Char(cp, len) => {
                    out.push(cp);
                    *pos += len;
                }
                Dec::Incomplete => {
                    result = Status::IncompleteInput;
                    break;
                }
                Dec::Illegal { skip, marks } => {
                    if !ignore {
                        result = Status::IllegalInput;
                        break;
                    }
                    if marks {
                        result = Status::IllegalInput;
                    }
                    *pos += skip;
                }
            }
        }
        result
    }
}

/// The encoding step (glibc's internal UCS-4 to the charset).
struct Encoder {
    cs: Charset,
    invocations: u64,
}

impl Encoder {
    /// `MIN_NEEDED_TO` of the module.
    fn min_needed(&self) -> usize {
        match self.cs {
            Charset::Ucs2 | Charset::Ucs2Be | Charset::Unicode => 2,
            Charset::Utf16 | Charset::Utf16Le | Charset::Utf16Be => 2,
            Charset::Ucs4 | Charset::Ucs4Le => 4,
            Charset::Utf32 | Charset::Utf32Le | Charset::Utf32Be => 4,
            _ => 1,
        }
    }

    fn encode(&self, cp: u32, out: &mut Vec<u8>, room: usize) -> Enc {
        let mut put = |bytes: &[u8]| {
            if bytes.len() > room {
                Enc::Full
            } else {
                out.extend_from_slice(bytes);
                Enc::Bytes
            }
        };
        match self.cs {
            Charset::Utf8 => {
                if cp < 0x80 {
                    put(&[cp as u8])
                } else if cp <= 0x7FFF_FFFF && !(0xD800..=0xDFFF).contains(&cp) {
                    let mut step = 2;
                    while step < 6 && cp & (!0u32 << (5 * step + 1)) != 0 {
                        step += 1;
                    }
                    let mut buf = [0u8; 6];
                    let mut wc = cp;
                    for i in (1..step).rev() {
                        buf[i] = 0x80 | (wc & 0x3f) as u8;
                        wc >>= 6;
                    }
                    buf[0] = ((!0xffu32 >> step) as u8) | wc as u8;
                    put(&buf[..step])
                } else {
                    Enc::Unmappable
                }
            }
            Charset::Ascii => {
                if cp > 0x7F {
                    unmappable_or_tag(cp)
                } else {
                    put(&[cp as u8])
                }
            }
            Charset::Iso8859_1 => {
                if cp > 0xFF {
                    unmappable_or_tag(cp)
                } else {
                    put(&[cp as u8])
                }
            }
            Charset::Table(t) => match t.encode(cp) {
                Some(b) => put(&[b]),
                None => unmappable_or_tag(cp),
            },
            Charset::Ucs2 | Charset::Ucs2Be | Charset::Unicode => {
                if cp >= 0x10000 {
                    unmappable_or_tag(cp)
                } else if (0xD800..0xE000).contains(&cp) {
                    Enc::Surrogate {
                        marks: !matches!(self.cs, Charset::Ucs2Be),
                    }
                } else if matches!(self.cs, Charset::Ucs2Be) {
                    put(&(cp as u16).to_be_bytes())
                } else {
                    put(&(cp as u16).to_le_bytes())
                }
            }
            Charset::Utf16 | Charset::Utf16Le | Charset::Utf16Be => {
                let be = matches!(self.cs, Charset::Utf16Be);
                let unit = |u: u32| {
                    if be {
                        (u as u16).to_be_bytes()
                    } else {
                        (u as u16).to_le_bytes()
                    }
                };
                if (0xD800..0xE000).contains(&cp) {
                    Enc::Surrogate { marks: true }
                } else if cp >= 0x10000 {
                    if cp >= 0x110000 {
                        return Enc::Unmappable;
                    }
                    let (hi, lo) = (unit(0xD7C0 + (cp >> 10)), unit(0xDC00 + (cp & 0x3ff)));
                    put(&[hi[0], hi[1], lo[0], lo[1]])
                } else {
                    put(&unit(cp))
                }
            }
            Charset::Utf32 | Charset::Utf32Le | Charset::Utf32Be => {
                if cp >= 0x110000 {
                    Enc::Unmappable
                } else if (0xD800..0xE000).contains(&cp) {
                    Enc::Surrogate { marks: true }
                } else if matches!(self.cs, Charset::Utf32Be) {
                    put(&cp.to_be_bytes())
                } else {
                    put(&cp.to_le_bytes())
                }
            }
            Charset::Ucs4 => put(&cp.to_be_bytes()),
            Charset::Ucs4Le => put(&cp.to_le_bytes()),
        }
    }

    /// The step function: `PREPARE_LOOP` (byte order mark on the first invocation) and one run of
    /// the loop over `chars`. Returns the number of characters consumed and the status.
    fn call(
        &mut self,
        chars: &[u32],
        out: &mut Vec<u8>,
        cap: usize,
        ignore: bool,
        translit: bool,
    ) -> (usize, Status) {
        if self.invocations == 0 {
            let bom: &[u8] = match self.cs {
                Charset::Utf16 | Charset::Unicode => b"\xFF\xFE",
                Charset::Utf32 => b"\xFF\xFE\x00\x00",
                _ => b"",
            };
            if out.len() + bom.len() > cap {
                return (0, Status::FullOutput);
            }
            out.extend_from_slice(bom);
        }
        let result = self.run(chars, out, cap, ignore, translit);
        self.invocations += 1;
        result
    }

    /// The step function called by glibc's `__gconv_transliterate()` on one replacement: its
    /// `PREPARE_LOOP` (which emits the byte order mark again while the step's first run has not
    /// finished) and the loop without error recovery. The output is kept only on success
    /// (`EmptyInput`).
    fn convert_candidate(&self, cand: &[u32], out: &mut Vec<u8>, cap: usize) -> Status {
        let mark = out.len();
        if self.invocations == 0 {
            let bom: &[u8] = match self.cs {
                Charset::Utf16 | Charset::Unicode => b"\xFF\xFE",
                Charset::Utf32 => b"\xFF\xFE\x00\x00",
                _ => b"",
            };
            if out.len() + bom.len() > cap {
                return Status::FullOutput;
            }
            out.extend_from_slice(bom);
        }
        let min = self.min_needed();
        for &c in cand {
            let status = if out.len() + min > cap {
                Status::FullOutput
            } else {
                match self.encode(c, out, cap - out.len()) {
                    Enc::Bytes | Enc::TagSkip => continue,
                    Enc::Full => Status::FullOutput,
                    Enc::Unmappable | Enc::Surrogate { .. } => Status::IllegalInput,
                }
            };
            out.truncate(mark);
            return status;
        }
        Status::EmptyInput
    }

    /// glibc `__gconv_transliterate()` (iconv/gconv_trans.c) with the C.UTF-8 locale's table:
    /// the candidates of `cp` in order, then `default_missing` (`?`); the first one the charset can
    /// represent completely is written. `Ok` when transliterated, `FullOutput`, or `IllegalInput`.
    fn transliterate(&self, cp: u32, out: &mut Vec<u8>, cap: usize) -> Status {
        if let Ok(k) = tables::TRANSLIT_FROM.binary_search(&cp) {
            let entry = translit_entry(k);
            for cand in entry.split('\u{1}') {
                let cand: Vec<u32> = cand.chars().map(u32::from).collect();
                match self.convert_candidate(&cand, out, cap) {
                    Status::IllegalInput => continue,
                    Status::EmptyInput => return Status::Ok,
                    other => return other,
                }
            }
        }
        match self.convert_candidate(tables::TRANSLIT_DEFAULT_MISSING, out, cap) {
            Status::EmptyInput => Status::Ok,
            other => other,
        }
    }

    fn run(
        &self,
        chars: &[u32],
        out: &mut Vec<u8>,
        cap: usize,
        ignore: bool,
        translit: bool,
    ) -> (usize, Status) {
        let min = self.min_needed();
        if let Charset::Ucs4 | Charset::Ucs4Le = self.cs {
            // internal_ucs4_loop / internal_ucs4le_loop
            let n = chars.len().min((cap - out.len()) / 4);
            for &cp in &chars[..n] {
                self.encode(cp, out, 4);
            }
            let status = if n == chars.len() {
                Status::EmptyInput
            } else {
                Status::FullOutput
            };
            return (n, status);
        }
        let mut result = Status::EmptyInput;
        let mut i = 0;
        while i < chars.len() {
            if out.len() + min > cap {
                result = Status::FullOutput;
                break;
            }
            let cp = chars[i];
            match self.encode(cp, out, cap - out.len()) {
                Enc::Bytes | Enc::TagSkip => {}
                Enc::Full => {
                    result = Status::FullOutput;
                    break;
                }
                Enc::Unmappable => {
                    // STANDARD_TO_LOOP_ERR_HANDLER: a successful transliteration resets the result
                    if translit {
                        match self.transliterate(cp, out, cap) {
                            Status::IllegalInput => {}
                            Status::FullOutput => {
                                result = Status::FullOutput;
                                break;
                            }
                            _ => {
                                result = Status::Ok;
                                i += 1;
                                continue;
                            }
                        }
                    }
                    result = Status::IllegalInput;
                    if !ignore {
                        break;
                    }
                }
                Enc::Surrogate { marks } => {
                    if !ignore {
                        result = Status::IllegalInput;
                        break;
                    }
                    if marks {
                        result = Status::IllegalInput;
                    }
                }
            }
            i += 1;
        }
        (i, result)
    }
}

/// An open conversion descriptor (`iconv_t`) for a two-step conversion through glibc's internal
/// UCS-4.
struct Conversion {
    dec: Decoder,
    enc: Encoder,
    ignore: bool,
    translit: bool,
}

impl Conversion {
    /// glibc `__gconv()` with the first step's skeleton (iconv/skeleton.c): rounds of up to
    /// [`NCHAR_GOAL`] characters, each handed to the second step, whose error (or full output
    /// buffer) wins and rewinds the input to the first character it did not consume.
    fn gconv(&mut self, input: &[u8], pos: &mut usize, out: &mut Vec<u8>, cap: usize) -> Status {
        if let Some(status) = self.dec.prepare(input, pos) {
            return status;
        }
        loop {
            let round_start = *pos;
            let mut chars = Vec::new();
            let mut status = self
                .dec
                .run(input, pos, &mut chars, NCHAR_GOAL, self.ignore);
            self.dec.invocations += 1;
            if !chars.is_empty() {
                let (consumed, result) =
                    self.enc.call(&chars, out, cap, self.ignore, self.translit);
                if result != Status::EmptyInput {
                    if consumed < chars.len() {
                        *pos = round_start;
                        let mut redo = Vec::new();
                        self.dec.run(input, pos, &mut redo, consumed, self.ignore);
                        if consumed == 0 {
                            self.dec.invocations -= 1;
                        }
                    }
                    status = result;
                } else if status == Status::FullOutput {
                    status = Status::Ok;
                }
            }
            if status != Status::Ok {
                return status;
            }
        }
    }
}

/// The outcome of the conversion part of `php_iconv_string()`.
enum IconvOutcome {
    Converted(Vec<u8>),
    /// `PHP_ICONV_ERR_ILLEGAL_SEQ` (`EILSEQ`).
    IllegalSequence,
    /// `PHP_ICONV_ERR_ILLEGAL_CHAR` (`EINVAL`).
    IncompleteCharacter,
    /// `PHP_ICONV_ERR_TOO_BIG` (`E2BIG`).
    TooBig,
}

/// The loop of `php_iconv_string()` (PHPSRC/ext/iconv/iconv.c) around `iconv()`: the output
/// buffer starts at `len + 32` bytes and grows by `len` on `E2BIG`; with `ICONV_BROKEN_IGNORE`
/// an `EILSEQ` under `//IGNORE` either ends the conversion successfully (at most one byte left) or
/// skips one input byte and continues.
fn php_iconv_loop(conv: &mut Conversion, input: &[u8], php_ignore: bool) -> IconvOutcome {
    let in_len = input.len();
    let mut cap = in_len + 32;
    let mut pos = 0;
    let mut out = Vec::with_capacity(cap);
    while pos < in_len {
        let status = conv.gconv(input, &mut pos, &mut out, cap);
        match status {
            Status::Ok | Status::EmptyInput => break,
            Status::IllegalInput if php_ignore => {
                if in_len - pos <= 1 {
                    break;
                }
                pos += 1;
            }
            Status::FullOutput if pos < in_len => cap += in_len,
            Status::FullOutput => return IconvOutcome::TooBig,
            Status::IllegalInput => return IconvOutcome::IllegalSequence,
            Status::IncompleteInput => return IconvOutcome::IncompleteCharacter,
        }
    }
    // None of the supported modules emits anything when flushed.
    IconvOutcome::Converted(out)
}

/// `_php_check_ignore()` (PHPSRC/ext/iconv/iconv.c, `ICONV_BROKEN_IGNORE`): whether the output
/// charset ends with the case-sensitive `//IGNORE` or `//IGNORE//TRANSLIT`.
fn php_check_ignore(charset: &[u8]) -> bool {
    let len = charset.len();
    (len >= 9 && charset.ends_with(b"//IGNORE"))
        || (len >= 19 && charset.ends_with(b"//IGNORE//TRANSLIT"))
}

/// The body of `PHP_FUNCTION(iconv)` (PHPSRC/ext/iconv/iconv.c) on parsed arguments: converts
/// `input` from `in_charset` to `out_charset` like PHP 8.5.13 on glibc 2.41. `Ok(None)` is PHP's
/// `false`, after the warning or notice:
///
/// * a charset name of 64 bytes or more: `Warning: iconv(): Encoding parameter exceeds the maximum
///   allowed length of 64 characters`;
/// * an unknown charset (or no conversion path): `Warning: iconv(): Wrong encoding, conversion
///   from "<in>" to "<out>" is not allowed`;
/// * invalid input or a character the output charset cannot represent: `Notice: iconv(): Detected
///   an illegal character in input string`;
/// * input ending inside a multi-byte sequence: `Notice: iconv(): Detected an incomplete multibyte
///   character in input string`.
///
/// Charsets known to glibc but not supported here, and conversions that would need `//TRANSLIT`,
/// throw an `Error` (see the module documentation).
pub fn iconv_bytes(
    in_charset: &[u8],
    out_charset: &[u8],
    input: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<PhpString>> {
    if in_charset.len() >= ICONV_CSNMAXLEN || out_charset.len() >= ICONV_CSNMAXLEN {
        diag.warn(format!(
            "iconv(): Encoding parameter exceeds the maximum allowed length of {ICONV_CSNMAXLEN} characters"
        ))?;
        return Ok(None);
    }
    let (in_c, out_c) = (c_string(in_charset), c_string(out_charset));
    let php_ignore = php_check_ignore(out_c);
    let (from_code, _, _) = parse_code(in_c);
    let (to_code, translit, ignore) = parse_code(out_c);
    let (from, to) = (strip(&from_code), strip(&to_code));
    let wrong = |diag: &mut dyn Diagnostics| -> PhpResult<Option<PhpString>> {
        diag.warn(PhpString::concat(&[
            b"iconv(): Wrong encoding, conversion from \"",
            in_c,
            b"\" to \"",
            out_c,
            b"\" is not allowed",
        ]))?;
        Ok(None)
    };
    let (Some(from_module), Some(to_module)) = (resolve(&from), resolve(&to)) else {
        return wrong(diag);
    };
    let unsupported = |name: &[u8]| {
        PhpError::error(PhpString::concat(&[
            b"iconv(): conversion of charset \"",
            name,
            b"\" is not supported by twig-rs",
        ]))
    };
    let (from_cs, to_cs) = match (module_charset(from_module), module_charset(to_module)) {
        (Module::Supported(f), Module::Supported(t)) => (f, t),
        (Module::Internal, Module::Internal) => return wrong(diag),
        (Module::Supported(_), _) => return Err(unsupported(out_c)),
        _ => return Err(unsupported(in_c)),
    };
    let mut conv = Conversion {
        dec: Decoder::new(from_cs),
        enc: Encoder {
            cs: to_cs,
            invocations: 0,
        },
        ignore,
        translit,
    };
    match php_iconv_loop(&mut conv, input, php_ignore) {
        IconvOutcome::Converted(out) => Ok(Some(PhpString::from(out))),
        IconvOutcome::IllegalSequence => {
            diag.notice("iconv(): Detected an illegal character in input string")?;
            Ok(None)
        }
        IconvOutcome::IncompleteCharacter => {
            diag.notice("iconv(): Detected an incomplete multibyte character in input string")?;
            Ok(None)
        }
        IconvOutcome::TooBig => {
            diag.warn("iconv(): Buffer length exceeded")?;
            Ok(None)
        }
    }
}

/// Port of `PHP_FUNCTION(iconv)` (`iconv(string $from_encoding, string $to_encoding, string
/// $string): string|false`); `Ok(None)` is `false`. See [`iconv_bytes`].
pub fn iconv(
    from_encoding: &Value,
    to_encoding: &Value,
    string: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<PhpString>> {
    let from = str_param(from_encoding, "iconv", 1, "from_encoding", diag)?;
    let to = str_param(to_encoding, "iconv", 2, "to_encoding", diag)?;
    let s = str_param(string, "iconv", 3, "string", diag)?;
    iconv_bytes(&from, &to, &s, diag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(
            parse_code(b"UTF-8//TRANSLIT//IGNORE"),
            (b"UTF-8".to_vec(), true, true)
        );
        assert_eq!(
            parse_code(b"utf-8 //ignore"),
            (b"utf-8".to_vec(), false, true)
        );
        assert_eq!(strip(b"utf 8"), "UTF8//");
        assert_eq!(strip(b"ISO-10646/UTF8"), "ISO-10646/UTF8/");
        assert_eq!(resolve("LATIN1//"), Some("ISO-8859-1//"));
        assert_eq!(resolve("//"), Some("ISO-10646/UTF8/"));
        assert_eq!(resolve("UTF_8//"), None);
        let mut sorted = tables::NAMES.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, tables::NAMES);
    }

    #[test]
    fn conversions() {
        let mut d: Vec<crate::diag::Diagnostic> = Vec::new();
        let conv = |f: &str, t: &str, s: &[u8], d: &mut Vec<crate::diag::Diagnostic>| {
            iconv_bytes(f.as_bytes(), t.as_bytes(), s, d)
                .unwrap()
                .map(|s| s.to_vec())
        };
        assert_eq!(
            conv("ISO-8859-1", "UTF-8", b"a\xE9", &mut d),
            Some(b"a\xC3\xA9".to_vec())
        );
        assert_eq!(
            conv("UTF-8", "UTF-16", b"a", &mut d),
            Some(b"\xFF\xFEa\x00".to_vec())
        );
        assert_eq!(
            conv("UTF-8", "ASCII//IGNORE", b"a\xC3\xA9b", &mut d),
            Some(b"ab".to_vec())
        );
        assert_eq!(
            conv("CP1252", "UTF-8", b"\x80", &mut d),
            Some("\u{20AC}".as_bytes().to_vec())
        );
        assert!(d.is_empty());
        assert_eq!(conv("UTF-8", "ISO-8859-1", b"a\xC3", &mut d), None);
        assert_eq!(
            d[0].message,
            "iconv(): Detected an incomplete multibyte character in input string"
        );
    }
}
