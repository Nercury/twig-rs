//! `iconv()` for most of glibc's charsets, written without glibc code, under every name glibc gives them (1,093 of
//! the 1,180 names `iconv -l` lists in the oracle):
//!
//! - the Unicode charsets: UTF-8, UTF-16, UTF-16BE, UTF-16LE, UTF-32, UTF-32BE, UTF-32LE, UCS-2, UCS-2BE, UCS-2LE,
//!   UCS-4, UCS-4BE, UCS-4LE;
//! - ANSI_X3.4-1968 (ASCII) and ISO-8859-1;
//! - the 218 single-byte charsets with tables (`codepage.rs`): ISO-8859-2 to ISO-8859-16 and ISO-8859-9E, the
//!   Windows, IBM PC, OS/2 and EBCDIC codepages, KOI8, the Mac charsets, the ISO 646 variants and more (CP1255,
//!   CP1258 and TCVN-5712 with the letters and marks they combine);
//! - the multi-byte charsets (`multibyte.rs`): SJIS, CP932, EUC-JP, EUC-JP-MS, GBK, GB18030, EUC-CN (GB 2312),
//!   BIG5, BIG5-HKSCS, EUC-KR, UHC, JOHAB and EUC-TW;
//! - the stateful ISO-2022-JP, ISO-2022-JP-2 and ISO-2022-KR (`iso2022.rs`).
//!
//! Any other charset glibc knows (ISO-2022-CN, UTF-7, JIS X 0213, the EBCDIC double-byte codepages, ...) raises
//! `Error: iconv(): conversion of charset "X" is not supported by twig-rs`, and so do the conversions between GB 2312
//! and GBK or BIG5, which glibc does with modules of their own (`direct_module`; its module for IBM420 and CP1008 is
//! the byte table `direct_table.rs`); a name glibc does not know behaves as in PHP.
//!
//! Clean room: the PHP layer ports `PHP_FUNCTION(iconv)` and `php_iconv_string()` (PHPSRC/ext/iconv/iconv.c), the
//! Unicode charsets follow their published specifications (RFC 3629, RFC 2781, ISO/IEC 10646, ISO/IEC 8859-1), and
//! everything glibc 2.41 decides on its own — which values each charset rejects, byte order marks, the tables of the
//! single-byte and multi-byte charsets (compared with the Unicode Consortium's mapping files and the WHATWG Encoding
//! Standard's indexes, whose differences they record), which sequences each decoder skips, which characters CP1255,
//! CP1258 and TCVN-5712 combine, the ISO-2022 designations, charset names and their aliases, the //TRANSLIT and
//! //IGNORE suffixes, the transliteration tables, how a call reports errors and where it stops, which pairs of
//! charsets it converts with a module of their own — was learned black-box from the oracle (`twig-oracle:3.31.1`, PHP
//! 8.5.13 on glibc 2.41, `LC_CTYPE=C.UTF-8`) through PHP and through the public `iconv_open()`/`iconv()` interface
//! (the probes `multibyte_probe.c` and `translit_probe.c`). No glibc source was read. The vectors in
//! tests/vectors/string/charset_basic and their generators show the observations.
//!
//! - `names`: how glibc parses a charset name, and the names it knows (`name_table.rs`, from `iconv -l`);
//! - `codec`: decoding and encoding each charset;
//! - `codepage`: the tables of the single-byte charsets (`codepage_table.rs`);
//! - `multibyte`: the multi-byte charsets (`multibyte_table.rs`, `multibyte_differences.md`);
//! - `iso2022`: the ISO-2022 charsets, their designations and shifts;
//! - `engine`: one `iconv(3)` call (rounds of 8160 characters, errors, `E2BIG`, //TRANSLIT and //IGNORE, the
//!   characters decoders hold back, the room glibc wants before a character) and the flush;
//! - `translit`: the //TRANSLIT replacements (`translit_table.rs`);
//! - this module: PHP's loop around `iconv(3)`, its notices and its `false`, and the conversions glibc does with a
//!   module of their own (`direct_table.rs`).

