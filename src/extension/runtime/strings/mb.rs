//! The encoding argument of the mbstring functions Twig calls with the environment charset (`mb_strtoupper`,
//! `mb_strtolower`, `mb_convert_case`, `mb_substr`, `mb_strlen`).
//!
//! `twig-unicode` implements mbstring for UTF-8; the single-byte encodings are supported where no character table is
//! needed: `mb_strlen()` and `mb_substr()` count bytes for all of them, and the case conversions work for the encodings
//! whose bytes are their code points (ISO-8859-1, 8bit, and ASCII and 7bit below 0x80). Other encodings fail with a
//! twig-rs `Error` (reference/deviations.md). An encoding name is
//! resolved like PHP's `php_mb_get_encoding()` (PHPSRC/ext/mbstring/mbstring.c) and `mbfl_name2encoding()`
//! (PHPSRC/ext/mbstring/libmbfl/mbfl/mbfl_encoding.c), so that a name mbstring does not know fails with PHP's
//! `ValueError` and a name it knows but twig-rs cannot handle fails with a twig-rs `Error`.

use twig_value::{Diagnostics, PhpError, PhpString};

use crate::extension::runtime::CallResult;

/// An mbstring encoding.
struct Encoding {
    /// The canonical name (`mbfl_encoding::name`).
    name: &'static str,
    /// The MIME name (`mbfl_encoding::mime_name`).
    mime: Option<&'static str>,
    /// The aliases (`mbfl_encoding::aliases`).
    aliases: &'static [&'static str],
    /// `MBFL_ENCTYPE_SBCS`: one byte per character (PHPSRC/ext/mbstring/libmbfl/filters/mbfilter_singlebyte.c,
    /// `mbfilter_7bit.c`, `mbfilter_uuencode.c`, PHPSRC/ext/mbstring/libmbfl/mbfl/mbfilter_8bit.c).
    single_byte: bool,
}

/// What twig-rs can do with an encoding argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Charset {
    /// UTF-8.
    Utf8,
    /// A single-byte encoding: one character per byte.
    SingleByte {
        /// The bytes below `identity` are their own code points and the other bytes are invalid (`0x100` for
        /// ISO-8859-1 and 8bit, `0x80` for ASCII and 7bit); 0 for the encodings that need a character table, which
        /// twig-rs does not have.
        identity: u32,
    },
}

