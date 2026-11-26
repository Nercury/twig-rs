//! glibc's multi-byte CJK modules without shift sequences: EUC-JP, SJIS, CP932, EUC-CN, GBK,
//! GB18030, EUC-KR, UHC, BIG5 and BIG5-HKSCS (iconvdata/euc-jp.c, sjis.c, cp932.c, euc-cn.c,
//! gbk.c, gb18030.c, euc-kr.c, uhc.c, big5.c, big5hkscs.c), EUC-JISX0213 and Shift_JISX0213
//! (euc-jisx0213.c, shift_jisx0213.c), the coded character sets they share with ISO-2022
//! (jis0201.h, jis0208.h, jis0212.h, gb2312.h, ksc5601.h, jisx0213.h), and the direct
//! conversions of gbbig5.c and gbgbk.c.
//!
//! The decoders follow the modules' validity checks and skip lengths; the mapping tables and
//! the encoders' maps were measured in the oracle (`cjk_tables.gen.php`).

use std::sync::OnceLock;

use super::cjk_tables as t;
use super::tables::{self as tb, EncodeMap, Table};
use super::unicode::{char1, illegal, is_tag, put};
use super::{DStep, Decode, Direct, EStep, Encode, Status};

/// `__UNKNOWN_10646_CHAR` / `0` results of glibc's table helpers.
pub(super) enum Lookup {
    Char(u32),
    /// Not enough input (the helper returned 0).
    Incomplete,
    /// `__UNKNOWN_10646_CHAR`.
    Unknown,
}

/// `jisx0201_to_ucs4()` (iconvdata/jis0201.h): `None` for `__UNKNOWN_10646_CHAR`.
pub(super) fn jisx0201_to_ucs4(b: u8) -> Option<u32> {
    match b {
        0x5C => Some(0xA5),
        0x7E => Some(0x203E),
        0x00..=0x7F => Some(u32::from(b)),
        0xA1..=0xDF => Some(u32::from(b) + 0xFEC0),
        _ => None,
    }
}

/// `ucs4_to_jisx0201()` (iconvdata/jis0201.h).
pub(super) fn ucs4_to_jisx0201(cp: u32) -> Option<u8> {
    match cp {
        0xA5 => Some(0x5C),
        0x203E => Some(0x7E),
        0..=0x7D if cp != 0x5C => Some(cp as u8),
        0xFF61..=0xFF9F => Some((cp - 0xFEC0) as u8),
        _ => None,
    }
}

fn cell(table: Table, row: u8, col: u8) -> Lookup {
    match table.get(row, col) {
        0 => Lookup::Unknown,
        cp => Lookup::Char(cp),
    }
}

/// `jisx0208_to_ucs4()` (iconvdata/jis0208.h) on `s` (all the input left).
pub(super) fn jisx0208_to_ucs4(s: &[u8], offset: u8) -> Lookup {
    let ch = s[0];
    if ch < offset || ch - offset <= 0x20 {
        return Lookup::Unknown;
    }
    if s.len() < 2 {
        return Lookup::Incomplete;
    }
    let ch2 = s[1];
    if ch2 < offset || ch2 - offset <= 0x20 || ch2 - offset >= 0x7F {
        return Lookup::Unknown;
    }
    let idx = usize::from(ch - offset - 0x21) * 94 + usize::from(ch2 - offset - 0x21);
    if idx >= 0x1E80 {
        return Lookup::Unknown;
    }
    cell(tb::JIS0208, ch - offset, ch2 - offset)
}

/// `jisx0212_to_ucs4()` (iconvdata/jis0212.h).
pub(super) fn jisx0212_to_ucs4(s: &[u8], offset: u8) -> Lookup {
    let ch = s[0];
    if ch < offset || ch - offset < 0x22 || ch - offset > 0x6D {
        return Lookup::Unknown;
    }
    if s.len() < 2 {
        return Lookup::Incomplete;
    }
    let ch2 = s[1];
    if ch2 < offset || ch2 - offset <= 0x20 || ch2 - offset >= 0x7F {
        return Lookup::Unknown;
    }
    cell(tb::JIS0212, ch - offset, ch2 - offset)
}

/// `gb2312_to_ucs4()` (iconvdata/gb2312.h).
pub(super) fn gb2312_to_ucs4(s: &[u8], offset: u8) -> Lookup {
    let ch = s[0];
    if ch < offset || ch - offset <= 0x20 || ch - offset > 0x77 {
        return Lookup::Unknown;
    }
    if s.len() < 2 {
        return Lookup::Incomplete;
    }
    let ch2 = i32::from(s[1]) - i32::from(offset);
    if ch2 <= 0x20 || ch2 >= 0x7F {
        return Lookup::Unknown;
    }
    let idx = usize::from(ch - offset - 0x21) * 94 + (ch2 - 0x21) as usize;
    if idx > 0x1FF1 {
        return Lookup::Unknown;
    }
    cell(tb::GB2312, ch - offset, ch2 as u8)
}

/// `ksc5601_to_ucs4()` (iconvdata/ksc5601.h); it checks the available input first.
pub(super) fn ksc5601_to_ucs4(s: &[u8], offset: u8) -> Lookup {
    if s.len() < 2 {
        return Lookup::Incomplete;
    }
    let (ch, ch2) = (s[0], s[1]);
    if ch < offset || ch - offset <= 0x20 || ch - offset >= 0x7E || ch - offset == 0x49 {
        return Lookup::Unknown;
    }
    if ch2 < offset || ch2 - offset <= 0x20 || ch2 - offset >= 0x7F {
        return Lookup::Unknown;
    }
    cell(tb::KSC5601, ch - offset, ch2 - offset)
}

/// `jisx0213_to_ucs4()` (iconvdata/jisx0213.h): a code point, or below 0x80 a 1-based index into
/// `JISX0213_PAIRS`, or 0.
pub(super) fn jisx0213_to_ucs4(row: u32, col: u32) -> u32 {
    let plane2 = match row {
        0x121..=0x17E => false,
        0x221 | 0x223..=0x225 | 0x228 | 0x22C..=0x22F | 0x26E..=0x27E => true,
        _ => return 0,
    };
    if !(0x21..=0x7E).contains(&col) {
        return 0;
    }
    let table = if plane2 {
        tb::JISX0213_2
    } else {
        tb::JISX0213_1
    };
    table.get((row & 0xFF) as u8, col as u8)
}

