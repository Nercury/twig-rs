//! The multi-byte charsets without shift states: Shift_JIS (`SJIS`, `CP932`), `EUC-JP` and `EUC-JP-MS` (with the
//! three-byte JIS X 0212 sequences), `GBK`, `GB18030` (with its four-byte sequences), `EUC-CN`, `BIG5`, `BIG5-HKSCS`,
//! `EUC-KR`, `UHC`, `JOHAB` and `EUC-TW` (with its four-byte sequences of the CNS 11643 planes).
//!
//! The tables (`multibyte_table.rs`) are what glibc 2.41's iconv does in the oracle, measured black-box by
//! `multibyte.gen.php` through `multibyte_probe.c`: every byte sequence each charset's structure allows decoded and
//! every code point encoded, compared with the published mapping files (multibyte_differences.md lists every
//! difference).
//!
//! A charset decodes a byte as a character of its own ([`Spec::single`]), as invalid, or as the lead byte of a longer
//! sequence; the two-byte sequences are the cells of a grid ([`Plane`]), the longer ones follow the charset's
//! [`Family`]. The grids are stored compressed: the coded character sets several charsets share (JIS X 0208,
//! JIS X 0212, GB 2312, KS X 1001, Big5, the CNS 11643 planes) once each, as [`Stream`]s, and every grid is assembled
//! from them by its [`Step`]s the first time a conversion needs it. The encoder of a charset is the inverse of its
//! decoder (the first or the last of the sequences of a character, [`Spec::encode_last`]) with the exceptions glibc
//! makes ([`Spec::encode`]), also built on first use.
//!
//! An invalid sequence makes //IGNORE skip one byte or the whole sequence: for two bytes the trail bytes of a lead
//! byte's window ([`Plane::windows`]) skip both, the others only the lead byte; the longer sequences follow their
//! family's rule, and a few sequences are irregular ([`Quirk`]).

use std::sync::OnceLock;

use super::codec::Decoded;
pub(crate) use super::multibyte_table::Multibyte;
use super::multibyte_table::{
    ALL, GB18030_BMP_END, GB18030_RANGES, PAIR, PAIRS, PLANE_COUNT, PLANES,
};

/// How a charset decodes the sequences longer than two bytes.
#[derive(Debug)]
pub(super) enum Family {
    /// Two-byte sequences only.
    Dbcs,
    /// EUC-JP: after 8F, a second byte in `wait` waits for a third, and the pair of them is a cell of `plane`; any
    /// other second byte is invalid (one byte skipped), an invalid three-byte sequence skips `skip3` bytes.
    EucJp {
        plane: u8,
        wait: (u8, u8),
        skip3: u8,
    },
    /// EUC-TW: 8E, a plane byte A1..B0 and two bytes, a cell of the plane listed for the plane byte (A1 is the plane of
    /// the two-byte sequences); anything else after 8E is invalid, and an invalid sequence skips one byte.
    EucTw { planes: &'static [(u8, u8)] },
    /// GB18030: a second byte 30..39 starts a four-byte sequence `[81-FE][30-39][81-FE][30-39]`, which maps linearly
    /// to the BMP code points the two-byte sequences lack ([`GB18030_RANGES`]) and from 90 30 81 30 to U+10000 and up.
    /// The input must hold all four bytes; a third byte outside 81..FE makes //IGNORE skip three, else four.
    Gb18030,
}

/// The irregular sequences of a charset.
#[derive(Debug)]
pub(super) enum Quirk {
    /// None.
    None,
    /// Under //IGNORE this two-byte sequence skips four bytes and needs them (UHC's A2 E8, where glibc reads past the
    /// end of a shorter input; the oracle's PHP reports the incomplete character); without //IGNORE it is invalid,
    /// but glibc has moved past it ([`Decoded::IllegalPast`]).
    IgnoreSkip4(u8, u8),
    /// Under //IGNORE `lead` followed by a byte of `trails` decodes to `cp` (EUC-JP-MS: U+FFFD), without //IGNORE it
    /// is invalid.
    IgnoreReplacement { lead: u8, trails: (u8, u8), cp: u32 },
}

/// How glibc converts one multi-byte charset.
#[derive(Debug)]
pub(super) struct Spec {
    /// The sequences longer than two bytes.
    pub(super) family: Family,
    /// The bytes that are characters of their own: ranges of consecutive bytes and the code point of the first.
    pub(super) single: &'static [(u8, u8, u32)],
    /// The bytes that start a longer sequence (a bitmap); every other byte is invalid.
    pub(super) lead: [u32; 8],
    /// The plane of the two-byte sequences (an index into [`PLANES`]).
    pub(super) plane: u8,
    /// //IGNORE skips invalid input without making the call report `EILSEQ` (CP932).
    pub(super) silent: bool,
    /// The irregular sequences.
    pub(super) quirk: Quirk,
    /// The planes of the encoder's inverse, in order, with the bytes before their lead and trail bytes.
    pub(super) encode_planes: &'static [(u8, u32)],
    /// The inverse takes the last of several sequences of a character, else the first.
    pub(super) encode_last: bool,
    /// The exceptions of the encoder, sorted: a code point and its code (`u32::MAX`: none).
    pub(super) encode: &'static [(u32, u32)],
}

/// The grid of a charset's sequences of two bytes after a prefix: a row per lead byte and a column per trail byte,
/// assembled by the steps; and the skip windows of its lead bytes.
#[derive(Debug)]
pub(super) struct Plane {
    /// The lead bytes (the rows).
    pub(super) leads: &'static [(u8, u8)],
    /// The trail bytes (the columns).
    pub(super) trails: &'static [(u8, u8)],
    /// How the grid is assembled.
    pub(super) steps: &'static [Step],
    /// For each lead byte in the order of `leads`, the index of its window (empty: all use `windows[0]`).
    pub(super) window_of: &'static [u8],
    /// The trail bytes at which an invalid pair makes //IGNORE skip both bytes (elsewhere the lead byte only).
    pub(super) windows: &'static [&'static [(u8, u8)]],
}

