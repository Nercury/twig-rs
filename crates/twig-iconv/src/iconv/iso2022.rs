//! glibc's stateful ISO-2022 modules: ISO-2022-JP and ISO-2022-JP-2 (iconvdata/iso-2022-jp.c),
//! ISO-2022-JP-3 (iconvdata/iso-2022-jp-3.c) and ISO-2022-KR (iconvdata/iso-2022-kr.c), with
//! their escape sequences, shift states, language tags (ISO-2022-JP-2), buffered composition
//! bases (ISO-2022-JP-3) and header (ISO-2022-KR).

use super::cjk::{
    Lookup, gb2312_to_ucs4, jisx0201_to_ucs4, jisx0208_to_ucs4, jisx0212_to_ucs4,
    jisx0213_added_in_2004, jisx0213_compose, jisx0213_pair, jisx0213_to_ucs4, ksc5601_to_ucs4,
    ucs4_to_gb2312, ucs4_to_jisx0201, ucs4_to_jisx0208, ucs4_to_jisx0212, ucs4_to_jisx0213,
    ucs4_to_ksc5601,
};
use super::cjk_tables as t;
use super::unicode::{char1, illegal, is_tag};
use super::{DStep, Decode, EStep, Encode, Status};

const ESC: u8 = 0x1B;

/// The decoder and encoder of an ISO-2022 module.
pub(super) fn codec(module: &str) -> Option<(Box<dyn Decode>, Box<dyn Encode>)> {
    match module {
        "ISO-2022-JP//" | "ISO-2022-JP-2//" => {
            let jp2 = module == "ISO-2022-JP-2//";
            Some((
                Box::new(JpDec {
                    jp2,
                    set: Set::Ascii,
                    set2: G2::None,
                }),
                Box::new(JpEnc {
                    jp2,
                    set: Set::Ascii,
                    set2: G2::None,
                    tag: Tag::None,
                }),
            ))
        }
        "ISO-2022-JP-3//" => Some((
            Box::new(Jp3Dec {
                set: Set::Ascii,
                pending: 0,
            }),
            Box::new(Jp3Enc {
                set: Set::Ascii,
                lasttwo: 0,
            }),
        )),
        "ISO-2022-KR//" => Some((
            Box::new(KrDec { ksc: false }),
            Box::new(KrEnc { ksc: false }),
        )),
        _ => None,
    }
}

/// The selected G0 set (`CURRENT_SEL_MASK`), with glibc's numbering.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Set {
    Ascii = 0,
    Jisx0208_1978 = 1,
    Jisx0208_1983 = 2,
    Jisx0201Roman = 3,
    Jisx0201Kana = 4,
    /// ISO-2022-JP-2: GB 2312; ISO-2022-JP-3: JIS X 0213 plane 1 (2000).
    Gb2312OrX0213_1_2000 = 5,
    /// ISO-2022-JP-2: KS C 5601; ISO-2022-JP-3: JIS X 0213 plane 2.
    Ksc5601OrX0213_2 = 6,
    /// ISO-2022-JP-2: JIS X 0212; ISO-2022-JP-3: JIS X 0213 plane 1 (2004).
    Jisx0212OrX0213_1_2004 = 7,
}

impl Set {
    fn from_bits(bits: u32) -> Set {
        match bits & 7 {
            0 => Set::Ascii,
            1 => Set::Jisx0208_1978,
            2 => Set::Jisx0208_1983,
            3 => Set::Jisx0201Roman,
            4 => Set::Jisx0201Kana,
            5 => Set::Gb2312OrX0213_1_2000,
            6 => Set::Ksc5601OrX0213_2,
            _ => Set::Jisx0212OrX0213_1_2004,
        }
    }
}

/// The G2 designation of ISO-2022-JP-2 (`CURRENT_ASSIGN_MASK`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum G2 {
    None = 0,
    Latin1 = 1,
    Greek = 2,
}

/// The language tag state of the ISO-2022-JP-2 encoder (`CURRENT_TAG_MASK`); values from
/// `Language` on are temporary parsing states.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Debug)]
enum Tag {
    None = 0,
    Ja = 1,
    Ko = 2,
    Zh = 3,
    Language = 4,
    LanguageJ = 5,
    LanguageK = 6,
    LanguageZ = 7,
}

