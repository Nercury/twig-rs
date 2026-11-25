// Ported from PHPSRC/ext/mbstring/php_unicode.c and php_unicode.h (php-src 8.5.13),
// Copyright (c) The PHP Group, PHP License 3.01 (https://www.php.net/license/3_01.txt).
// php_unicode.c is based on code from ucdata-2.5, which has the following Copyright:
//
//   Copyright 2001 Computing Research Labs, New Mexico State University
//
//   Permission is hereby granted, free of charge, to any person obtaining a
//   copy of this software and associated documentation files (the "Software"),
//   to deal in the Software without restriction, including without limitation
//   the rights to use, copy, modify, merge, publish, distribute, sublicense,
//   and/or sell copies of the Software, and to permit persons to whom the
//   Software is furnished to do so, subject to the following conditions:
//
//   The above copyright notice and this permission notice shall be included in
//   all copies or substantial portions of the Software.

//! Case mapping with the tables and algorithm of PHP mbstring.
//!
//! Port of `PHPSRC/ext/mbstring/php_unicode.c` (PHP License 3.01): minimal-perfect-hash lookups
//! of the simple and full (special-casing) mappings, and `php_unicode_convert_case()` with its
//! context rules (final sigma, title case), which process the input in chunks of 64 decoded
//! code points exactly as PHP does.
//!
//! Notable properties of mbstring's case mapping (Unicode 17.0):
//!
//! - Only unconditional `SpecialCasing.txt` entries are used: no Turkish/Lithuanian rules
//!   (`ucgendat.php` skips conditional entries). `ß` uppercases to `SS`, `İ` lowercases to
//!   `i̇`, `ŉ` uppercases to `ʼN`, `ﬁ` to `FI`.
//! - The final-sigma rule (`Σ` → `ς` at the end of a word) is applied by the full lower-case
//!   and title-case conversions. Its look-behind only sees the current 64-code-point chunk and
//!   the not yet overwritten part of the previous chunk's converted output.
//! - Title case starts every word with the title-case form (falling back to upper case) and
//!   lowercases the rest; a word ends at any code point that is neither `Cased` nor
//!   `Case_Ignorable`.

use crate::props::{is_case_ignorable, is_cased};
use crate::tables::{
    UCCASE_EXTRA_TABLE, UCCASE_FOLD_G, UCCASE_FOLD_TABLE, UCCASE_LOWER_G, UCCASE_LOWER_TABLE,
    UCCASE_TITLE_G, UCCASE_TITLE_TABLE, UCCASE_UPPER_G, UCCASE_UPPER_TABLE,
};
use crate::utf8;

/// A case conversion: the `MB_CASE_*` constants of PHP, `php_case_mode` in
/// `PHPSRC/ext/mbstring/php_unicode.h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CaseMode {
    /// `MB_CASE_UPPER` (0): full upper-case mapping (`mb_strtoupper()`).
    Upper = 0,
    /// `MB_CASE_LOWER` (1): full lower-case mapping with the final-sigma rule
    /// (`mb_strtolower()`).
    Lower = 1,
    /// `MB_CASE_TITLE` (2): full title-case mapping with the final-sigma rule.
    Title = 2,
    /// `MB_CASE_FOLD` (3): full case folding.
    Fold = 3,
    /// `MB_CASE_UPPER_SIMPLE` (4): one-to-one upper-case mapping.
    UpperSimple = 4,
    /// `MB_CASE_LOWER_SIMPLE` (5): one-to-one lower-case mapping, no final sigma.
    LowerSimple = 5,
    /// `MB_CASE_TITLE_SIMPLE` (6): one-to-one title-case mapping, no final sigma.
    TitleSimple = 6,
    /// `MB_CASE_FOLD_SIMPLE` (7): one-to-one case folding.
    FoldSimple = 7,
}

impl CaseMode {
    /// Every mode, in `MB_CASE_*` order.
    pub const ALL: [CaseMode; 8] = [
        CaseMode::Upper,
        CaseMode::Lower,
        CaseMode::Title,
        CaseMode::Fold,
        CaseMode::UpperSimple,
        CaseMode::LowerSimple,
        CaseMode::TitleSimple,
        CaseMode::FoldSimple,
    ];

    /// The mode for a PHP `MB_CASE_*` value, or `None` outside `0..=7`
    /// (`PHP_UNICODE_CASE_MODE_MAX` is 8).
    pub const fn from_php(mode: i64) -> Option<CaseMode> {
        if mode < 0 || mode >= Self::ALL.len() as i64 {
            None
        } else {
            Some(Self::ALL[mode as usize])
        }
    }

