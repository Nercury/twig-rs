//! The stateful ISO-2022 charsets `ISO-2022-JP`, `ISO-2022-JP-2` and `ISO-2022-KR`, as glibc 2.41's iconv converts
//! them in the oracle (observed black-box: every sequence of up to four bytes decoded from every designation, every
//! code point encoded from every state, and random sequences; see tests/vectors/string/charset_basic). The graphic
//! sets are the coded character sets of `multibyte.rs` (JIS X 0208, JIS X 0212, GB 2312, KS X 1001, which the
//! measurements show identical), JIS X 0201 (Roman and Katakana), and the upper halves of ISO-8859-1 and ISO-8859-7.
//!
//! Decoding (RFC 1468, RFC 1554, RFC 1557 for the structure; the details are glibc's):
//!
//! - ISO-2022-JP: `ESC ( B` ASCII, `ESC ( J` JIS X 0201 Roman, `ESC $ @` and `ESC $ B` JIS X 0208 into G0;
//!   ISO-2022-JP-2 adds `ESC $ A` GB 2312, `ESC $ ( C` KS X 1001, `ESC $ ( D` JIS X 0212, `ESC ( I` JIS X 0201
//!   Katakana, and `ESC . A`, `ESC . F` (ISO-8859-1, ISO-8859-7) into G2, whose characters follow `ESC N`;
//! - an escape sequence glibc does not know is the character ESC and what follows, but the decoder waits for two
//!   bytes after ESC (and for all of a sequence it knows the start of); in the two-byte sets the control characters
//!   (below 0x21 and 0x7F) are themselves, and GB 2312 and JIS X 0212 reject a first byte beyond their rows;
//! - ISO-2022-KR: the header `ESC $ ) C` (anywhere, as nothing), SO and SI switch between ASCII and KS X 1001; after
//!   SO every byte but SI, SO and those above 0x7F starts a pair;
//! - an invalid sequence makes //IGNORE skip one byte, but three for `ESC N` and a byte ISO-8859-7 lacks.
//!
//! Encoding: a character goes into the set designated where it can (G0, then G2), else into the first set that has
//! it in the order ASCII, JIS X 0201 Roman, JIS X 0208, JIS X 0212, GB 2312, ISO-8859-1, ISO-8859-7, KS X 1001,
//! JIS X 0201 Katakana (ISO-2022-JP: the first three), after its designation; ISO-8859-1 holds U+0080..U+00FF and
//! JIS X 0212 also takes `~`. ISO-2022-KR writes its header first (also into an empty output), and SO/SI around KS
//! X 1001. A designation is written (and holds) even when the character after it does not fit (`E2BIG`); a line
//! feed makes ISO-2022-JP-2 forget its G2 designation; the flush returns to ASCII (`ESC ( B`, SI). The tag
//! characters are invalid in ISO-2022-JP and vanish in ISO-2022-JP-2 and ISO-2022-KR.

use super::codec::Decoded;
use super::codepage::Codepage;
use super::multibyte::{self, Family, Multibyte};

/// An ISO-2022 charset this module converts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Iso2022 {
    /// `ISO-2022-JP`.
    Jp,
    /// `ISO-2022-JP-2`.
    Jp2,
    /// `ISO-2022-KR`.
    Kr,
}

/// A graphic set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Set {
    /// ASCII.
    #[default]
    Ascii,
    /// JIS X 0201 Roman.
    Roman,
    /// JIS X 0208.
    Jis0208,
    /// JIS X 0212.
    Jis0212,
    /// GB 2312.
    Gb2312,
    /// KS X 1001.
    Ksc,
    /// JIS X 0201 Katakana.
    Kana,
    /// The upper half of ISO-8859-1 (G2).
    Latin1,
    /// The upper half of ISO-8859-7 (G2).
    Greek,
}

/// The state of a decoder or an encoder: the sets designated and whether SO is in effect (ISO-2022-KR); for
/// ISO-2022-JP-2's encoder also the language a tag selected and the tag being read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct State {
    g0: Set,
    g2: Option<Set>,
    so: bool,
    lang: Lang,
    tag: Tag,
}

