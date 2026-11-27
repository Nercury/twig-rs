//! The single-byte charsets converted with tables, every one of glibc's whose bytes below 0x80 are ASCII (95): the
//! ISO-8859 family (2 to 16 and 9E), the Windows codepages 874 and 1250 to 1258, the KOI8 family (KOI8-R, KOI8-U,
//! KOI8-RU, KOI8-T, KOI8), the IBM PC and OS/2 codepages (437, 737, 770 to 775, 850 to 869, 874, 901 to 922, 1004,
//! 1008, 1046, 1124 to 1163, 4909, 5347), the Mac charsets (MACINTOSH, MAC-CENTRALEUROPE, MAC-CYRILLIC, MAC-IS,
//! MAC-SAMI, CP10007) and ARMSCII-8, CWI, DEC-MCS, ECMA-CYRILLIC, GEORGIAN-ACADEMY, GEORGIAN-PS, GOST_19768-74,
//! HP-GREEK8, HP-ROMAN8, HP-ROMAN9, HP-TURKISH8, IEC_P27-1, ISIRI-3342, ISO_10367-BOX, ISO-IR-197, ISO-IR-209, MIK,
//! PT154, RK1048, TIS-620 and WIN-SAMI-2.
//!
//! The tables (`codepage_table.rs`) are what glibc 2.41's iconv does in the oracle, measured black-box by
//! `codepage.gen.php`: every byte decoded and every code point encoded with one PHP `iconv()` call each. They agree
//! with the Unicode Consortium's mapping files (<https://www.unicode.org/Public/MAPPINGS/>) where there is one,
//! except where the documentation of each charset says otherwise. A few bytes decode to a character the encoder
//! writes as another byte, and a few characters are written as a byte that decodes to another one (U+2022 as 0x07 in
//! CP901), as each charset's documentation lists.
//!
//! CP1255 (Hebrew) and CP1258 (Vietnamese) also combine characters, as the oracle shows: their encoders write
//! characters without a byte of their own as a letter followed by its marks ([`Table::decompose`]), and their decoders
//! hold back the characters of a range of code points ([`Table::hold`]: the Hebrew letters, and U+0041..U+01B0 for
//! CP1258) until the next character shows whether the two combine into one ([`Table::compose`]). A combined character
//! is held back again only in a few cases ([`Table::hold_combined`]). A held character is written when the next one
//! does not combine with it or when the caller flushes the descriptor (PHP does after converting); the engine models
//! what that means for where a call stops.

pub(crate) use super::codepage_table::Codepage;

/// A byte the charset does not define, in [`Table::high`].
pub(super) const INVALID: u16 = 0xFFFF;

/// How glibc converts one codepage.
#[derive(Debug)]
pub(crate) struct Table {
    /// The code points of bytes 0x80..=0xFF ([`INVALID`]: glibc rejects the byte). Bytes below 0x80 are ASCII.
    pub(super) high: [u16; 128],
    /// The code points from U+0080 up that encode to one byte, sorted.
    pub(super) encode: &'static [(u16, u8)],
    /// The code points that encode to two or three bytes (a letter and its marks), sorted; unused bytes are 0.
    pub(super) decompose: &'static [(u16, [u8; 3])],
    /// What the decoder combines, sorted: a character it holds back and the next character give the third.
    pub(super) compose: &'static [(u16, u16, u16)],
    /// The ranges of code points the decoder holds back when it reads them, sorted.
    pub(super) hold: &'static [(u16, u16)],
    /// The combined characters the decoder holds back again, sorted.
    pub(super) hold_combined: &'static [u16],
}

impl Codepage {
    /// The code point of byte `b`, `None` when glibc rejects it.
    pub(super) fn decode(self, b: u8) -> Option<u32> {
        if b < 0x80 {
            return Some(u32::from(b));
        }
        match self.table().high[usize::from(b - 0x80)] {
            INVALID => None,
            c => Some(u32::from(c)),
        }
    }

    /// The bytes `c` encodes to (one to three, in the first `len` of the array), `None` when the charset cannot
    /// represent it.
    pub(super) fn encode(self, c: u32) -> Option<([u8; 3], usize)> {
        if c < 0x80 {
            return Some(([c as u8, 0, 0], 1));
        }
        let c = u16::try_from(c).ok()?;
        let table = self.table();
        if let Ok(i) = table.encode.binary_search_by_key(&c, |&(cp, _)| cp) {
            return Some(([table.encode[i].1, 0, 0], 1));
        }
        let i = table
            .decompose
            .binary_search_by_key(&c, |&(cp, _)| cp)
            .ok()?;
        let bytes = table.decompose[i].1;
        Some((bytes, if bytes[2] == 0 { 2 } else { 3 }))
    }

