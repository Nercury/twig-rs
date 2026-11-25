//! UTF-8 decoding and encoding with the observable behaviour of PHP mbstring's UTF-8 encoding.
//!
//! mbstring decodes a string into "units": one per well-formed code point and one
//! [`BAD_INPUT`] marker per ill-formed sequence. On output every marker becomes the substitute
//! character `?` (mbstring's default `mbstring.substitute_character` and illegal-character
//! mode). The markers count as characters in `mb_strlen()`, `mb_substr()` and friends.
//!
//! An ill-formed sequence is a *maximal subpart* (Unicode 17.0 §3.9, "U+FFFD Substitution of
//! Maximal Subparts"): the longest prefix of a well-formed sequence (Table 3-7) that the input
//! starts with, or one byte if no well-formed sequence starts with that byte. Examples:
//! `E2 82 41` is a marker for `E2 82` followed by `A`; `E0 80 80` is three markers (no
//! well-formed sequence starts with `E0 80`); `F0 9F 98` at the end of the input is one marker.
//!
//! This module is written from the Unicode standard and checked against PHP 8.5.13 with the
//! oracle vectors in `tests/vectors/utf8.json` (every string of up to four bytes over an
//! alphabet that covers each decision boundary). It contains no code from libmbfl, mbstring's
//! LGPL-licensed conversion library.

/// Marker for one ill-formed sequence in decoded input (libmbfl's `MBFL_BAD_INPUT`,
/// `(uint32_t) -1`). Every value above `0xFFFFFF` is treated as such a marker by the case
/// conversion, and [`from_wchar`] writes `?` for it.
pub const BAD_INPUT: u32 = 0xFFFF_FFFF;

/// The substitute character mbstring writes for [`BAD_INPUT`] (`mbstring.substitute_character`
/// default, illegal-character mode "char").
pub const SUBSTITUTE: u8 = b'?';

/// Returns whether `b` is a continuation byte (`80..=BF`).
#[inline]
const fn is_continuation(b: u8) -> bool {
    b & 0xC0 == 0x80
}

/// Decodes the unit at the start of `bytes`: a code point, or [`BAD_INPUT`] for an ill-formed
/// sequence (maximal subpart, see the [module documentation](self)). Returns the unit and the
/// number of bytes it consumed (at least 1).
///
/// # Panics
///
/// Panics if `bytes` is empty.
#[inline]
pub fn decode_unit(bytes: &[u8]) -> (u32, usize) {
    let lead = bytes[0];
    // Unicode Table 3-7: sequence length and the range of the second byte for each lead byte.
    let (len, second_lo, second_hi) = match lead {
        0x00..=0x7F => return (u32::from(lead), 1),
        0xC2..=0xDF => (2, 0x80, 0xBF),
        0xE0 => (3, 0xA0, 0xBF),
        0xE1..=0xEC | 0xEE..=0xEF => (3, 0x80, 0xBF),
        0xED => (3, 0x80, 0x9F),
        0xF0 => (4, 0x90, 0xBF),
        0xF1..=0xF3 => (4, 0x80, 0xBF),
        0xF4 => (4, 0x80, 0x8F),
        // 80..=C1 and F5..=FF never start a well-formed sequence.
        _ => return (BAD_INPUT, 1),
    };
    match bytes.get(1) {
        Some(&b) if (second_lo..=second_hi).contains(&b) => {}
        _ => return (BAD_INPUT, 1),
    }
    for i in 2..len {
        match bytes.get(i) {
            Some(&b) if is_continuation(b) => {}
            _ => return (BAD_INPUT, i),
        }
    }
    let payload_mask = match len {
        2 => 0x1F,
        3 => 0x0F,
        _ => 0x07,
    };
    let cp = bytes[1..len]
        .iter()
        .fold(u32::from(lead & payload_mask), |cp, &b| {
            (cp << 6) | u32::from(b & 0x3F)
        });
    (cp, len)
}

/// Decodes units from the front of `input` into `out` until `out` is full or the input is
/// exhausted, and advances `input` past the consumed bytes. Returns the number of units
/// written.
///
/// This is the shape of mbstring's `to_wchar` encoding hook, which the ported mbstring
/// functions call with the same buffer sizes as PHP (64 code points for case conversion,
/// 128 for `mb_substr()`), so chunk-dependent behaviour (final sigma) comes out identical.
pub fn to_wchar(input: &mut &[u8], out: &mut [u32]) -> usize {
    let mut rest = *input;
    let mut n = 0;
    while n < out.len() && !rest.is_empty() {
        let (unit, consumed) = decode_unit(rest);
        out[n] = unit;
        n += 1;
        rest = &rest[consumed..];
    }
    *input = rest;
    n
}

/// Appends the UTF-8 encoding of `units` to `out` (mbstring's `from_wchar` hook for UTF-8).
///
/// [`BAD_INPUT`], and any other value that is not a Unicode scalar value, is written as the
/// substitute character `?`. (Case conversion and decoding only ever produce scalar values and
/// [`BAD_INPUT`].)
pub fn from_wchar(units: &[u32], out: &mut Vec<u8>) {
    for &unit in units {
        match char::from_u32(unit) {
            Some(c) => {
                let mut buf = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            None => out.push(SUBSTITUTE),
        }
    }
}

/// Iterator over the units of a byte string, see [`units()`].
#[derive(Clone, Debug)]
pub struct Units<'a> {
    rest: &'a [u8],
}