/// The language of an ISO-2022-JP-2 language tag (U+E0001 and tag characters, of which the first two count, as
/// letters of either case: `ja-JP`, `JA` and `jA` are `ja`, `j-a` and ` ja` none): once one is selected the encoder
/// keeps the set designated only when it is one of that language's (and has the character), else it takes the first
/// set that has the character in the language's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Lang {
    /// No tag (or one for another language, or U+E007F CANCEL TAG): the set designated first, then the default order.
    #[default]
    None,
    /// `ja`: the default order.
    Ja,
    /// `ko`: KS X 1001 first.
    Ko,
    /// `zh`: GB 2312 first.
    Zh,
}

impl Lang {
    /// Whether the encoder keeps the designated `set` for a character it has.
    fn keeps(self, set: Set) -> bool {
        match self {
            Lang::None => true,
            Lang::Ja => matches!(
                set,
                Set::Ascii | Set::Roman | Set::Jis0208 | Set::Jis0212 | Set::Kana
            ),
            Lang::Ko => matches!(set, Set::Ascii | Set::Ksc),
            Lang::Zh => matches!(set, Set::Ascii | Set::Gb2312),
        }
    }
}

/// A language tag being read: `Letters(n, first)` after U+E0001 and `n` tag characters (U+E0000..U+E007E but
/// U+E0001), the first two of them in `first` (their low byte).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Tag {
    #[default]
    None,
    Letters(u8, [u8; 2]),
}

const ESC: u8 = 0x1B;
const SO: u8 = 0x0E;
const SI: u8 = 0x0F;

/// The escape sequences of ISO-2022-JP (after ESC) and the sets they designate.
const JP_ESCAPES: [(&[u8], Set); 4] = [
    (b"$@", Set::Jis0208),
    (b"$B", Set::Jis0208),
    (b"(B", Set::Ascii),
    (b"(J", Set::Roman),
];

/// The escape sequences ISO-2022-JP-2 adds.
const JP2_ESCAPES: [(&[u8], Set); 6] = [
    (b"$A", Set::Gb2312),
    (b"(I", Set::Kana),
    (b".A", Set::Latin1),
    (b".F", Set::Greek),
    (b"$(C", Set::Ksc),
    (b"$(D", Set::Jis0212),
];

/// The KS X 1001 designation of ISO-2022-KR, its header (after ESC).
const KR_HEADER: &[u8] = b"$)C";

/// The header ISO-2022-KR writes before its output.
pub(crate) const KR_PREFIX: &[u8] = b"\x1b$)C";

/// The order in which the encoder looks for a set that has a character.
const JP_ORDER: [Set; 3] = [Set::Ascii, Set::Roman, Set::Jis0208];
const JP2_ORDER: [Set; 9] = [
    Set::Ascii,
    Set::Roman,
    Set::Jis0208,
    Set::Jis0212,
    Set::Gb2312,
    Set::Latin1,
    Set::Greek,
    Set::Ksc,
    Set::Kana,
];
const KO_ORDER: [Set; 9] = [
    Set::Ascii,
    Set::Ksc,
    Set::Latin1,
    Set::Greek,
    Set::Jis0208,
    Set::Jis0212,
    Set::Gb2312,
    Set::Roman,
    Set::Kana,
];
const ZH_ORDER: [Set; 9] = [
    Set::Ascii,
    Set::Gb2312,
    Set::Latin1,
    Set::Greek,
    Set::Jis0208,
    Set::Jis0212,
    Set::Ksc,
    Set::Roman,
    Set::Kana,
];

