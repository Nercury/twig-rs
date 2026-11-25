//! glibc's Unicode modules: the builtin UTF-8, UCS-2 and UCS-4 conversions and ASCII
//! (iconv/gconv_simple.c), ISO-8859-1 (iconvdata/iso8859-1.c), `UNICODE` (iconvdata/unicode.c),
//! UTF-16 (iconvdata/utf-16.c) and UTF-32 (iconvdata/utf-32.c).

use super::{DStep, Decode, EStep, Encode, Status};

/// `UNICODE_TAG_HANDLER`: Unicode tag characters (U+E0000..U+E007F) are dropped silently by
/// encoders that cannot represent them.
pub(super) fn is_tag(cp: u32) -> bool {
    cp >> 7 == 0xE0000 >> 7
}

/// The encoder outcome for a code point the charset has no mapping for: a dropped tag character
/// or `STANDARD_TO_LOOP_ERR_HANDLER(4)`.
pub(super) fn unmappable_or_tag(cp: u32) -> EStep {
    if is_tag(cp) {
        EStep::Ok(1)
    } else {
        EStep::Unmappable
    }
}

/// Writes `bytes` if they fit into `cap` (one consumed character), else `Full`.
pub(super) fn put(out: &mut Vec<u8>, cap: usize, bytes: &[u8]) -> EStep {
    if out.len() + bytes.len() > cap {
        EStep::Full
    } else {
        out.extend_from_slice(bytes);
        EStep::Ok(1)
    }
}

/// One decoded character of `len` bytes.
pub(super) fn char1(out: &mut Vec<u32>, cp: u32, len: usize) -> DStep {
    out.push(cp);
    DStep::Ok(len)
}

/// `STANDARD_FROM_LOOP_ERR_HANDLER(skip)`.
pub(super) fn illegal(skip: usize) -> DStep {
    DStep::Illegal { skip, marks: true }
}

fn read16(s: &[u8], swap: bool) -> u32 {
    let v = u16::from_le_bytes([s[0], s[1]]);
    u32::from(if swap { v.swap_bytes() } else { v })
}

fn read32(s: &[u8], swap: bool) -> u32 {
    let v = u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    if swap { v.swap_bytes() } else { v }
}

/// The decoder and encoder of a Unicode module (or ASCII, ISO-8859-1).
pub(super) fn codec(module: &str) -> Option<(Box<dyn Decode>, Box<dyn Encode>)> {
    use Form::*;
    let form = match module {
        "ISO-10646/UTF8/" => {
            return Some((Box::new(Utf8), Box::new(Utf8)));
        }
        "ANSI_X3.4-1968//" => {
            return Some((Box::new(Latin(0x7F)), Box::new(Latin(0x7F))));
        }
        "ISO-8859-1//" => {
            return Some((Box::new(Latin(0xFF)), Box::new(Latin(0xFF))));
        }
        "ISO-10646/UCS4/" => {
            return Some((Box::new(Ucs4 { big: true }), Box::new(Ucs4 { big: true })));
        }
        "UCS-4LE//" => {
            return Some((Box::new(Ucs4 { big: false }), Box::new(Ucs4 { big: false })));
        }
        "ISO-10646/UCS2/" => Ucs2,
        "UNICODEBIG//" => Ucs2Be,
        "UNICODE//" => Unicode,
        "UTF-16//" => Utf16,
        "UTF-16LE//" => Utf16Le,
        "UTF-16BE//" => Utf16Be,
        "UTF-32//" => Utf32,
        "UTF-32LE//" => Utf32Le,
        "UTF-32BE//" => Utf32Be,
        _ => return None,
    };
    Some((
        Box::new(Wide {
            form,
            swap: matches!(form, Utf16Be | Utf32Be),
        }),
        Box::new(Wide { form, swap: false }),
    ))
}