/// A step of assembling a grid.
#[derive(Debug)]
pub(super) enum Step {
    /// The cells of a coded character set (`rows` x `cols` cells of a stream) placed by `layout`.
    Set {
        stream: &'static [u8],
        rows: u8,
        cols: u8,
        layout: Layout,
    },
    /// A stream over the cells of a region (lead bytes `leads` that are rows, trail bytes `trails`) still empty.
    Holes {
        stream: &'static [u8],
        leads: (u8, u8),
        trails: &'static [(u8, u8)],
    },
    /// The Hangul syllables U+AC00..U+D7A3 the grid does not hold yet, in order, into the empty cells of a region.
    Hangul {
        leads: (u8, u8),
        trails: &'static [(u8, u8)],
    },
    /// Cells set (`lead << 8 | trail` and a code point, 0 to empty the cell, or [`PAIR`] and an index into [`PAIRS`]).
    Patch(&'static [(u16, u32)]),
}

/// Where a coded character set's cell (row `r`, column `c`, from 0) goes.
#[derive(Clone, Copy, Debug)]
pub(super) enum Layout {
    /// Lead byte `.0 + r`, trail byte `.1 + c`.
    Offset(u8, u8),
    /// JIS X 0208 in Shift_JIS.
    Sjis,
    /// KS X 1001's rows 1 to 12 and 42 to 93 in Johab (the others are left out).
    Johab,
}

impl Layout {
    fn place(self, r: u8, c: u8) -> Option<(u8, u8)> {
        match self {
            Layout::Offset(l, t) => Some((l + r, t + c)),
            Layout::Sjis => {
                let lead = r / 2 + if r < 62 { 0x81 } else { 0xC1 };
                let trail = if r % 2 == 1 {
                    c + 0x9F
                } else if c < 63 {
                    c + 0x40
                } else {
                    c + 0x41
                };
                Some((lead, trail))
            }
            Layout::Johab => {
                let (lead, odd) = match r {
                    0..12 => (0xD9 + r / 2, r % 2 == 1),
                    41..93 => (0xE0 + (r - 41) / 2, (r - 41) % 2 == 1),
                    _ => return None,
                };
                let trail = if odd {
                    c + 0xA1
                } else if c < 78 {
                    c + 0x31
                } else {
                    c + 0x43
                };
                Some((lead, trail))
            }
        }
    }
}

/// A compressed list of cells, each a code point or empty (see multibyte.gen.php). `prev` starts at 0 and is the last
/// code point listed; after the end every cell is empty.
///
/// - `0x00..=0x3F`: one cell, `prev + 1 + t`;
/// - `0x40..=0x7F`: a run of `t - 0x40 + 2` cells `prev + 1`, `prev + 2`, ...;
/// - `0x80..=0x9F`: `t - 0x80 + 1` empty cells;
/// - `0xA0..=0xDF b`: one cell, `prev + d`, `d` the 14-bit two's complement `(t - 0xA0) << 8 | b`;
/// - `0xE0..=0xFF b1 b2`: one cell, `(t - 0xE0) << 16 | b1 << 8 | b2`.
struct Stream {
    data: &'static [u8],
    pos: usize,
    prev: u32,
    run: u32,
    gap: u32,
}

impl Stream {
    fn new(data: &'static [u8]) -> Self {
        Stream {
            data,
            pos: 0,
            prev: 0,
            run: 0,
            gap: 0,
        }
    }