    /// Whether the decoder combines characters (CP1255, CP1258).
    pub(super) fn composes(self) -> bool {
        !self.table().compose.is_empty()
    }

    /// Whether the decoder holds back `c`, a character it has just read.
    pub(super) fn holds(self, c: u32) -> bool {
        self.table()
            .hold
            .iter()
            .any(|&(lo, hi)| (u32::from(lo)..=u32::from(hi)).contains(&c))
    }

    /// The character the held `first` and the next character `second` combine into, and whether the decoder holds
    /// it back again.
    pub(super) fn compose(self, first: u32, second: u32) -> Option<(u32, bool)> {
        let table = self.table();
        let i = table
            .compose
            .binary_search_by(|&(a, b, _)| (u32::from(a), u32::from(b)).cmp(&(first, second)))
            .ok()?;
        let combined = table.compose[i].2;
        Some((
            u32::from(combined),
            table.hold_combined.binary_search(&combined).is_ok(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::codepage_table::ALL;
    use super::*;

    #[test]
    fn tables_are_consistent() {
        for cp in ALL {
            let t = cp.table();
            assert!(
                t.encode.windows(2).all(|w| w[0].0 < w[1].0),
                "{}",
                cp.name()
            );
            assert!(
                t.decompose.windows(2).all(|w| w[0].0 < w[1].0),
                "{}",
                cp.name()
            );
            assert!(
                t.compose
                    .windows(2)
                    .all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)),
                "{}",
                cp.name()
            );
            assert!(t.hold.windows(2).all(|w| w[0].1 < w[1].0), "{}", cp.name());
            assert!(
                t.hold_combined.windows(2).all(|w| w[0] < w[1]),
                "{}",
                cp.name()
            );
            assert_eq!(t.compose.is_empty(), t.hold.is_empty(), "{}", cp.name());
            // every character the decoder gives encodes to one byte (itself but in a few codepages, see their
            // documentation)
            for b in 0..=255u8 {
                if let Some(c) = cp.decode(b) {
                    assert!(matches!(cp.encode(c), Some((_, 1))), "{} {b:#x}", cp.name());
                }
            }
            assert_eq!(cp.encode(0x1_0000), None);
            assert_eq!(cp.encode(0xD800), None);
        }
    }

    #[test]
    fn samples() {
        assert_eq!(Codepage::Iso8859_2.decode(0xA1), Some(0x104));
        assert_eq!(Codepage::Iso8859_3.decode(0xA5), None);
        assert_eq!(Codepage::Cp1252.encode(0x20AC), Some(([0x80, 0, 0], 1)));
        assert_eq!(Codepage::Koi8R.encode(0xE9), None);
        assert_eq!(Codepage::Macintosh.decode(0xF0), Some(0xE01E));
        // CP1258: U+00C3 has no byte of its own; U+0340 is taken as U+0300
        assert_eq!(Codepage::Cp1258.encode(0xC3), Some(([0x41, 0xDE, 0], 2)));
        assert_eq!(Codepage::Cp1258.encode(0x340), Some(([0xCC, 0, 0], 1)));
        assert_eq!(
            Codepage::Cp1255.encode(0xFB2C),
            Some(([0xF9, 0xCC, 0xD1], 3))
        );
        assert!(Codepage::Cp1258.holds(0x46) && !Codepage::Cp1258.holds(0x31));
        assert!(Codepage::Cp1258.holds(0x1B0) && !Codepage::Cp1258.holds(0x300));
        assert!(Codepage::Cp1255.holds(0x5E2) && !Codepage::Cp1255.holds(0x5BC));
        assert_eq!(Codepage::Cp1258.compose(0x41, 0x300), Some((0xC0, false)));
        assert_eq!(Codepage::Cp1258.compose(0x46, 0x300), None);
        assert_eq!(Codepage::Cp1255.compose(0x5E9, 0x5BC), Some((0xFB49, true)));
        assert_eq!(
            Codepage::Cp1255.compose(0xFB49, 0x5C1),
            Some((0xFB2C, false))
        );
        assert!(!Codepage::Cp1252.composes() && Codepage::Cp1255.composes());
    }
}
