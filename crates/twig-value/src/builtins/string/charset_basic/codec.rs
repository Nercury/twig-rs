//! The charsets: how glibc's iconv (as observed in the oracle) decodes each one into code points and encodes code
//! points into it. Code points are glibc's internal UCS-4 values, `0..=0x7FFF_FFFF`: beyond Unicode where UTF-8 and
//! UCS-4 allow it.
//!
//! The encodings themselves follow RFC 3629 (UTF-8, here with the original five- and six-byte forms of ISO/IEC
//! 10646), RFC 2781 (UTF-16) and ISO/IEC 10646 (UCS-2, UCS-4, UTF-32); which values each decoder and encoder rejects,
//! the byte order of the unmarked forms and the byte-order-mark handling are what the oracle shows (see the vectors
//! in tests/vectors/string/charset_basic).

/// A charset this module converts. Every name glibc maps to the same behaviour shares one variant (`UCS-2LE` is
/// `UCS-2`, `UCS-4BE` is `UCS-4`; see `name_table.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Charset {
    /// UTF-8 with five- and six-byte forms (up to U+7FFFFFFF); surrogates and overlong forms are invalid.
    Utf8,
    /// UTF-16 with a byte order mark: decoding honours a leading mark (none: little endian), encoding writes `FF FE`
    /// and little-endian code units.
    Utf16,
    /// UTF-16 big endian, no byte order mark handling.
    Utf16Be,
    /// UTF-16 little endian, no byte order mark handling.
    Utf16Le,
    /// UTF-32 with a byte order mark, like [`Charset::Utf16`].
    Utf32,
    /// UTF-32 big endian.
    Utf32Be,
    /// UTF-32 little endian.
    Utf32Le,
    /// UCS-2 in little-endian order (the oracle's host order), also `UCS-2LE`.
    Ucs2,
    /// UCS-2 big endian (`UNICODEBIG`), which skips surrogates silently under //IGNORE.
    Ucs2Be,
    /// UCS-4 big endian, values up to U+7FFFFFFF, surrogates included.
    Ucs4,
    /// UCS-4 little endian.
    Ucs4Le,
    /// ANSI_X3.4-1968 (US-ASCII).
    Ascii,
    /// ISO-8859-1.
    Latin1,
}

/// One step of decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decoded {
    /// A character of `len` bytes.
    Char(u32, usize),
    /// An invalid sequence; //IGNORE skips `len` bytes. `silent`: the decoder skips it without making the call
    /// report `EILSEQ` afterwards.
    Invalid { len: usize, silent: bool },
    /// The input ends inside a character (`EINVAL`).
    Incomplete,
}

/// The decoder state that lives as long as the conversion descriptor: whether the byte order mark of UTF-16 and
/// UTF-32 has been looked for, and which byte order it selected.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DecodeState {
    /// The byte order mark has been looked for (UTF-16, UTF-32).
    started: bool,
    /// The marked form turned out to be big endian.
    big_endian: bool,
}

/// What a character looks like in the target charset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoded {
    /// The bytes (`len` of `bytes`).
    Bytes([u8; 6], usize),
    /// Not representable: //TRANSLIT may replace it, //IGNORE may skip it.
    Unmappable,
    /// Rejected outright (a surrogate in UTF-16, UTF-32 or UCS-2): //TRANSLIT does not apply, //IGNORE skips it
    /// (`silent`: without reporting `EILSEQ`).
    Illegal { silent: bool },
}

impl Charset {
    /// Reads the byte order mark at the start of a UTF-16 or UTF-32 stream: `Some(len)` consumed (0 when there is
    /// none or the charset has no marked form), `None` when the input is too short to tell (`EINVAL`).
    pub(crate) fn read_bom(self, input: &[u8], state: &mut DecodeState) -> Option<usize> {
        let (width, big, little): (usize, &[u8], &[u8]) = match self {
            Charset::Utf16 => (2, &[0xFE, 0xFF], &[0xFF, 0xFE]),
            Charset::Utf32 => (4, &[0x00, 0x00, 0xFE, 0xFF], &[0xFF, 0xFE, 0x00, 0x00]),
            _ => return Some(0),
        };
        if state.started {
            return Some(0);
        }
        if input.len() < width {
            return None;
        }
        state.started = true;
        if &input[..width] == big {
            state.big_endian = true;
            Some(width)
        } else if &input[..width] == little {
            Some(width)
        } else {
            Some(0)
        }
    }