/// The two characters of a JIS X 0213 position that decodes to a base and a combining
/// character (`__jisx0213_to_ucs_combining`).
pub(super) fn jisx0213_pair(index: u32) -> (u32, u32) {
    t::JISX0213_PAIRS[index as usize - 1]
}

static JIS0208_ENC: OnceLock<EncodeMap> = OnceLock::new();
static JIS0212_ENC: OnceLock<EncodeMap> = OnceLock::new();
static GB2312_ENC: OnceLock<EncodeMap> = OnceLock::new();
static KSC5601_ENC: OnceLock<EncodeMap> = OnceLock::new();

fn two(map: &EncodeMap, cp: u32) -> Option<[u8; 2]> {
    map.get(cp).map(|b| [b[0], b[1]])
}

/// `ucs4_to_jisx0208()` (iconvdata/jis0208.h) as measured: the 7-bit code.
pub(super) fn ucs4_to_jisx0208(cp: u32) -> Option<[u8; 2]> {
    two(
        tb::set_encoder(tb::JIS0208, t::SET_JIS0208, &JIS0208_ENC),
        cp,
    )
}

/// `ucs4_to_jisx0212()` (iconvdata/jis0212.h) as measured.
pub(super) fn ucs4_to_jisx0212(cp: u32) -> Option<[u8; 2]> {
    two(
        tb::set_encoder(tb::JIS0212, t::SET_JIS0212, &JIS0212_ENC),
        cp,
    )
}

/// `ucs4_to_gb2312()` (iconvdata/gb2312.h) as measured.
pub(super) fn ucs4_to_gb2312(cp: u32) -> Option<[u8; 2]> {
    two(tb::set_encoder(tb::GB2312, t::SET_GB2312, &GB2312_ENC), cp)
}

/// `ucs4_to_ksc5601()` (iconvdata/ksc5601.h) as measured.
pub(super) fn ucs4_to_ksc5601(cp: u32) -> Option<[u8; 2]> {
    two(
        tb::set_encoder(tb::KSC5601, t::SET_KSC5601, &KSC5601_ENC),
        cp,
    )
}

/// `ucs4_to_jisx0213()` (iconvdata/jisx0213.h): row byte (bit 15 set for plane 2) and column
/// byte (bit 7 set for characters that may start a combination in `comp_table_data`), 0 for
/// none.
pub(super) fn ucs4_to_jisx0213(cp: u32) -> u16 {
    static MAP: OnceLock<EncodeMap> = OnceLock::new();
    static BASES: OnceLock<Vec<u32>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let plane1 = tb::JISX0213_1
            .entries()
            .filter(|&(_, _, v)| v >= 0x80)
            .map(|(r, c, v)| (v, vec![r, c]));
        let plane2 = tb::JISX0213_2
            .entries()
            .filter(|&(_, _, v)| v >= 0x80)
            .map(|(r, c, v)| (v, vec![r | 0x80, c]));
        EncodeMap::build(plane1.chain(plane2), t::SET_JISX0213)
    });
    let Some(b) = map.get(cp) else {
        return 0;
    };
    let bases = BASES.get_or_init(|| {
        let mut v: Vec<u32> = t::JISX0213_PAIRS.iter().map(|p| p.0).collect();
        v.sort_unstable();
        v.dedup();
        v
    });
    let flag = if bases.binary_search(&cp).is_ok() {
        0x80
    } else {
        0
    };
    (u16::from(b[0]) << 8) | u16::from(b[1]) | flag
}

/// The composition of JIS X 0213 (`comp_table_data`): the plane 1 position composed of the
/// position `base` (7-bit, without flags) and the combining character `comb`.
pub(super) fn jisx0213_compose(base: u16, comb: u32) -> Option<u16> {
    static TABLE: OnceLock<Vec<(u16, u32, u16)>> = OnceLock::new();
    if !matches!(comb, 0x02E5 | 0x02E9 | 0x0300 | 0x0301 | 0x309A) {
        return None;
    }
    let table = TABLE.get_or_init(|| {
        tb::JISX0213_1
            .entries()
            .filter(|&(_, _, v)| v < 0x80)
            .map(|(r, c, v)| {
                let (u1, u2) = jisx0213_pair(v);
                (
                    ucs4_to_jisx0213(u1) & 0x7F7F,
                    u2,
                    (u16::from(r) << 8) | u16::from(c),
                )
            })
            .collect()
    });
    table
        .iter()
        .find(|&&(b, c, _)| b == base && c == comb)
        .map(|&(_, _, composed)| composed)
}

/// `jisx0213_added_in_2004_p()` (iconvdata/jisx0213.h): the ten plane 1 characters JIS X
/// 0213:2004 added (ISO-IR-233).
pub(super) fn jisx0213_added_in_2004(jch: u16) -> bool {
    matches!(
        jch,
        0x2E21 | 0x2F7E | 0x4F54 | 0x4F7E | 0x7427 | 0x7E7A..=0x7E7E
    )
}

/// The stateless multi-byte charsets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    EucJp,
    Sjis,
    Cp932,
    EucCn,
    Gbk,
    Gb18030,
    EucKr,
    Uhc,
    Big5,
    Big5Hkscs,
    EucJpMs,
}