impl Iterator for Units<'_> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<u32> {
        if self.rest.is_empty() {
            return None;
        }
        let (unit, consumed) = decode_unit(self.rest);
        self.rest = &self.rest[consumed..];
        Some(unit)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.rest.len().div_ceil(4), Some(self.rest.len()))
    }
}

impl std::iter::FusedIterator for Units<'_> {}

/// Iterates over the units of `bytes`: code points, and [`BAD_INPUT`] for each ill-formed
/// sequence.
pub fn units(bytes: &[u8]) -> Units<'_> {
    Units { rest: bytes }
}

/// Returns whether `bytes` is well-formed UTF-8 (RFC 3629: no overlong forms, no surrogates,
/// nothing above U+10FFFF). This is `mb_check_encoding($bytes, 'UTF-8')`.
#[inline]
pub fn is_valid(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

/// Byte length of a character as judged from its first byte by mbstring's UTF-8 length table,
/// which `mb_str_split()` uses to cut strings without validating them: `C2..=DF` → 2,
/// `E0..=EF` → 3, `F0..=F4` → 4, every other byte → 1.
///
/// The values were read off PHP 8.5.13 (`mb_str_split(chr($b) . "AAAAAAA")` for every byte,
/// stored in `tests/vectors/substr_split.json`).
#[inline]
pub const fn mblen(lead: u8) -> usize {
    match lead {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(bytes: &[u8]) -> Vec<u32> {
        units(bytes).collect()
    }

    #[test]
    fn decodes_well_formed_sequences() {
        assert_eq!(
            decode_all(b"a\xC3\xA9\xE2\x82\xAC\xF0\x9F\x98\x80"),
            [0x61, 0xE9, 0x20AC, 0x1F600]
        );
        assert_eq!(
            decode_all(b"\xF4\x8F\xBF\xBF\xEF\xBF\xBF"),
            [0x10FFFF, 0xFFFF]
        );
    }

    #[test]
    fn ill_formed_sequences_are_maximal_subparts() {
        const B: u32 = BAD_INPUT;
        assert_eq!(decode_all(b"a\xFFb\xE2\x82"), [0x61, B, 0x62, B]);
        assert_eq!(decode_all(b"\xF0\x9F\x98"), [B]);
        assert_eq!(decode_all(b"\xED\xA0\x80"), [B, B, B]);
        assert_eq!(decode_all(b"\xE0\x80\x80"), [B, B, B]);
        assert_eq!(decode_all(b"\xE2\x82\x41"), [B, 0x41]);
        assert_eq!(decode_all(b"\xF4\x90\x80\x80"), [B, B, B, B]);
        assert_eq!(decode_all(b"\xC0\xAF\xC1"), [B, B, B]);
        assert_eq!(decode_all(b"\xF1\x80\x80\xC3\xA9"), [B, 0xE9]);
    }

    #[test]
    fn units_agree_with_std_lossy_decoding() {
        // std's lossy decoding also substitutes maximal subparts; compare on pseudo-random
        // strings over bytes that hit every branch.
        const ALPHABET: &[u8] = &[
            0x41, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xC1, 0xC2, 0xDF, 0xE0, 0xE1,
            0xED, 0xEF, 0xF0, 0xF1, 0xF4, 0xF5, 0xFF,
        ];
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..20_000 {
            let mut s = Vec::new();
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = (state % 9) as usize;
            for _ in 0..len {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                s.push(ALPHABET[(state % ALPHABET.len() as u64) as usize]);
            }
            let ours: Vec<u32> = units(&s)
                .map(|u| if u == BAD_INPUT { 0xFFFD } else { u })
                .collect();
            let std: Vec<u32> = String::from_utf8_lossy(&s).chars().map(u32::from).collect();
            assert_eq!(ours, std, "{s:x?}");
        }
    }

    #[test]
    fn to_wchar_fills_the_buffer_and_advances() {
        let mut input: &[u8] = b"ab\xFFcd";
        let mut buf = [0; 2];
        assert_eq!(to_wchar(&mut input, &mut buf), 2);
        assert_eq!(buf, [0x61, 0x62]);
        assert_eq!(to_wchar(&mut input, &mut buf), 2);
        assert_eq!(buf, [BAD_INPUT, 0x63]);
        assert_eq!(to_wchar(&mut input, &mut buf), 1);
        assert_eq!(buf[0], 0x64);
        assert!(input.is_empty());
        assert_eq!(to_wchar(&mut input, &mut buf), 0);
    }

    #[test]
    fn from_wchar_substitutes_markers() {
        let mut out = Vec::new();
        from_wchar(
            &[0x41, BAD_INPUT, 0xE9, 0xD800, 0x110000, 0x1F600],
            &mut out,
        );
        assert_eq!(out, b"A?\xC3\xA9??\xF0\x9F\x98\x80");
    }
}