impl Set {
    /// The escape sequence that designates the set.
    fn escape(self) -> &'static [u8] {
        match self {
            Set::Ascii => b"\x1b(B",
            Set::Roman => b"\x1b(J",
            Set::Jis0208 => b"\x1b$B",
            Set::Jis0212 => b"\x1b$(D",
            Set::Gb2312 => b"\x1b$A",
            Set::Ksc => b"\x1b$(C",
            Set::Kana => b"\x1b(I",
            Set::Latin1 => b"\x1b.A",
            Set::Greek => b"\x1b.F",
        }
    }

    /// The plane of `multibyte.rs` that holds a two-byte set (in its EUC form).
    fn plane(self) -> Option<u8> {
        match self {
            Set::Jis0208 => Some(Multibyte::EucJp.spec().plane),
            Set::Jis0212 => match Multibyte::EucJp.spec().family {
                Family::EucJp { plane, .. } => Some(plane),
                _ => None,
            },
            Set::Gb2312 => Some(Multibyte::EucCn.spec().plane),
            Set::Ksc => Some(Multibyte::EucKr.spec().plane),
            _ => None,
        }
    }

    /// The first bytes of the set's rows that glibc reads a second byte for.
    fn leads(self) -> (u8, u8) {
        match self {
            Set::Gb2312 => (0x21, 0x77),
            Set::Jis0212 => (0x22, 0x6D),
            _ => (0x21, 0x7E),
        }
    }

    /// The bytes of `c` in the set (one or two; for G2 sets the byte after `ESC N`), `None` when it lacks it.
    fn encode(self, c: u32) -> Option<([u8; 2], usize)> {
        match self {
            Set::Ascii => (c < 0x80).then_some(([c as u8, 0], 1)),
            Set::Roman => match c {
                0xA5 => Some(([0x5C, 0], 1)),
                0x203E => Some(([0x7E, 0], 1)),
                0x21..=0x7E if c != 0x5C && c != 0x7E => Some(([c as u8, 0], 1)),
                _ => None,
            },
            Set::Kana => (0xFF61..=0xFF9F)
                .contains(&c)
                .then(|| ([(c - 0xFF61 + 0x21) as u8, 0], 1)),
            Set::Latin1 => (0x80..=0xFF)
                .contains(&c)
                .then(|| ([(c - 0x80) as u8, 0], 1)),
            Set::Greek => match Codepage::Iso8859_7.encode(c) {
                Some((b, 1)) if b[0] >= 0xA0 => Some(([b[0] - 0x80, 0], 1)),
                _ => None,
            },
            Set::Jis0212 if c == 0x7E => Some(([0x22, 0x37], 2)),
            _ => {
                let (lead, trail) = multibyte::set_encode(self.plane()?, c)?;
                (lead >= 0xA1).then_some(([lead & 0x7F, trail & 0x7F], 2))
            }
        }
    }

    /// The character of a pair in a two-byte set.
    fn decode2(self, b0: u8, b1: u8) -> Option<u32> {
        if !(0x21..=0x7E).contains(&b0) || !(0x21..=0x7E).contains(&b1) {
            return None;
        }
        let plane = self.plane()?;
        match multibyte::set_decode(plane, b0 | 0x80, b1 | 0x80) {
            0 => None,
            cp => Some(cp),
        }
    }

    fn is_g2(self) -> bool {
        matches!(self, Set::Latin1 | Set::Greek)
    }
}

const INVALID1: Decoded = Decoded::Invalid {
    len: 1,
    silent: false,
};

/// What the encoder writes for a character: first a designation or a shift (which holds once written), then the
/// character's bytes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Step {
    /// The designation or shift before the character (empty: none).
    pub(crate) shift: &'static [u8],
    /// The state once the shift is written.
    pub(crate) shifted: State,
    /// The bytes of the character (`len` of them).
    pub(crate) bytes: [u8; 4],
    pub(crate) len: usize,
    /// The state once the character is written.
    pub(crate) after: State,
}

impl Iso2022 {
    /// Decodes the next character of `input` (not empty) in state `st`.
    pub(crate) fn decode(self, input: &[u8], st: State) -> Decoded {
        match self {
            Iso2022::Kr => decode_kr(input, st),
            _ => decode_jp(self == Iso2022::Jp2, input, st),
        }
    }

    /// What the encoder writes for `c` in state `st`; `None` when the charset cannot represent it.
    pub(crate) fn encode(self, c: u32, st: State) -> Option<Step> {
        match self {
            Iso2022::Kr => encode_kr(c, st),
            _ => encode_jp(self, c, st),
        }
    }