/// The decoder and encoder of a multi-byte module.
pub(super) fn codec(module: &str) -> Option<(Box<dyn Decode>, Box<dyn Encode>)> {
    use Kind::*;
    let kind = match module {
        "EUC-JP//" => EucJp,
        "SJIS//" => Sjis,
        "CP932//" => Cp932,
        "EUC-CN//" => EucCn,
        "GBK//" => Gbk,
        "GB18030//" => Gb18030,
        "EUC-KR//" => EucKr,
        "UHC//" => Uhc,
        "BIG5//" => Big5,
        "BIG5HKSCS//" => Big5Hkscs,
        "EUC-JP-MS//" => EucJpMs,
        "EUC-JISX0213//" => {
            return Some((
                Box::new(X0213Dec {
                    shift: false,
                    pending: 0,
                }),
                Box::new(X0213Enc {
                    shift: false,
                    lasttwo: 0,
                }),
            ));
        }
        "SHIFT_JISX0213//" => {
            return Some((
                Box::new(X0213Dec {
                    shift: true,
                    pending: 0,
                }),
                Box::new(X0213Enc {
                    shift: true,
                    lasttwo: 0,
                }),
            ));
        }
        _ => return None,
    };
    Some((
        Box::new(MbDec {
            kind,
            pending: 0,
            ignore: false,
        }),
        Box::new(MbEnc {
            kind,
            map: encode_map(kind),
            lasttwo: 0,
        }),
    ))
}

/// The decoder of a stateless multi-byte charset (BIG5-HKSCS keeps the second character of a
/// split pair in its state).
struct MbDec {
    kind: Kind,
    /// BIG5-HKSCS: the queued second character of a pair.
    pending: u32,
    /// `ignore_errors_p()` (EUC-JP-MS writes U+FFFD for an unknown half-width katakana then).
    ignore: bool,
}

impl Decode for MbDec {
    fn max_chars(&self) -> usize {
        if self.kind == Kind::Big5Hkscs { 2 } else { 1 }
    }

    fn body(&mut self, input: &[u8], pos: usize, out: &mut Vec<u32>, max: usize) -> DStep {
        let s = &input[pos..];
        match self.kind {
            Kind::EucJp => euc_jp(s, out),
            Kind::Sjis => sjis(s, out),
            Kind::Cp932 => cp932(s, out),
            Kind::EucCn => euc_cn(s, out),
            Kind::Gbk => gbk(s, out),
            Kind::Gb18030 => gb18030(s, out),
            Kind::EucKr => euc_kr(s, out),
            Kind::Uhc => uhc(s, out),
            Kind::Big5 => big5(s, out),
            Kind::Big5Hkscs => self.big5hkscs(s, out, max),
            Kind::EucJpMs => euc_jp_ms(s, out, self.ignore),
        }
    }

    fn save(&self) -> u32 {
        self.pending
    }

    fn restore(&mut self, state: u32) {
        self.pending = state;
    }

    /// BIG5-HKSCS's `EMIT_SHIFT_TO_INIT`: the queued character.
    fn emit_shift_to_init(&mut self, out: &mut Vec<u32>) {
        if self.pending != 0 {
            out.push(self.pending);
            self.pending = 0;
        }
    }

    fn set_ignore(&mut self, ignore: bool) {
        self.ignore = ignore;
    }
}

/// `from_eucjp_ms` (iconvdata/euc-jp-ms.c): every error leaves the loop result alone when
/// ignored, skipping the bytes of the sequence (three after 0x8F even if the last one is not a
/// trail byte); an unknown half-width katakana becomes U+FFFD under `//IGNORE`.
fn euc_jp_ms(s: &[u8], out: &mut Vec<u32>, ignore: bool) -> DStep {
    let quiet = |skip| DStep::Illegal { skip, marks: false };
    let ch = s[0];
    if ch < 0x8E || (0x90..=0x9F).contains(&ch) {
        return char1(out, u32::from(ch), 1);
    }
    if ch == 0xFF {
        return quiet(1);
    }
    if s.len() < 2 {
        return DStep::Incomplete;
    }
    let ch2 = s[1];
    if ch2 < 0xA1 {
        return quiet(1);
    }
    if ch == 0x8E {
        return match jisx0201_to_ucs4(ch2) {
            Some(cp) => char1(out, cp, 2),
            None if ignore => char1(out, 0xFFFD, 2),
            None => quiet(2),
        };
    }
    let (cp, len) = if ch == 0x8F {
        if s.len() < 3 {
            return DStep::Incomplete;
        }
        let ch3 = s[2];
        let cp = if ch3 == 0xFF || ch3 < 0xA1 {
            0
        } else {
            tb::EUCJPMS_3.get(ch2, ch3)
        };
        (cp, 3)
    } else if ch >= 0xA1 {
        let cp = if ch2 == 0xFF {
            0
        } else {
            tb::EUCJPMS.get(ch, ch2)
        };
        (cp, 2)
    } else {
        (0, 1)
    };
    if cp == 0 {
        return quiet(len);
    }
    char1(out, cp, len)
}

/// `from_euc_jp` (iconvdata/euc-jp.c).
fn euc_jp(s: &[u8], out: &mut Vec<u32>) -> DStep {
    let ch = s[0];
    if ch < 0x8E || (0x90..=0x9F).contains(&ch) {
        return char1(out, u32::from(ch), 1);
    }
    if ch == 0xFF {
        return illegal(1);
    }
    if s.len() < 2 {
        return DStep::Incomplete;
    }
    let ch2 = s[1];
    if ch2 < 0xA1 {
        return illegal(1);
    }
    if ch == 0x8E {
        return match jisx0201_to_ucs4(ch2) {
            Some(cp) => char1(out, cp, 2),
            None => illegal(1),
        };
    }
    let (r, len) = if ch == 0x8F {
        (jisx0212_to_ucs4(&s[1..], 0x80), 3)
    } else {
        (jisx0208_to_ucs4(s, 0x80), 2)
    };
    match r {
        Lookup::Char(cp) => char1(out, cp, len),
        Lookup::Incomplete => DStep::Incomplete,
        Lookup::Unknown => illegal(1),
    }
}