    /// Decodes the next character of `input` (not empty), after [`Charset::read_bom`].
    pub(crate) fn decode(self, input: &[u8], state: &DecodeState) -> Decoded {
        match self {
            Charset::Utf8 => decode_utf8(input),
            Charset::Utf16 => decode_utf16(input, state.big_endian),
            Charset::Utf16Be => decode_utf16(input, true),
            Charset::Utf16Le => decode_utf16(input, false),
            Charset::Utf32 => decode_utf32(input, state.big_endian),
            Charset::Utf32Be => decode_utf32(input, true),
            Charset::Utf32Le => decode_utf32(input, false),
            Charset::Ucs2 => decode_ucs2(input, false),
            Charset::Ucs2Be => decode_ucs2(input, true),
            Charset::Ucs4 => decode_ucs4(input, true),
            Charset::Ucs4Le => decode_ucs4(input, false),
            Charset::Ascii => match input[0] {
                b @ 0..=0x7F => Decoded::Char(u32::from(b), 1),
                _ => Decoded::Invalid {
                    len: 1,
                    silent: false,
                },
            },
            Charset::Latin1 => Decoded::Char(u32::from(input[0]), 1),
        }
    }

    /// Encodes `c` (at most U+7FFFFFFF).
    pub(crate) fn encode(self, c: u32) -> Encoded {
        let surrogate = (0xD800..=0xDFFF).contains(&c);
        match self {
            Charset::Utf8 => {
                if surrogate {
                    return Encoded::Unmappable;
                }
                encode_utf8(c)
            }
            Charset::Utf16 | Charset::Utf16Le | Charset::Utf16Be => {
                let big = self == Charset::Utf16Be;
                if surrogate {
                    Encoded::Illegal { silent: false }
                } else if c < 0x1_0000 {
                    unit16(c as u16, big)
                } else if c <= 0x10_FFFF {
                    let v = c - 0x1_0000;
                    let hi = 0xD800 | (v >> 10) as u16;
                    let lo = 0xDC00 | (v & 0x3FF) as u16;
                    let (h, l) = if big {
                        (hi.to_be_bytes(), lo.to_be_bytes())
                    } else {
                        (hi.to_le_bytes(), lo.to_le_bytes())
                    };
                    Encoded::Bytes([h[0], h[1], l[0], l[1], 0, 0], 4)
                } else {
                    Encoded::Unmappable
                }
            }
            Charset::Utf32 | Charset::Utf32Le | Charset::Utf32Be => {
                if surrogate {
                    Encoded::Illegal { silent: false }
                } else if c <= 0x10_FFFF {
                    unit32(c, self == Charset::Utf32Be)
                } else {
                    Encoded::Unmappable
                }
            }
            Charset::Ucs2 | Charset::Ucs2Be => {
                let big = self == Charset::Ucs2Be;
                if surrogate {
                    Encoded::Illegal { silent: big }
                } else if c < 0x1_0000 {
                    unit16(c as u16, big)
                } else {
                    Encoded::Unmappable
                }
            }
            Charset::Ucs4 => unit32(c, true),
            Charset::Ucs4Le => unit32(c, false),
            Charset::Ascii => byte_if(c, 0x80),
            Charset::Latin1 => byte_if(c, 0x100),
        }
    }

    /// The output space glibc wants to have left before it converts the next character (even one it then skips).
    pub(crate) fn unit(self) -> usize {
        match self {
            Charset::Utf8 | Charset::Ascii | Charset::Latin1 => 1,
            Charset::Utf16
            | Charset::Utf16Be
            | Charset::Utf16Le
            | Charset::Ucs2
            | Charset::Ucs2Be => 2,
            Charset::Utf32
            | Charset::Utf32Be
            | Charset::Utf32Le
            | Charset::Ucs4
            | Charset::Ucs4Le => 4,
        }
    }

    /// The byte order mark the encoder writes before its first character (UTF-16 and UTF-32: little endian).
    pub(crate) fn bom(self) -> &'static [u8] {
        match self {
            Charset::Utf16 => &[0xFF, 0xFE],
            Charset::Utf32 => &[0xFF, 0xFE, 0x00, 0x00],
            _ => &[],
        }
    }
}

