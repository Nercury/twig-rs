//! glibc's single-byte modules: the stateless ones (iconvdata/8bit-gap.c, 8bit-generic.c,
//! armscii-8.c, iso646.c, iso_11548-1.c) with tables measured in the oracle (`iconv_tables.rs`),
//! and the composing ones (iconvdata/cp1255.c, cp1258.c, tcvn5712-1.c).

use std::sync::OnceLock;

use super::cjk_tables as t;
use super::tables::EncodeMap;
use super::unicode::{char1, illegal, put, unmappable_or_tag};
use super::{DStep, Decode, EStep, Encode, Sbcs, names};

/// The decoder and encoder of a single-byte module.
pub(super) fn codec(module: &str) -> Option<(Box<dyn Decode>, Box<dyn Encode>)> {
    if let Some(composing) = Composing::get(module) {
        return Some((
            Box::new(ComposingDec {
                cs: composing,
                last: 0,
            }),
            Box::new(ComposingEnc { cs: composing }),
        ));
    }
    let i = names::SBCS
        .binary_search_by(|s| s.module.cmp(module))
        .ok()?;
    let table = Table {
        info: &names::SBCS[i],
        decode: &names::SBCS_DATA[512 * i..512 * (i + 1)],
    };
    Some((Box::new(table), Box::new(table)))
}

/// A single-byte charset: the code point of every byte (`0xFFFF` = invalid) and the encoder's
/// deviations from the inverse of that table.
#[derive(Clone, Copy)]
struct Table {
    info: &'static Sbcs,
    /// 256 little-endian `u16`.
    decode: &'static [u8],
}

impl Table {
    fn code_point(&self, b: u8) -> u16 {
        let i = 2 * usize::from(b);
        u16::from_le_bytes([self.decode[i], self.decode[i + 1]])
    }

    /// The encoder of `8bit-gap.c`/`8bit-generic.c` as measured: the inverse of the decoding
    /// table (lowest byte first) with the deviations; nothing at or above U+FFFF.
    fn encode(&self, cp: u32) -> Option<u8> {
        if cp >= 0xFFFF {
            return None;
        }
        let cp = cp as u16;
        if let Some(&(_, b)) = self.info.extra.iter().find(|&&(c, _)| c == cp) {
            return Some(b);
        }
        if self.info.missing.contains(&cp) {
            return None;
        }
        if cp < 256 && self.code_point(cp as u8) == cp {
            return Some(cp as u8);
        }
        (0..=255u8).find(|&b| self.code_point(b) == cp)
    }
}

impl Decode for Table {
    fn fixed_width(&self) -> Option<usize> {
        Some(1)
    }

    fn body(&mut self, input: &[u8], pos: usize, out: &mut Vec<u32>, _max: usize) -> DStep {
        match self.code_point(input[pos]) {
            0xFFFF => illegal(1),
            cp => char1(out, u32::from(cp), 1),
        }
    }
}

impl Encode for Table {
    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let cp = chars[i];
        match self.encode(cp) {
            Some(b) => put(out, cap, &[b]),
            None => unmappable_or_tag(cp),
        }
    }
}

/// A single-byte charset whose decoder composes a base letter with a following combining mark
/// and whose encoder decomposes precomposed characters: CP1255 (Hebrew points), CP1258 and
/// TCVN5712-1 (Vietnamese tone marks).
struct Composing {
    /// The code point of every byte (0xFFFF: invalid).
    decode: &'static [u16; 256],
    /// `(buffered, combining, composed)`, sorted.
    compose: &'static [(u32, u32, u32)],
    /// Characters the decoder buffers until it sees the next one (`must_buffer_ch`).
    buffered: (u32, u32),
    /// Characters the decoder tries to combine with the buffered one.
    combining: (u32, u32),
    /// The encoder's map: the inverse of `decode` with the decompositions.
    deviations: &'static [(u32, &'static [u8])],
    map: &'static OnceLock<EncodeMap>,
}

static CP1255_MAP: OnceLock<EncodeMap> = OnceLock::new();
static CP1258_MAP: OnceLock<EncodeMap> = OnceLock::new();
static TCVN_MAP: OnceLock<EncodeMap> = OnceLock::new();