/// `from_sjis` (iconvdata/sjis.c).
fn sjis(s: &[u8], out: &mut Vec<u32>) -> DStep {
    let ch = s[0];
    match ch {
        0x5C => return char1(out, 0xA5, 1),
        0x7E => return char1(out, 0x203E, 1),
        0x00..=0x7F => return char1(out, u32::from(ch), 1),
        0xA1..=0xDF => return char1(out, u32::from(ch) + 0xFEC0, 1),
        _ => {}
    }
    if ch > 0xEA || ch == 0xA0 || ch <= 0x80 {
        return illegal(1);
    }
    if s.len() < 2 {
        return DStep::Incomplete;
    }
    let ch2 = s[1];
    let idx = u32::from(ch) * 256 + u32::from(ch2);
    if ch2 < 0x40 {
        return illegal(1);
    }
    if (idx > 0x84BE && idx < 0x889F)
        || (idx > 0x88FC && idx < 0x8940)
        || (idx > 0x9FFC && idx < 0xE040)
        || idx > 0xEAA4
    {
        return illegal(2);
    }
    match tb::SJIS.get(ch, ch2) {
        0 => illegal(2),
        cp => char1(out, cp, 2),
    }
}

/// `from_cp932` (iconvdata/cp932.c): ignored errors leave the loop result alone.
fn cp932(s: &[u8], out: &mut Vec<u32>) -> DStep {
    let quiet = |skip| DStep::Illegal { skip, marks: false };
    let ch = s[0];
    match ch {
        0x00..=0x7F => return char1(out, u32::from(ch), 1),
        0xA1..=0xDF => return char1(out, u32::from(ch) + 0xFEC0, 1),
        _ => {}
    }
    if ch == 0xA0 || ch <= 0x80 || ch > 0xFC {
        return quiet(1);
    }
    if s.len() < 2 {
        return DStep::Incomplete;
    }
    let ch2 = s[1];
    let idx = u32::from(ch) * 256 + u32::from(ch2);
    if !(0x40..=0xFC).contains(&ch2)
        || ch2 == 0x7F
        || (idx > 0x84BE && idx < 0x8740)
        || (idx > 0x879C && idx < 0x889F)
        || (idx > 0x88FC && idx < 0x8940)
        || (idx > 0x9FFC && idx < 0xE040)
        || (idx > 0xEAA4 && idx < 0xED40)
        || (idx > 0xEEFC && idx < 0xF040)
        || idx > 0xFC4B
    {
        return quiet(1);
    }
    match tb::CP932.get(ch, ch2) {
        0 => quiet(2),
        cp => char1(out, cp, 2),
    }
}

/// `from_euc_cn` (iconvdata/euc-cn.c).
fn euc_cn(s: &[u8], out: &mut Vec<u32>) -> DStep {
    let ch = s[0];
    if ch <= 0x7F {
        return char1(out, u32::from(ch), 1);
    }
    if (ch <= 0xA0 && ch != 0x8E && ch != 0x8F) || ch > 0xFE {
        return illegal(1);
    }
    if s.len() < 2 {
        return DStep::Incomplete;
    }
    if s[1] < 0xA1 {
        return illegal(1);
    }
    match gb2312_to_ucs4(&s[..2], 0x80) {
        Lookup::Char(cp) => char1(out, cp, 2),
        _ => illegal(2),
    }
}

/// `from_gbk` (iconvdata/gbk.c).
fn gbk(s: &[u8], out: &mut Vec<u32>) -> DStep {
    let ch = s[0];
    if ch <= 0x7F {
        return char1(out, u32::from(ch), 1);
    }
    if ch <= 0x80 || ch > 0xFE {
        return if ch == 0x80 {
            char1(out, 0x20AC, 1)
        } else {
            illegal(1)
        };
    }
    if s.len() < 2 {
        return DStep::Incomplete;
    }
    let ch2 = s[1];
    if ch2 < 0x40 || (ch == 0xFE && ch2 > 0xA0) {
        return illegal(1);
    }
    match tb::GBK.get(ch, ch2) {
        0 => illegal(2),
        cp => char1(out, cp, 2),
    }
}

/// `from_gb18030` (iconvdata/gb18030.c).
fn gb18030(s: &[u8], out: &mut Vec<u32>) -> DStep {
    let ch = s[0];
    if ch <= 0x7F {
        return char1(out, u32::from(ch), 1);
    }
    if !(0x81..=0xFE).contains(&ch) {
        return illegal(1);
    }
    if s.len() < 2 {
        return DStep::Incomplete;
    }
    let ch2 = s[1];
    if ch2 < 0x30 {
        return illegal(2);
    }
    if ch2 <= 0x39 {
        if s.len() < 4 {
            return DStep::Incomplete;
        }
        let ch3 = s[2];
        if !(0x81..=0xFE).contains(&ch3) {
            return illegal(3);
        }
        let ch4 = s[3];
        if !(0x30..=0x39).contains(&ch4) {
            return illegal(4);
        }
        let idx = ((u32::from(ch - 0x81) * 10 + u32::from(ch2 - 0x30)) * 126
            + u32::from(ch3 - 0x81))
            * 10
            + u32::from(ch4 - 0x30);
        let cp = if idx < 0x99FC {
            tb::gb18030_four(idx)
        } else if (0x2E248..=0x12E247).contains(&idx) {
            idx - 0x1E248
        } else {
            0
        };
        return if cp == 0 {
            illegal(4)
        } else {
            char1(out, cp, 4)
        };
    }
    if ch2 >= 0x40 {
        let idx = u32::from(ch - 0x81) * 192 + u32::from(ch2 - 0x40);
        if idx > 0x5E7E {
            return illegal(2);
        }
        return match tb::GB18030.get(ch, ch2) {
            0 => illegal(2),
            cp => char1(out, cp, 2),
        };
    }
    illegal(2)
}

/// `from_euc_kr` (iconvdata/euc-kr.c).
fn euc_kr(s: &[u8], out: &mut Vec<u32>) -> DStep {
    let ch = s[0];
    if ch <= 0x9F {
        return char1(out, u32::from(ch), 1);
    }
    if ch == 0xA0 {
        return illegal(1);
    }
    match ksc5601_to_ucs4(s, 0x80) {
        Lookup::Char(cp) => char1(out, cp, 2),
        Lookup::Incomplete => DStep::Incomplete,
        Lookup::Unknown => illegal(2),
    }
}

