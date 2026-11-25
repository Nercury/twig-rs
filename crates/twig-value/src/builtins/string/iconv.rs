//! `iconv()` (reference/php/stdlib.md §7): Twig's `convert_encoding` filter and the non-UTF-8
//! paths of the escaper.
//!
//! PHP's `iconv()` (PHPSRC/ext/iconv/iconv.c) hands the work to glibc 2.41 in the oracle image.
//! This module reproduces that combination: the charset-name parsing of `iconv_open()`
//! (suffixes, `strip()`, aliases, the gconv cache), the conversion through glibc's internal UCS-4
//! with each gconv module's validity rules, shift states and error handling (`//IGNORE`,
//! `//TRANSLIT`, Unicode tag characters, byte order marks), the round structure of glibc's
//! conversion driver (`iconv/skeleton.c`: rounds of 8160 characters, reruns of the first step
//! with saved shift state, `RESET_INPUT_BUFFER`, flushing with `EMIT_SHIFT_TO_INIT`), and the
//! loop of `php_iconv_string()` around `iconv()` including the `ICONV_BROKEN_IGNORE` workaround
//! PHP compiles in for glibc and the final flush of shift sequences. Mapping tables were
//! measured with the oracle (`iconv_tables.gen.php`, `iconv/cjk_tables.gen.php`); the module
//! algorithms follow glibc's `iconvdata` sources and are checked by oracle vectors.
//!
//! # Supported charsets
//!
//! Every name glibc's gconv cache resolves to one of these modules works, under every alias:
//!
//! * Unicode: UTF-8 (glibc's: up to six bytes, code points up to `0x7FFFFFFF`), UCS-2
//!   (`UCS-2`/`UCS-2LE` little endian, `UCS-2BE`/`UNICODEBIG`), UCS-4 (`UCS-4` big endian,
//!   `UCS-4LE`), `UNICODE` (UCS-2 with byte order mark), UTF-16, UTF-16LE, UTF-16BE, UTF-32,
//!   UTF-32LE and UTF-32BE.
//! * ASCII (`ANSI_X3.4-1968`), ISO-8859-1 and the 221 stateless single-byte modules of glibc:
//!   ISO-8859-2 to -16 and -9E, the Windows codepages CP1250 to CP1254, CP1256 and CP1257, KOI8-R,
//!   KOI8-U, KOI8-RU, KOI8-T, KOI-8, the DOS/IBM codepages (CP437, CP737, CP775, CP850 to CP869,
//!   CP1125, ...), the EBCDIC codepages (IBM037, IBM273, ..., IBM1047, IBM1140 to IBM1149, the
//!   `EBCDIC-*` national variants, ...), the Mac charsets (MACINTOSH, MAC-CENTRALEUROPE, MAC-IS,
//!   MAC-SAMI, MAC-UK, CP10007), the ISO 646 national variants, HP, Georgian, Armenian, Thai
//!   (TIS-620), Vietnamese (VISCII) and the remaining ISO/ECMA sets (`iconv_tables.rs` lists them).
//! * CP1255, CP1258 and TCVN5712-1, with glibc's composition of base letters and combining marks
//!   when decoding (the last character waits in the shift state and is written on flush) and
//!   decomposition when encoding.
//! * Japanese: EUC-JP, EUC-JP-MS (`EUCJP-WIN`), SJIS (`SHIFT_JIS`), CP932 (`WINDOWS-31J`),
//!   ISO-2022-JP, ISO-2022-JP-2 (with its G2 sets and language tags), ISO-2022-JP-3,
//!   EUC-JISX0213 and Shift_JISX0213 (with the JIS X 0213 composition of kana and IPA letters
//!   with combining marks).
//! * Chinese: EUC-CN (`GB2312`), GBK (`CP936`), GB18030, BIG5 (`CP950`) and BIG5-HKSCS (with its
//!   combining sequences for Ê and ê).
//! * Korean: EUC-KR, UHC (`CP949`) and ISO-2022-KR (with its `ESC $ ) C` header, written even for
//!   empty input and again inside transliterations of the first call, as glibc does).
//! * The modules glibc's gconv cache uses for some pairs instead of going through UCS-4: GB2312
//!   to and from BIG5 (unmapped characters become box characters under `//IGNORE`), GBK to and
//!   from GB2312 (GB2312 to GBK copies bytes unchecked and reports input that ends in a two-byte
//!   character as incomplete), and IBM1008 to and from IBM420. glibc never transliterates there.
//!
//! glibc's bugs are part of the behaviour: for example UHC's rejected U+327E (`A2 E8`) skips four
//! bytes under `//IGNORE` because the table helper has already consumed two, and ISO-2022-JP-3
//! buffers a composition base without checking for room. Where glibc reads past the end of the
//! input (that UHC case at the end of the string) this port reports the incomplete character the
//! oracle shows.
//!
//! `//TRANSLIT` uses the transliteration table of the oracle's `C.UTF-8` locale (`LC_CTYPE`
//! `translit_neutral` and `translit_combining`, 6492 entries, `default_missing` `?`), read from
//! the compiled locale by `iconv_tables.gen.php`.
//!
//! # Not supported
//!
//! * Charsets glibc knows but this port does not convert (listed at [`iconv_bytes`]): the
//!   conversion throws an `Error` naming the charset instead of guessing. Unknown names
//!   (including `ISO-2022-JP-1`, which glibc does not have) behave like PHP (`Wrong encoding`
//!   warning, `false`).