/// The ISO 8859-7 upper half as the G2 set of ISO-2022-JP-2 (`iso88597_to_ucs4`).
fn g2_greek(b: u8) -> u32 {
    u32::from(t::G2_GREEK[usize::from(b - 0x20)])
}

/// `iso88597_from_ucs4` of ISO-2022-JP-2: the byte (0xA0..0xFF) of `cp`.
fn g2_greek_from_ucs4(cp: u32) -> Option<u8> {
    if let Some(&(_, b)) = t::G2_GREEK_ENC.iter().find(|&&(c, _)| c == cp) {
        return b.first().copied();
    }
    (0x20..0x80u8)
        .find(|&b| g2_greek(b) == cp && cp != 0)
        .map(|b| b | 0x80)
}

/// `from_iso2022jp_loop` (iconvdata/iso-2022-jp.c).
struct JpDec {
    jp2: bool,
    set: Set,
    set2: G2,
}

impl Decode for JpDec {
    fn body(&mut self, input: &[u8], pos: usize, out: &mut Vec<u32>, _max: usize) -> DStep {
        let s = &input[pos..];
        let ch = s[0];
        if ch == ESC {
            if s.len() < 3 || (self.jp2 && s[1] == b'$' && s[2] == b'(' && s.len() < 4) {
                return DStep::Incomplete;
            }
            let designation = match (s[1], s[2]) {
                (b'(', b'B') => Some((Set::Ascii, 3)),
                (b'(', b'J') => Some((Set::Jisx0201Roman, 3)),
                (b'(', b'I') if self.jp2 => Some((Set::Jisx0201Kana, 3)),
                (b'$', b'@') => Some((Set::Jisx0208_1978, 3)),
                (b'$', b'B') => Some((Set::Jisx0208_1983, 3)),
                (b'$', b'A') if self.jp2 => Some((Set::Gb2312OrX0213_1_2000, 3)),
                (b'$', b'(') if self.jp2 && s[3] == b'C' => Some((Set::Ksc5601OrX0213_2, 4)),
                (b'$', b'(') if self.jp2 && s[3] == b'D' => Some((Set::Jisx0212OrX0213_1_2004, 4)),
                _ => None,
            };
            if let Some((set, len)) = designation {
                self.set = set;
                return DStep::Ok(len);
            }
            if self.jp2 && s[1] == b'.' && (s[2] == b'A' || s[2] == b'F') {
                self.set2 = if s[2] == b'A' { G2::Latin1 } else { G2::Greek };
                return DStep::Ok(3);
            }
            if self.jp2 && s[1] == b'N' {
                return match self.set2 {
                    G2::Latin1 => char1(out, u32::from(s[2] | 0x80), 3),
                    G2::Greek => {
                        if s[2] < 0x20 || s[2] >= 0x80 {
                            illegal(1)
                        } else {
                            match g2_greek(s[2]) {
                                0 => illegal(3),
                                cp => char1(out, cp, 3),
                            }
                        }
                    }
                    G2::None => illegal(1),
                };
            }
        }
        if ch >= 0x80 {
            return illegal(1);
        }
        if self.set == Set::Ascii || ch < 0x21 || ch == 0x7F {
            return char1(out, u32::from(ch), 1);
        }
        match self.set {
            Set::Jisx0201Roman | Set::Jisx0201Kana => {
                let b = if self.set == Set::Jisx0201Kana {
                    ch + 0x80
                } else {
                    ch
                };
                match jisx0201_to_ucs4(b) {
                    Some(cp) => char1(out, cp, 1),
                    None => illegal(1),
                }
            }
            _ => {
                let r = match self.set {
                    Set::Jisx0208_1978 | Set::Jisx0208_1983 => jisx0208_to_ucs4(s, 0),
                    Set::Jisx0212OrX0213_1_2004 => jisx0212_to_ucs4(s, 0),
                    Set::Gb2312OrX0213_1_2000 => gb2312_to_ucs4(s, 0),
                    _ => ksc5601_to_ucs4(s, 0),
                };
                match r {
                    Lookup::Char(cp) => char1(out, cp, 2),
                    Lookup::Incomplete => DStep::Incomplete,
                    Lookup::Unknown => illegal(1),
                }
            }
        }
    }