    /// The PHP `MB_CASE_*` value of this mode.
    pub const fn to_php(self) -> i64 {
        self as i64
    }
}

/// `CODE_NOT_FOUND` of php_unicode.c.
const CODE_NOT_FOUND: u32 = u32::MAX;

/// Port of `mph_hash()` (PHPSRC/ext/mbstring/php_unicode.c).
#[inline]
const fn mph_hash(d: u32, x: u32) -> u32 {
    let x = x ^ d;
    ((x >> 16) ^ x).wrapping_mul(0x45d9f3b)
}

/// Port of `mph_lookup()` (PHPSRC/ext/mbstring/php_unicode.c): the "hash, displace and
/// compress" minimal perfect hash that `ucgendat.php` builds. `table` holds `[code, mapping]`
/// pairs.
#[inline]
fn mph_lookup(code: u32, g_table: &[i16], table: &[u32]) -> u32 {
    let g_table_size = g_table.len() as u32;
    let table_size = (table.len() / 2) as u32;
    let g = g_table[(mph_hash(0, code) % g_table_size) as usize];
    let idx = if g <= 0 {
        (-i32::from(g)) as u32
    } else {
        mph_hash(g as u32, code) % table_size
    } as usize;
    if table[2 * idx] == code {
        table[2 * idx + 1]
    } else {
        CODE_NOT_FOUND
    }
}

/// The four mapping tables (`CASE_LOOKUP(code, type)` in php_unicode.c).
#[derive(Clone, Copy)]
enum Table {
    Upper,
    Lower,
    Title,
    Fold,
}

#[inline]
fn case_lookup(code: u32, table: Table) -> u32 {
    match table {
        Table::Upper => mph_lookup(code, &UCCASE_UPPER_G, &UCCASE_UPPER_TABLE),
        Table::Lower => mph_lookup(code, &UCCASE_LOWER_G, &UCCASE_LOWER_TABLE),
        Table::Title => mph_lookup(code, &UCCASE_TITLE_G, &UCCASE_TITLE_TABLE),
        Table::Fold => mph_lookup(code, &UCCASE_FOLD_G, &UCCASE_FOLD_TABLE),
    }
}

/// Port of `php_unicode_toupper_raw()` (PHPSRC/ext/mbstring/php_unicode.c). A result above
/// `0xFFFFFF` encodes a special-casing sequence. `iso_8859_9` selects the Turkish rule PHP
/// applies when the string's encoding is ISO-8859-9 (`i` → `İ`).
fn toupper_raw(code: u32, iso_8859_9: bool) -> u32 {
    // After the ASCII characters, the first codepoint with an uppercase version is 0xB5
    // (MICRO SIGN).
    if code < 0xB5 {
        // Fast path for ASCII
        if (0x61..=0x7A).contains(&code) {
            if iso_8859_9 && code == 0x69 {
                return 0x130;
            }
            return code - 0x20;
        }
        code
    } else {
        let new_code = case_lookup(code, Table::Upper);
        if new_code != CODE_NOT_FOUND {
            return new_code;
        }
        code
    }
}

/// Port of `php_unicode_tolower_raw()` (PHPSRC/ext/mbstring/php_unicode.c). See
/// [`toupper_raw`] for the result encoding and `iso_8859_9` (`I` → `ı`, `İ` → `i`).
fn tolower_raw(code: u32, iso_8859_9: bool) -> u32 {
    // After the ASCII characters, the first codepoint with a lowercase version is 0xC0
    // (LATIN CAPITAL LETTER A WITH GRAVE).
    if code < 0xC0 {
        // Fast path for ASCII
        if (0x41..=0x5A).contains(&code) {
            if iso_8859_9 && code == 0x49 {
                return 0x131;
            }
            return code + 0x20;
        }
        code
    } else {
        let new_code = case_lookup(code, Table::Lower);
        if new_code != CODE_NOT_FOUND {
            if iso_8859_9 && code == 0x130 {
                return 0x69;
            }
            return new_code;
        }
        code
    }
}

/// Port of `php_unicode_totitle_raw()` (PHPSRC/ext/mbstring/php_unicode.c).
fn totitle_raw(code: u32, iso_8859_9: bool) -> u32 {
    let new_code = case_lookup(code, Table::Title);
    if new_code != CODE_NOT_FOUND {
        return new_code;
    }
    // No dedicated title-case variant, use to-upper instead
    toupper_raw(code, iso_8859_9)
}