    /// The output space the encoder wants to have left before it converts a character (even one it then skips or
    /// transliterates) in the state `st`: two bytes while JIS X 0208 is designated, else one (the unit, which is all a
    /// tag character that vanishes wants).
    pub(crate) fn room(self, st: State) -> usize {
        if self != Iso2022::Kr && st.g0 == Set::Jis0208 {
            2
        } else {
            1
        }
    }

    /// Whether the tag characters U+E0000..U+E007F vanish (ISO-2022-JP rejects them).
    pub(crate) fn tags_vanish(self) -> bool {
        self != Iso2022::Jp
    }

    /// The state after the tag character `c` (ISO-2022-JP-2 reads language tags; the others ignore it). U+E0001
    /// starts a tag (again), U+E007F CANCEL TAG ends it and the language; what follows a tag's first two characters
    /// does not count, and the tag characters outside a tag do nothing.
    pub(crate) fn tag(self, c: u32, st: State) -> State {
        if self != Iso2022::Jp2 {
            return st;
        }
        match (c, st.tag) {
            (0xE_0001, _) => State {
                tag: Tag::Letters(0, [0; 2]),
                ..st
            },
            (0xE_007F, _) => State {
                lang: Lang::None,
                tag: Tag::None,
                ..st
            },
            (_, Tag::Letters(n, mut letters)) => {
                if usize::from(n) < letters.len() {
                    letters[usize::from(n)] = (c - 0xE_0000) as u8;
                }
                State {
                    tag: Tag::Letters(n.saturating_add(1), letters),
                    ..st
                }
            }
            _ => st,
        }
    }

    /// The state once a character other than a tag character follows (a tag being read selects its language).
    pub(crate) fn untag(self, st: State) -> State {
        match st.tag {
            Tag::Letters(..) => commit(st),
            Tag::None => st,
        }
    }

    /// What the flush writes to return to the initial state.
    pub(crate) fn reset(self, st: State) -> &'static [u8] {
        match self {
            Iso2022::Kr if st.so => &[SI],
            Iso2022::Kr => &[],
            _ if st.g0 != Set::Ascii => Set::Ascii.escape(),
            _ => &[],
        }
    }
}

/// The language of the tag being read.
fn commit(st: State) -> State {
    // the first two tag characters, letters in either case (`jax` and `JA` are `ja`, `j-a` is none)
    let lang = match st.tag {
        Tag::Letters(2.., letters) => match &letters.map(|b| b.to_ascii_lowercase()) {
            b"ja" => Lang::Ja,
            b"ko" => Lang::Ko,
            b"zh" => Lang::Zh,
            _ => Lang::None,
        },
        _ => Lang::None,
    };
    State {
        lang,
        tag: Tag::None,
        ..st
    }
}

fn decode_jp(jp2: bool, input: &[u8], st: State) -> Decoded {
    let b0 = input[0];
    if b0 == ESC {
        let rest = &input[1..];
        let escapes = JP_ESCAPES
            .iter()
            .chain(if jp2 { &JP2_ESCAPES[..] } else { &[] });
        for &(seq, set) in escapes.clone() {
            if rest.starts_with(seq) {
                let mut next = st;
                if set.is_g2() {
                    next.g2 = Some(set);
                } else {
                    next.g0 = set;
                }
                return Decoded::Shift(1 + seq.len(), next);
            }
        }
        if jp2 && rest.first() == Some(&b'N') {
            // a character of G2
            let Some(&b) = rest.get(1) else {
                return Decoded::Incomplete;
            };
            return match st.g2 {
                None => INVALID1,
                Some(Set::Latin1) => Decoded::Char(u32::from(b | 0x80), 3),
                Some(_) if !(0x20..0x80).contains(&b) => INVALID1,
                Some(_) => match Codepage::Iso8859_7.decode(b | 0x80) {
                    Some(cp) => Decoded::Char(cp, 3),
                    None => Decoded::Invalid {
                        len: 3,
                        silent: false,
                    },
                },
            };
        }
        if rest.len() < 2
            || escapes
                .clone()
                .any(|&(seq, _)| seq.len() > rest.len() && seq.starts_with(rest))
        {
            return Decoded::Incomplete;
        }
        return Decoded::Char(u32::from(ESC), 1);
    }
    if b0 >= 0x80 {
        return INVALID1;
    }
    match st.g0 {
        Set::Ascii => Decoded::Char(u32::from(b0), 1),
        Set::Roman => Decoded::Char(
            match b0 {
                0x5C => 0xA5,
                0x7E => 0x203E,
                _ => u32::from(b0),
            },
            1,
        ),
        Set::Kana => match b0 {
            0x21..=0x5F => Decoded::Char(0xFF61 + u32::from(b0 - 0x21), 1),
            0x00..=0x20 | 0x7F => Decoded::Char(u32::from(b0), 1),
            _ => INVALID1,
        },
        set => {
            if b0 < 0x21 || b0 == 0x7F {
                return Decoded::Char(u32::from(b0), 1);
            }
            let (lo, hi) = set.leads();
            if !(lo..=hi).contains(&b0) {
                return INVALID1;
            }
            let Some(&b1) = input.get(1) else {
                return Decoded::Incomplete;
            };
            match set.decode2(b0, b1) {
                Some(cp) => Decoded::Char(cp, 2),
                None => INVALID1,
            }
        }
    }
}