    fn save(&self) -> u32 {
        self.set as u32 | (self.set2 as u32) << 3
    }

    fn restore(&mut self, state: u32) {
        self.set = Set::from_bits(state);
        self.set2 = match (state >> 3) & 3 {
            1 => G2::Latin1,
            2 => G2::Greek,
            _ => G2::None,
        };
    }
}

/// The candidate sets of the ISO-2022-JP-2 encoder in the order of the language tag
/// (`conversion_lists`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conv {
    European,
    Japanese,
    Chinese,
    Korean,
    Other,
}

/// `to_iso2022jp_loop` (iconvdata/iso-2022-jp.c).
struct JpEnc {
    jp2: bool,
    set: Set,
    set2: G2,
    tag: Tag,
}

/// Room left in the output buffer.
fn room(out: &[u8], cap: usize) -> usize {
    cap - out.len()
}

impl JpEnc {
    /// One attempt of the conversion list: `None` when the set does not have the character,
    /// `Some(Full)` or `Some(Ok(1))` otherwise.
    fn try_conv(&mut self, conv: Conv, ch: u32, out: &mut Vec<u8>, cap: usize) -> Option<EStep> {
        match conv {
            Conv::European => {
                if (0x80..=0xFF).contains(&ch) {
                    if self.set2 != G2::Latin1 {
                        if room(out, cap) < 3 {
                            return Some(EStep::Full);
                        }
                        out.extend_from_slice(&[ESC, b'.', b'A']);
                        self.set2 = G2::Latin1;
                    }
                    if room(out, cap) < 3 {
                        return Some(EStep::Full);
                    }
                    out.extend_from_slice(&[ESC, b'N', (ch - 0x80) as u8]);
                    return Some(EStep::Ok(1));
                }
                let b = if ch < 0xFFFF {
                    g2_greek_from_ucs4(ch)
                } else {
                    None
                }?;
                if self.set2 != G2::Greek {
                    if room(out, cap) < 3 {
                        return Some(EStep::Full);
                    }
                    out.extend_from_slice(&[ESC, b'.', b'F']);
                    self.set2 = G2::Greek;
                }
                if room(out, cap) < 3 {
                    return Some(EStep::Full);
                }
                out.extend_from_slice(&[ESC, b'N', b - 0x80]);
                Some(EStep::Ok(1))
            }
            Conv::Japanese => {
                if let Some(b) = ucs4_to_jisx0201(ch).filter(|&b| b > 0x20 && b < 0x80) {
                    return Some(self.designate(Set::Jisx0201Roman, b"(J", &[b], out, cap));
                }
                if let Some(b) = ucs4_to_jisx0208(ch) {
                    return Some(self.designate(Set::Jisx0208_1983, b"$B", &b, out, cap));
                }
                if !self.jp2 {
                    return None;
                }
                let b = ucs4_to_jisx0212(ch)?;
                Some(self.designate(Set::Jisx0212OrX0213_1_2004, b"$(D", &b, out, cap))
            }
            Conv::Chinese => {
                let b = ucs4_to_gb2312(ch)?;
                Some(self.designate(Set::Gb2312OrX0213_1_2000, b"$A", &b, out, cap))
            }
            Conv::Korean => {
                let b = ucs4_to_ksc5601(ch)?;
                Some(self.designate(Set::Ksc5601OrX0213_2, b"$(C", &b, out, cap))
            }
            Conv::Other => {
                let b = ucs4_to_jisx0201(ch).filter(|&b| b >= 0x80)?;
                Some(self.designate(Set::Jisx0201Kana, b"(I", &[b - 0x80], out, cap))
            }
        }
    }

    /// Writes the designation `ESC <seq>` unless `set` is selected, then the bytes.
    fn designate(
        &mut self,
        set: Set,
        seq: &[u8],
        bytes: &[u8],
        out: &mut Vec<u8>,
        cap: usize,
    ) -> EStep {
        if self.set != set {
            if room(out, cap) < 1 + seq.len() {
                return EStep::Full;
            }
            out.push(ESC);
            out.extend_from_slice(seq);
            self.set = set;
        }
        if room(out, cap) < bytes.len() {
            return EStep::Full;
        }
        out.extend_from_slice(bytes);
        EStep::Ok(1)
    }