/// CP1255 (iconvdata/cp1255.c).
static CP1255: Composing = Composing {
    decode: &t::CP1255_DECODE,
    compose: t::CP1255_COMPOSE,
    buffered: (0x05D0, 0x05F2),
    combining: (0x05B0, 0x05C4),
    deviations: t::ENC_CP1255,
    map: &CP1255_MAP,
};

/// CP1258 (iconvdata/cp1258.c).
static CP1258: Composing = Composing {
    decode: &t::CP1258_DECODE,
    compose: t::CP1258_COMPOSE,
    buffered: (0x0041, 0x01B0),
    combining: (0x0300, 0x033F),
    deviations: t::ENC_CP1258,
    map: &CP1258_MAP,
};

/// TCVN5712-1 (iconvdata/tcvn5712-1.c).
static TCVN: Composing = Composing {
    decode: &t::TCVN_DECODE,
    compose: t::TCVN_COMPOSE,
    buffered: (0x0041, 0x01B0),
    combining: (0x0300, 0x033F),
    deviations: t::ENC_TCVN,
    map: &TCVN_MAP,
};

impl Composing {
    fn get(module: &str) -> Option<&'static Composing> {
        match module {
            "CP1255//" => Some(&CP1255),
            "CP1258//" => Some(&CP1258),
            "TCVN5712-1//" => Some(&TCVN),
            _ => None,
        }
    }

    fn composed(&self, last: u32, comb: u32) -> Option<u32> {
        self.compose
            .binary_search_by(|&(a, b, _)| (a, b).cmp(&(last, comb)))
            .ok()
            .map(|i| self.compose[i].2)
    }

    /// Whether a composed character stays buffered for a second combining mark (CP1255's
    /// U+FB2A, U+FB2B and U+FB49).
    fn rebuffered(&self, composed: u32) -> bool {
        self.compose.iter().any(|&(a, _, _)| a == composed)
    }

    fn map(&self) -> &'static EncodeMap {
        self.map.get_or_init(|| {
            EncodeMap::build(
                (0..=255u8).filter_map(|b| match self.decode[usize::from(b)] {
                    0xFFFF => None,
                    cp => Some((u32::from(cp), vec![b])),
                }),
                self.deviations,
            )
        })
    }
}

/// `from_cp1255` / `from_cp1258` / `from_tcvn5712_1`: the buffered character lives in the state
/// and is written out on flush (`EMIT_SHIFT_TO_INIT`).
struct ComposingDec {
    cs: &'static Composing,
    last: u32,
}

impl Decode for ComposingDec {
    fn body(&mut self, input: &[u8], pos: usize, out: &mut Vec<u32>, max: usize) -> DStep {
        let ch = match self.cs.decode[usize::from(input[pos])] {
            0xFFFF => return illegal(1),
            cp => u32::from(cp),
        };
        let must_buffer = (self.cs.buffered.0..=self.cs.buffered.1).contains(&ch);
        if self.last != 0 {
            if (self.cs.combining.0..=self.cs.combining.1).contains(&ch)
                && let Some(composed) = self.cs.composed(self.last, ch)
            {
                if self.cs.rebuffered(composed) {
                    self.last = composed;
                } else {
                    out.push(composed);
                    self.last = 0;
                }
                return DStep::Ok(1);
            }
            out.push(self.last);
            self.last = 0;
            if !must_buffer && out.len() >= max {
                return DStep::Ok(0);
            }
        }
        if must_buffer {
            self.last = ch;
        } else {
            out.push(ch);
        }
        DStep::Ok(1)
    }

    fn save(&self) -> u32 {
        self.last
    }

    fn restore(&mut self, state: u32) {
        self.last = state;
    }

    fn emit_shift_to_init(&mut self, out: &mut Vec<u32>) {
        if self.last != 0 {
            out.push(self.last);
            self.last = 0;
        }
    }
}

/// `to_cp1255` / `to_cp1258` / `to_tcvn5712_1`: direct bytes, else the canonical decomposition.
struct ComposingEnc {
    cs: &'static Composing,
}

impl Encode for ComposingEnc {
    fn body(&mut self, chars: &[u32], i: usize, out: &mut Vec<u8>, cap: usize) -> EStep {
        let cp = chars[i];
        match self.cs.map().get(cp) {
            Some(b) => put(out, cap, b),
            None => unmappable_or_tag(cp),
        }
    }
}