#[rustfmt::skip]
mod cjk_tables;
mod cjk;
mod iso2022;
mod sbcs;
mod tables;
mod unicode;

use std::sync::OnceLock;

use super::iconv_tables as names;
use super::str_param;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::string::PhpString;
use crate::value::Value;

/// `ICONV_CSNMAXLEN` (PHPSRC/ext/iconv/php_iconv.h).
const ICONV_CSNMAXLEN: usize = 64;

/// `GCONV_NCHAR_GOAL` (glibc iconv/gconv_open.c): the intermediate buffer of a step holds this many
/// times the step's `__max_needed_to` bytes.
const NCHAR_GOAL: usize = 8160;

/// A stateless single-byte charset of glibc (an entry of `iconv_tables::SBCS`): its gconv module
/// name and the encoder's deviations from the inverse of the decoding table.
pub(super) struct Sbcs {
    /// The gconv module name (`"KOI8-RU//"`).
    pub(super) module: &'static str,
    /// Code points the encoder maps to a byte other than the inverse of the decoding table.
    pub(super) extra: &'static [(u16, u8)],
    /// Code points some byte decodes to but the encoder rejects.
    pub(super) missing: &'static [u16],
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

/// The outcome of one `BODY` of a decoding loop.
enum DStep {
    /// The body consumed this many bytes and pushed its characters (possibly none).
    Ok(usize),
    /// The body consumed this many bytes and pushed what fits, then stops the loop with
    /// `__GCONV_FULL_OUTPUT` (the second character of a pair went into the shift state).
    Full(usize),
    /// `__GCONV_INCOMPLETE_INPUT`.
    Incomplete,
    /// Invalid input: `STANDARD_FROM_LOOP_ERR_HANDLER(skip)` (`marks`) or a module handler that
    /// leaves the loop result alone when it ignores the error.
    Illegal { skip: usize, marks: bool },
}

/// The outcome of one `BODY` of an encoding loop.
enum EStep {
    /// The body consumed this many characters (0: it only wrote out buffered state) and wrote
    /// their bytes; a dropped Unicode tag character (`UNICODE_TAG_HANDLER`) consumes one.
    Ok(usize),
    /// `__GCONV_FULL_OUTPUT`; shift state the body changed stays changed.
    Full,
    /// `STANDARD_TO_LOOP_ERR_HANDLER(4)`: transliteration, `//IGNORE`, or an error.
    Unmappable,
    /// A character rejected outside the standard handler (surrogates for the UCS-2, UTF-16 and
    /// UTF-32 encoders): no transliteration; `marks` keeps `__GCONV_ILLEGAL_INPUT` when ignored.
    Illegal { marks: bool },
}

/// A decoding step (a gconv module in the from direction: charset to glibc's internal UCS-4).
trait Decode {
    /// `FROM_LOOP_MIN_NEEDED_FROM`: the loop stops with `__GCONV_INCOMPLETE_INPUT` when fewer
    /// bytes are left.
    fn min_needed(&self) -> usize {
        1
    }

    /// `FROM_LOOP_MAX_NEEDED_TO / 4`: the intermediate buffer holds `NCHAR_GOAL` times this many
    /// characters.
    fn max_chars(&self) -> usize {
        1
    }