    /// The next cell: `Some(None)` empty, `None` after the end.
    fn next_cell(&mut self) -> Option<Option<u32>> {
        if self.run > 0 {
            self.run -= 1;
            self.prev += 1;
            return Some(Some(self.prev));
        }
        if self.gap > 0 {
            self.gap -= 1;
            return Some(None);
        }
        let &t = self.data.get(self.pos)?;
        self.pos += 1;
        match t {
            0x00..=0x3F => self.prev += 1 + u32::from(t),
            0x40..=0x7F => {
                self.run = u32::from(t - 0x40) + 1;
                self.prev += 1;
            }
            0x80..=0x9F => {
                self.gap = u32::from(t - 0x80);
                return Some(None);
            }
            0xA0..=0xDF => {
                let v = u32::from(t - 0xA0) << 8 | u32::from(self.data[self.pos]);
                self.pos += 1;
                // the sign of the 14-bit delta
                self.prev = if v >= 0x2000 {
                    self.prev - (0x4000 - v)
                } else {
                    self.prev + v
                };
            }
            0xE0..=0xFF => {
                self.prev = u32::from(t - 0xE0) << 16
                    | u32::from(self.data[self.pos]) << 8
                    | u32::from(self.data[self.pos + 1]);
                self.pos += 2;
            }
        }
        Some(Some(self.prev))
    }
}

/// An assembled grid.
#[derive(Debug)]
struct Grid {
    /// The row of each lead byte, `NONE` for the others.
    row: [u8; 256],
    /// The column of each trail byte, `NONE` for the others.
    col: [u8; 256],
    cols: usize,
    /// The cells, row by row: a code point, 0 for none, or `PAIR | i`.
    cells: Box<[u32]>,
}

const NONE: u8 = u8::MAX;

impl Grid {
    fn build(plane: &Plane) -> Grid {
        let index = |ranges: &[(u8, u8)]| {
            let mut map = [NONE; 256];
            let mut n = 0u8;
            for &(lo, hi) in ranges {
                for b in lo..=hi {
                    map[usize::from(b)] = n;
                    n += 1;
                }
            }
            (map, usize::from(n))
        };
        let (row, rows) = index(plane.leads);
        let (col, cols) = index(plane.trails);
        let mut grid = Grid {
            row,
            col,
            cols,
            cells: vec![0; rows * cols].into_boxed_slice(),
        };
        for step in plane.steps {
            match *step {
                Step::Set {
                    stream,
                    rows,
                    cols,
                    layout,
                } => {
                    let mut s = Stream::new(stream);
                    'cells: for r in 0..rows {
                        for c in 0..cols {
                            let Some(cell) = s.next_cell() else {
                                break 'cells;
                            };
                            if let (Some(cp), Some((lead, trail))) = (cell, layout.place(r, c)) {
                                let i =
                                    grid.index(lead, trail).expect("a set cell inside the grid");
                                grid.cells[i] = cp;
                            }
                        }
                    }
                }
                Step::Holes {
                    stream,
                    leads,
                    trails,
                } => {
                    let mut s = Stream::new(stream);
                    for i in grid.empty_cells(leads, trails) {
                        match s.next_cell() {
                            Some(Some(cp)) => grid.cells[i] = cp,
                            Some(None) => {}
                            None => break,
                        }
                    }
                }
                Step::Hangul { leads, trails } => {
                    let mut held = vec![false; 11172];
                    for &cp in grid.cells.iter() {
                        if (0xAC00..=0xD7A3).contains(&cp) {
                            held[(cp - 0xAC00) as usize] = true;
                        }
                    }
                    let mut syllables =
                        (0xAC00..=0xD7A3u32).filter(|&cp| !held[(cp - 0xAC00) as usize]);
                    for i in grid.empty_cells(leads, trails) {
                        match syllables.next() {
                            Some(cp) => grid.cells[i] = cp,
                            None => break,
                        }
                    }
                }
                Step::Patch(cells) => {
                    for &(code, cp) in cells {
                        let [lead, trail] = code.to_be_bytes();
                        let i = grid
                            .index(lead, trail)
                            .expect("a patched cell inside the grid");
                        grid.cells[i] = cp;
                    }
                }
            }
        }
        grid
    }