/// `from_uhc` (iconvdata/uhc.c).
fn uhc(s: &[u8], out: &mut Vec<u32>) -> DStep {
    let ch = s[0];
    if ch <= 0x7F {
        return char1(out, u32::from(ch), 1);
    }
    if ch <= 0x80 || ch >= 0xFE || ch == 0xC9 {
        return illegal(1);
    }
    if s.len() < 2 {
        return DStep::Incomplete;
    }
    let ch2 = s[1];
    if ch < 0xA1 || ch2 < 0xA1 {
        if ch > 0xC6
            || !(0x41..=0xFE).contains(&ch2)
            || (ch2 > 0x5A && ch2 < 0x61)
            || (ch2 > 0x7A && ch2 < 0x81)
            || (ch == 0xC6 && ch2 > 0x52)
        {
            return illegal(1);
        }
        return match tb::UHC.get(ch, ch2) {
            0 => illegal(2),
            cp => char1(out, cp, 2),
        };
    }
    match ksc5601_to_ucs4(&s[..2], 0x80) {
        // ksc5601_to_ucs4() has advanced the input past U+327E before uhc.c rejects it, so the
        // error handler skips four bytes
        Lookup::Char(0x327E) => illegal(4),
        Lookup::Char(cp) => char1(out, cp, 2),
        _ => illegal(2),
    }
}

/// `from_big5` (iconvdata/big5.c).
fn big5(s: &[u8], out: &mut Vec<u32>) -> DStep {
    let ch = s[0];
    if (0xA1..=0xF9).contains(&ch) {
        if s.len() < 2 {
            return DStep::Incomplete;
        }
        let ch2 = s[1];
        if !(0x40..=0x7E).contains(&ch2) && !(0xA1..=0xFE).contains(&ch2) {
            return illegal(1);
        }
        return match tb::BIG5.get(ch, ch2) {
            0 => illegal(2),
            cp => char1(out, cp, 2),
        };
    }
    if ch <= 0x80 {
        return char1(out, u32::from(ch), 1);
    }
    illegal(1)
}

impl MbDec {
    /// `from_big5hkscs` (iconvdata/big5hkscs.c): four positions decode to a base and a
    /// combining character; when only one fits, the second waits in the state.
    fn big5hkscs(&mut self, s: &[u8], out: &mut Vec<u32>, max: usize) -> DStep {
        if self.pending != 0 {
            out.push(self.pending);
            self.pending = 0;
            return DStep::Ok(0);
        }
        let ch = s[0];
        if (0x81..=0xFE).contains(&ch) {
            if s.len() < 2 {
                return DStep::Incomplete;
            }
            let ch2 = s[1];
            let cp = if ch < 0x87 || !(0x40..=0xFE).contains(&ch2) {
                0
            } else {
                tb::BIG5HKSCS.get(ch, ch2)
            };
            if cp != 0 {
                return char1(out, cp, 2);
            }
            let (first, second) = match (ch, ch2) {
                (0x88, 0x62) => (0xCA, 0x304),
                (0x88, 0x64) => (0xCA, 0x30C),
                (0x88, 0xA3) => (0xEA, 0x304),
                (0x88, 0xA5) => (0xEA, 0x30C),
                _ => return illegal(1),
            };
            out.push(first);
            if out.len() < max {
                out.push(second);
                return DStep::Ok(2);
            }
            self.pending = second;
            return DStep::Full(2);
        }
        if ch == 0xFF {
            return illegal(1);
        }
        char1(out, u32::from(ch), 1)
    }
}

/// The encoder map of a stateless charset: the inverse of its decoder over every one-, two-,
/// (EUC-JP) three- and (GB18030) four-byte sequence that decodes to one character, shortest
/// and lowest first, with the measured deviations.
fn encode_map(kind: Kind) -> &'static EncodeMap {
    static MAPS: [OnceLock<EncodeMap>; 11] = [const { OnceLock::new() }; 11];
    let (slot, deviations) = match kind {
        Kind::EucJp => (0, t::ENC_EUC_JP),
        Kind::Sjis => (1, t::ENC_SJIS),
        Kind::Cp932 => (2, t::ENC_CP932),
        Kind::EucCn => (3, t::ENC_EUC_CN),
        Kind::Gbk => (4, t::ENC_GBK),
        Kind::Gb18030 => (5, t::ENC_GB18030),
        Kind::EucKr => (6, t::ENC_EUC_KR),
        Kind::Uhc => (7, t::ENC_UHC),
        Kind::Big5 => (8, t::ENC_BIG5),
        Kind::Big5Hkscs => (9, t::ENC_BIG5HKSCS),
        Kind::EucJpMs => (10, t::ENC_EUC_JP_MS),
    };
    MAPS[slot].get_or_init(|| {
        let single = |seq: Vec<u8>| -> Option<(u32, Vec<u8>)> {
            let mut dec = MbDec {
                kind,
                pending: 0,
                ignore: false,
            };
            let mut out = Vec::new();
            match dec.body(&seq, 0, &mut out, 2) {
                DStep::Ok(n) if n == seq.len() && out.len() == 1 => Some((out[0], seq)),
                _ => None,
            }
        };
        let one = (0..=255u8).map(|b| vec![b]);
        let two = (0x80..=0xFFu8).flat_map(|l| (0..=255u8).map(move |c| vec![l, c]));
        let three = (0xA1..=0xFEu8).flat_map(|r| (0xA1..=0xFEu8).map(move |c| vec![0x8F, r, c]));
        let three = three.filter(|_| matches!(kind, Kind::EucJp | Kind::EucJpMs));
        let mut candidates: Vec<(u32, Vec<u8>)> =
            one.chain(two).chain(three).filter_map(single).collect();
        if kind == Kind::Gb18030 {
            candidates.extend((0..0x99FC).filter_map(|idx| match tb::gb18030_four(idx) {
                0 => None,
                cp => Some((cp, tb::gb18030_four_bytes(idx).to_vec())),
            }));
        }
        EncodeMap::build(candidates.into_iter(), deviations)
    })
}

/// The encoder of a stateless charset (BIG5-HKSCS buffers Ê and ê to combine them with a
/// following U+0304 or U+030C).
struct MbEnc {
    kind: Kind,
    map: &'static EncodeMap,
    /// BIG5-HKSCS: the buffered two bytes.
    lasttwo: u16,
}