    /// Bytes per character when glibc defines `RESET_INPUT_BUFFER` for the module (fixed width on
    /// both sides, no shift state).
    fn fixed_width(&self) -> Option<usize> {
        None
    }

    /// `PREPARE_LOOP` with effects (byte order mark detection on the first invocation);
    /// `Some(status)` returns from the step function early.
    fn prepare(&mut self, _input: &[u8], _pos: &mut usize, _invocations: u64) -> Option<Status> {
        None
    }

    /// A module loop that is not built from `iconv/loop.c` (`ucs4_internal_loop`).
    fn special_loop(
        &mut self,
        _input: &[u8],
        _pos: &mut usize,
        _out: &mut Vec<u32>,
        _max: usize,
        _ignore: bool,
        _irreversible: &mut usize,
    ) -> Option<Status> {
        None
    }

    /// One `BODY` at `input[pos..]` (not empty, at least `min_needed()` bytes); `out` has room
    /// for at least one more character (`max` is its capacity).
    fn body(&mut self, input: &[u8], pos: usize, out: &mut Vec<u32>, max: usize) -> DStep;

    /// `SAVE_RESET_STATE(1)`: the shift state.
    fn save(&self) -> u32 {
        0
    }

    /// `SAVE_RESET_STATE(0)`.
    fn restore(&mut self, _state: u32) {}

    /// Flushing: `EMIT_SHIFT_TO_INIT` in the from direction (characters still buffered in the
    /// state), or clearing the state for modules without it.
    fn emit_shift_to_init(&mut self, _out: &mut Vec<u32>) {
        self.restore(0);
    }

    /// Tells the step about `//IGNORE` for modules whose output depends on `ignore_errors_p()`
    /// beyond skipping (EUC-JP-MS).
    fn set_ignore(&mut self, _ignore: bool) {}
}

/// An encoding step (a gconv module in the to direction: glibc's internal UCS-4 to the charset).
trait Encode {
    /// `TO_LOOP_MIN_NEEDED_TO`: the loop stops with `__GCONV_FULL_OUTPUT` when less room is left.
    fn min_needed(&self) -> usize {
        1
    }

    /// `PREPARE_LOOP` with output (byte order marks, the ISO-2022-KR header), run on every call
    /// of the step function including the calls of the transliteration; `Some(status)` returns
    /// early.
    fn prepare(&mut self, _out: &mut Vec<u8>, _cap: usize, _invocations: u64) -> Option<Status> {
        None
    }

    /// A module loop that is not built from `iconv/loop.c` (`internal_ucs4_loop`): the number of
    /// characters consumed and the status.
    fn special_loop(
        &mut self,
        _chars: &[u32],
        _out: &mut Vec<u8>,
        _cap: usize,
    ) -> Option<(usize, Status)> {
        None
    }

    /// One `BODY` for `chars[i]` (with `chars[i + 1..]` visible for modules that look ahead);
    /// at least `min_needed()` bytes of room are left (`cap` is the buffer size).
    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep;

    /// Flushing: `PREPARE_LOOP` and `EMIT_SHIFT_TO_INIT` in the to direction (shift sequences,
    /// buffered characters), or clearing the state for modules without it.
    fn flush(&mut self, _out: &mut Vec<u8>, _cap: usize, _invocations: u64) -> Status {
        Status::Ok
    }
}

/// `MIN_NEEDED_OUTPUT` check of `iconv/loop.c` for decoders: room for one more character.
fn decode_loop(
    dec: &mut dyn Decode,
    input: &[u8],
    pos: &mut usize,
    out: &mut Vec<u32>,
    max: usize,
    ignore: bool,
    irreversible: &mut usize,
) -> Status {
    if let Some(status) = dec.special_loop(input, pos, out, max, ignore, irreversible) {
        return status;
    }
    let (end, min) = (input.len(), dec.min_needed());
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
        match dec.body(input, *pos, out, max) {
            DStep::Ok(n) => *pos += n,
            DStep::Full(n) => {
                *pos += n;
                result = Status::FullOutput;
                break;
            }
            DStep::Incomplete => {
                result = Status::IncompleteInput;
                break;
            }
            DStep::Illegal { skip, marks } => {
                if !ignore {
                    result = Status::IllegalInput;
                    break;
                }
                if *pos + skip > end {
                    // UHC's U+327E skips beyond the end of the input: glibc then reads past the
                    // buffer until the next lead byte, which ends the conversion as incomplete
                    result = Status::IncompleteInput;
                    break;
                }
                if marks {
                    result = Status::IllegalInput;
                }
                *irreversible += 1;
                *pos += skip;
            }
        }
    }
    result
}