    fn index(&self, lead: u8, trail: u8) -> Option<usize> {
        let (r, c) = (self.row[usize::from(lead)], self.col[usize::from(trail)]);
        (r != NONE && c != NONE).then(|| usize::from(r) * self.cols + usize::from(c))
    }

    /// The indices of the empty cells of a region, in grid order (lead bytes that are not rows are left out).
    fn empty_cells(&self, leads: (u8, u8), trails: &[(u8, u8)]) -> Vec<usize> {
        let mut cells = Vec::new();
        for lead in leads.0..=leads.1 {
            for &(lo, hi) in trails {
                for trail in lo..=hi {
                    if let Some(i) = self.index(lead, trail)
                        && self.cells[i] == 0
                    {
                        cells.push(i);
                    }
                }
            }
        }
        cells
    }

    /// The cell of `lead`, `trail`: 0 when there is none.
    fn get(&self, lead: u8, trail: u8) -> u32 {
        self.index(lead, trail).map_or(0, |i| self.cells[i])
    }
}

/// The grids, assembled on first use.
static GRIDS: [OnceLock<Grid>; PLANE_COUNT] = [const { OnceLock::new() }; PLANE_COUNT];

fn grid(plane: u8) -> &'static Grid {
    GRIDS[usize::from(plane)].get_or_init(|| Grid::build(&PLANES[usize::from(plane)]))
}

/// An encoder: the code points it writes, sorted, and their codes; and the pairs of code points it writes as one code
/// (sorted), the first of which it holds back until it sees the second.
#[derive(Debug)]
struct Encoder {
    map: Box<[(u32, u32)]>,
    pairs: Box<[(u32, u32, u32)]>,
}

/// The encoders, built on first use.
static ENCODERS: [OnceLock<Encoder>; ALL.len()] = [const { OnceLock::new() }; ALL.len()];

/// The decoding of a cell of `len` bytes.
fn cell(cp: u32, len: usize) -> Decoded {
    if cp & PAIR != 0 {
        let (a, b) = PAIRS[(cp & !PAIR) as usize];
        Decoded::Pair(a, b, len)
    } else {
        Decoded::Char(cp, len)
    }
}

/// The bytes of a code (an integer of one to four bytes; 0 is the byte 0).
fn code_bytes(code: u32) -> ([u8; 4], usize) {
    let len = (4 - code.leading_zeros() as usize / 8).max(1);
    let b = code.to_be_bytes();
    let mut out = [0; 4];
    out[..len].copy_from_slice(&b[4 - len..]);
    (out, len)
}