mod codec;
mod codepage;
#[rustfmt::skip]
mod codepage_table;
#[rustfmt::skip]
mod direct_table;
mod engine;
mod iso2022;
mod multibyte;
#[rustfmt::skip]
mod multibyte_table;
#[rustfmt::skip]
mod name_table;
mod names;
mod translit;
#[rustfmt::skip]
mod translit_table;

use codec::Charset;
use engine::{Converter, Status};
use names::Lookup;

use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::Value;

/// `ICONV_CSNMAXLEN` (PHPSRC/ext/iconv/php_iconv.h): charset names must be shorter.
const ICONV_CSNMAXLEN: usize = 64;

/// PHP's `iconv(string $from_encoding, string $to_encoding, string $string): string|false`; `None` is `false`.
///
/// Port of `PHP_FUNCTION(iconv)` (PHPSRC/ext/iconv/iconv.c): the arguments are parsed in coercive mode, then
/// [`iconv_bytes`] converts.
///
/// # Errors
///
/// Argument `TypeError`s, the `Error` for a charset twig-rs does not convert, and diagnostics the sink turns into
/// exceptions.
pub fn iconv(
    from_encoding: &Value,
    to_encoding: &Value,
    string: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<PhpString>> {
    let from = param::parse_str(
        from_encoding,
        &Arg::new("iconv", 1, "from_encoding").declared("string"),
        diag,
    )?;
    let to = param::parse_str(
        to_encoding,
        &Arg::new("iconv", 2, "to_encoding").declared("string"),
        diag,
    )?;
    let string = param::parse_str(
        string,
        &Arg::new("iconv", 3, "string").declared("string"),
        diag,
    )?;
    iconv_bytes(&from, &to, &string, diag)
}

/// `iconv($from, $to, $string)` on PHP strings: `Some(converted)`, or `None` (PHP's `false`) after the warning or
/// notice PHP raises.
///
/// Port of the body of `PHP_FUNCTION(iconv)` after argument parsing, with `php_iconv_string()` and
/// `_php_iconv_show_error()` (PHPSRC/ext/iconv/iconv.c) on top of `engine::Converter`, the model of glibc's `iconv(3)`.
///
/// # Errors
///
/// `Error: iconv(): conversion of charset "X" is not supported by twig-rs` for a charset glibc knows but this module
/// does not convert (`X` is the name as given; the source charset is reported first), and diagnostics the sink
/// turns into exceptions.
pub fn iconv_bytes(
    from: &[u8],
    to: &[u8],
    string: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<PhpString>> {
    if from.len() >= ICONV_CSNMAXLEN || to.len() >= ICONV_CSNMAXLEN {
        diag.warn(format!(
            "iconv(): Encoding parameter exceeds the maximum allowed length of {ICONV_CSNMAXLEN} characters"
        ))?;
        return Ok(None);
    }
    // C sees the names up to their first NUL
    let in_charset = c_str(from);
    let out_charset = c_str(to);
    let source = names::parse(in_charset);
    let target = names::parse(out_charset);
    let (from_charset, to_charset) = match (source.lookup, target.lookup) {
        (Lookup::Unknown, _) | (_, Lookup::Unknown) => {
            // PHP_ICONV_ERR_WRONG_CHARSET
            diag.warn(PhpString::concat(&[
                b"iconv(): Wrong encoding, conversion from \"",
                in_charset,
                b"\" to \"",
                out_charset,
                b"\" is not allowed",
            ]))?;
            return Ok(None);
        }
        (Lookup::Unsupported, _) => return Err(unsupported(in_charset)),
        (_, Lookup::Unsupported) => return Err(unsupported(out_charset)),
        (Lookup::Supported(f), Lookup::Supported(t)) => (f, t),
    };
    match direct_module(from_charset, to_charset) {
        Some(Direct::Refused) => return Err(unsupported(in_charset)),
        // every byte converts: one iconv(3) call with room to spare, whatever the suffixes
        Some(Direct::Bytes(bytes)) => {
            let out: Vec<u8> = string.iter().map(|&b| bytes[usize::from(b)]).collect();
            return Ok(Some(PhpString::from(out)));
        }
        None => {}
    }
    let mut converter = Converter::new(from_charset, to_charset, target.translit, target.ignore);
    match php_iconv_string(string, &mut converter, php_check_ignore(out_charset)) {
        Ok(out) => Ok(Some(PhpString::from(out))),
        Err(Status::Incomplete) => {
            diag.notice("iconv(): Detected an incomplete multibyte character in input string")?;
            Ok(None)
        }
        Err(Status::Illegal) => {
            diag.notice("iconv(): Detected an illegal character in input string")?;
            Ok(None)
        }
        Err(Status::Unsupported) => Err(unsupported(out_charset)),
        Err(Status::TooBig | Status::Done) => {
            // "should not happen" in PHP: glibc never reports E2BIG with the input used up
            diag.warn("iconv(): Buffer length exceeded")?;
            Ok(None)
        }
    }
}

/// Port of the conversion loop of `php_iconv_string()` (PHPSRC/ext/iconv/iconv.c): an output buffer of the input
/// length plus 32 bytes that grows by the input length on `E2BIG`, and, with `ignore_ilseq`, glibc's `EILSEQ` taken
/// as success when at most one input byte is left, else skipping one input byte and calling again. Then the flush
/// (`iconv(cd, NULL, NULL, ...)`, the buffer growing by 16 bytes on `E2BIG`), which writes the character CP1255 and
/// CP1258 may still hold back. `Err` is the status that makes PHP return `false`.
fn php_iconv_string(
    input: &[u8],
    converter: &mut Converter,
    ignore_ilseq: bool,
) -> Result<Vec<u8>, Status> {
    let in_len = input.len();
    let mut out = Vec::with_capacity(in_len + 32);
    let mut buffer_size = in_len + 32;
    let mut pos = 0;
    while pos < in_len {
        let space = buffer_size - out.len();
        let (consumed, status) = converter.call(&input[pos..], &mut out, space);
        pos += consumed;
        match status {
            Status::Done => {}
            Status::Illegal if ignore_ilseq => {
                if in_len - pos <= 1 {
                    break;
                }
                pos += 1;
            }
            Status::TooBig if pos < in_len => buffer_size += in_len,
            other => return Err(other),
        }
    }
    // the flush, growing the buffer by 16 bytes on E2BIG
    loop {
        let space = buffer_size - out.len();
        match converter.flush(&mut out, space) {
            Status::Done => return Ok(out),
            Status::TooBig => buffer_size += 16,
            other => return Err(other),
        }
    }
}

/// `_php_check_ignore()` (PHPSRC/ext/iconv/iconv.c, compiled in because glibc's //IGNORE makes `iconv()` fail):
/// whether the target name ends in `//IGNORE` or `//IGNORE//TRANSLIT`, case-sensitively.
fn php_check_ignore(charset: &[u8]) -> bool {
    (charset.len() >= 9 && charset.ends_with(b"//IGNORE"))
        || (charset.len() >= 19 && charset.ends_with(b"//IGNORE//TRANSLIT"))
}

/// A conversion glibc does with a module of its own instead of through its internal UCS-4. Found black-box: its
/// results differ from the composition of the two charsets' conversions.
#[derive(Clone, Copy, Debug)]
enum Direct {
    /// Byte for byte, every byte converting under every suffix (`direct_table.rs`, measured by `direct.gen.php`, which
    /// compares every byte of each pair of single-byte charsets): IBM420 and CP1008, in both directions.
    Bytes(&'static [u8; 256]),
    /// With tables and rules this module does not model, so not converted (reference/deviations.md): GB 2312
    /// (`EUC-CN`) and `GBK`, GB 2312 and `BIG5` (whose table maps simplified characters to traditional ones and back),
    /// in both directions.
    Refused,
}

/// Whether glibc converts from `from` to `to` with a module of its own, and how.
fn direct_module(from: Charset, to: Charset) -> Option<Direct> {
    use multibyte::Multibyte::{Big5, EucCn, Gbk};
    if matches!(
        (from, to),
        (Charset::Multibyte(EucCn), Charset::Multibyte(Gbk | Big5))
            | (Charset::Multibyte(Gbk | Big5), Charset::Multibyte(EucCn))
    ) {
        return Some(Direct::Refused);
    }
    direct_table::DIRECT
        .iter()
        .find(|&&(f, t, _)| f == from && t == to)
        .map(|(_, _, bytes)| Direct::Bytes(bytes))
}

/// The bytes before the first NUL.
fn c_str(s: &[u8]) -> &[u8] {
    memchr::memchr(0, s).map_or(s, |end| &s[..end])
}

/// The `Error` for a charset glibc knows but twig-rs does not convert (reference/deviations.md).
fn unsupported(charset: &[u8]) -> PhpError {
    PhpError::error(PhpString::concat(&[
        b"iconv(): conversion of charset \"",
        charset,
        b"\" is not supported by twig-rs",
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{Diagnostic, Level};

    fn conv(from: &str, to: &str, s: &[u8]) -> (Option<Vec<u8>>, Vec<Diagnostic>) {
        let mut diag: Vec<Diagnostic> = Vec::new();
        let r = iconv_bytes(from.as_bytes(), to.as_bytes(), s, &mut diag).expect("no exception");
        (r.map(|s| s.as_bytes().to_vec()), diag)
    }

    #[test]
    fn basics() {
        assert_eq!(
            conv("UTF-8", "ISO-8859-1", "a\u{e9}".as_bytes()).0.unwrap(),
            b"a\xe9"
        );
        assert_eq!(conv("UTF-8", "UTF-16", b"a").0.unwrap(), b"\xff\xfea\x00");
        assert_eq!(
            conv("UTF-8", "ASCII//TRANSLIT", "\u{20ac}".as_bytes())
                .0
                .unwrap(),
            b"EUR"
        );
        assert_eq!(
            conv("UTF-8", "ASCII//IGNORE", "a\u{e9}b".as_bytes())
                .0
                .unwrap(),
            b"ab"
        );
        let (r, d) = conv("UTF-8", "ASCII", "a\u{e9}b".as_bytes());
        assert_eq!(r, None);
        assert_eq!(
            d,
            [Diagnostic::new(
                Level::Notice,
                "iconv(): Detected an illegal character in input string"
            )]
        );
        let (r, d) = conv("UTF-8", "UTF-16", b"a\xc3");
        assert_eq!(r, None);
        assert_eq!(
            d,
            [Diagnostic::new(
                Level::Notice,
                "iconv(): Detected an incomplete multibyte character in input string"
            )]
        );
        let (r, d) = conv("UTF-8", "NOPE", b"a");
        assert_eq!(r, None);
        assert_eq!(
            d,
            [Diagnostic::new(
                Level::Warning,
                "iconv(): Wrong encoding, conversion from \"UTF-8\" to \"NOPE\" is not allowed"
            )]
        );
    }

    #[test]
    fn php_ignore_skips_a_byte_after_an_early_stop() {
        // glibc stops after its first round with EILSEQ; PHP then skips one input byte
        let mut input = "\u{20ac}".as_bytes().to_vec();
        input.extend(std::iter::repeat_n(b'a', 9000));
        assert_eq!(
            conv("UTF-8", "ISO-8859-1//IGNORE", &input).0.unwrap().len(),
            8999
        );
        // with //ignore PHP does not know glibc skips; the lost status of an early round makes it succeed
        input[..3].copy_from_slice(b"\xff\xff\xff");
        assert_eq!(
            conv("UTF-8", "ISO-8859-1//ignore", &input).0.unwrap().len(),
            9000
        );
    }

    #[test]
    fn codepages() {
        assert_eq!(
            conv("UTF-8", "CP1252", "\u{20ac}".as_bytes()).0.unwrap(),
            b"\x80"
        );
        assert_eq!(
            conv("koi8r", "UTF-8", b"\xc1").0.unwrap(),
            "\u{430}".as_bytes()
        );
        // the Cyrillic alternative of U+20BD where the target has Cyrillic letters
        assert_eq!(
            conv("UTF-8", "KOI8-R//TRANSLIT", "\u{20bd}".as_bytes())
                .0
                .unwrap(),
            b"\xd2\xd5\xc2"
        );
        // CP1258 combines a letter and a mark, and writes a character it has no byte for as both
        assert_eq!(
            conv("CP1258", "UTF-8", b"A\xcc").0.unwrap(),
            "\u{c0}".as_bytes()
        );
        assert_eq!(
            conv("UTF-8", "CP1258", "\u{c3}".as_bytes()).0.unwrap(),
            b"A\xde"
        );
        // the flush writes the held last character, which //IGNORE cannot skip without failing
        assert_eq!(
            conv("CP1258", "ISO-8859-1//IGNORE", b"\xd5a").0.unwrap(),
            b"a"
        );
        let (r, d) = conv("CP1258", "ISO-8859-1//IGNORE", b"a\xd5");
        assert_eq!(r, None);
        assert_eq!(
            d,
            [Diagnostic::new(
                Level::Notice,
                "iconv(): Detected an illegal character in input string"
            )]
        );
    }

    #[test]
    fn modules_of_their_own() {
        // IBM420 and CP1008: bytes neither charset's own conversion takes, under any suffix and name
        assert_eq!(
            conv("IBM420", "CP1008", b"\x45\xfa").0.unwrap(),
            b"\xa9\xff"
        );
        assert_eq!(
            conv("IBM-1008", "EBCDIC-CP-AR1//TRANSLIT", b"\xff\x5b")
                .0
                .unwrap(),
            b"\xfa\x53"
        );
        let (r, d) = conv("IBM420", "UTF-8", b"\x45");
        assert_eq!((r, d.len()), (None, 1));
        // GB 2312 and GBK
        let mut diag: Vec<Diagnostic> = Vec::new();
        assert!(iconv_bytes(b"GB2312", b"GBK", b"a", &mut diag).is_err());
    }

    #[test]
    fn unsupported_charsets() {
        let mut diag: Vec<Diagnostic> = Vec::new();
        let e = iconv_bytes(b"IBM930", b"UTF-8", b"a", &mut diag).unwrap_err();
        assert_eq!(
            e.to_string(),
            "Error: iconv(): conversion of charset \"IBM930\" is not supported by twig-rs"
        );
        let e = iconv_bytes(b"UTF-8", b"ibm-930//TRANSLIT", b"", &mut diag).unwrap_err();
        assert_eq!(
            e.message_lossy(),
            "iconv(): conversion of charset \"ibm-930//TRANSLIT\" is not supported by twig-rs"
        );
        // a name glibc does not know wins: PHP's warning
        assert_eq!(
            iconv_bytes(b"IBM930", b"NOPE", b"a", &mut diag).unwrap(),
            None
        );
        assert_eq!(diag.len(), 1);
    }

    /// The call vectors (tests/vectors/string/charset_basic/calls.json, see calls.gen.php there): single calls of
    /// glibc's `iconv()` from UCS-4BE, with the output space of each set, against `engine::Converter`.
    #[test]
    fn calls() {
        fn unhex(h: &str) -> Vec<u8> {
            (0..h.len() / 2)
                .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap())
                .collect()
        }
        fn hex(b: &[u8]) -> String {
            b.iter().map(|x| format!("{x:02x}")).collect()
        }
        fn status(s: Status) -> &'static str {
            match s {
                Status::Done => "OK",
                Status::TooBig => "E2BIG",
                Status::Illegal => "EILSEQ",
                Status::Incomplete => "EINVAL",
                Status::Unsupported => "UNSUPPORTED",
            }
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/vectors/string/charset_basic/calls.json"
        );
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("calls.json")).unwrap();
        let names: Vec<Vec<u8>> = file["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| unhex(h.as_str().unwrap()))
            .collect();
        let cases = file["cases"].as_array().unwrap();
        // an alternative no target shows of which more than what translit.gen.php measures shows here, where the room
        // runs out (reference/deviations.md): U+33A8 SQUARE M OVER S SQUARED into ISO-2022-JP after JIS X 0208
        const KNOWN: [&str; 1] = ["ISO-2022-JP//TRANSLIT 00003042000033a800000061"];
        let mut known = 0;
        let mut failures = Vec::new();
        for case in cases {
            let case = case.as_array().unwrap();
            let to = &names[case[1].as_u64().unwrap() as usize];
            let input = unhex(case[2].as_str().unwrap());
            let target = names::parse(to);
            let Lookup::Supported(t) = target.lookup else {
                panic!("{}: not supported", String::from_utf8_lossy(to));
            };
            let new = || Converter::new(Charset::Ucs4, t, target.translit, target.ignore);
            let mut got = String::new();
            if case[0] == "seq" {
                let mut conv = new();
                let mut pos = 0;
                for size in case[3].as_str().unwrap().split(',') {
                    let mut out = Vec::new();
                    if size == "f" {
                        let s = conv.flush(&mut out, 64);
                        got += &format!(" [flush {} out={}]", status(s), hex(&out));
                    } else {
                        let (consumed, s) =
                            conv.call(&input[pos..], &mut out, size.parse().unwrap());
                        pos += consumed;
                        got += &format!(" [{} c={consumed} out={}]", status(s), hex(&out));
                    }
                }
            } else {
                let first = case[3].as_u64().unwrap() as usize;
                for k in 1..=12 {
                    let mut conv = new();
                    let mut out = Vec::new();
                    let (c1, s1) = conv.call(&input, &mut out, first);
                    if k == 1 {
                        got += &format!(" first={},{c1},{}", status(s1), hex(&out));
                    }
                    out.clear();
                    let (c2, s2) = conv.call(&input[c1..], &mut out, k);
                    if c2 > 0 || s2 != Status::TooBig || !out.is_empty() {
                        got += &format!(" {k}:{}:{c2}:{}", status(s2), hex(&out));
                    }
                    if c2 > 0 || s2 != Status::TooBig {
                        break;
                    }
                }
            }
            if got.len() > 64 {
                // fnv1a64
                let h = got.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
                });
                got = format!("#{h:016x}");
            }
            let line = format!(
                "{} {}",
                String::from_utf8_lossy(to),
                case[2].as_str().unwrap()
            );
            if KNOWN.contains(&line.as_str()) {
                assert_ne!(got, case[4].as_str().unwrap(), "{line} is modelled now");
                known += 1;
            } else if got != case[4].as_str().unwrap() {
                failures.push(format!(
                    "{} {} {}: got{got}, want{}",
                    case[0],
                    String::from_utf8_lossy(to),
                    case[2],
                    case[4]
                ));
            }
        }
        assert!(cases.len() > 10_000);
        assert_eq!(known, KNOWN.len());
        assert!(
            failures.is_empty(),
            "{} of {} calls differ:
{}",
            failures.len(),
            cases.len(),
            failures[..failures.len().min(20)].join(
                "
"
            )
        );
    }
}