impl MbEnc {
    fn tags_dropped(&self) -> bool {
        match self.kind {
            Kind::EucJp => t::TAGS_DROPPED_EUC_JP,
            Kind::Sjis => t::TAGS_DROPPED_SJIS,
            Kind::Cp932 => t::TAGS_DROPPED_CP932,
            Kind::EucCn => t::TAGS_DROPPED_EUC_CN,
            Kind::Gbk => t::TAGS_DROPPED_GBK,
            Kind::Gb18030 => t::TAGS_DROPPED_GB18030,
            Kind::EucKr => t::TAGS_DROPPED_EUC_KR,
            Kind::Uhc => t::TAGS_DROPPED_UHC,
            Kind::Big5 => t::TAGS_DROPPED_BIG5,
            Kind::Big5Hkscs => t::TAGS_DROPPED_BIG5HKSCS,
            Kind::EucJpMs => t::TAGS_DROPPED_EUC_JP_MS,
        }
    }

    /// The bytes of `cp`; GB18030 maps the supplementary planes linearly.
    fn bytes(&self, cp: u32) -> Option<Vec<u8>> {
        match self.map.lookup(cp) {
            Some(b) if !b.is_empty() => Some(b.to_vec()),
            Some(_) => None,
            None if self.kind == Kind::Gb18030 && (0x10000..=0x10FFFF).contains(&cp) => {
                Some(tb::gb18030_four_bytes(cp + 0x1E248).to_vec())
            }
            None => None,
        }
    }

    fn unmapped(&self, cp: u32) -> EStep {
        if self.tags_dropped() && is_tag(cp) {
            EStep::Ok(1)
        } else {
            EStep::Unmappable
        }
    }

    /// `to_big5hkscs` (iconvdata/big5hkscs.c).
    fn big5hkscs(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let ch = chars[i];
        if self.lasttwo != 0 {
            let combined = match (ch, self.lasttwo) {
                (0x304, 0x8866) => Some(0x8862u16),
                (0x30C, 0x8866) => Some(0x8864),
                (0x304, 0x88A7) => Some(0x88A3),
                (0x30C, 0x88A7) => Some(0x88A5),
                _ => None,
            };
            if out.len() + 2 > cap {
                return EStep::Full;
            }
            out.extend_from_slice(&combined.unwrap_or(self.lasttwo).to_be_bytes());
            self.lasttwo = 0;
            return EStep::Ok(usize::from(combined.is_some()));
        }
        if ch <= 0x80 {
            out.push(ch as u8);
            return EStep::Ok(1);
        }
        let Some(b) = self.bytes(ch) else {
            return self.unmapped(ch);
        };
        if ch == 0xCA || ch == 0xEA {
            self.lasttwo = u16::from_be_bytes([b[0], b[1]]);
            return EStep::Ok(1);
        }
        put(out, cap, &b)
    }
}

impl Encode for MbEnc {
    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        if self.kind == Kind::Big5Hkscs {
            return self.big5hkscs(chars, i, out, cap);
        }
        let ch = chars[i];
        // EUC-JP checks for room for two bytes before it looks the character up
        if self.kind == Kind::EucJp
            && !(ch < 0x8E || (0x90..=0x9F).contains(&ch) || ch == 0xA5 || ch == 0x203E)
            && out.len() + 2 > cap
        {
            return EStep::Full;
        }
        match self.bytes(ch) {
            Some(b) => put(out, cap, &b),
            None => self.unmapped(ch),
        }
    }

    /// BIG5-HKSCS's `EMIT_SHIFT_TO_INIT`: the buffered character.
    fn flush(&mut self, out: &mut Vec<u8>, cap: usize, _invocations: u64) -> Status {
        if self.lasttwo != 0 {
            if out.len() + 2 > cap {
                return Status::FullOutput;
            }
            out.extend_from_slice(&self.lasttwo.to_be_bytes());
            self.lasttwo = 0;
        }
        Status::Ok
    }
}

/// The decoder of EUC-JISX0213 and Shift_JISX0213: positions that decode to two characters
/// queue the second one when only one fits.
struct X0213Dec {
    /// Shift_JISX0213 instead of EUC-JISX0213.
    shift: bool,
    pending: u32,
}

impl X0213Dec {
    /// The JIS X 0213 position of a two-byte Shift_JISX0213 sequence (`from_shift_jisx0213`).
    fn sjis_position(ch: u8, ch2: u8) -> (u32, u32) {
        let mut row = u32::from(if ch < 0xE0 { ch - 0x81 } else { ch - 0xC1 });
        let mut col = u32::from(if ch2 < 0x80 { ch2 - 0x40 } else { ch2 - 0x41 });
        row *= 2;
        if col >= 0x5E {
            col -= 0x5E;
            row += 1;
        }
        col += 0x21;
        if row >= 0x5E {
            if row >= 0x67 {
                row += 230;
            } else if row >= 0x63 || row == 0x5F {
                row += 168;
            } else {
                row += 162;
            }
        }
        (0x121 + row, col)
    }

    /// One JIS X 0213 value: a character or a pair (the second one queued if it does not fit).
    fn emit(&mut self, v: u32, len: usize, out: &mut Vec<u32>, max: usize) -> DStep {
        if v == 0 {
            return illegal(1);
        }
        if v >= 0x80 {
            return char1(out, v, len);
        }
        let (u1, u2) = jisx0213_pair(v);
        out.push(u1);
        if out.len() < max {
            out.push(u2);
            return DStep::Ok(len);
        }
        self.pending = u2;
        DStep::Full(len)
    }
}

impl Decode for X0213Dec {
    fn max_chars(&self) -> usize {
        2
    }