    /// Whether the selected set may be used under the current language tag.
    fn tag_allows(&self, preferred: Tag) -> bool {
        self.tag == Tag::None || self.tag == preferred
    }
}

impl Encode for JpEnc {
    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let mut ch = chars[i];
        if self.jp2 {
            if is_tag(ch) {
                ch &= 0x7F;
                if (0x41..=0x5A).contains(&ch) {
                    ch += 0x20;
                }
                self.tag = match (ch, self.tag) {
                    (0x01, _) => Tag::Language,
                    (0x6A, Tag::Language) => Tag::LanguageJ,
                    (0x61, Tag::LanguageJ) => Tag::Ja,
                    (0x6B, Tag::Language) => Tag::LanguageK,
                    (0x6F, Tag::LanguageK) => Tag::Ko,
                    (0x7A, Tag::Language) => Tag::LanguageZ,
                    (0x68, Tag::LanguageZ) => Tag::Zh,
                    (0x7F, _) => Tag::None,
                    (_, tag) if tag >= Tag::Language => Tag::None,
                    (_, tag) => tag,
                };
                return EStep::Ok(1);
            }
            if self.tag >= Tag::Language {
                self.tag = Tag::None;
            }
        }

        // the selected set, unless the language tag prefers another
        let mut written = false;
        match self.set {
            Set::Ascii => {
                if ch <= 0x7F {
                    out.push(ch as u8);
                    written = true;
                    if self.jp2 && ch == 0x0A {
                        self.set2 = G2::None;
                    }
                }
            }
            Set::Jisx0201Roman | Set::Jisx0201Kana if self.tag_allows(Tag::Ja) => {
                let kana = self.set == Set::Jisx0201Kana;
                if let Some(b) = ucs4_to_jisx0201(ch) {
                    if !kana && b > 0x20 && b < 0x80 {
                        out.push(b);
                        written = true;
                    } else if kana && b > 0xA0 && b < 0xE0 {
                        out.push(b - 0x80);
                        written = true;
                    }
                }
            }
            Set::Jisx0201Roman | Set::Jisx0201Kana => {}
            _ => {
                let (allowed, b) = match self.set {
                    Set::Jisx0208_1978 | Set::Jisx0208_1983 => {
                        // ucs4_to_jisx0208() checks the room before the lookup
                        if self.tag_allows(Tag::Ja) && room(out, cap) < 2 {
                            return EStep::Full;
                        }
                        (self.tag_allows(Tag::Ja), ucs4_to_jisx0208(ch))
                    }
                    Set::Jisx0212OrX0213_1_2004 => (self.tag_allows(Tag::Ja), ucs4_to_jisx0212(ch)),
                    Set::Gb2312OrX0213_1_2000 => (self.tag_allows(Tag::Zh), ucs4_to_gb2312(ch)),
                    _ => (self.tag_allows(Tag::Ko), ucs4_to_ksc5601(ch)),
                };
                if allowed && let Some(b) = b {
                    if room(out, cap) < 2 {
                        return EStep::Full;
                    }
                    out.extend_from_slice(&b);
                    written = true;
                }
            }
        }

        if !written && self.tag == Tag::None {
            // the designated G2 set
            match self.set2 {
                G2::Latin1 if (0x80..=0xFF).contains(&ch) => {
                    if room(out, cap) < 3 {
                        return EStep::Full;
                    }
                    out.extend_from_slice(&[ESC, b'N', (ch & 0x7F) as u8]);
                    written = true;
                }
                G2::Greek if ch < 0xFFFF => {
                    if let Some(b) = g2_greek_from_ucs4(ch) {
                        if room(out, cap) < 3 {
                            return EStep::Full;
                        }
                        out.extend_from_slice(&[ESC, b'N', b & 0x7F]);
                        written = true;
                    }
                }
                _ => {}
            }
        }