/// glibc's UTF-8 (`utf8_internal_loop` / `internal_utf8_loop`, iconv/gconv_simple.c): five- and
/// six-byte forms and code points up to `0x7FFFFFFF` are accepted, overlong forms and surrogates
/// are not.
struct Utf8;

impl Decode for Utf8 {
    fn body(&mut self, s: &[u8], pos: usize, out: &mut Vec<u32>, _max: usize) -> DStep {
        let s = &s[pos..];
        let first = u32::from(s[0]);
        if first < 0x80 {
            return char1(out, first, 1);
        }
        let (cnt, mut ch) = if (0xC2..0xE0).contains(&first) {
            (2, first & 0x1f)
        } else if first & 0xF0 == 0xE0 {
            (3, first & 0x0f)
        } else if first & 0xF8 == 0xF0 {
            (4, first & 0x07)
        } else if first & 0xFC == 0xF8 {
            (5, first & 0x03)
        } else if first & 0xFE == 0xFC {
            (6, first & 0x01)
        } else {
            let mut i = 1;
            while i < s.len() && s[i] & 0xC0 == 0x80 && i < 5 {
                i += 1;
            }
            return illegal(i);
        };
        if cnt > s.len() {
            let mut i = 1;
            while i < s.len() && s[i] & 0xC0 == 0x80 {
                i += 1;
            }
            if i == s.len() {
                return DStep::Incomplete;
            }
            return illegal(i);
        }
        let mut i = 1;
        while i < cnt {
            let b = s[i];
            if b & 0xC0 != 0x80 {
                break;
            }
            ch = (ch << 6) | u32::from(b & 0x3f);
            i += 1;
        }
        if i < cnt || (cnt > 2 && ch >> (5 * cnt - 4) == 0) || (0xD800..=0xDFFF).contains(&ch) {
            return illegal(i);
        }
        char1(out, ch, cnt)
    }
}

impl Encode for Utf8 {
    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let cp = chars[i];
        if cp < 0x80 {
            put(out, cap, &[cp as u8])
        } else if cp <= 0x7FFF_FFFF && !(0xD800..=0xDFFF).contains(&cp) {
            let mut step = 2;
            while step < 6 && cp & (!0u32 << (5 * step + 1)) != 0 {
                step += 1;
            }
            let mut buf = [0u8; 6];
            let mut wc = cp;
            for i in (1..step).rev() {
                buf[i] = 0x80 | (wc & 0x3f) as u8;
                wc >>= 6;
            }
            buf[0] = ((!0xffu32 >> step) as u8) | wc as u8;
            put(out, cap, &buf[..step])
        } else {
            EStep::Unmappable
        }
    }
}

/// ASCII (`internal_ascii_loop`, highest code point 0x7F) and ISO-8859-1 (highest 0xFF): bytes
/// above the highest are invalid.
struct Latin(u32);

impl Decode for Latin {
    fn fixed_width(&self) -> Option<usize> {
        Some(1)
    }

    fn body(&mut self, s: &[u8], pos: usize, out: &mut Vec<u32>, _max: usize) -> DStep {
        let b = u32::from(s[pos]);
        if b > self.0 {
            illegal(1)
        } else {
            char1(out, b, 1)
        }
    }
}

impl Encode for Latin {
    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let cp = chars[i];
        if cp > self.0 {
            unmappable_or_tag(cp)
        } else {
            put(out, cap, &[cp as u8])
        }
    }
}

/// UCS-4 in big (`ISO-10646/UCS4/`) or little endian (`UCS-4LE`): values above `0x7FFFFFFF` are
/// invalid input; every character can be encoded.
struct Ucs4 {
    big: bool,
}

impl Decode for Ucs4 {
    fn min_needed(&self) -> usize {
        4
    }

    fn fixed_width(&self) -> Option<usize> {
        Some(4)
    }