/// The replacement candidates of transliteration entry `k` (`TRANSLIT_TO` split at its NUL
/// terminators, indexed once).
fn translit_entry(k: usize) -> &'static str {
    static INDEX: OnceLock<Vec<&'static str>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut entries: Vec<&'static str> = names::TRANSLIT_TO.split('\0').collect();
        entries.pop(); // after the last terminator
        debug_assert_eq!(entries.len(), names::TRANSLIT_FROM.len());
        entries
    });
    index[k]
}

/// The encoding step of a conversion: the module and its `__invocation_counter`.
struct EncStep {
    enc: Box<dyn Encode>,
    invocations: u64,
}

impl EncStep {
    /// The step function called by `__gconv_transliterate()` on one replacement (`irreversible`
    /// is `NULL`): its `PREPARE_LOOP` and the loop without error recovery. Shift state changes
    /// stay; the caller decides about the output.
    fn convert_candidate(&mut self, cand: &[u32], out: &mut Vec<u8>, cap: usize) -> Status {
        if let Some(status) = self.enc.prepare(out, cap, self.invocations) {
            return status;
        }
        let min = self.enc.min_needed();
        let mut i = 0;
        while i < cand.len() {
            if out.len() + min > cap {
                return Status::FullOutput;
            }
            match self.enc.body(cand, i, out, cap) {
                EStep::Ok(n) => i += n,
                EStep::Full => return Status::FullOutput,
                EStep::Unmappable | EStep::Illegal { .. } => return Status::IllegalInput,
            }
        }
        Status::EmptyInput
    }

    /// glibc `__gconv_transliterate()` (iconv/gconv_trans.c) with the C.UTF-8 locale's table:
    /// the candidates of `cp` in order, then `default_missing` (`?`); the first one the module
    /// converts completely is written. `Ok` when transliterated, `FullOutput`, or `IllegalInput`.
    fn transliterate(
        &mut self,
        cp: u32,
        out: &mut Vec<u8>,
        cap: usize,
        irreversible: &mut usize,
    ) -> Status {
        if let Ok(k) = names::TRANSLIT_FROM.binary_search(&cp) {
            for cand in translit_entry(k).split('\u{1}') {
                let cand: Vec<u32> = cand.chars().map(u32::from).collect();
                let mark = out.len();
                match self.convert_candidate(&cand, out, cap) {
                    Status::EmptyInput => {
                        *irreversible += 1;
                        return Status::Ok;
                    }
                    Status::IllegalInput => out.truncate(mark),
                    status => {
                        // "Do not increment the output pointer if we could not store the entire
                        // output."
                        out.truncate(mark);
                        return status;
                    }
                }
            }
        }
        let mark = out.len();
        match self.convert_candidate(names::TRANSLIT_DEFAULT_MISSING, out, cap) {
            Status::EmptyInput => {
                *irreversible += 1;
                Status::Ok
            }
            Status::IllegalInput => {
                out.truncate(mark);
                Status::IllegalInput
            }
            // the default replacement keeps what it wrote even when the buffer ran full
            status => status,
        }
    }