fn byte_if(c: u32, limit: u32) -> Encoded {
    if c < limit {
        Encoded::Bytes([c as u8, 0, 0, 0, 0, 0], 1)
    } else {
        Encoded::Unmappable
    }
}

fn unit16(u: u16, big: bool) -> Encoded {
    let b = if big {
        u.to_be_bytes()
    } else {
        u.to_le_bytes()
    };
    Encoded::Bytes([b[0], b[1], 0, 0, 0, 0], 2)
}

fn unit32(c: u32, big: bool) -> Encoded {
    let b = if big {
        c.to_be_bytes()
    } else {
        c.to_le_bytes()
    };
    Encoded::Bytes([b[0], b[1], b[2], b[3], 0, 0], 4)
}

/// UTF-8 as glibc reads it: up to six bytes (U+7FFFFFFF), no overlong forms, no surrogates. A sequence cut short by
/// the end of the input is incomplete when every byte present is a continuation byte, else invalid; an invalid
/// sequence is skipped one byte at a time.
fn decode_utf8(input: &[u8]) -> Decoded {
    const INVALID: Decoded = Decoded::Invalid {
        len: 1,
        silent: false,
    };
    let b0 = input[0];
    let (need, min, init) = match b0 {
        0x00..=0x7F => return Decoded::Char(u32::from(b0), 1),
        0xC2..=0xDF => (1, 0x80, u32::from(b0 & 0x1F)),
        0xE0..=0xEF => (2, 0x800, u32::from(b0 & 0x0F)),
        0xF0..=0xF7 => (3, 0x1_0000, u32::from(b0 & 0x07)),
        0xF8..=0xFB => (4, 0x20_0000, u32::from(b0 & 0x03)),
        0xFC..=0xFD => (5, 0x400_0000, u32::from(b0 & 0x01)),
        _ => return INVALID,
    };
    let available = input.len() - 1;
    for &b in &input[1..=need.min(available)] {
        if b & 0xC0 != 0x80 {
            return INVALID;
        }
    }
    if available < need {
        return Decoded::Incomplete;
    }
    let c = input[1..=need]
        .iter()
        .fold(init, |acc, &b| (acc << 6) | u32::from(b & 0x3F));
    if c < min || (0xD800..=0xDFFF).contains(&c) {
        return INVALID;
    }
    Decoded::Char(c, need + 1)
}

fn read16(input: &[u8], big: bool) -> u16 {
    let b = [input[0], input[1]];
    if big {
        u16::from_be_bytes(b)
    } else {
        u16::from_le_bytes(b)
    }
}

fn read32(input: &[u8], big: bool) -> u32 {
    let b = [input[0], input[1], input[2], input[3]];
    if big {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    }
}

/// UTF-16: a lone low surrogate, or a high surrogate not followed by a low one, is invalid (two bytes are skipped);
/// a high surrogate at the end is incomplete.
fn decode_utf16(input: &[u8], big: bool) -> Decoded {
    if input.len() < 2 {
        return Decoded::Incomplete;
    }
    let u = read16(input, big);
    match u {
        0xD800..=0xDBFF => {
            if input.len() < 4 {
                return Decoded::Incomplete;
            }
            let u2 = read16(&input[2..], big);
            if (0xDC00..=0xDFFF).contains(&u2) {
                let c = 0x1_0000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(u2) - 0xDC00);
                Decoded::Char(c, 4)
            } else {
                Decoded::Invalid {
                    len: 2,
                    silent: false,
                }
            }
        }
        0xDC00..=0xDFFF => Decoded::Invalid {
            len: 2,
            silent: false,
        },
        _ => Decoded::Char(u32::from(u), 2),
    }
}

/// UCS-2: surrogates are invalid; the big-endian module skips them silently under //IGNORE.
fn decode_ucs2(input: &[u8], big: bool) -> Decoded {
    if input.len() < 2 {
        return Decoded::Incomplete;
    }
    let u = read16(input, big);
    if (0xD800..=0xDFFF).contains(&u) {
        Decoded::Invalid {
            len: 2,
            silent: big,
        }
    } else {
        Decoded::Char(u32::from(u), 2)
    }
}