    /// `ucs4_internal_loop` / `ucs4le_internal_loop`.
    fn special_loop(
        &mut self,
        input: &[u8],
        pos: &mut usize,
        out: &mut Vec<u32>,
        max: usize,
        ignore: bool,
        irreversible: &mut usize,
    ) -> Option<Status> {
        let end = input.len();
        while *pos + 4 <= end && out.len() < max {
            let v = read32(&input[*pos..], self.big);
            if v > 0x7FFF_FFFF {
                if ignore {
                    *irreversible += 1;
                    *pos += 4;
                    continue;
                }
                return Some(Status::IllegalInput);
            }
            out.push(v);
            *pos += 4;
        }
        Some(if *pos == end {
            Status::EmptyInput
        } else if out.len() >= max {
            Status::FullOutput
        } else {
            Status::IncompleteInput
        })
    }

    fn body(&mut self, _s: &[u8], _pos: usize, _out: &mut Vec<u32>, _max: usize) -> DStep {
        unreachable!("UCS-4 uses its own loop")
    }
}

impl Encode for Ucs4 {
    fn min_needed(&self) -> usize {
        4
    }

    /// `internal_ucs4_loop` / `internal_ucs4le_loop`.
    fn special_loop(
        &mut self,
        chars: &[u32],
        out: &mut Vec<u8>,
        cap: usize,
    ) -> Option<(usize, Status)> {
        let n = chars.len().min((cap - out.len()) / 4);
        for &cp in &chars[..n] {
            out.extend_from_slice(&if self.big {
                cp.to_be_bytes()
            } else {
                cp.to_le_bytes()
            });
        }
        let status = if n == chars.len() {
            Status::EmptyInput
        } else {
            Status::FullOutput
        };
        Some((n, status))
    }

    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let cp = chars[i];
        put(
            out,
            cap,
            &if self.big {
                cp.to_be_bytes()
            } else {
                cp.to_le_bytes()
            },
        )
    }
}

/// The 16- and 32-bit forms.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    /// `ISO-10646/UCS2/`: host (little-endian) byte order.
    Ucs2,
    /// `UNICODEBIG`: big-endian UCS-2 (glibc's "ucs2reverse" on little-endian hosts).
    Ucs2Be,
    /// `UNICODE`: UCS-2 with a byte order mark.
    Unicode,
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
}

/// A 16- or 32-bit Unicode form.
struct Wide {
    form: Form,
    /// Decoding: the input is in the other byte order (from a byte order mark or the name).
    swap: bool,
}

impl Wide {
    fn unit(&self) -> usize {
        match self.form {
            Form::Utf32 | Form::Utf32Le | Form::Utf32Be => 4,
            _ => 2,
        }
    }

    /// The byte order mark the module writes and detects.
    fn bom(&self) -> &'static [u8] {
        match self.form {
            Form::Utf16 | Form::Unicode => b"\xFF\xFE",
            Form::Utf32 => b"\xFF\xFE\x00\x00",
            _ => b"",
        }
    }
}

impl Decode for Wide {
    fn min_needed(&self) -> usize {
        self.unit()
    }

    fn fixed_width(&self) -> Option<usize> {
        match self.form {
            Form::Utf16 | Form::Utf16Le | Form::Utf16Be => None,
            _ => Some(self.unit()),
        }
    }

    /// The `PREPARE_LOOP` byte order mark detection of `UTF-16`, `UTF-32` and `UNICODE` on the
    /// first invocation.
    fn prepare(&mut self, input: &[u8], pos: &mut usize, invocations: u64) -> Option<Status> {
        let bom = self.bom();
        if invocations != 0 || bom.is_empty() {
            return None;
        }
        let n = bom.len();
        let rest = &input[*pos..];
        if rest.len() < n {
            return Some(if rest.is_empty() {
                Status::EmptyInput
            } else {
                Status::IncompleteInput
            });
        }
        if &rest[..n] == bom {
            *pos += n;
        } else if rest[..n].iter().eq(bom.iter().rev()) {
            self.swap = true;
            *pos += n;
        }
        None
    }