/// Port of `php_unicode_tofold_raw()` (PHPSRC/ext/mbstring/php_unicode.c).
fn tofold_raw(code: u32, iso_8859_9: bool) -> u32 {
    if code < 0x80 {
        // Fast path for ASCII
        if (0x41..=0x5A).contains(&code) {
            if iso_8859_9 && code == 0x49 {
                return 0x131;
            }
            return code + 0x20;
        }
        code
    } else {
        let new_code = case_lookup(code, Table::Fold);
        if new_code != CODE_NOT_FOUND {
            if iso_8859_9 && code == 0x130 {
                return 0x69;
            }
            return new_code;
        }
        code
    }
}

/// Resolves a raw result to its simple mapping: the first entry of a special-casing sequence
/// (the `php_unicode_to*_simple()` helpers of php_unicode.c).
#[inline]
fn simple(code: u32) -> u32 {
    if code > 0xFF_FFFF {
        UCCASE_EXTRA_TABLE[(code & 0xFF_FFFF) as usize]
    } else {
        code
    }
}

/// Port of `php_unicode_tolower_simple()`.
#[inline]
fn tolower_simple(code: u32, iso_8859_9: bool) -> u32 {
    simple(tolower_raw(code, iso_8859_9))
}

/// Port of `php_unicode_toupper_simple()`.
#[inline]
fn toupper_simple(code: u32, iso_8859_9: bool) -> u32 {
    simple(toupper_raw(code, iso_8859_9))
}

/// Port of `php_unicode_totitle_simple()`.
#[inline]
fn totitle_simple(code: u32, iso_8859_9: bool) -> u32 {
    simple(totitle_raw(code, iso_8859_9))
}

/// Port of `php_unicode_tofold_simple()`.
#[inline]
fn tofold_simple(code: u32, iso_8859_9: bool) -> u32 {
    simple(tofold_raw(code, iso_8859_9))
}

/// Simple (one-to-one) upper-case mapping of a code point, as `MB_CASE_UPPER_SIMPLE`.
pub fn to_upper_simple(code: u32) -> u32 {
    toupper_simple(code, false)
}

/// Simple (one-to-one) lower-case mapping of a code point, as `MB_CASE_LOWER_SIMPLE`.
pub fn to_lower_simple(code: u32) -> u32 {
    tolower_simple(code, false)
}

/// Simple (one-to-one) title-case mapping of a code point (upper case if there is no
/// dedicated title-case form), as `MB_CASE_TITLE_SIMPLE` applies to the start of a word.
pub fn to_title_simple(code: u32) -> u32 {
    totitle_simple(code, false)
}

/// Simple (one-to-one) case folding of a code point, as `MB_CASE_FOLD_SIMPLE`.
pub fn to_fold_simple(code: u32) -> u32 {
    tofold_simple(code, false)
}

/// The full case mapping of one code point: one to three code points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FullMapping {
    len: u8,
    code_points: [u32; 3],
}

impl FullMapping {
    fn from_raw(raw: u32) -> FullMapping {
        let mut code_points = [0; 3];
        if raw > 0xFF_FFFF {
            let mut buf = [0; 3];
            let len = emit_special_casing_sequence(raw, &mut buf, 0);
            code_points[..len].copy_from_slice(&buf[..len]);
            FullMapping {
                len: len as u8,
                code_points,
            }
        } else {
            code_points[0] = raw;
            FullMapping {
                len: 1,
                code_points,
            }
        }
    }

    /// The mapped code points.
    pub fn as_slice(&self) -> &[u32] {
        &self.code_points[..usize::from(self.len)]
    }
}

impl std::ops::Deref for FullMapping {
    type Target = [u32];

    fn deref(&self) -> &[u32] {
        self.as_slice()
    }
}

/// Full upper-case mapping of a code point, including unconditional special casing
/// (`ß` → `SS`), as `MB_CASE_UPPER` maps it.
pub fn to_upper_full(code: u32) -> FullMapping {
    FullMapping::from_raw(toupper_raw(code, false))
}

/// Full lower-case mapping of a code point without context rules (`Σ` → `σ`; the final-sigma
/// rule needs the surrounding text, see [`convert_case`]).
pub fn to_lower_full(code: u32) -> FullMapping {
    FullMapping::from_raw(tolower_raw(code, false))
}

/// Full title-case mapping of a code point (`ǆ` → `ǅ`, `ß` → `Ss`), falling back to the full
/// upper-case mapping.
pub fn to_title_full(code: u32) -> FullMapping {
    FullMapping::from_raw(totitle_raw(code, false))
}

