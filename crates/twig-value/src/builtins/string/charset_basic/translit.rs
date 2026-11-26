//! What //TRANSLIT writes for a character the target charset cannot represent. The tables (`translit_table.rs`) are
//! measured in the oracle by `translit.gen.php`: glibc 2.41 under `LC_CTYPE=C.UTF-8`, one character at a time (the
//! locale has no multi-character rules, also checked there). Everything not in a table becomes `?`.

use super::codec::Charset;
use super::translit_table::{
    ASCII_DATA, ASCII_KEYS, ASCII_SPANS, LATIN1_DATA, LATIN1_KEYS, LATIN1_SPANS, UCS2_DATA,
    UCS2_KEYS, UCS2_SPANS,
};

/// A replacement: code points, all representable in the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Replacement {
    /// Code points below U+0100.
    Narrow(&'static [u8]),
    /// BMP code points.
    Wide(&'static [u16]),
}

impl Replacement {
    /// The code points.
    pub(crate) fn chars(self) -> impl Iterator<Item = u32> {
        let (narrow, wide) = match self {
            Replacement::Narrow(s) => (s, &[][..]),
            Replacement::Wide(s) => (&[][..], s),
        };
        narrow
            .iter()
            .map(|&b| u32::from(b))
            .chain(wide.iter().map(|&u| u32::from(u)))
    }
}

/// glibc's replacement when it knows no transliteration.
const QUESTION_MARK: Replacement = Replacement::Narrow(b"?");

fn find<T>(keys: &[u32], spans: &[(u16, u8)], data: &'static [T], c: u32) -> Option<&'static [T]> {
    let i = keys.binary_search(&c).ok()?;
    let (offset, len) = spans[i];
    Some(&data[usize::from(offset)..usize::from(offset) + usize::from(len)])
}

/// The //TRANSLIT replacement of `c` for `target`, which cannot represent it.
pub(crate) fn replacement(target: Charset, c: u32) -> Replacement {
    match target {
        Charset::Ascii => find(&ASCII_KEYS, &ASCII_SPANS, &ASCII_DATA, c)
            .map_or(QUESTION_MARK, Replacement::Narrow),
        Charset::Latin1 => find(&LATIN1_KEYS, &LATIN1_SPANS, &LATIN1_DATA, c)
            .or_else(|| find(&ASCII_KEYS, &ASCII_SPANS, &ASCII_DATA, c))
            .map_or(QUESTION_MARK, Replacement::Narrow),
        Charset::Ucs2 | Charset::Ucs2Be => {
            find(&UCS2_KEYS, &UCS2_SPANS, &UCS2_DATA, c).map_or(QUESTION_MARK, Replacement::Wide)
        }
        // UTF-8 (surrogates), UTF-16 and UTF-32 (beyond U+10FFFF): glibc has no rule for these
        _ => QUESTION_MARK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(r: Replacement) -> String {
        r.chars().map(|c| char::from_u32(c).unwrap()).collect()
    }

    #[test]
    fn samples() {
        assert_eq!(text(replacement(Charset::Ascii, 0xE9)), "e");
        assert_eq!(text(replacement(Charset::Ascii, 0x20AC)), "EUR");
        assert_eq!(text(replacement(Charset::Ascii, 0x200B)), "");
        assert_eq!(text(replacement(Charset::Ascii, 0x4E00)), "?");
        assert_eq!(text(replacement(Charset::Ascii, 0xD800)), "?");
        assert_eq!(text(replacement(Charset::Latin1, 0x339F)), "mm\u{b2}");
        assert_eq!(text(replacement(Charset::Ascii, 0x339F)), "mm^2");
        assert_eq!(text(replacement(Charset::Latin1, 0x20AC)), "EUR");
        assert_eq!(text(replacement(Charset::Ucs2, 0x1F600)), ":-D");
        assert_eq!(text(replacement(Charset::Ucs2Be, 0x2F800)), "\u{4e3d}");
        assert_eq!(text(replacement(Charset::Utf16, 0x11_0000)), "?");
    }

    #[test]
    fn tables_are_sorted() {
        for keys in [&ASCII_KEYS[..], &LATIN1_KEYS[..], &UCS2_KEYS[..]] {
            assert!(keys.windows(2).all(|w| w[0] < w[1]));
        }
    }

    #[test]
    fn replacements_fit_the_encoder_buffer() {
        // engine.rs encodes a replacement into 24 bytes: at most 6 bytes per character in UTF-8 would not fit, but
        // replacements only exist for ASCII, ISO-8859-1 (one byte each) and UCS-2 (two bytes each)
        let narrow = ASCII_SPANS
            .iter()
            .chain(&LATIN1_SPANS)
            .map(|&(_, n)| usize::from(n));
        assert!(narrow.max().unwrap() <= 24);
        assert!(
            UCS2_SPANS
                .iter()
                .map(|&(_, n)| 2 * usize::from(n))
                .max()
                .unwrap()
                <= 24
        );
    }
}