        if written {
            return EStep::Ok(1);
        }
        if ch <= 0x7F {
            if room(out, cap) < 3 {
                return EStep::Full;
            }
            out.extend_from_slice(&[ESC, b'(', b'B']);
            self.set = Set::Ascii;
            if room(out, cap) < 1 {
                return EStep::Full;
            }
            out.push(ch as u8);
            if self.jp2 && ch == 0x0A {
                self.set2 = G2::None;
            }
            return EStep::Ok(1);
        }
        let list: &[Conv] = if !self.jp2 {
            &[Conv::Japanese]
        } else {
            match self.tag {
                Tag::Ko => &[
                    Conv::Korean,
                    Conv::European,
                    Conv::Japanese,
                    Conv::Chinese,
                    Conv::Other,
                ],
                Tag::Zh => &[
                    Conv::Chinese,
                    Conv::European,
                    Conv::Japanese,
                    Conv::Korean,
                    Conv::Other,
                ],
                _ => &[
                    Conv::Japanese,
                    Conv::European,
                    Conv::Chinese,
                    Conv::Korean,
                    Conv::Other,
                ],
            }
        };
        for &conv in list {
            if let Some(step) = self.try_conv(conv, ch, out, cap) {
                return step;
            }
        }
        EStep::Unmappable
    }

    /// `EMIT_SHIFT_TO_INIT`: `ESC ( B` unless ASCII is selected; also clears the G2 designation
    /// and the language tag.
    fn flush(&mut self, out: &mut Vec<u8>, cap: usize, _invocations: u64) -> Status {
        if self.set == Set::Ascii && self.set2 == G2::None && self.tag == Tag::None {
            return Status::Ok;
        }
        if self.set != Set::Ascii {
            if room(out, cap) < 3 {
                return Status::FullOutput;
            }
            out.extend_from_slice(&[ESC, b'(', b'B']);
        }
        self.set = Set::Ascii;
        self.set2 = G2::None;
        self.tag = Tag::None;
        Status::Ok
    }
}

/// `from_iso2022jp3_loop` (iconvdata/iso-2022-jp-3.c).
struct Jp3Dec {
    set: Set,
    /// The queued second character of a JIS X 0213 pair.
    pending: u32,
}

impl Decode for Jp3Dec {
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
        if ch == ESC {
            if s.len() < 3 || (s[1] == b'$' && s[2] == b'(' && s.len() < 4) {
                return DStep::Incomplete;
            }
            let designation = match (s[1], s[2]) {
                (b'(', b'B') => Some((Set::Ascii, 3)),
                (b'(', b'J') => Some((Set::Jisx0201Roman, 3)),
                (b'(', b'I') => Some((Set::Jisx0201Kana, 3)),
                (b'$', b'@') => Some((Set::Jisx0208_1978, 3)),
                (b'$', b'B') => Some((Set::Jisx0208_1983, 3)),
                (b'$', b'(') if s[3] == b'O' || s[3] == b'Q' => {
                    Some((Set::Jisx0212OrX0213_1_2004, 4))
                }
                (b'$', b'(') if s[3] == b'P' => Some((Set::Ksc5601OrX0213_2, 4)),
                _ => None,
            };
            if let Some((set, len)) = designation {
                self.set = set;
                return DStep::Ok(len);
            }
        }
        if ch >= 0x80 {
            return illegal(1);
        }
        if self.set == Set::Ascii || ch < 0x21 || ch == 0x7F {
            return char1(out, u32::from(ch), 1);
        }
        match self.set {
            Set::Jisx0201Roman | Set::Jisx0201Kana => {
                let b = if self.set == Set::Jisx0201Kana {
                    ch + 0x80
                } else {
                    ch
                };
                match jisx0201_to_ucs4(b) {
                    Some(cp) => char1(out, cp, 1),
                    None => illegal(1),
                }
            }
            Set::Jisx0208_1978 | Set::Jisx0208_1983 => match jisx0208_to_ucs4(s, 0) {
                Lookup::Char(cp) => char1(out, cp, 2),
                Lookup::Incomplete => DStep::Incomplete,
                Lookup::Unknown => illegal(1),
            },
            _ => {
                if s.len() < 2 {
                    return DStep::Incomplete;
                }
                // ((JISX0213_1_2004_set - set + (1 << 3)) << 5) + ch: plane 1 rows 0x121.., plane 2
                // rows 0x221..
                let plane = if self.set == Set::Ksc5601OrX0213_2 {
                    0x200
                } else {
                    0x100
                };
                let v = jisx0213_to_ucs4(plane + u32::from(ch), u32::from(s[1]));
                if v == 0 {
                    return illegal(1);
                }
                if v >= 0x80 {
                    return char1(out, v, 2);
                }
                let (u1, u2) = jisx0213_pair(v);
                out.push(u1);
                if out.len() < max {
                    out.push(u2);
                    return DStep::Ok(2);
                }
                self.pending = u2;
                DStep::Full(2)
            }
        }
    }

    fn save(&self) -> u32 {
        self.set as u32 | self.pending << 6
    }

    fn restore(&mut self, state: u32) {
        self.set = Set::from_bits(state);
        self.pending = state >> 6;
    }

    /// `EMIT_SHIFT_TO_INIT` in the from direction: the queued character.
    fn emit_shift_to_init(&mut self, out: &mut Vec<u32>) {
        if self.pending != 0 {
            out.push(self.pending);
        }
        self.pending = 0;
        self.set = Set::Ascii;
    }
}