    /// The loop of the step with `STANDARD_TO_LOOP_ERR_HANDLER`: a successful transliteration
    /// resets the loop result to `__GCONV_OK`.
    fn run(
        &mut self,
        chars: &[u32],
        out: &mut Vec<u8>,
        cap: usize,
        ignore: bool,
        translit: bool,
        irreversible: &mut usize,
    ) -> (usize, Status) {
        if let Some(result) = self.enc.special_loop(chars, out, cap) {
            return result;
        }
        let min = self.enc.min_needed();
        let mut result = Status::EmptyInput;
        let mut i = 0;
        while i < chars.len() {
            if out.len() + min > cap {
                result = Status::FullOutput;
                break;
            }
            match self.enc.body(chars, i, out, cap) {
                EStep::Ok(n) => i += n,
                EStep::Full => {
                    result = Status::FullOutput;
                    break;
                }
                EStep::Unmappable => {
                    if translit {
                        match self.transliterate(chars[i], out, cap, irreversible) {
                            Status::IllegalInput => {}
                            Status::FullOutput => {
                                result = Status::FullOutput;
                                break;
                            }
                            status => {
                                result = status;
                                i += 1;
                                continue;
                            }
                        }
                    }
                    result = Status::IllegalInput;
                    if !ignore {
                        break;
                    }
                    *irreversible += 1;
                    i += 1;
                }
                EStep::Illegal { marks } => {
                    if !ignore {
                        result = Status::IllegalInput;
                        break;
                    }
                    if marks {
                        result = Status::IllegalInput;
                    }
                    *irreversible += 1;
                    i += 1;
                }
            }
        }
        (i, result)
    }

    /// The step function as the last step of a conversion: `PREPARE_LOOP`, one run of the loop,
    /// and the invocation counter.
    fn call(
        &mut self,
        chars: &[u32],
        out: &mut Vec<u8>,
        cap: usize,
        ignore: bool,
        translit: bool,
        irreversible: &mut usize,
    ) -> (usize, Status) {
        if let Some(status) = self.enc.prepare(out, cap, self.invocations) {
            return (0, status);
        }
        let result = self.run(chars, out, cap, ignore, translit, irreversible);
        self.invocations += 1;
        result
    }
}

/// An open conversion descriptor (`iconv_t`) for a two-step conversion through glibc's internal
/// UCS-4.
struct Conversion {
    dec: Box<dyn Decode>,
    /// `__invocation_counter` of the first step.
    dec_invocations: u64,
    enc: EncStep,
    ignore: bool,
    translit: bool,
}