/// Full case folding of a code point (`ß` → `ss`), as `MB_CASE_FOLD` maps it.
pub fn to_fold_full(code: u32) -> FullMapping {
    FullMapping::from_raw(tofold_raw(code, false))
}

/// Port of `emit_special_casing_sequence()` (PHPSRC/ext/mbstring/php_unicode.c): writes the
/// full mapping that `w` refers to at `out[p..]` and returns the new write position.
#[inline]
fn emit_special_casing_sequence(w: u32, out: &mut [u32], mut p: usize) -> usize {
    let len = (w >> 24) as usize;
    let idx = (w & 0xFF_FFFF) as usize;
    // The entry at `idx` is the simple mapping; the sequence follows it.
    for &code in &UCCASE_EXTRA_TABLE[idx + 1..=idx + len] {
        out[p] = code;
        p += 1;
    }
    p
}

/// Port of `scan_ahead_for_cased_letter()` (PHPSRC/ext/mbstring/php_unicode.c): used when
/// determining whether special casing rules should be applied to Greek letter sigma. Decodes
/// the rest of the input in chunks of 64 code points; a code point that is both `Cased` and
/// `Case_Ignorable` counts as cased here.
fn scan_ahead_for_cased_letter(mut input: &[u8]) -> bool {
    let mut wchar_buf = [0u32; 64];
    while !input.is_empty() {
        let out_len = utf8::to_wchar(&mut input, &mut wchar_buf);
        for &w in &wchar_buf[..out_len] {
            if is_cased(w) {
                return true;
            }
            if !is_case_ignorable(w) {
                return false;
            }
        }
    }
    false
}

/// Port of `scan_back_for_cased_letter()` (PHPSRC/ext/mbstring/php_unicode.c): scans the
/// previous chunk's converted code points `buf[begin..end]` backwards (`end` is `None` before
/// the first chunk has been converted, the C code's `NULL`).
fn scan_back_for_cased_letter(buf: &[u32], begin: usize, end: Option<usize>) -> bool {
    if let Some(mut end) = end {
        while end > begin {
            end -= 1;
            let w = buf[end];
            if is_cased(w) {
                return true;
            }
            if !is_case_ignorable(w) {
                return false;
            }
        }
    }
    false
}

/// Decides whether the capital sigma at `wchar_buf[i]` is word-final: preceded (skipping
/// `Case_Ignorable` code points) by a `Cased` one and not followed (skipping `Case_Ignorable`
/// code points) by a `Cased` one. This is the condition `php_unicode_convert_case()` spells out
/// twice, for `PHP_UNICODE_CASE_LOWER` and `PHP_UNICODE_CASE_TITLE`, kept in its evaluation
/// order: the look-ahead only runs when the look-behind succeeded.
///
/// While the special casing rules say we should scan backwards through "any number" of
/// case-ignorable codepoints, PHP only scans back through the codepoints in `wchar_buf`, and
/// if it hits the beginning of the buffer, whatever codepoints have not yet been overwritten
/// in the latter part of `converted_buf`.
fn is_final_sigma(
    wchar_buf: &[u32],
    i: usize,
    converted_buf: &[u32],
    p: usize,
    converted_end: Option<usize>,
    rest: &[u8],
) -> bool {
    let out_len = wchar_buf.len();
    let mut j = i as isize - 1;
    while j >= 0 && is_case_ignorable(wchar_buf[j as usize]) {
        j -= 1;
    }
    let preceded_by_cased = if j >= 0 {
        is_cased(wchar_buf[j as usize])
    } else {
        scan_back_for_cased_letter(converted_buf, p, converted_end)
    };
    if !preceded_by_cased {
        return false;
    }
    // Now scan ahead to look for a cased letter
    let mut j = i + 1;
    while j < out_len && is_case_ignorable(wchar_buf[j]) {
        j += 1;
    }
    // If we hit the end of wchar_buf, convert more of the input string into codepoints and
    // continue scanning
    if j >= out_len {
        !scan_ahead_for_cased_letter(rest)
    } else {
        !is_cased(wchar_buf[j])
    }
}

/// Writes a raw mapping result at `out[p..]`: a special-casing sequence or one code point.
#[inline]
fn emit_raw(w: u32, out: &mut [u32], p: usize) -> usize {
    if w > 0xFF_FFFF {
        emit_special_casing_sequence(w, out, p)
    } else {
        out[p] = w;
        p + 1
    }
}