/// `to_iso2022jp3_loop` (iconvdata/iso-2022-jp-3.c).
struct Jp3Enc {
    set: Set,
    /// The buffered JIS X 0208/0213 position (7-bit, bit 16: `ESC $ B` must precede it).
    lasttwo: u32,
}

impl Jp3Enc {
    fn x0213_set(jch: u16) -> Set {
        if jch & 0x8000 != 0 {
            Set::Ksc5601OrX0213_2
        } else if jisx0213_added_in_2004(jch & 0x7F7F) {
            Set::Jisx0212OrX0213_1_2004
        } else {
            Set::Gb2312OrX0213_1_2000
        }
    }

    /// Whether the selected JIS X 0213 set can write `jch`.
    fn x0213_fits(&self, jch: u16) -> bool {
        if jch & 0x8000 != 0 {
            self.set == Set::Ksc5601OrX0213_2
        } else {
            self.set == Set::Jisx0212OrX0213_1_2004
                || (self.set == Set::Gb2312OrX0213_1_2000 && !jisx0213_added_in_2004(jch & 0x7F7F))
        }
    }

    fn two(jch: u16) -> [u8; 2] {
        [((jch >> 8) & 0x7F) as u8, (jch & 0x7F) as u8]
    }
}

impl Encode for Jp3Enc {
    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let ch = chars[i];
        if self.lasttwo != 0 {
            let base = (self.lasttwo & 0x7F7F) as u16;
            if let Some(composed) = jisx0213_compose(base, ch) {
                let x0213_1 = matches!(
                    self.set,
                    Set::Gb2312OrX0213_1_2000 | Set::Jisx0212OrX0213_1_2004
                );
                let need = if self.lasttwo >> 16 != 0 || !x0213_1 {
                    4
                } else {
                    0
                };
                if room(out, cap) < need + 2 {
                    return EStep::Full;
                }
                if need != 0 {
                    out.extend_from_slice(&[ESC, b'$', b'(', b'O']);
                    self.set = Set::Gb2312OrX0213_1_2000;
                }
                out.extend_from_slice(&Self::two(composed));
                self.lasttwo = 0;
                return EStep::Ok(1);
            }
            let need = if self.lasttwo >> 16 != 0 { 3 } else { 0 };
            if room(out, cap) < need + 2 {
                return EStep::Full;
            }
            if need != 0 {
                out.extend_from_slice(&[ESC, b'$', b'B']);
            }
            out.extend_from_slice(&Self::two(base));
            self.lasttwo = 0;
            return EStep::Ok(0);
        }

        // the selected set
        match self.set {
            Set::Ascii => {
                if ch <= 0x7F {
                    out.push(ch as u8);
                    return EStep::Ok(1);
                }
            }
            Set::Jisx0201Roman => {
                if let Some(b) = ucs4_to_jisx0201(ch).filter(|&b| b > 0x20 && b < 0x80) {
                    out.push(b);
                    return EStep::Ok(1);
                }
            }
            Set::Jisx0201Kana => {
                if let Some(b) = ucs4_to_jisx0201(ch).filter(|&b| b >= 0x80) {
                    out.push(b - 0x80);
                    return EStep::Ok(1);
                }
            }
            Set::Jisx0208_1983 => {
                // ucs4_to_jisx0208() returns 0 ("no room") before it looks the character up, and
                // the module buffers a composition base before it checks that result
                let no_room = room(out, cap) < 2;
                let b = if no_room { None } else { ucs4_to_jisx0208(ch) };
                if no_room || b.is_some() {
                    let jch = ucs4_to_jisx0213(ch);
                    if jch & 0x80 != 0 {
                        self.lasttwo = u32::from(jch & 0x7F7F);
                        return EStep::Ok(1);
                    }
                    let Some(b) = b else {
                        return EStep::Full;
                    };
                    out.extend_from_slice(&b);
                    return EStep::Ok(1);
                }
            }
            Set::Jisx0208_1978 => {}
            _ => {
                let jch = ucs4_to_jisx0213(ch);
                if jch != 0 && self.x0213_fits(jch) {
                    if jch & 0x80 != 0 {
                        self.lasttwo = u32::from(jch & 0x7F7F);
                        return EStep::Ok(1);
                    }
                    if room(out, cap) < 2 {
                        return EStep::Full;
                    }
                    out.extend_from_slice(&Self::two(jch));
                    return EStep::Ok(1);
                }
            }
        }

