//! What //TRANSLIT writes for a character the target charset cannot represent. The table (`translit_table.rs`) is
//! measured in the oracle by `translit.gen.php`: glibc 2.41 under `LC_CTYPE=C.UTF-8`, one character at a time (the
//! locale has no multi-character rules, also checked there), for every target with a smaller repertoire than
//! Unicode. A character may have several alternatives (U+339F SQUARE MM SQUARED is "mm²", or "mm^2" where the target
//! has no "²"): glibc tries them in order and writes the first one the target can represent, or its default `?` (see
//! `engine.rs` for what happens when the output space runs out meanwhile). Some alternatives fail in every target;
//! the table keeps what shows of them (the code point 0xFFFF_FFFF, which no charset has, for their first character
//! no target has; measured with `translit_probe.c`): they make glibc want room for their first characters, and change
//! designations in the ISO-2022 charsets.

use super::iso2022::Iso2022;
use super::translit_table::{
    DATA, ISO_2022_JP_2_UNMODELLED, ISO_2022_JP_UNMODELLED, ISO_2022_KR_UNMODELLED, KEYS, SPANS,
};

/// The alternatives of `c`, in the order glibc tries them.
pub(crate) fn alternatives(c: u32) -> impl Iterator<Item = &'static [u32]> {
    let first = KEYS.partition_point(|&k| k < c);
    KEYS[first..]
        .iter()
        .take_while(move |&&k| k == c)
        .zip(&SPANS[first..])
        .map(|(_, &(offset, len))| {
            let offset = usize::from(offset);
            &DATA[offset..offset + usize::from(len)]
        })
}

/// Whether the //TRANSLIT replacement of `c` in an ISO-2022 charset is beyond the table: glibc tries an alternative
/// that fails partway in that charset and keeps the designations the attempt reached, and the table lacks it (no
/// target shows it; measured by translit.gen.php from a few states). twig-rs does not convert such a character (four
/// into ISO-2022-JP, one into ISO-2022-JP-2).
pub(crate) fn unmodelled(target: Iso2022, c: u32) -> bool {
    let list: &[u32] = match target {
        Iso2022::Jp => &ISO_2022_JP_UNMODELLED,
        Iso2022::Jp2 => &ISO_2022_JP_2_UNMODELLED,
        Iso2022::Kr => &ISO_2022_KR_UNMODELLED,
    };
    list.binary_search(&c).is_ok()
}

#[cfg(test)]
mod tests {
    use super::super::codec::{Charset, Encoded};
    use super::super::codepage::Codepage;
    use super::*;

    /// What a conversion with room to spare writes for `c`.
    fn text(target: Charset, c: u32) -> String {
        alternatives(c)
            .find(|alternative| {
                alternative
                    .iter()
                    .all(|&u| matches!(target.encode(u), Encoded::Bytes(..)))
            })
            .unwrap_or(&[0x3F])
            .iter()
            .map(|&u| char::from_u32(u).unwrap())
            .collect()
    }

    #[test]
    fn samples() {
        assert_eq!(text(Charset::Ascii, 0xE9), "e");
        assert_eq!(text(Charset::Ascii, 0x20AC), "EUR");
        assert_eq!(text(Charset::Ascii, 0x200B), "");
        assert_eq!(text(Charset::Ascii, 0x4E00), "?");
        assert_eq!(text(Charset::Ascii, 0xD800), "?");
        assert_eq!(text(Charset::Latin1, 0x339F), "mm\u{b2}");
        assert_eq!(text(Charset::Ascii, 0x339F), "mm^2");
        assert_eq!(text(Charset::Latin1, 0x20AC), "EUR");
        assert_eq!(text(Charset::Ucs2, 0x1F600), ":-D");
        assert_eq!(text(Charset::Ucs2Be, 0x2F800), "\u{4e3d}");
        assert_eq!(text(Charset::Utf16, 0x11_0000), "?");
        assert_eq!(text(Charset::Codepage(Codepage::Koi8R), 0x20BD), "руб");
        assert_eq!(text(Charset::Latin1, 0x20BD), "RUB");
        assert_eq!(text(Charset::Codepage(Codepage::Iso8859_7), 0x1F00), "α");
        assert_eq!(text(Charset::Ascii, 0x1F00), "?");
    }

    /// The table's stand-in for a character of an alternative that no charset can represent.
    const PHANTOM: u32 = 0xFFFF_FFFF;

    #[test]
    fn no_charset_has_the_phantom() {
        use super::super::iso2022;
        let charsets = [
            Charset::Utf8,
            Charset::Utf16,
            Charset::Utf16Be,
            Charset::Utf16Le,
            Charset::Utf32,
            Charset::Utf32Be,
            Charset::Utf32Le,
            Charset::Ucs2,
            Charset::Ucs2Be,
            Charset::Ucs4,
            Charset::Ucs4Le,
            Charset::Ascii,
            Charset::Latin1,
        ]
        .into_iter()
        .chain(super::super::codepage_table::ALL.map(Charset::Codepage))
        .chain(super::super::multibyte_table::ALL.map(Charset::Multibyte));
        for cs in charsets {
            assert!(!matches!(cs.encode(PHANTOM), Encoded::Bytes(..)), "{cs:?}");
        }
        for cs in [Iso2022::Jp, Iso2022::Jp2, Iso2022::Kr] {
            assert!(cs.encode(PHANTOM, iso2022::State::default()).is_none());
        }
        assert!(KEYS.iter().zip(&SPANS).any(|(_, &(offset, len))| {
            DATA[usize::from(offset)..usize::from(offset) + usize::from(len)].contains(&PHANTOM)
        }));
    }

    #[test]
    fn table_is_sorted() {
        assert!(KEYS.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(KEYS.len(), SPANS.len());
    }
}
