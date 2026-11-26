//! What //TRANSLIT writes for a character the target charset cannot represent. The table (`translit_table.rs`) is
//! measured in the oracle by `translit.gen.php`: glibc 2.41 under `LC_CTYPE=C.UTF-8`, one character at a time (the
//! locale has no multi-character rules, also checked there), for every target with a smaller repertoire than
//! Unicode. A character may have several alternatives (U+339F SQUARE MM SQUARED is "mm²", or "mm^2" where the target
//! has no "²"): the target gets the first one it can represent. Everything else becomes `?`.

use super::codec::{Charset, Encoded};
use super::translit_table::{DATA, KEYS, SPANS};

/// glibc's replacement when it knows no transliteration the target can represent.
const QUESTION_MARK: &[u16] = &[0x3F];

/// The alternatives of `c`, in the order glibc tries them.
fn alternatives(c: u32) -> impl Iterator<Item = &'static [u16]> {
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

/// The //TRANSLIT replacement of `c` for `target`, which cannot represent it: code points `target` can represent.
pub(crate) fn replacement(target: Charset, c: u32) -> &'static [u16] {
    if c > KEYS[KEYS.len() - 1] {
        // most of the code space
        return QUESTION_MARK;
    }
    alternatives(c)
        .find(|alternative| {
            alternative
                .iter()
                .all(|&u| matches!(target.encode(u32::from(u)), Encoded::Bytes(..)))
        })
        .unwrap_or(QUESTION_MARK)
}

#[cfg(test)]
mod tests {
    use super::super::codepage::Codepage;
    use super::super::codepage_table::ALL;
    use super::*;

    fn text(target: Charset, c: u32) -> String {
        replacement(target, c)
            .iter()
            .map(|&u| char::from_u32(u32::from(u)).unwrap())
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

    #[test]
    fn table_is_sorted() {
        assert!(KEYS.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(KEYS.len(), SPANS.len());
    }

    #[test]
    fn replacements_fit_the_encoder_buffer() {
        // engine.rs encodes a replacement into 24 bytes: the longest alternative in its longest form (three bytes per
        // character in CP1255 and CP1258, two in UCS-2) must fit
        let targets = ALL.iter().map(|&cp| Charset::Codepage(cp)).chain([
            Charset::Ascii,
            Charset::Latin1,
            Charset::Ucs2,
        ]);
        for target in targets {
            for &(offset, len) in &SPANS {
                let offset = usize::from(offset);
                let bytes: Option<usize> = DATA[offset..offset + usize::from(len)]
                    .iter()
                    .map(|&u| match target.encode(u32::from(u)) {
                        Encoded::Bytes(_, n) => Some(n),
                        _ => None,
                    })
                    .sum();
                assert!(bytes.unwrap_or(0) <= 24, "{target:?}");
            }
        }
    }
}