        // another set
        if ch <= 0x7F {
            if room(out, cap) < 3 {
                return EStep::Full;
            }
            out.extend_from_slice(&[ESC, b'(', b'B']);
            self.set = Set::Ascii;
            if room(out, cap) < 1 {
                return EStep::Full;
            }
            out.push(ch as u8);
            return EStep::Ok(1);
        }
        if let Some(b) = ucs4_to_jisx0201(ch).filter(|&b| b > 0x20 && b < 0x80) {
            if self.set != Set::Jisx0201Roman {
                if room(out, cap) < 3 {
                    return EStep::Full;
                }
                out.extend_from_slice(&[ESC, b'(', b'J']);
                self.set = Set::Jisx0201Roman;
            }
            if room(out, cap) < 1 {
                return EStep::Full;
            }
            out.push(b);
            return EStep::Ok(1);
        }
        let jch = ucs4_to_jisx0213(ch);
        if let Some(b) = ucs4_to_jisx0208(ch) {
            if jch & 0x80 != 0 {
                let flag = u32::from(self.set != Set::Jisx0208_1983) << 16;
                self.lasttwo = flag | u32::from(jch & 0x7F7F);
                self.set = Set::Jisx0208_1983;
                return EStep::Ok(1);
            }
            if self.set != Set::Jisx0208_1983 {
                if room(out, cap) < 3 {
                    return EStep::Full;
                }
                out.extend_from_slice(&[ESC, b'$', b'B']);
                self.set = Set::Jisx0208_1983;
            }
            if room(out, cap) < 2 {
                return EStep::Full;
            }
            out.extend_from_slice(&b);
            return EStep::Ok(1);
        }
        if jch != 0 {
            let new_set = Self::x0213_set(jch);
            if self.set != new_set {
                if room(out, cap) < 4 {
                    return EStep::Full;
                }
                let final_byte = match new_set {
                    Set::Gb2312OrX0213_1_2000 => b'O',
                    Set::Ksc5601OrX0213_2 => b'P',
                    _ => b'Q',
                };
                out.extend_from_slice(&[ESC, b'$', b'(', final_byte]);
                self.set = new_set;
            }
            if jch & 0x80 != 0 {
                self.lasttwo = u32::from(jch & 0x7F7F);
                return EStep::Ok(1);
            }
            if room(out, cap) < 2 {
                return EStep::Full;
            }
            out.extend_from_slice(&Self::two(jch));
            return EStep::Ok(1);
        }
        if let Some(b) = ucs4_to_jisx0201(ch).filter(|&b| b >= 0x80) {
            if self.set != Set::Jisx0201Kana {
                if room(out, cap) < 3 {
                    return EStep::Full;
                }
                out.extend_from_slice(&[ESC, b'(', b'I']);
                self.set = Set::Jisx0201Kana;
            }
            if room(out, cap) < 1 {
                return EStep::Full;
            }
            out.push(b - 0x80);
            return EStep::Ok(1);
        }
        if is_tag(ch) {
            return EStep::Ok(1);
        }
        EStep::Unmappable
    }

    /// `EMIT_SHIFT_TO_INIT`: the buffered character (with its pending `ESC $ B`) and `ESC ( B`.
    fn flush(&mut self, out: &mut Vec<u8>, cap: usize, _invocations: u64) -> Status {
        if self.set == Set::Ascii && self.lasttwo == 0 {
            return Status::Ok;
        }
        let mut need = 0;
        if self.lasttwo != 0 {
            need += 2 + if self.lasttwo >> 16 != 0 { 3 } else { 0 };
        }
        if self.set != Set::Ascii {
            need += 3;
        }
        if room(out, cap) < need {
            return Status::FullOutput;
        }
        if self.lasttwo != 0 {
            if self.lasttwo >> 16 != 0 {
                out.extend_from_slice(&[ESC, b'$', b'B']);
            }
            out.extend_from_slice(&Self::two((self.lasttwo & 0x7F7F) as u16));
        }
        if self.set != Set::Ascii {
            out.extend_from_slice(&[ESC, b'(', b'B']);
        }
        self.set = Set::Ascii;
        self.lasttwo = 0;
        Status::Ok
    }
}