fn decode_kr(input: &[u8], st: State) -> Decoded {
    let b0 = input[0];
    match b0 {
        ESC => {
            let rest = &input[1..];
            if rest.starts_with(KR_HEADER) {
                Decoded::Shift(4, st)
            } else if rest.len() < KR_HEADER.len() && KR_HEADER.starts_with(rest) {
                Decoded::Incomplete
            } else if st.so {
                INVALID1
            } else {
                Decoded::Char(u32::from(ESC), 1)
            }
        }
        SO => Decoded::Shift(1, State { so: true, ..st }),
        SI => Decoded::Shift(1, State { so: false, ..st }),
        0x80..=0xFF => INVALID1,
        _ if !st.so => Decoded::Char(u32::from(b0), 1),
        _ => {
            let Some(&b1) = input.get(1) else {
                return Decoded::Incomplete;
            };
            match Set::Ksc.decode2(b0, b1) {
                Some(cp) => Decoded::Char(cp, 2),
                None => INVALID1,
            }
        }
    }
}

fn step(shift: &'static [u8], shifted: State, bytes: &[u8], after: State) -> Step {
    let mut b = [0; 4];
    b[..bytes.len()].copy_from_slice(bytes);
    Step {
        shift,
        shifted,
        bytes: b,
        len: bytes.len(),
        after,
    }
}

fn encode_jp(cs: Iso2022, c: u32, st: State) -> Option<Step> {
    // a line feed makes ISO-2022-JP-2 forget its G2 designation
    let after = |s: State| {
        if c == 0x0A {
            State { g2: None, ..s }
        } else {
            s
        }
    };
    if st.lang.keeps(st.g0)
        && let Some((b, n)) = st.g0.encode(c)
    {
        return Some(step(&[], st, &b[..n], after(st)));
    }
    if let Some(g2) = st.g2
        && st.lang.keeps(g2)
        && let Some((b, _)) = g2.encode(c)
    {
        return Some(step(&[], st, &[ESC, b'N', b[0]], after(st)));
    }
    let order: &[Set] = match (cs, st.lang) {
        (Iso2022::Jp, _) => &JP_ORDER,
        (_, Lang::Ko) => &KO_ORDER,
        (_, Lang::Zh) => &ZH_ORDER,
        _ => &JP2_ORDER,
    };
    order.iter().find_map(|&set| {
        let (b, n) = set.encode(c)?;
        // the set designated (a language tag makes the encoder take it only when it comes first)
        if set == st.g0 {
            return Some(step(&[], st, &b[..n], after(st)));
        }
        if set.is_g2() && st.g2 == Some(set) {
            return Some(step(&[], st, &[ESC, b'N', b[0]], after(st)));
        }
        Some(if set.is_g2() {
            let shifted = State {
                g2: Some(set),
                ..st
            };
            step(set.escape(), shifted, &[ESC, b'N', b[0]], after(shifted))
        } else {
            let shifted = State { g0: set, ..st };
            step(set.escape(), shifted, &b[..n], after(shifted))
        })
    })
}