impl Multibyte {
    /// Decodes the next character of `input` (not empty); `ignore`: the target has //IGNORE, which some irregular
    /// sequences depend on.
    pub(super) fn decode(self, input: &[u8], ignore: bool) -> Decoded {
        let spec = self.spec();
        let invalid = |len: usize| Decoded::Invalid {
            len,
            silent: spec.silent,
        };
        let b0 = input[0];
        if let Some(&(lo, _, base)) = spec
            .single
            .iter()
            .find(|&&(lo, hi, _)| (lo..=hi).contains(&b0))
        {
            return Decoded::Char(base + u32::from(b0 - lo), 1);
        }
        if spec.lead[usize::from(b0 >> 5)] & (1 << (b0 & 31)) == 0 {
            return invalid(1);
        }
        let Some(&b1) = input.get(1) else {
            return Decoded::Incomplete;
        };
        match spec.family {
            Family::EucJp { plane, wait, skip3 } if b0 == 0x8F => {
                if !(wait.0..=wait.1).contains(&b1) {
                    return invalid(1);
                }
                let Some(&b2) = input.get(2) else {
                    return Decoded::Incomplete;
                };
                return match grid(plane).get(b1, b2) {
                    0 => invalid(usize::from(skip3)),
                    cp => cell(cp, 3),
                };
            }
            Family::EucTw { planes } if b0 == 0x8E => {
                if !(0xA1..=0xB0).contains(&b1) {
                    return invalid(1);
                }
                if input.len() < 4 {
                    return Decoded::Incomplete;
                }
                let plane = if b1 == 0xA1 {
                    Some(spec.plane)
                } else {
                    planes.iter().find(|&&(b, _)| b == b1).map(|&(_, p)| p)
                };
                return match plane.map_or(0, |p| grid(p).get(input[2], input[3])) {
                    0 => invalid(1),
                    cp => cell(cp, 4),
                };
            }
            Family::Gb18030 if (0x30..=0x39).contains(&b1) => {
                return decode_gb18030_four(input).unwrap_or_else(invalid);
            }
            _ => {}
        }
        match spec.quirk {
            Quirk::IgnoreSkip4(lead, trail) if !ignore && b0 == lead && b1 == trail => {
                return Decoded::IllegalPast(2);
            }
            Quirk::IgnoreSkip4(lead, trail) if b0 == lead && b1 == trail => {
                return if input.len() < 4 {
                    Decoded::PastEnd
                } else {
                    // reported, unlike the other skips of a charset that skips silently
                    Decoded::Invalid {
                        len: 4,
                        silent: false,
                    }
                };
            }
            Quirk::IgnoreReplacement { lead, trails, cp }
                if ignore && b0 == lead && (trails.0..=trails.1).contains(&b1) =>
            {
                return Decoded::Char(cp, 2);
            }
            _ => {}
        }
        let g = grid(spec.plane);
        match g.get(b0, b1) {
            0 => {
                let plane = &PLANES[usize::from(spec.plane)];
                let row = usize::from(g.row[usize::from(b0)]);
                let window = plane.windows[plane.window_of.get(row).map_or(0, |&w| usize::from(w))];
                let both = window.iter().any(|&(lo, hi)| (lo..=hi).contains(&b1));
                invalid(if both { 2 } else { 1 })
            }
            cp => cell(cp, 2),
        }
    }

    /// The bytes `c` encodes to (one to four, in the first `len` of the array), `None` when the charset cannot
    /// represent it.
    pub(super) fn encode(self, c: u32) -> Option<([u8; 4], usize)> {
        let map = &self.encoder().map;
        if let Ok(i) = map.binary_search_by_key(&c, |&(cp, _)| cp) {
            return Some(code_bytes(map[i].1));
        }
        if self == Multibyte::Gb18030 {
            return encode_gb18030_four(c);
        }
        None
    }

    /// Whether the encoder holds `c` back: the first character of a pair it writes as one code.
    pub(super) fn encoder_holds(self, c: u32) -> bool {
        self.encoder().pairs.iter().any(|&(a, _, _)| a == c)
    }

    /// The code of the pair `held`, `c`.
    pub(super) fn encode_pair(self, held: u32, c: u32) -> Option<([u8; 4], usize)> {
        self.encoder()
            .pairs
            .iter()
            .find(|&&(a, b, _)| a == held && b == c)
            .map(|&(_, _, code)| code_bytes(code))
    }