impl Conversion {
    /// One `iconv()` call: glibc `__gconv()` with the first step's skeleton (iconv/skeleton.c):
    /// rounds of up to `NCHAR_GOAL` characters, each handed to the second step, whose error (or
    /// full output buffer) wins and puts the input back to the first character it did not
    /// consume (`RESET_INPUT_BUFFER`, or a rerun of the round with the saved shift state).
    fn gconv(&mut self, input: &[u8], pos: &mut usize, out: &mut Vec<u8>, cap: usize) -> Status {
        if let Some(status) = self.dec.prepare(input, pos, self.dec_invocations) {
            return status;
        }
        let max = NCHAR_GOAL * self.dec.max_chars();
        loop {
            let round_start = *pos;
            let saved = self.dec.save();
            let mut chars = Vec::new();
            let mut dec_irreversible = 0;
            let mut status = decode_loop(
                self.dec.as_mut(),
                input,
                pos,
                &mut chars,
                max,
                self.ignore,
                &mut dec_irreversible,
            );
            self.dec_invocations += 1;
            if !chars.is_empty() {
                let mut enc_irreversible = 0;
                let (consumed, result) = self.enc.call(
                    &chars,
                    out,
                    cap,
                    self.ignore,
                    self.translit,
                    &mut enc_irreversible,
                );
                if result != Status::EmptyInput {
                    if consumed < chars.len() {
                        match self.dec.fixed_width() {
                            Some(width) if dec_irreversible == 0 && enc_irreversible == 0 => {
                                *pos -= (chars.len() - consumed) * width;
                            }
                            _ => {
                                *pos = round_start;
                                self.dec.restore(saved);
                                let mut redo = Vec::new();
                                let mut ignored = 0;
                                decode_loop(
                                    self.dec.as_mut(),
                                    input,
                                    pos,
                                    &mut redo,
                                    consumed,
                                    self.ignore,
                                    &mut ignored,
                                );
                                if consumed == 0 {
                                    self.dec_invocations -= 1;
                                }
                            }
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

    /// `iconv(cd, NULL, NULL, &out, &outleft)`: the first step's `EMIT_SHIFT_TO_INIT` (buffered
    /// characters go through the second step; a failure there restores the state), then the
    /// second step's; on success glibc clears the invocation counters.
    fn flush(&mut self, out: &mut Vec<u8>, cap: usize) -> Status {
        let saved = self.dec.save();
        let mut pending = Vec::new();
        self.dec.emit_shift_to_init(&mut pending);
        let mut status = Status::Ok;
        if !pending.is_empty() {
            let mut irreversible = 0;
            let (consumed, result) = self.enc.call(
                &pending,
                out,
                cap,
                self.ignore,
                self.translit,
                &mut irreversible,
            );
            if result != Status::EmptyInput {
                if consumed < pending.len() {
                    self.dec.restore(saved);
                }
                status = result;
            }
        }
        if status == Status::Ok {
            status = self.enc.enc.flush(out, cap, self.enc.invocations);
        }
        if status == Status::Ok {
            self.dec_invocations = 0;
            self.enc.invocations = 0;
        }
        status
    }
}

/// A conversion glibc does with one gconv module that converts between two charsets directly
/// (the gconv cache's extra conversions: GB2312/BIG5, GBK/GB2312).
trait Direct {
    /// One call of the step function (the last and only step): one run of the module's loop
    /// from `input[*pos..]` into `out` (capacity `cap`).
    fn run(&mut self, input: &[u8], pos: &mut usize, out: &mut Vec<u8>, cap: usize) -> Status;
}

/// An open conversion descriptor.
enum Descriptor {
    Steps(Box<Conversion>),
    Direct(Box<dyn Direct>),
}

impl Descriptor {
    fn gconv(&mut self, input: &[u8], pos: &mut usize, out: &mut Vec<u8>, cap: usize) -> Status {
        match self {
            Descriptor::Steps(conv) => conv.gconv(input, pos, out, cap),
            Descriptor::Direct(direct) => direct.run(input, pos, out, cap),
        }
    }

    fn flush(&mut self, out: &mut Vec<u8>, cap: usize) -> Status {
        match self {
            Descriptor::Steps(conv) => conv.flush(out, cap),
            Descriptor::Direct(_) => Status::Ok,
        }
    }
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
    names::NAMES
        .binary_search_by(|(alias, _)| alias.cmp(&name))
        .ok()
        .map(|i| names::NAMES[i].1)
}

/// The C string of a PHP string argument (everything before the first NUL byte).
fn c_string(s: &[u8]) -> &[u8] {
    match memchr::memchr(0, s) {
        Some(nul) => &s[..nul],
        None => s,
    }
}

/// What a gconv module name stands for.
enum Module {
    Codec(Box<dyn Decode>, Box<dyn Encode>),
    /// glibc's `INTERNAL` (`WCHAR_T`).
    Internal,
    /// A module glibc has but this port does not convert.
    Unsupported,
}

/// The decoder and encoder of a gconv module.
fn module(name: &'static str) -> Module {
    if name == "INTERNAL" {
        return Module::Internal;
    }
    if let Some((dec, enc)) = unicode::codec(name)
        .or_else(|| sbcs::codec(name))
        .or_else(|| cjk::codec(name))
        .or_else(|| iso2022::codec(name))
    {
        return Module::Codec(dec, enc);
    }
    Module::Unsupported
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

/// The loops of `php_iconv_string()` (PHPSRC/ext/iconv/iconv.c) around `iconv()`: the output
/// buffer starts at `len + 32` bytes and grows by `len` on `E2BIG`; with `ICONV_BROKEN_IGNORE`
/// an `EILSEQ` under `//IGNORE` either ends the conversion successfully (at most one byte left) or
/// skips one input byte and continues. After a successful conversion the shift-out sequences are
/// flushed, growing the buffer by 16 bytes on `E2BIG`; a flush error fails the conversion even
/// under `//IGNORE`.
fn php_iconv_loop(cd: &mut Descriptor, input: &[u8], php_ignore: bool) -> IconvOutcome {
    let in_len = input.len();
    let mut cap = in_len + 32;
    let mut pos = 0;
    let mut out = Vec::with_capacity(cap);
    let mut failure = None;
    while pos < in_len {
        match cd.gconv(input, &mut pos, &mut out, cap) {
            Status::Ok | Status::EmptyInput => break,
            Status::IllegalInput if php_ignore => {
                if in_len - pos <= 1 {
                    break;
                }
                pos += 1;
            }
            Status::FullOutput if pos < in_len => cap += in_len,
            status => {
                failure = Some(status);
                break;
            }
        }
    }
    if failure.is_none() {
        loop {
            match cd.flush(&mut out, cap) {
                Status::Ok | Status::EmptyInput => break,
                Status::FullOutput => cap += 16,
                status => {
                    failure = Some(status);
                    break;
                }
            }
        }
    }
    match failure {
        None => IconvOutcome::Converted(out),
        Some(Status::IllegalInput) => IconvOutcome::IllegalSequence,
        Some(Status::IncompleteInput) => IconvOutcome::IncompleteCharacter,
        Some(_) => IconvOutcome::TooBig,
    }
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
///   character in input string`;
/// * an output buffer overflow PHP cannot recover from: `Warning: iconv(): Buffer length
///   exceeded`.
///
/// Charsets glibc knows but this port does not convert throw `Error: iconv(): conversion of
/// charset "<name>" is not supported by twig-rs`: UTF-7 and UTF-7-IMAP, EUC-TW, ISO-2022-CN and
/// ISO-2022-CN-EXT (CNS 11643), IBM932 and IBM943 (IBM's Shift_JIS), JOHAB, the
/// stateful EBCDIC DBCS codepages (IBM930, IBM933, IBM935, IBM937, IBM939, IBM1364, IBM1371,
/// IBM1388, IBM1390, IBM1399), TSCII, the ISO 6937 family (ANSI_X3.110, ISO_6937, ISO_6937-2,
/// T.61), and `WCHAR_T`/`INTERNAL` on one side.
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
    let mut cd = if let Some(direct) = cjk::direct(from_module, to_module, ignore) {
        Descriptor::Direct(direct)
    } else {
        match (module(from_module), module(to_module)) {
            (Module::Codec(mut dec, _), Module::Codec(_, enc)) => {
                dec.set_ignore(ignore);
                Descriptor::Steps(Box::new(Conversion {
                    dec,
                    dec_invocations: 0,
                    enc: EncStep {
                        enc,
                        invocations: 0,
                    },
                    ignore,
                    translit,
                }))
            }
            (Module::Internal, Module::Internal) => return wrong(diag),
            (Module::Codec(..), _) => return Err(unsupported(out_c)),
            _ => return Err(unsupported(in_c)),
        }
    };
    match php_iconv_loop(&mut cd, input, php_ignore) {
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
        let mut sorted = names::NAMES.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, names::NAMES);
        let modules: Vec<&str> = names::SBCS.iter().map(|s| s.module).collect();
        let mut sorted = modules.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, modules);
    }

    #[test]
    fn every_module_name_resolves_to_a_known_module() {
        // every name resolves to a module this port converts, INTERNAL, or one it lists as
        // unsupported in the documentation of `iconv_bytes`
        const UNSUPPORTED: &[&str] = &[
            "ANSI_X3.110//",
            "EUC-TW//",
            "IBM1364//",
            "IBM1371//",
            "IBM1388//",
            "IBM1390//",
            "IBM1399//",
            "IBM930//",
            "IBM932//",
            "IBM933//",
            "IBM935//",
            "IBM937//",
            "IBM939//",
            "IBM943//",
            "ISO-2022-CN-EXT//",
            "ISO-2022-CN//",
            "ISO_6937-2//",
            "ISO_6937//",
            "JOHAB//",
            "T.61-8BIT//",
            "TSCII//",
            "UTF-7-IMAP//",
            "UTF-7//",
        ];
        for &(_, m) in names::NAMES.iter() {
            match module(m) {
                Module::Codec(..) | Module::Internal => {}
                Module::Unsupported => assert!(UNSUPPORTED.contains(&m), "{m}"),
            }
        }
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
        let jp = "愛していますか？".as_bytes();
        let encoded = conv("UTF-8", "ISO-2022-JP", jp, &mut d).unwrap();
        assert_eq!(
            encoded,
            b"\x1b$B0&$7$F$$$^$9$+!)\x1b(B".to_vec(),
            "{encoded:x?}"
        );
        assert_eq!(
            conv("ISO-2022-JP", "UTF-8", &encoded, &mut d),
            Some(jp.to_vec())
        );
        assert_eq!(
            conv("UTF-8", "ISO-2022-KR", b"", &mut d),
            Some(b"\x1b$)C".to_vec())
        );
        assert!(d.is_empty());
        assert_eq!(conv("UTF-8", "ISO-8859-1", b"a\xC3", &mut d), None);
        assert_eq!(
            d[0].message,
            "iconv(): Detected an incomplete multibyte character in input string"
        );
    }
}