fn encode_kr(c: u32, st: State) -> Option<Step> {
    if c < 0x80 {
        let shifted = State { so: false, ..st };
        return Some(step(
            if st.so { &[SI] } else { &[] },
            shifted,
            &[c as u8],
            shifted,
        ));
    }
    let (b, n) = Set::Ksc.encode(c)?;
    let shifted = State { so: true, ..st };
    Some(step(
        if st.so { &[] } else { &[SO] },
        shifted,
        &b[..n],
        shifted,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(cs: Iso2022, input: &[u8]) -> Vec<u32> {
        let mut st = State::default();
        let mut out = Vec::new();
        let mut p = 0;
        while p < input.len() {
            match cs.decode(&input[p..], st) {
                Decoded::Char(c, n) => {
                    out.push(c);
                    p += n;
                }
                Decoded::Shift(n, next) => {
                    st = next;
                    p += n;
                }
                other => panic!("{other:?} at {p}"),
            }
        }
        out
    }

    #[test]
    fn decoding() {
        assert_eq!(decode_all(Iso2022::Jp, b"\x1b$B$\"\x1b(Ba"), [0x3042, 0x61]);
        assert_eq!(decode_all(Iso2022::Jp, b"\x1b(J\\~"), [0xA5, 0x203E]);
        assert_eq!(
            decode_all(Iso2022::Jp, b"\x1b$Aa"),
            [0x1B, 0x24, 0x41, 0x61]
        );
        assert_eq!(decode_all(Iso2022::Jp2, b"\x1b$A0!"), [0x554A]);
        assert_eq!(decode_all(Iso2022::Jp2, b"\x1b.A\x1bNA"), [0xC1]);
        assert_eq!(
            decode_all(Iso2022::Kr, b"\x1b$)C\x0e0!\x0fa"),
            [0xAC00, 0x61]
        );
        assert_eq!(
            Iso2022::Jp.decode(b"\x1ba", State::default()),
            Decoded::Incomplete
        );
    }

    #[test]
    fn encoding() {
        let st = State::default();
        let s = Iso2022::Jp.encode(0x3042, st).unwrap();
        assert_eq!((s.shift, &s.bytes[..s.len]), (&b"\x1b$B"[..], &b"$\""[..]));
        assert_eq!(Iso2022::Jp.reset(s.after), b"\x1b(B");
        assert!(Iso2022::Jp.encode(0x4E02, st).is_none());
        let s = Iso2022::Jp2.encode(0x4E02, st).unwrap();
        assert_eq!(s.shift, b"\x1b$(D");
        let s = Iso2022::Jp2.encode(0xA0, st).unwrap();
        assert_eq!(
            (s.shift, &s.bytes[..s.len]),
            (&b"\x1b.A"[..], &b"\x1bN "[..])
        );
        let s = Iso2022::Kr.encode(0xAC00, st).unwrap();
        assert_eq!((s.shift, &s.bytes[..s.len]), (&[SO][..], &b"0!"[..]));
    }

    #[test]
    fn language_tags() {
        // the set U+4E2D goes into after a tag
        let shift = |tag: &str| {
            let mut st = State::default();
            for c in std::iter::once(0xE_0001).chain(tag.bytes().map(|b| 0xE_0000 + u32::from(b))) {
                st = Iso2022::Jp2.tag(c, st);
            }
            let st = Iso2022::Jp2.untag(st);
            Iso2022::Jp2.encode(0x4E2D, st).unwrap().shift
        };
        // the first two tag characters, letters in either case
        for tag in ["ko", "KO", "Ko", "kO", "ko-KR", "kox"] {
            assert_eq!(shift(tag), b"\x1b$(C", "{tag}");
        }
        assert_eq!(shift("ZH"), b"\x1b$A");
        for tag in ["", "k", "k-o", "-ko", " ko", "en"] {
            assert_eq!(shift(tag), b"\x1b$B", "{tag}");
        }
    }
}