    fn encoder(self) -> &'static Encoder {
        ENCODERS[self as usize].get_or_init(|| self.build_encoder())
    }

    /// The inverse of the decoder with the exceptions of the encoder.
    fn build_encoder(self) -> Encoder {
        let spec = self.spec();
        let mut entries: Vec<(u32, u32)> = Vec::new();
        let mut pairs: Vec<(u32, u32, u32)> = Vec::new();
        for &(lo, hi, base) in spec.single {
            for b in lo..=hi {
                entries.push((base + u32::from(b - lo), u32::from(b)));
            }
        }
        for &(plane, prefix) in spec.encode_planes {
            let g = grid(plane);
            let p = &PLANES[usize::from(plane)];
            for &(llo, lhi) in p.leads {
                for lead in llo..=lhi {
                    for &(tlo, thi) in p.trails {
                        for trail in tlo..=thi {
                            let cp = g.get(lead, trail);
                            let code = prefix << 16 | u32::from(lead) << 8 | u32::from(trail);
                            if cp & PAIR != 0 {
                                let (a, b) = PAIRS[(cp & !PAIR) as usize];
                                pairs.push((a, b, code));
                            } else if cp != 0 {
                                entries.push((cp, code));
                            }
                        }
                    }
                }
            }
        }
        // a stable sort keeps the order of the sequences of a character
        entries.sort_by_key(|&(cp, _)| cp);
        let mut map: Vec<(u32, u32)> = Vec::with_capacity(entries.len());
        for (cp, code) in entries {
            match map.last_mut() {
                Some(last) if last.0 == cp => {
                    if spec.encode_last {
                        last.1 = code;
                    }
                }
                _ => map.push((cp, code)),
            }
        }
        for &(cp, code) in spec.encode {
            match map.binary_search_by_key(&cp, |&(c, _)| c) {
                Ok(i) if code == u32::MAX => {
                    map.remove(i);
                }
                Ok(i) => map[i].1 = code,
                Err(_) if code == u32::MAX => {}
                Err(i) => map.insert(i, (cp, code)),
            }
        }
        pairs.sort_unstable();
        Encoder {
            map: map.into_boxed_slice(),
            pairs: pairs.into_boxed_slice(),
        }
    }
}

/// The linear index of a four-byte GB18030 sequence.
fn gb18030_linear(b: &[u8]) -> u32 {
    let [b0, b1, b2, b3] = [b[0], b[1], b[2], b[3]].map(u32::from);
    (((b0 - 0x81) * 10 + (b1 - 0x30)) * 126 + (b2 - 0x81)) * 10 + (b3 - 0x30)
}

/// GB18030 from a lead byte and a second byte 30..39: the character, or the number of bytes //IGNORE skips.
fn decode_gb18030_four(input: &[u8]) -> Result<Decoded, usize> {
    if input.len() < 4 {
        return Ok(Decoded::Incomplete);
    }
    if !(0x81..=0xFE).contains(&input[2]) {
        return Err(3);
    }
    if !(0x30..=0x39).contains(&input[3]) {
        return Err(4);
    }
    let i = gb18030_linear(input);
    if i <= GB18030_BMP_END {
        let r = GB18030_RANGES.partition_point(|&(first, _)| u32::from(first) <= i) - 1;
        let (first, cp) = GB18030_RANGES[r];
        return Ok(Decoded::Char(u32::from(cp) + i - u32::from(first), 4));
    }
    // U+10000.. from 90 30 81 30 (linear index 189000)
    match i.checked_sub(189_000) {
        Some(n) if n <= 0x10_FFFF - 0x1_0000 => Ok(Decoded::Char(0x1_0000 + n, 4)),
        _ => Err(4),
    }
}