/// The encodings of mbstring (`mb_list_encodings()` on PHP 8.5.13, in `mbfl_encoding_ptr_list` order): name, MIME
/// name (`mb_preferred_mime_name()`) and aliases (`mb_encoding_aliases()`), as the oracle lists them, and the
/// `MBFL_ENCTYPE_SBCS` flag from the libmbfl sources:
///
/// ```php
/// foreach (mb_list_encodings() as $e) { $o[] = [$e, @mb_preferred_mime_name($e), mb_encoding_aliases($e)]; }
/// ```
const ENCODINGS: &[Encoding] = &[
    Encoding {
        name: "BASE64",
        mime: Some("BASE64"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "UUENCODE",
        mime: Some("x-uuencode"),
        aliases: &[],
        single_byte: true,
    },
    Encoding {
        name: "HTML-ENTITIES",
        mime: Some("HTML-ENTITIES"),
        aliases: &["HTML", "html"],
        single_byte: false,
    },
    Encoding {
        name: "Quoted-Printable",
        mime: Some("Quoted-Printable"),
        aliases: &["qprint"],
        single_byte: false,
    },
    Encoding {
        name: "7bit",
        mime: Some("7bit"),
        aliases: &[],
        single_byte: true,
    },
    Encoding {
        name: "8bit",
        mime: Some("8bit"),
        aliases: &["binary"],
        single_byte: true,
    },
    Encoding {
        name: "UCS-4",
        mime: Some("UCS-4"),
        aliases: &["ISO-10646-UCS-4", "UCS4"],
        single_byte: false,
    },
    Encoding {
        name: "UCS-4BE",
        mime: Some("UCS-4BE"),
        aliases: &["byte4be"],
        single_byte: false,
    },
    Encoding {
        name: "UCS-4LE",
        mime: Some("UCS-4LE"),
        aliases: &["byte4le"],
        single_byte: false,
    },
    Encoding {
        name: "UCS-2",
        mime: Some("UCS-2"),
        aliases: &["ISO-10646-UCS-2", "UCS2", "UNICODE"],
        single_byte: false,
    },
    Encoding {
        name: "UCS-2BE",
        mime: Some("UCS-2BE"),
        aliases: &["byte2be"],
        single_byte: false,
    },
    Encoding {
        name: "UCS-2LE",
        mime: Some("UCS-2LE"),
        aliases: &["byte2le"],
        single_byte: false,
    },
    Encoding {
        name: "UTF-32",
        mime: Some("UTF-32"),
        aliases: &["utf32"],
        single_byte: false,
    },
    Encoding {
        name: "UTF-32BE",
        mime: Some("UTF-32BE"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "UTF-32LE",
        mime: Some("UTF-32LE"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "UTF-16",
        mime: Some("UTF-16"),
        aliases: &["utf16"],
        single_byte: false,
    },
    Encoding {
        name: "UTF-16BE",
        mime: Some("UTF-16BE"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "UTF-16LE",
        mime: Some("UTF-16LE"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "UTF-8",
        mime: Some("UTF-8"),
        aliases: &["utf8"],
        single_byte: false,
    },
    Encoding {
        name: "UTF-7",
        mime: Some("UTF-7"),
        aliases: &["utf7"],
        single_byte: false,
    },
    Encoding {
        name: "UTF7-IMAP",
        mime: None,
        aliases: &["mUTF-7"],
        single_byte: false,
    },
    Encoding {
        name: "ASCII",
        mime: Some("US-ASCII"),
        aliases: &[
            "ANSI_X3.4-1968",
            "iso-ir-6",
            "ANSI_X3.4-1986",
            "ISO_646.irv:1991",
            "US-ASCII",
            "ISO646-US",
            "us",
            "IBM367",
            "IBM-367",
            "cp367",
            "csASCII",
        ],
        single_byte: true,
    },
    Encoding {
        name: "EUC-JP",
        mime: Some("EUC-JP"),
        aliases: &["EUC", "EUC_JP", "eucJP", "x-euc-jp"],
        single_byte: false,
    },
    Encoding {
        name: "SJIS",
        mime: Some("Shift_JIS"),
        aliases: &["x-sjis", "SHIFT-JIS"],
        single_byte: false,
    },
    Encoding {
        name: "eucJP-win",
        mime: Some("EUC-JP"),
        aliases: &["eucJP-open", "eucJP-ms"],
        single_byte: false,
    },
    Encoding {
        name: "EUC-JP-2004",
        mime: Some("EUC-JP"),
        aliases: &["EUC_JP-2004"],
        single_byte: false,
    },
    Encoding {
        name: "SJIS-Mobile#DOCOMO",
        mime: Some("Shift_JIS"),
        aliases: &["SJIS-DOCOMO", "shift_jis-imode", "x-sjis-emoji-docomo"],
        single_byte: false,
    },
    Encoding {
        name: "SJIS-Mobile#KDDI",
        mime: Some("Shift_JIS"),
        aliases: &["SJIS-KDDI", "shift_jis-kddi", "x-sjis-emoji-kddi"],
        single_byte: false,
    },
    Encoding {
        name: "SJIS-Mobile#SOFTBANK",
        mime: Some("Shift_JIS"),
        aliases: &[
            "SJIS-SOFTBANK",
            "shift_jis-softbank",
            "x-sjis-emoji-softbank",
        ],
        single_byte: false,
    },
    Encoding {
        name: "SJIS-mac",
        mime: Some("Shift_JIS"),
        aliases: &["MacJapanese", "x-Mac-Japanese"],
        single_byte: false,
    },
    Encoding {
        name: "SJIS-2004",
        mime: Some("Shift_JIS"),
        aliases: &["SJIS2004", "Shift_JIS-2004"],
        single_byte: false,
    },
    Encoding {
        name: "UTF-8-Mobile#DOCOMO",
        mime: Some("UTF-8"),
        aliases: &["UTF-8-DOCOMO", "UTF8-DOCOMO"],
        single_byte: false,
    },
    Encoding {
        name: "UTF-8-Mobile#KDDI-A",
        mime: Some("UTF-8"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "UTF-8-Mobile#KDDI-B",
        mime: Some("UTF-8"),
        aliases: &["UTF-8-Mobile#KDDI", "UTF-8-KDDI", "UTF8-KDDI"],
        single_byte: false,
    },
    Encoding {
        name: "UTF-8-Mobile#SOFTBANK",
        mime: Some("UTF-8"),
        aliases: &["UTF-8-SOFTBANK", "UTF8-SOFTBANK"],
        single_byte: false,
    },
    Encoding {
        name: "CP932",
        mime: Some("Shift_JIS"),
        aliases: &["MS932", "Windows-31J", "MS_Kanji"],
        single_byte: false,
    },
    Encoding {
        name: "SJIS-win",
        mime: Some("Shift_JIS"),
        aliases: &["SJIS-ms", "SJIS-open"],
        single_byte: false,
    },
    Encoding {
        name: "CP51932",
        mime: Some("CP51932"),
        aliases: &["cp51932"],
        single_byte: false,
    },
    Encoding {
        name: "JIS",
        mime: Some("ISO-2022-JP"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "ISO-2022-JP",
        mime: Some("ISO-2022-JP"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "ISO-2022-JP-MS",
        mime: Some("ISO-2022-JP"),
        aliases: &["ISO2022JPMS"],
        single_byte: false,
    },
    Encoding {
        name: "GB18030",
        mime: Some("GB18030"),
        aliases: &["gb-18030", "gb-18030-2000"],
        single_byte: false,
    },
    Encoding {
        name: "GB18030-2022",
        mime: Some("GB18030-2022"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "Windows-1252",
        mime: Some("Windows-1252"),
        aliases: &["cp1252"],
        single_byte: true,
    },
    Encoding {
        name: "Windows-1254",
        mime: Some("Windows-1254"),
        aliases: &["CP1254", "CP-1254", "WINDOWS-1254"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-1",
        mime: Some("ISO-8859-1"),
        aliases: &["ISO8859-1", "latin1"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-2",
        mime: Some("ISO-8859-2"),
        aliases: &["ISO8859-2", "latin2"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-3",
        mime: Some("ISO-8859-3"),
        aliases: &["ISO8859-3", "latin3"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-4",
        mime: Some("ISO-8859-4"),
        aliases: &["ISO8859-4", "latin4"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-5",
        mime: Some("ISO-8859-5"),
        aliases: &["ISO8859-5", "cyrillic"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-6",
        mime: Some("ISO-8859-6"),
        aliases: &["ISO8859-6", "arabic"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-7",
        mime: Some("ISO-8859-7"),
        aliases: &["ISO8859-7", "greek"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-8",
        mime: Some("ISO-8859-8"),
        aliases: &["ISO8859-8", "hebrew"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-9",
        mime: Some("ISO-8859-9"),
        aliases: &["ISO8859-9", "latin5"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-10",
        mime: Some("ISO-8859-10"),
        aliases: &["ISO8859-10", "latin6"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-13",
        mime: Some("ISO-8859-13"),
        aliases: &["ISO8859-13"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-14",
        mime: Some("ISO-8859-14"),
        aliases: &["ISO8859-14", "latin8"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-15",
        mime: Some("ISO-8859-15"),
        aliases: &["ISO8859-15"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-8859-16",
        mime: Some("ISO-8859-16"),
        aliases: &["ISO8859-16"],
        single_byte: true,
    },
    Encoding {
        name: "EUC-CN",
        mime: Some("CN-GB"),
        aliases: &["CN-GB", "EUC_CN", "eucCN", "x-euc-cn", "gb2312"],
        single_byte: false,
    },
    Encoding {
        name: "CP936",
        mime: Some("CP936"),
        aliases: &["CP-936", "GBK"],
        single_byte: false,
    },
    Encoding {
        name: "HZ",
        mime: Some("HZ-GB-2312"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "EUC-TW",
        mime: Some("EUC-TW"),
        aliases: &["EUC_TW", "eucTW", "x-euc-tw"],
        single_byte: false,
    },
    Encoding {
        name: "BIG-5",
        mime: Some("BIG5"),
        aliases: &["CN-BIG5", "BIG-FIVE", "BIGFIVE"],
        single_byte: false,
    },
    Encoding {
        name: "CP950",
        mime: Some("BIG5"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "EUC-KR",
        mime: Some("EUC-KR"),
        aliases: &["EUC_KR", "eucKR", "x-euc-kr"],
        single_byte: false,
    },
    Encoding {
        name: "UHC",
        mime: Some("UHC"),
        aliases: &["CP949"],
        single_byte: false,
    },
    Encoding {
        name: "ISO-2022-KR",
        mime: Some("ISO-2022-KR"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "Windows-1251",
        mime: Some("Windows-1251"),
        aliases: &["CP1251", "CP-1251", "WINDOWS-1251"],
        single_byte: true,
    },
    Encoding {
        name: "CP866",
        mime: Some("CP866"),
        aliases: &["CP-866", "IBM866", "IBM-866"],
        single_byte: true,
    },
    Encoding {
        name: "KOI8-R",
        mime: Some("KOI8-R"),
        aliases: &["KOI8R"],
        single_byte: true,
    },
    Encoding {
        name: "KOI8-U",
        mime: Some("KOI8-U"),
        aliases: &["KOI8U"],
        single_byte: true,
    },
    Encoding {
        name: "ArmSCII-8",
        mime: Some("ArmSCII-8"),
        aliases: &["ArmSCII8", "ARMSCII-8", "ARMSCII8"],
        single_byte: true,
    },
    Encoding {
        name: "CP850",
        mime: Some("CP850"),
        aliases: &["CP-850", "IBM850", "IBM-850"],
        single_byte: true,
    },
    Encoding {
        name: "ISO-2022-JP-2004",
        mime: Some("ISO-2022-JP-2004"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "ISO-2022-JP-MOBILE#KDDI",
        mime: Some("ISO-2022-JP"),
        aliases: &["ISO-2022-JP-KDDI"],
        single_byte: false,
    },
    Encoding {
        name: "CP50220",
        mime: Some("ISO-2022-JP"),
        aliases: &["cp50220raw", "cp50220-raw", "JIS-ms"],
        single_byte: false,
    },
    Encoding {
        name: "CP50221",
        mime: Some("ISO-2022-JP"),
        aliases: &[],
        single_byte: false,
    },
    Encoding {
        name: "CP50222",
        mime: Some("ISO-2022-JP"),
        aliases: &[],
        single_byte: false,
    },
];

/// The encoding an mbstring encoding name designates, `None` for a name mbstring does not know: the name, then the
/// MIME names, then the aliases, compared case-insensitively (ASCII), with the name cut at its first NUL byte
/// (`mbfl_name2encoding(ZSTR_VAL(name))` takes a C string).
///
/// Port of `mbfl_name2encoding_ex()` (PHPSRC/ext/mbstring/libmbfl/mbfl/mbfl_encoding.c).
fn name_to_encoding(name: &[u8]) -> Option<&'static Encoding> {
    let name = c_string(name);
    let matches = |candidate: &str| candidate.as_bytes().eq_ignore_ascii_case(name);
    ENCODINGS
        .iter()
        .find(|encoding| matches(encoding.name))
        .or_else(|| {
            ENCODINGS
                .iter()
                .find(|encoding| encoding.mime.is_some_and(matches))
        })
        .or_else(|| {
            ENCODINGS
                .iter()
                .find(|encoding| encoding.aliases.iter().any(|alias| matches(alias)))
        })
}

/// The bytes of `s` up to its first NUL byte (what C sees of a PHP string).
fn c_string(s: &[u8]) -> &[u8] {
    match memchr::memchr(0, s) {
        Some(end) => &s[..end],
        None => s,
    }
}

/// The `Error` for an encoding mbstring knows but twig-rs does not implement for `function`.
fn unsupported(function: &str, encoding: &[u8]) -> PhpError {
    PhpError::error(PhpString::concat(&[
        format!("{function}(): encoding \"").as_bytes(),
        c_string(encoding),
        b"\" is not supported by twig-rs",
    ]))
}

/// Resolves the encoding argument `encoding` (argument `position` of `function`).
///
/// Port of `php_mb_get_encoding()` (PHPSRC/ext/mbstring/mbstring.c), including the `E_DEPRECATED` for the
/// Base64, Uuencode, HTML entities and Quoted-Printable pseudo-encodings.
///
/// # Errors
///
/// - an unknown name: PHP's `ValueError` `%s(): Argument #%d ($encoding) must be a valid encoding, "%s" given`;
/// - an encoding that is neither UTF-8 nor single-byte: `Error` `%s(): encoding "%s" is not supported by twig-rs`
///   (reference/deviations.md);
/// - a deprecation the diagnostics sink turned into an exception.
pub(crate) fn get_encoding(
    function: &str,
    position: u32,
    encoding: &[u8],
    diag: &mut dyn Diagnostics,
) -> CallResult<Charset> {
    let Some(found) = name_to_encoding(encoding) else {
        return Err(PhpError::value_error(PhpString::concat(&[
            format!("{function}(): Argument #{position} ($encoding) must be a valid encoding, \"")
                .as_bytes(),
            c_string(encoding),
            b"\" given",
        ]))
        .into());
    };
    let deprecated = match found.name {
        "BASE64" => Some(
            "Handling Base64 via mbstring is deprecated; use base64_encode/base64_decode instead",
        ),
        "Quoted-Printable" => Some(
            "Handling QPrint via mbstring is deprecated; use quoted_printable_encode/quoted_printable_decode instead",
        ),
        "HTML-ENTITIES" => Some(
            "Handling HTML entities via mbstring is deprecated; use htmlspecialchars, htmlentities, or \
             mb_encode_numericentity/mb_decode_numericentity instead",
        ),
        "UUENCODE" => Some(
            "Handling Uuencode via mbstring is deprecated; use convert_uuencode/convert_uudecode instead",
        ),
        _ => None,
    };
    if let Some(message) = deprecated {
        diag.deprecated(format!("{function}(): {message}"))?;
    }
    if found.name == "UTF-8" {
        Ok(Charset::Utf8)
    } else if found.single_byte {
        let identity = match found.name {
            "ISO-8859-1" | "8bit" => 0x100,
            "ASCII" | "7bit" => 0x80,
            _ => 0,
        };
        Ok(Charset::SingleByte { identity })
    } else {
        Err(unsupported(function, encoding).into())
    }
}

/// Converts the case of `string` in `charset`: `mb_strtoupper()`, `mb_strtolower()` and `mb_convert_case()` once
/// the arguments are parsed (`function` and `encoding` name the call in errors).
///
/// Port of `php_unicode_convert_case()` (PHPSRC/ext/mbstring/php_unicode.c) through `twig-unicode`: the characters
/// are decoded (an invalid byte becomes an error marker, printed `?`), mapped in Unicode, and encoded back (a code
/// point the charset cannot hold becomes `?`, mbstring's substitution character).
///
/// # Errors
///
/// The twig-rs `Error` of [`get_encoding`] for a single-byte encoding without identity range.
pub(super) fn convert_case(
    function: &str,
    encoding: &[u8],
    mode: twig_unicode::CaseMode,
    string: &[u8],
    charset: Charset,
) -> CallResult<PhpString> {
    let identity = match charset {
        Charset::Utf8 => return Ok(PhpString::from(twig_unicode::convert_case(mode, string))),
        Charset::SingleByte { identity: 0 } => return Err(unsupported(function, encoding).into()),
        Charset::SingleByte { identity } => identity,
    };
    // decode: a byte outside the identity range is an error marker (0xFF is never valid UTF-8)
    let mut utf8 = Vec::with_capacity(string.len());
    let mut buffer = [0; 4];
    for &byte in string {
        if u32::from(byte) < identity {
            utf8.extend_from_slice(char::from(byte).encode_utf8(&mut buffer).as_bytes());
        } else {
            utf8.push(0xFF);
        }
    }
    let converted = twig_unicode::convert_case(mode, &utf8);
    let converted = String::from_utf8_lossy(&converted);
    Ok(PhpString::from(
        converted
            .chars()
            .map(|character| match u32::from(character) {
                code if code < identity => code as u8,
                _ => b'?',
            })
            .collect::<Vec<u8>>(),
    ))
}

/// `mb_strlen($string, $encoding)` once the encoding is resolved: code points for UTF-8 (an ill-formed sequence
/// counts as one), bytes for a single-byte encoding.
///
/// Port of `mb_get_strlen()` (PHPSRC/ext/mbstring/mbstring.c).
pub(crate) fn strlen(string: &[u8], charset: Charset) -> i64 {
    match charset {
        Charset::Utf8 => twig_unicode::mb_strlen(string) as i64,
        Charset::SingleByte { .. } => string.len() as i64,
    }
}

/// `mb_substr($string, $start, $length, $encoding)` once the arguments are parsed and the encoding resolved (see
/// [`substr_range_error`] for the checks PHP makes first).
///
/// Port of `PHP_FUNCTION(mb_substr)` and `mb_get_substr()` (PHPSRC/ext/mbstring/mbstring.c): UTF-8 through
/// `twig-unicode` (ill-formed sequences become `?`), single-byte encodings by byte offsets.
///
/// # Errors
///
/// The `ValueError`s of [`substr_range_error`].
pub(crate) fn substr(
    string: &[u8],
    start: i64,
    length: Option<i64>,
    charset: Charset,
) -> CallResult<PhpString> {
    if let Some(error) = substr_range_error(start, length) {
        return Err(error.into());
    }
    match charset {
        Charset::Utf8 => Ok(PhpString::from(
            twig_unicode::mb_substr(string, start, length)
                .map_err(|error| PhpError::value_error(error.message().to_owned()))?,
        )),
        Charset::SingleByte { .. } => {
            let len = string.len() as u64;
            let mblen = if start < 0 || length.is_some_and(|l| l < 0) {
                len
            } else {
                0
            };
            let from = if start >= 0 {
                start as u64
            } else if start.unsigned_abs() < mblen {
                mblen - start.unsigned_abs()
            } else {
                0
            };
            let count = match length {
                None => u64::MAX,
                Some(l) if l >= 0 => l as u64,
                Some(l) if from < mblen && l.unsigned_abs() < mblen - from => {
                    (mblen - from) - l.unsigned_abs()
                }
                Some(_) => 0,
            };
            if count == 0 || from >= len {
                return Ok(PhpString::empty());
            }
            let end = from.saturating_add(count).min(len);
            Ok(PhpString::from(&string[from as usize..end as usize]))
        }
    }
}

/// The `ValueError` of `mb_substr()` for `$start` or `$length` equal to `PHP_INT_MIN`, raised before the encoding
/// is resolved.
pub(crate) fn substr_range_error(start: i64, length: Option<i64>) -> Option<PhpError> {
    let range = format!("must be between {} and {}", i64::MIN + 1, i64::MAX);
    if start == i64::MIN {
        Some(PhpError::value_error(format!(
            "mb_substr(): Argument #2 ($start) {range}"
        )))
    } else if length == Some(i64::MIN) {
        Some(PhpError::value_error(format!(
            "mb_substr(): Argument #3 ($length) {range}"
        )))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use twig_unicode::CaseMode;

    fn canonical(name: &[u8]) -> Option<&'static str> {
        name_to_encoding(name).map(|encoding| encoding.name)
    }

    #[test]
    fn resolves_names_like_mbstring() {
        assert_eq!(canonical(b"UTF-8"), Some("UTF-8"));
        assert_eq!(canonical(b"utf8"), Some("UTF-8"));
        assert_eq!(canonical(b"Utf-8\0junk"), Some("UTF-8"));
        assert_eq!(canonical(b"latin1"), Some("ISO-8859-1"));
        assert_eq!(canonical(b"shift_jis"), Some("SJIS"));
        assert_eq!(canonical(b"html"), Some("HTML-ENTITIES"));
        assert_eq!(canonical(b"UTF_8"), None);
        assert_eq!(canonical(b"pass"), None);
        assert_eq!(canonical(b""), None);
    }

    #[test]
    fn reports_unknown_and_unsupported_encodings() {
        let mut diag: Vec<twig_value::Diagnostic> = Vec::new();
        assert_eq!(
            get_encoding("mb_strlen", 2, b"UTF-8", &mut diag).ok(),
            Some(Charset::Utf8)
        );
        assert_eq!(
            get_encoding("mb_strlen", 2, b"KOI8-R", &mut diag).ok(),
            Some(Charset::SingleByte { identity: 0 })
        );
        let unknown = get_encoding("mb_strlen", 2, b"NOPE-1", &mut diag).unwrap_err();
        assert_eq!(
            unknown.message(),
            "mb_strlen(): Argument #2 ($encoding) must be a valid encoding, \"NOPE-1\" given"
        );
        let unsupported = get_encoding("mb_substr", 4, b"SJIS", &mut diag).unwrap_err();
        assert_eq!(
            unsupported.message(),
            "mb_substr(): encoding \"SJIS\" is not supported by twig-rs"
        );
        let koi8 = get_encoding("mb_strtoupper", 2, b"KOI8-R", &mut diag).unwrap();
        let case =
            convert_case("mb_strtoupper", b"KOI8-R", CaseMode::Upper, b"a", koi8).unwrap_err();
        assert_eq!(
            case.message(),
            "mb_strtoupper(): encoding \"KOI8-R\" is not supported by twig-rs"
        );
        assert!(diag.is_empty());
        assert_eq!(
            get_encoding("mb_strlen", 2, b"UUENCODE", &mut diag).ok(),
            Some(Charset::SingleByte { identity: 0 })
        );
        assert_eq!(diag.len(), 1);
    }

    #[test]
    fn single_byte_substr_follows_mb_substr() {
        let sb = Charset::SingleByte { identity: 0 };
        let s = |start, length| substr(b"abcdef", start, length, sb).unwrap();
        assert_eq!(s(1, Some(2)), "bc");
        assert_eq!(s(-2, None), "ef");
        assert_eq!(s(-10, Some(2)), "ab");
        assert_eq!(s(1, Some(-1)), "bcde");
        assert_eq!(s(4, Some(-3)), "");
        assert_eq!(s(10, None), "");
        assert_eq!(s(2, Some(100)), "cdef");
        assert!(substr(b"x", i64::MIN, None, sb).is_err());
    }

    #[test]
    fn converts_the_case_of_identity_encodings() {
        let latin1 = Charset::SingleByte { identity: 0x100 };
        let ascii = Charset::SingleByte { identity: 0x80 };
        let upper = |s: &[u8], charset| {
            convert_case("mb_strtoupper", b"x", CaseMode::Upper, s, charset).unwrap()
        };
        assert_eq!(
            upper(b"ab\xe9\xdf\xff\xb5", latin1),
            b"AB\xc9SS??".as_slice()
        );
        assert_eq!(upper(b"ab\xe9", ascii), b"AB?".as_slice());
        let title = convert_case(
            "mb_convert_case",
            b"x",
            CaseMode::Title,
            b"\xe9t\xe9 x",
            latin1,
        )
        .unwrap();
        assert_eq!(title, b"\xc9t\xe9 X".as_slice());
    }
}