/// UTF-32: surrogates and values above U+10FFFF are invalid.
fn decode_utf32(input: &[u8], big: bool) -> Decoded {
    if input.len() < 4 {
        return Decoded::Incomplete;
    }
    let c = read32(input, big);
    if c > 0x10_FFFF || (0xD800..=0xDFFF).contains(&c) {
        Decoded::Invalid {
            len: 4,
            silent: false,
        }
    } else {
        Decoded::Char(c, 4)
    }
}

/// UCS-4: values above U+7FFFFFFF are invalid and skipped silently under //IGNORE.
fn decode_ucs4(input: &[u8], big: bool) -> Decoded {
    if input.len() < 4 {
        return Decoded::Incomplete;
    }
    let c = read32(input, big);
    if c > 0x7FFF_FFFF {
        Decoded::Invalid {
            len: 4,
            silent: true,
        }
    } else {
        Decoded::Char(c, 4)
    }
}

/// UTF-8 encoding of `c` (no surrogates, at most U+7FFFFFFF).
fn encode_utf8(c: u32) -> Encoded {
    let mut b = [0u8; 6];
    let len = match c {
        0..=0x7F => {
            b[0] = c as u8;
            return Encoded::Bytes(b, 1);
        }
        0x80..=0x7FF => 2,
        0x800..=0xFFFF => 3,
        0x1_0000..=0x1F_FFFF => 4,
        0x20_0000..=0x3FF_FFFF => 5,
        _ => 6,
    };
    let lead: u8 = [0, 0, 0xC0, 0xE0, 0xF0, 0xF8, 0xFC][len];
    let mut v = c;
    for i in (1..len).rev() {
        b[i] = 0x80 | (v & 0x3F) as u8;
        v >>= 6;
    }
    b[0] = lead | v as u8;
    Encoded::Bytes(b, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(cs: Charset, input: &[u8]) -> Decoded {
        cs.decode(input, &DecodeState::default())
    }

    #[test]
    fn utf8_forms() {
        assert_eq!(dec(Charset::Utf8, b"\xc3\xa9"), Decoded::Char(0xE9, 2));
        assert_eq!(
            dec(Charset::Utf8, b"\xfd\xbf\xbf\xbf\xbf\xbf"),
            Decoded::Char(0x7FFF_FFFF, 6)
        );
        assert_eq!(dec(Charset::Utf8, b"\xe0\x80"), Decoded::Incomplete);
        assert!(matches!(
            dec(Charset::Utf8, b"\xe0\x80\x80"),
            Decoded::Invalid { .. }
        ));
        assert!(matches!(
            dec(Charset::Utf8, b"\xed\xa0\x80"),
            Decoded::Invalid { .. }
        ));
        assert!(matches!(
            dec(Charset::Utf8, b"\xc0"),
            Decoded::Invalid { .. }
        ));
        for c in [
            0,
            0x7F,
            0x80,
            0x7FF,
            0x800,
            0xFFFF,
            0x1_0000,
            0x10_FFFF,
            0x20_0000,
            0x7FFF_FFFF,
        ] {
            let Encoded::Bytes(b, n) = encode_utf8(c) else {
                panic!()
            };
            assert_eq!(dec(Charset::Utf8, &b[..n]), Decoded::Char(c, n), "{c:x}");
        }
    }

    #[test]
    fn utf16_bom() {
        let mut state = DecodeState::default();
        assert_eq!(
            Charset::Utf16.read_bom(b"\xfe\xff\x00a", &mut state),
            Some(2)
        );
        assert_eq!(
            Charset::Utf16.decode(b"\x00a", &state),
            Decoded::Char(0x61, 2)
        );
        assert_eq!(Charset::Utf16.read_bom(b"\xfe\xff", &mut state), Some(0));
        let mut fresh = DecodeState::default();
        assert_eq!(Charset::Utf16.read_bom(b"\xfe", &mut fresh), None);
        assert_eq!(Charset::Utf16.read_bom(b"a\x00", &mut fresh), Some(0));
        assert_eq!(
            Charset::Utf16.decode(b"a\x00", &fresh),
            Decoded::Char(0x61, 2)
        );
    }
}