/// GB18030's four-byte sequence of a code point the two-byte table lacks.
fn encode_gb18030_four(c: u32) -> Option<([u8; 4], usize)> {
    let i = if c >= 0x1_0000 {
        if c > 0x10_FFFF {
            return None;
        }
        189_000 + c - 0x1_0000
    } else {
        // the ranges are sorted by index, not by code point
        (0..GB18030_RANGES.len()).find_map(|r| {
            let (first, cp) = GB18030_RANGES[r];
            let end = GB18030_RANGES
                .get(r + 1)
                .map_or(GB18030_BMP_END + 1, |&(next, _)| u32::from(next));
            let (first, cp) = (u32::from(first), u32::from(cp));
            (c >= cp && c - cp < end - first).then(|| first + c - cp)
        })?
    };
    let bytes = [
        0x81 + i / 12600,
        0x30 + i / 1260 % 10,
        0x81 + i / 10 % 126,
        0x30 + i % 10,
    ]
    .map(|b| b as u8);
    Some((bytes, 4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_tokens() {
        static DATA: [u8; 9] = [0x00, 0x41, 0x81, 0xDF, 0xFE, 0xE2, 0x00, 0x0B, 0x3F];
        let mut s = Stream::new(&DATA);
        let mut cells = Vec::new();
        while let Some(c) = s.next_cell() {
            cells.push(c);
        }
        assert_eq!(
            cells,
            [
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                None,
                None,
                // 3 - 1
                Some(2),
                Some(0x2_000B),
                Some(0x2_004B),
            ]
        );
    }

    #[test]
    fn grids_hold_their_cells() {
        for (i, plane) in PLANES.iter().enumerate() {
            let g = grid(i as u8);
            assert!(g.cells.iter().any(|&c| c != 0), "plane {i}");
            let rows: usize = plane
                .leads
                .iter()
                .map(|&(lo, hi)| usize::from(hi - lo) + 1)
                .sum();
            assert!(
                plane.window_of.is_empty() || plane.window_of.len() == rows,
                "plane {i}"
            );
        }
    }

    #[test]
    fn decoders_and_encoders_agree() {
        for cs in ALL {
            let spec = cs.spec();
            assert!(
                spec.single.windows(2).all(|w| w[0].1 < w[1].0),
                "{}",
                cs.name()
            );
            assert!(
                spec.encode.windows(2).all(|w| w[0].0 < w[1].0),
                "{}",
                cs.name()
            );
            // every character of a single byte encodes to a byte that decodes to it, or is an exception
            for &(lo, hi, base) in spec.single {
                for b in lo..=hi {
                    let c = base + u32::from(b - lo);
                    let (bytes, len) = cs.encode(c).expect("single bytes encode");
                    assert_eq!(
                        cs.decode(&bytes[..len], false),
                        Decoded::Char(c, len),
                        "{} {b:#x}",
                        cs.name()
                    );
                }
            }
        }
    }

    #[test]
    fn samples() {
        let dec = |cs: Multibyte, b: &[u8]| cs.decode(b, false);
        assert_eq!(dec(Multibyte::Sjis, b"\x82\xa0"), Decoded::Char(0x3042, 2));
        assert_eq!(dec(Multibyte::Sjis, b"\x5c"), Decoded::Char(0xA5, 1));
        assert_eq!(dec(Multibyte::Sjis, b"\x82"), Decoded::Incomplete);
        assert_eq!(
            dec(Multibyte::EucJp, b"\x8f\xb0\xa1"),
            Decoded::Char(0x4E02, 3)
        );
        assert_eq!(dec(Multibyte::EucJp, b"\x8e\xb1"), Decoded::Char(0xFF71, 2));
        assert_eq!(
            dec(Multibyte::Gb18030, b"\x81\x30\x81\x30"),
            Decoded::Char(0x80, 4)
        );
        assert_eq!(
            dec(Multibyte::Gb18030, b"\x90\x30\x81\x30"),
            Decoded::Char(0x1_0000, 4)
        );
        assert_eq!(
            dec(Multibyte::Gb18030, b"\x81\x30\x81"),
            Decoded::Incomplete
        );
        assert_eq!(
            dec(Multibyte::Big5Hkscs, b"\x88\x62"),
            Decoded::Pair(0xCA, 0x304, 2)
        );
        assert_eq!(
            dec(Multibyte::EucTw, b"\x8e\xa2\xa1\xa1"),
            Decoded::Char(0x4E42, 4)
        );
        assert_eq!(dec(Multibyte::Uhc, b"\x81\x41"), Decoded::Char(0xAC02, 2));
        assert_eq!(Multibyte::Uhc.decode(b"\xa2\xe8", true), Decoded::PastEnd);
        assert_eq!(
            Multibyte::EucJpMs.decode(b"\x8e\xe0", true),
            Decoded::Char(0xFFFD, 2)
        );
        assert_eq!(
            Multibyte::Sjis.encode(0x3042),
            Some(([0x82, 0xA0, 0, 0], 2))
        );
        assert_eq!(Multibyte::Sjis.encode(0x5C), Some(([0x5C, 0, 0, 0], 1)));
        assert_eq!(
            Multibyte::Cp932.encode(0x2252),
            Some(([0x81, 0xE0, 0, 0], 2))
        );
        assert_eq!(
            Multibyte::Gb18030.encode(0x80),
            Some(([0x81, 0x30, 0x81, 0x30], 4))
        );
        assert_eq!(
            Multibyte::Gb18030.encode(0x1F600),
            Some(([0x94, 0x39, 0xFC, 0x36], 4))
        );
        assert_eq!(
            Multibyte::EucJp.encode(0x4E02),
            Some(([0x8F, 0xB0, 0xA1, 0], 3))
        );
        assert_eq!(Multibyte::Sjis.encode(0x4E02), None);
        assert_eq!(Multibyte::EucKr.encode(0), Some(([0, 0, 0, 0], 1)));
    }
}
