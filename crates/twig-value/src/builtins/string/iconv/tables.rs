//! Access to the measured multi-byte tables (`cjk_tables.rs`, `data/*.bin`) and the encoder maps
//! derived from them: the inverse of a decoding table plus the measured deviations, built once
//! per process on first use.

use std::sync::OnceLock;

use super::cjk_tables as t;

/// A table file of `cjk_tables.gen.php`: an 8-byte header (value width, lead range, trail
/// range), a row index per lead byte, then the rows.
#[derive(Clone, Copy)]
pub(super) struct Table(pub(super) &'static [u8]);

impl Table {
    fn width(self) -> usize {
        usize::from(self.0[0])
    }

    fn ranges(self) -> (u8, u8, u8, u8) {
        (self.0[1], self.0[2], self.0[3], self.0[4])
    }

    /// The value at `(lead, trail)`, 0 for none (also outside the table's ranges).
    pub(super) fn get(self, lead: u8, trail: u8) -> u32 {
        let (ll, lh, tl, th) = self.ranges();
        if lead < ll || lead > lh || trail < tl || trail > th {
            return 0;
        }
        let li = 8 + 2 * usize::from(lead - ll);
        let row = u16::from_le_bytes([self.0[li], self.0[li + 1]]);
        if row == 0xFFFF {
            return 0;
        }
        let rows_start = 8 + 2 * (usize::from(lh - ll) + 1);
        let cols = usize::from(th - tl) + 1;
        let w = self.width();
        let off = rows_start + (usize::from(row) * cols + usize::from(trail - tl)) * w;
        let b = &self.0[off..off + w];
        if w == 2 {
            u32::from(u16::from_le_bytes([b[0], b[1]]))
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        }
    }

    /// Every `(lead, trail, value)` with a value, in increasing byte order.
    pub(super) fn entries(self) -> impl Iterator<Item = (u8, u8, u32)> {
        let (ll, lh, tl, th) = self.ranges();
        (ll..=lh).flat_map(move |l| {
            (tl..=th).filter_map(move |c| match self.get(l, c) {
                0 => None,
                v => Some((l, c, v)),
            })
        })
    }
}

pub(super) const JIS0208: Table = Table(t::JIS0208);
pub(super) const JIS0212: Table = Table(t::JIS0212);
pub(super) const GB2312: Table = Table(t::GB2312);
pub(super) const KSC5601: Table = Table(t::KSC5601);
pub(super) const SJIS: Table = Table(t::SJIS);
pub(super) const CP932: Table = Table(t::CP932);
pub(super) const GBK: Table = Table(t::GBK);
pub(super) const GB18030: Table = Table(t::GB18030);
pub(super) const UHC: Table = Table(t::UHC);
pub(super) const BIG5: Table = Table(t::BIG5);
pub(super) const BIG5HKSCS: Table = Table(t::BIG5HKSCS);
pub(super) const EUCJPMS: Table = Table(t::EUCJPMS);
pub(super) const EUCJPMS_3: Table = Table(t::EUCJPMS_3);
pub(super) const JISX0213_1: Table = Table(t::JISX0213_1);
pub(super) const JISX0213_2: Table = Table(t::JISX0213_2);

/// GB18030's four-byte sequences of the BMP: the code point of linear index `idx` (0..0x99FC),
/// 0 for none.
pub(super) fn gb18030_four(idx: u32) -> u32 {
    let runs = t::GB18030_4;
    let n = runs.len() / 12;
    let run = |i: usize| {
        let r = &runs[12 * i..12 * i + 12];
        let w = |k: usize| u32::from_le_bytes([r[k], r[k + 1], r[k + 2], r[k + 3]]);
        (w(0), w(4), w(8))
    };
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let (start, cp, len) = run(mid);
        if idx < start {
            hi = mid;
        } else if idx >= start + len {
            lo = mid + 1;
        } else {
            return cp + (idx - start);
        }
    }
    0
}

/// The bytes of GB18030's four-byte sequence with linear index `idx`.
pub(super) fn gb18030_four_bytes(idx: u32) -> [u8; 4] {
    [
        (idx / 12600 + 0x81) as u8,
        (idx / 1260 % 10 + 0x30) as u8,
        (idx / 10 % 126 + 0x81) as u8,
        (idx % 10 + 0x30) as u8,
    ]
}

/// A code point to byte sequence map (at most four bytes), sorted by code point.
pub(super) struct EncodeMap {
    entries: Vec<(u32, u8, [u8; 4])>,
}

impl EncodeMap {
    /// Builds the map from `(code point, bytes)` candidates in priority order (the first
    /// candidate of a code point wins), then applies `deviations` (`b""` records a code point
    /// the encoder rejects).
    pub(super) fn build(
        candidates: impl Iterator<Item = (u32, Vec<u8>)>,
        deviations: &[(u32, &[u8])],
    ) -> EncodeMap {
        let mut entries: Vec<(u32, u8, [u8; 4])> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (cp, bytes) in candidates {
            if seen.insert(cp) {
                entries.push(pack(cp, &bytes));
            }
        }
        entries.sort_unstable_by_key(|e| e.0);
        for &(cp, bytes) in deviations {
            match entries.binary_search_by_key(&cp, |e| e.0) {
                Ok(i) => entries[i] = pack(cp, bytes),
                Err(i) => entries.insert(i, pack(cp, bytes)),
            }
        }
        EncodeMap { entries }
    }

    /// The bytes of `cp`: `None` when the map has no entry, `Some(b"")` when a deviation says
    /// the encoder rejects it.
    pub(super) fn lookup(&self, cp: u32) -> Option<&[u8]> {
        self.entries
            .binary_search_by_key(&cp, |e| e.0)
            .ok()
            .map(|i| {
                let (_, len, ref bytes) = self.entries[i];
                &bytes[..usize::from(len)]
            })
    }

    /// The bytes of `cp`, if mapped.
    pub(super) fn get(&self, cp: u32) -> Option<&[u8]> {
        self.lookup(cp).filter(|b| !b.is_empty())
    }
}

fn pack(cp: u32, bytes: &[u8]) -> (u32, u8, [u8; 4]) {
    let mut b = [0u8; 4];
    b[..bytes.len()].copy_from_slice(bytes);
    (cp, bytes.len() as u8, b)
}

/// The encoder of a 94x94 coded character set inside ISO-2022 (`ucs4_to_jisx0208()` and
/// friends): the inverse of the table (lowest position first) with the measured deviations.
pub(super) fn set_encoder(
    table: Table,
    deviations: &'static [(u32, &'static [u8])],
    cell: &'static OnceLock<EncodeMap>,
) -> &'static EncodeMap {
    cell.get_or_init(|| {
        EncodeMap::build(
            table.entries().map(|(r, c, cp)| (cp, vec![r, c])),
            deviations,
        )
    })
}