    fn body(&mut self, input: &[u8], pos: usize, out: &mut Vec<u32>, max: usize) -> DStep {
        if self.pending != 0 {
            out.push(self.pending);
            self.pending = 0;
            return DStep::Ok(0);
        }
        let s = &input[pos..];
        let ch = s[0];
        if self.shift {
            // from_shift_jisx0213
            match ch {
                0x5C => return char1(out, 0xA5, 1),
                0x7E => return char1(out, 0x203E, 1),
                0x00..=0x7F => return char1(out, u32::from(ch), 1),
                0xA1..=0xDF => return char1(out, u32::from(ch) + 0xFEC0, 1),
                0x81..=0x9F | 0xE0..=0xFC => {}
                _ => return illegal(1),
            }
            if s.len() < 2 {
                return DStep::Incomplete;
            }
            let ch2 = s[1];
            if ch2 < 0x40 || ch2 == 0x7F || ch2 > 0xFC {
                return illegal(1);
            }
            let (row, col) = Self::sjis_position(ch, ch2);
            return self.emit(jisx0213_to_ucs4(row, col), 2, out, max);
        }
        // from_euc_jisx0213
        if ch < 0x80 {
            return char1(out, u32::from(ch), 1);
        }
        if !((0xA1..=0xFE).contains(&ch) || ch == 0x8E || ch == 0x8F) {
            return illegal(1);
        }
        if s.len() < 2 {
            return DStep::Incomplete;
        }
        let ch2 = s[1];
        if !(0xA1..=0xFE).contains(&ch2) {
            return illegal(1);
        }
        if ch == 0x8E {
            if ch2 > 0xDF {
                return illegal(1);
            }
            return char1(out, u32::from(ch2) + 0xFEC0, 2);
        }
        if ch == 0x8F {
            if s.len() < 3 {
                return DStep::Incomplete;
            }
            let v = jisx0213_to_ucs4(0x200 - 0x80 + u32::from(ch2), u32::from(s[2] ^ 0x80));
            return self.emit(v, 3, out, max);
        }
        let v = jisx0213_to_ucs4(0x100 - 0x80 + u32::from(ch), u32::from(ch2 ^ 0x80));
        self.emit(v, 2, out, max)
    }

    fn save(&self) -> u32 {
        self.pending
    }

    fn restore(&mut self, state: u32) {
        self.pending = state;
    }

    fn emit_shift_to_init(&mut self, out: &mut Vec<u32>) {
        if self.pending != 0 {
            out.push(self.pending);
            self.pending = 0;
        }
    }
}

/// The encoder of EUC-JISX0213 and Shift_JISX0213: characters that may combine with a
/// following combining character are buffered.
struct X0213Enc {
    shift: bool,
    /// The buffered two bytes (in the charset's form).
    lasttwo: u16,
}

impl X0213Enc {
    /// The charset's two bytes of a JIS X 0213 value (flags ignored).
    fn form(&self, jch: u16) -> [u8; 2] {
        if !self.shift {
            return [((jch >> 8) as u8) | 0x80, (jch as u8) | 0x80];
        }
        let mut s1 = u32::from(jch >> 8);
        let mut s2 = u32::from(jch & 0x7F);
        s1 -= 0x21;
        s2 -= 0x21;
        if s1 >= 0x5E {
            if s1 >= 0xCD {
                s1 -= 102;
            } else if s1 >= 0x8B || s1 == 0x87 {
                s1 -= 40;
            } else {
                s1 -= 34;
            }
        }
        if s1 & 1 != 0 {
            s2 += 0x5E;
        }
        s1 >>= 1;
        s1 += if s1 < 0x1F { 0x81 } else { 0xC1 };
        s2 += if s2 < 0x3F { 0x40 } else { 0x41 };
        [s1 as u8, s2 as u8]
    }

    /// The JIS X 0213 position (7-bit) of the buffered bytes.
    fn base(&self) -> u16 {
        if !self.shift {
            return self.lasttwo & 0x7F7F;
        }
        // invert the shifted form of a plane 1 position
        let (s1, s2) = ((self.lasttwo >> 8) as u8, self.lasttwo as u8);
        let (row, col) = X0213Dec::sjis_position(s1, s2);
        (((row - 0x100) as u16) << 8) | col as u16
    }
}

impl Encode for X0213Enc {
    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let ch = chars[i];
        if self.lasttwo != 0 {
            if let Some(composed) = jisx0213_compose(self.base(), ch) {
                if out.len() + 2 > cap {
                    return EStep::Full;
                }
                let b = self.form(composed);
                out.extend_from_slice(&b);
                self.lasttwo = 0;
                return EStep::Ok(1);
            }
            if out.len() + 2 > cap {
                return EStep::Full;
            }
            out.extend_from_slice(&self.lasttwo.to_be_bytes());
            self.lasttwo = 0;
            return EStep::Ok(0);
        }
        if ch < 0x80 {
            out.push(ch as u8);
            return EStep::Ok(1);
        }
        if self.shift {
            if ch == 0xA5 {
                out.push(0x5C);
                return EStep::Ok(1);
            }
            if ch == 0x203E {
                out.push(0x7E);
                return EStep::Ok(1);
            }
            if (0xFF61..=0xFF9F).contains(&ch) {
                out.push((ch - 0xFEC0) as u8);
                return EStep::Ok(1);
            }
        } else if (0xFF61..=0xFF9F).contains(&ch) {
            return put(out, cap, &[0x8E, (ch - 0xFEC0) as u8]);
        }
        let jch = ucs4_to_jisx0213(ch);
        if jch == 0 {
            return if is_tag(ch) {
                EStep::Ok(1)
            } else {
                EStep::Unmappable
            };
        }
        let b = self.form(jch);
        if jch & 0x80 != 0 {
            self.lasttwo = u16::from_be_bytes(b);
            return EStep::Ok(1);
        }
        if !self.shift && jch & 0x8000 != 0 {
            return put(out, cap, &[0x8F, b[0], b[1]]);
        }
        put(out, cap, &b)
    }

    fn flush(&mut self, out: &mut Vec<u8>, cap: usize, _invocations: u64) -> Status {
        if self.lasttwo != 0 {
            if out.len() + 2 > cap {
                return Status::FullOutput;
            }
            out.extend_from_slice(&self.lasttwo.to_be_bytes());
            self.lasttwo = 0;
        }
        Status::Ok
    }
}