/// `from_iso2022kr_loop` (iconvdata/iso-2022-kr.c).
struct KrDec {
    /// `SO` selected KS C 5601.
    ksc: bool,
}

impl Decode for KrDec {
    fn body(&mut self, input: &[u8], pos: usize, out: &mut Vec<u32>, _max: usize) -> DStep {
        let s = &input[pos..];
        let ch = s[0];
        if ch > 0x7F {
            return illegal(1);
        }
        match ch {
            ESC => {
                if s.len() < 2 || (s[1] == b'$' && (s.len() < 3 || (s[2] == b')' && s.len() < 4))) {
                    return DStep::Incomplete;
                }
                if s[1] == b'$' && s[2] == b')' && s[3] == b'C' {
                    return DStep::Ok(4);
                }
            }
            0x0E => {
                self.ksc = true;
                return DStep::Ok(1);
            }
            0x0F => {
                self.ksc = false;
                return DStep::Ok(1);
            }
            _ => {}
        }
        if !self.ksc {
            return char1(out, u32::from(ch), 1);
        }
        match ksc5601_to_ucs4(s, 0) {
            Lookup::Char(cp) => char1(out, cp, 2),
            Lookup::Incomplete => DStep::Incomplete,
            Lookup::Unknown => illegal(1),
        }
    }

    fn save(&self) -> u32 {
        u32::from(self.ksc)
    }

    fn restore(&mut self, state: u32) {
        self.ksc = state != 0;
    }
}

/// `to_iso2022kr_loop` (iconvdata/iso-2022-kr.c).
struct KrEnc {
    ksc: bool,
}

impl KrEnc {
    /// `PREPARE_LOOP`: the designator `ESC $ ) C` before anything else, on every call of the
    /// step function while its invocation counter is 0.
    fn header(out: &mut Vec<u8>, cap: usize, invocations: u64) -> Option<Status> {
        if invocations != 0 {
            return None;
        }
        if room(out, cap) < 4 {
            return Some(Status::FullOutput);
        }
        out.extend_from_slice(b"\x1b$)C");
        None
    }
}

impl Encode for KrEnc {
    fn prepare(&mut self, out: &mut Vec<u8>, cap: usize, invocations: u64) -> Option<Status> {
        Self::header(out, cap, invocations)
    }

    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let ch = chars[i];
        if ch < 0x80 {
            if self.ksc {
                out.push(0x0F);
                self.ksc = false;
                if room(out, cap) == 0 {
                    return EStep::Full;
                }
            }
            out.push(ch as u8);
            return EStep::Ok(1);
        }
        let Some(b) = ucs4_to_ksc5601(ch) else {
            return if is_tag(ch) {
                EStep::Ok(1)
            } else {
                EStep::Unmappable
            };
        };
        if !self.ksc {
            out.push(0x0E);
            self.ksc = true;
        }
        if room(out, cap) < 2 {
            return EStep::Full;
        }
        out.extend_from_slice(&b);
        EStep::Ok(1)
    }

    /// `PREPARE_LOOP` and `EMIT_SHIFT_TO_INIT`: the header if nothing was converted, then `SI`.
    fn flush(&mut self, out: &mut Vec<u8>, cap: usize, invocations: u64) -> Status {
        if let Some(status) = Self::header(out, cap, invocations) {
            return status;
        }
        if self.ksc {
            if room(out, cap) == 0 {
                return Status::FullOutput;
            }
            out.push(0x0F);
            self.ksc = false;
        }
        Status::Ok
    }
}