    fn body(&mut self, input: &[u8], pos: usize, out: &mut Vec<u32>, _max: usize) -> DStep {
        let s = &input[pos..];
        match self.form {
            Form::Ucs2 | Form::Unicode => {
                let u = read16(s, self.swap);
                if (0xD800..0xE000).contains(&u) {
                    illegal(2)
                } else {
                    char1(out, u, 2)
                }
            }
            Form::Ucs2Be => {
                let u = read16(s, true);
                if (0xD800..0xE000).contains(&u) {
                    DStep::Illegal {
                        skip: 2,
                        marks: false,
                    }
                } else {
                    char1(out, u, 2)
                }
            }
            Form::Utf16 | Form::Utf16Le | Form::Utf16Be => {
                let u1 = read16(s, self.swap);
                if !(0xD800..=0xDFFF).contains(&u1) {
                    return char1(out, u1, 2);
                }
                if u1 >= 0xDC00 {
                    return illegal(2);
                }
                if s.len() < 4 {
                    return DStep::Incomplete;
                }
                let u2 = read16(&s[2..], self.swap);
                if !(0xDC00..=0xDFFF).contains(&u2) {
                    return illegal(2);
                }
                char1(out, ((u1 - 0xD7C0) << 10) + (u2 - 0xDC00), 4)
            }
            Form::Utf32 | Form::Utf32Le | Form::Utf32Be => {
                let u = read32(s, self.swap);
                if u >= 0x110000 || (0xD800..0xE000).contains(&u) {
                    illegal(4)
                } else {
                    char1(out, u, 4)
                }
            }
        }
    }
}

impl Encode for Wide {
    fn min_needed(&self) -> usize {
        self.unit()
    }

    /// `PREPARE_LOOP`: the byte order mark on the first invocation (also inside the
    /// transliteration calls of that invocation).
    fn prepare(&mut self, out: &mut Vec<u8>, cap: usize, invocations: u64) -> Option<Status> {
        let bom = self.bom();
        if invocations != 0 || bom.is_empty() {
            return None;
        }
        if out.len() + bom.len() > cap {
            return Some(Status::FullOutput);
        }
        out.extend_from_slice(bom);
        None
    }

    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let cp = chars[i];
        let surrogate = (0xD800..0xE000).contains(&cp);
        match self.form {
            Form::Ucs2 | Form::Ucs2Be | Form::Unicode => {
                if cp >= 0x10000 {
                    unmappable_or_tag(cp)
                } else if surrogate {
                    EStep::Illegal {
                        marks: self.form != Form::Ucs2Be,
                    }
                } else if self.form == Form::Ucs2Be {
                    put(out, cap, &(cp as u16).to_be_bytes())
                } else {
                    put(out, cap, &(cp as u16).to_le_bytes())
                }
            }
            Form::Utf16 | Form::Utf16Le | Form::Utf16Be => {
                let be = self.form == Form::Utf16Be;
                let unit = |u: u32| {
                    if be {
                        (u as u16).to_be_bytes()
                    } else {
                        (u as u16).to_le_bytes()
                    }
                };
                if surrogate {
                    EStep::Illegal { marks: true }
                } else if cp >= 0x10000 {
                    if cp >= 0x110000 {
                        return EStep::Unmappable;
                    }
                    let (hi, lo) = (unit(0xD7C0 + (cp >> 10)), unit(0xDC00 + (cp & 0x3ff)));
                    put(out, cap, &[hi[0], hi[1], lo[0], lo[1]])
                } else {
                    put(out, cap, &unit(cp))
                }
            }
            Form::Utf32 | Form::Utf32Le | Form::Utf32Be => {
                if cp >= 0x110000 {
                    EStep::Unmappable
                } else if surrogate {
                    EStep::Illegal { marks: true }
                } else if self.form == Form::Utf32Be {
                    put(out, cap, &cp.to_be_bytes())
                } else {
                    put(out, cap, &cp.to_le_bytes())
                }
            }
        }
    }
}