/// The module glibc's gconv cache uses for a conversion between two charsets directly, if any:
/// GB2312 to and from BIG5 (iconvdata/gbbig5.c), GBK to and from GB2312 (iconvdata/gbgbk.c),
/// IBM1008 to and from IBM420 (iconvdata/ibm1008_420.c). They are not steps from INTERNAL, so
/// glibc never transliterates in them.
pub(super) fn direct(from: &str, to: &str, ignore: bool) -> Option<Box<dyn Direct>> {
    Some(match (from, to) {
        ("EUC-CN//", "BIG5//") => Box::new(GbBig5 {
            to_big5: true,
            ignore,
        }),
        ("BIG5//", "EUC-CN//") => Box::new(GbBig5 {
            to_big5: false,
            ignore,
        }),
        ("GBK//", "EUC-CN//") => Box::new(GbGbk {
            to_gb: true,
            ignore,
        }),
        ("EUC-CN//", "GBK//") => Box::new(GbGbk {
            to_gb: false,
            ignore,
        }),
        ("IBM1008//", "IBM420//") => Box::new(ByteMap(&t::IBM1008_IBM420)),
        ("IBM420//", "IBM1008//") => Box::new(ByteMap(&t::IBM420_IBM1008)),
        _ => return None,
    })
}

/// `from_gb2312_to_big5` / `from_big5_to_gb2312` (iconvdata/gbbig5.c): positions without a
/// mapping become the box character (BIG5 A1BC, GB2312 A1F5) under `//IGNORE`.
struct GbBig5 {
    to_big5: bool,
    ignore: bool,
}

impl Direct for GbBig5 {
    fn run(&mut self, input: &[u8], pos: &mut usize, out: &mut Vec<u8>, cap: usize) -> Status {
        let table = Table(if self.to_big5 {
            t::GB2312_BIG5
        } else {
            t::BIG5_GB2312
        });
        let (lead_hi, boxed): (u8, [u8; 2]) = if self.to_big5 {
            (0xF7, [0xA1, 0xBC])
        } else {
            (0xF9, [0xA1, 0xF5])
        };
        let mut result = Status::EmptyInput;
        while *pos < input.len() {
            if out.len() >= cap {
                result = Status::FullOutput;
                break;
            }
            let s = &input[*pos..];
            let ch = s[0];
            if ch <= 0x7F {
                out.push(ch);
                *pos += 1;
                continue;
            }
            let illegal = if (0xA1..=lead_hi).contains(&ch) {
                if s.len() < 2 {
                    result = Status::IncompleteInput;
                    break;
                }
                let ch2 = s[1];
                let valid_trail = if self.to_big5 {
                    ch2 >= 0xA1
                } else {
                    (0x40..=0x7E).contains(&ch2) || (0xA1..=0xFE).contains(&ch2)
                };
                if valid_trail {
                    let v = table.get(ch, ch2);
                    if v == 0 {
                        result = Status::IllegalInput;
                        if !self.ignore {
                            break;
                        }
                        if out.len() + 2 > cap {
                            result = Status::FullOutput;
                            break;
                        }
                        out.extend_from_slice(&boxed);
                        *pos += 2;
                        continue;
                    }
                    if out.len() + 2 > cap {
                        result = Status::FullOutput;
                        break;
                    }
                    out.extend_from_slice(&(v as u16).to_be_bytes());
                    *pos += 2;
                    continue;
                }
                true
            } else {
                true
            };
            // STANDARD_FROM_LOOP_ERR_HANDLER(1)
            if illegal {
                result = Status::IllegalInput;
                if !self.ignore {
                    break;
                }
                *pos += 1;
            }
        }
        result
    }
}

/// `from_gbk_to_gb` / `from_gb_to_gbk` (iconvdata/gbgbk.c): GBK to GB2312 copies the sequences
/// GB2312 has; GB2312 to GBK copies everything, and like glibc it consumes a lead byte before it
/// finds the input incomplete or the output full (so input ending in a two-byte character is
/// "incomplete").
struct GbGbk {
    to_gb: bool,
    ignore: bool,
}

impl Direct for GbGbk {
    fn run(&mut self, input: &[u8], pos: &mut usize, out: &mut Vec<u8>, cap: usize) -> Status {
        let mut result = Status::EmptyInput;
        while *pos < input.len() {
            if out.len() >= cap {
                result = Status::FullOutput;
                break;
            }
            let s = &input[*pos..];
            let ch = s[0];
            if ch <= 0x7F {
                out.push(ch);
                *pos += 1;
                continue;
            }
            if !self.to_gb {
                *pos += 1;
                if s.len() < 3 {
                    result = Status::IncompleteInput;
                    break;
                }
                if cap - out.len() < 2 {
                    result = Status::FullOutput;
                    break;
                }
                out.extend_from_slice(&s[..2]);
                *pos += 1;
                continue;
            }
            if s.len() < 2 {
                result = Status::IncompleteInput;
                break;
            }
            if cap - out.len() < 2 {
                result = Status::FullOutput;
                break;
            }
            let mut code = (u32::from(ch) << 8) | u32::from(s[1]);
            if code == 0xA844 {
                code = 0xA1AA;
            }
            if !(0xA1A1..=0xF7FE).contains(&code)
                || s[1] < 0xA1
                || (0xA2A1..=0xA2AA).contains(&code)
                || (0xA6E0..=0xA6F5).contains(&code)
                || (0xA8BB..=0xA8C0).contains(&code)
            {
                // STANDARD_TO_LOOP_ERR_HANDLER(2) without transliteration
                result = Status::IllegalInput;
                if !self.ignore {
                    break;
                }
                *pos += 2;
                continue;
            }
            out.extend_from_slice(&s[..2]);
            *pos += 2;
        }
        result
    }
}

/// `from_ibm1008_to_ibm420` / `from_ibm420_to_ibm1008` (iconvdata/ibm1008_420.c): one byte to
/// one byte, no errors.
struct ByteMap(&'static [u8; 256]);

impl Direct for ByteMap {
    fn run(&mut self, input: &[u8], pos: &mut usize, out: &mut Vec<u8>, cap: usize) -> Status {
        while *pos < input.len() {
            if out.len() >= cap {
                return Status::FullOutput;
            }
            out.push(self.0[usize::from(input[*pos])]);
            *pos += 1;
        }
        Status::EmptyInput
    }
}