/// Converts the case of a UTF-8 byte string the way `mb_convert_case($input, $mode, 'UTF-8')`
/// does in PHP 8.5.13.
///
/// Port of `php_unicode_convert_case()` (PHPSRC/ext/mbstring/php_unicode.c) with UTF-8 as
/// source and destination encoding and mbstring's default illegal-character handling: every
/// ill-formed input sequence (see [`crate::utf8`]) is passed through and written as `?`; it
/// neither starts nor ends a word in title case. The input is decoded 64 code points at a time
/// into `wchar_buf`, converted into `converted_buf` (a code point can expand to at most three)
/// and encoded, which is what bounds the final-sigma look-behind.
pub fn convert_case(mode: CaseMode, input: &[u8]) -> Vec<u8> {
    const ISO_8859_9: bool = false;

    let mut wchar_buf = [0u32; 64];
    let mut converted_buf = [0u32; 192];
    let mut title_mode = false;
    let mut rest = input;
    // In rare cases, we need to scan backwards through the previously converted codepoints to
    // see if special conversion rules should be used for the Greek letter sigma.
    let mut converted_end: Option<usize> = None;

    let mut out = Vec::with_capacity(input.len() + 1);

    while !rest.is_empty() {
        let out_len = utf8::to_wchar(&mut rest, &mut wchar_buf);
        let wchars = &wchar_buf[..out_len];
        let mut p = 0usize;

        // In all cases, handle invalid characters early, as we assign special meaning to
        // codepoints > 0xFFFFFF.
        match mode {
            CaseMode::UpperSimple => {
                for &w in wchars {
                    converted_buf[p] = if w > 0xFF_FFFF {
                        w
                    } else {
                        toupper_simple(w, ISO_8859_9)
                    };
                    p += 1;
                }
            }
            CaseMode::LowerSimple => {
                for &w in wchars {
                    converted_buf[p] = if w > 0xFF_FFFF {
                        w
                    } else {
                        tolower_simple(w, ISO_8859_9)
                    };
                    p += 1;
                }
            }
            CaseMode::FoldSimple => {
                for &w in wchars {
                    converted_buf[p] = if w > 0xFF_FFFF {
                        w
                    } else {
                        tofold_simple(w, ISO_8859_9)
                    };
                    p += 1;
                }
            }
            CaseMode::TitleSimple => {
                for &w in wchars {
                    if w > 0xFF_FFFF {
                        converted_buf[p] = w;
                        p += 1;
                        continue;
                    }
                    converted_buf[p] = if title_mode {
                        tolower_simple(w, ISO_8859_9)
                    } else {
                        totitle_simple(w, ISO_8859_9)
                    };
                    p += 1;
                    if !is_case_ignorable(w) {
                        title_mode = is_cased(w);
                    }
                }
            }
            CaseMode::Upper => {
                for &w in wchars {
                    if w > 0xFF_FFFF {
                        converted_buf[p] = w;
                        p += 1;
                        continue;
                    }
                    p = emit_raw(toupper_raw(w, ISO_8859_9), &mut converted_buf, p);
                }
            }
            CaseMode::Lower => {
                for (i, &w) in wchars.iter().enumerate() {
                    if w > 0xFF_FFFF {
                        converted_buf[p] = w;
                        p += 1;
                        continue;
                    }
                    // For Greek capital letter sigma, there is a special casing rule; if it is
                    // the last letter in a word, it should be downcased to U+03C2 (GREEK SMALL
                    // LETTER FINAL SIGMA).
                    if w == 0x3A3
                        && is_final_sigma(wchars, i, &converted_buf, p, converted_end, rest)
                    {
                        converted_buf[p] = 0x3C2;
                        p += 1;
                        continue;
                    }
                    p = emit_raw(tolower_raw(w, ISO_8859_9), &mut converted_buf, p);
                }
            }
            CaseMode::Fold => {
                for &w in wchars {
                    if w > 0xFF_FFFF {
                        converted_buf[p] = w;
                        p += 1;
                        continue;
                    }
                    p = emit_raw(tofold_raw(w, ISO_8859_9), &mut converted_buf, p);
                }
            }
            CaseMode::Title => {
                for (i, &w) in wchars.iter().enumerate() {
                    if w > 0xFF_FFFF {
                        converted_buf[p] = w;
                        p += 1;
                        continue;
                    }
                    'convert: {
                        let w2 = if title_mode {
                            if w == 0x3A3
                                && is_final_sigma(wchars, i, &converted_buf, p, converted_end, rest)
                            {
                                converted_buf[p] = 0x3C2;
                                p += 1;
                                // `goto set_title_mode`
                                break 'convert;
                            }
                            tolower_raw(w, ISO_8859_9)
                        } else {
                            totitle_raw(w, ISO_8859_9)
                        };
                        p = emit_raw(w2, &mut converted_buf, p);
                    }
                    // set_title_mode:
                    if !is_case_ignorable(w) {
                        title_mode = is_cased(w);
                    }
                }
            }
        }

        converted_end = Some(p);
        debug_assert!(p <= converted_buf.len());
        utf8::from_wchar(&converted_buf[..p], &mut out);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::{UCCASE_FOLD_TABLE, UCCASE_LOWER_TABLE, UCCASE_TITLE_TABLE};

    fn conv(mode: CaseMode, s: &str) -> String {
        String::from_utf8(convert_case(mode, s.as_bytes())).unwrap()
    }

    #[test]
    fn every_table_entry_is_found_by_its_hash() {
        let tables: [(Table, &[u32]); 4] = [
            (Table::Upper, &UCCASE_UPPER_TABLE),
            (Table::Lower, &UCCASE_LOWER_TABLE),
            (Table::Title, &UCCASE_TITLE_TABLE),
            (Table::Fold, &UCCASE_FOLD_TABLE),
        ];
        for (table, entries) in tables {
            for &[code, mapping] in entries.as_chunks::<2>().0 {
                assert_eq!(case_lookup(code, table), mapping, "{code:#x}");
            }
        }
    }

    #[test]
    fn mode_numbers_match_php() {
        for (i, mode) in CaseMode::ALL.iter().enumerate() {
            assert_eq!(mode.to_php(), i as i64);
            assert_eq!(CaseMode::from_php(i as i64), Some(*mode));
        }
        assert_eq!(CaseMode::from_php(-1), None);
        assert_eq!(CaseMode::from_php(8), None);
    }

    #[test]
    fn special_casing() {
        assert_eq!(conv(CaseMode::Upper, "straße ǆ ﬁ ŉ"), "STRASSE Ǆ FI ʼN");
        assert_eq!(conv(CaseMode::Lower, "İSTANBUL ΣΑΣ ǅ"), "i̇stanbul σας ǆ");
        assert_eq!(conv(CaseMode::UpperSimple, "straße"), "STRAßE");
        assert_eq!(conv(CaseMode::Fold, "Straße"), "strasse");
        assert_eq!(conv(CaseMode::FoldSimple, "Straße"), "straße");
        assert_eq!(to_upper_full(u32::from('ß')).as_slice(), [0x53, 0x53]);
        assert_eq!(to_title_full(u32::from('ß')).as_slice(), [0x53, 0x73]);
        assert_eq!(to_title_simple(u32::from('ǆ')), u32::from('ǅ'));
        assert_eq!(to_lower_full(0x130).as_slice(), [0x69, 0x307]);
        assert_eq!(to_lower_simple(0x130), 0x69);
    }

    #[test]
    fn title_case_words() {
        assert_eq!(
            conv(
                CaseMode::Title,
                "hello wORLD o'neil mc-donald ǆungla straße"
            ),
            "Hello World O'neil Mc-Donald ǅungla Straße"
        );
        assert_eq!(
            conv(CaseMode::Title, "a.b,c_d1e 2f\tg's x’y «z» ŉx"),
            "A.b,C_D1E 2F\tG's X’y «Z» ʼNx"
        );
        assert_eq!(conv(CaseMode::TitleSimple, "ßa ǆungla"), "ßa ǅungla");
    }

    #[test]
    fn invalid_input_becomes_question_marks() {
        assert_eq!(convert_case(CaseMode::Upper, b"a\xFFb\xE2\x82"), b"A?B?");
        assert_eq!(convert_case(CaseMode::Title, b"a\xFFb"), b"A?b");
        assert_eq!(convert_case(CaseMode::Title, b"\xFFab"), b"?Ab");
    }

    #[test]
    fn final_sigma_look_behind_is_limited_to_the_previous_chunk() {
        // "a", k-1 apostrophes (Case_Ignorable), then a capital sigma at index k.
        for k in 1..140 {
            let s = format!("a{}Σ", "'".repeat(k - 1));
            let lower = conv(CaseMode::Lower, &s);
            let expected = if k <= 64 { 'ς' } else { 'σ' };
            assert_eq!(lower.chars().last(), Some(expected), "k = {k}");
        }
    }
}
