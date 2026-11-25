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

//! Character properties from mbstring's Unicode 17.0 tables.
//!
//! Port of the property lookups of `PHPSRC/ext/mbstring/php_unicode.c` and
//! `PHPSRC/ext/mbstring/php_unicode.h` (PHP License 3.01) over the converted
//! `unicode_data.h` tables.

use crate::tables::{UCPROP_OFFSETS, UCPROP_RANGES, UCPROP_SIZE};

/// A property in mbstring's tables: the `UC_*` constants of `PHPSRC/ext/mbstring/php_unicode.h`.
///
/// General categories, bidirectional classes and two derived core properties share one
/// numbering, as in `ucgendat.php`. Quirks of the generator carry over: punctuation categories
/// are merged into [`Prop::P`], `Cc`/`Cf` into [`Prop::C`], the bidi classes `BN`, `NSM`, `PDF`,
/// `LRE`, `LRO`, `RLE`, `RLO`, `LRI`, `RLI`, `FSI` and `PDI` into [`Prop::On`], and unassigned
/// code points are not listed, so [`Prop::Cn`] matches nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Prop {
    /// `UC_MN`: Mark, Non-Spacing.
    Mn = 0,
    /// `UC_MC`: Mark, Spacing Combining.
    Mc = 1,
    /// `UC_ME`: Mark, Enclosing.
    Me = 2,
    /// `UC_ND`: Number, Decimal Digit.
    Nd = 3,
    /// `UC_NL`: Number, Letter.
    Nl = 4,
    /// `UC_NO`: Number, Other.
    No = 5,
    /// `UC_ZS`: Separator, Space.
    Zs = 6,
    /// `UC_ZL`: Separator, Line.
    Zl = 7,
    /// `UC_ZP`: Separator, Paragraph.
    Zp = 8,
    /// `UC_OS`: Other, Surrogate (general category `Cs`).
    Os = 9,
    /// `UC_CO`: Other, Private Use.
    Co = 10,
    /// `UC_CN`: Other, Not Assigned (no code point is listed).
    Cn = 11,
    /// `UC_LU`: Letter, Uppercase.
    Lu = 12,
    /// `UC_LL`: Letter, Lowercase.
    Ll = 13,
    /// `UC_LT`: Letter, Titlecase.
    Lt = 14,
    /// `UC_LM`: Letter, Modifier.
    Lm = 15,
    /// `UC_LO`: Letter, Other.
    Lo = 16,
    /// `UC_SM`: Symbol, Math.
    Sm = 17,
    /// `UC_SC`: Symbol, Currency.
    Sc = 18,
    /// `UC_SK`: Symbol, Modifier.
    Sk = 19,
    /// `UC_SO`: Symbol, Other.
    So = 20,
    /// `UC_L`: bidi Left-To-Right.
    L = 21,
    /// `UC_R`: bidi Right-To-Left.
    R = 22,
    /// `UC_EN`: bidi European Number.
    En = 23,
    /// `UC_ES`: bidi European Number Separator.
    Es = 24,
    /// `UC_ET`: bidi European Number Terminator.
    Et = 25,
    /// `UC_AN`: bidi Arabic Number.
    An = 26,
    /// `UC_CS`: bidi Common Number Separator.
    Cs = 27,
    /// `UC_B`: bidi Block (Paragraph) Separator.
    B = 28,
    /// `UC_S`: bidi Segment Separator.
    S = 29,
    /// `UC_WS`: bidi Whitespace.
    Ws = 30,
    /// `UC_ON`: bidi Other Neutrals (plus the merged classes listed on [`Prop`]).
    On = 31,
    /// `UC_AL`: bidi Arabic Letter.
    Al = 32,
    /// `UC_C`: Control and Format (`Cc` and `Cf` merged).
    C = 33,
    /// `UC_P`: Punctuation (`Pc`, `Pd`, `Ps`, `Pe`, `Po`, `Pi`, `Pf` merged).
    P = 34,
    /// `UC_CASED`: derived property `Cased`.
    Cased = 35,
    /// `UC_CASE_IGNORABLE`: derived property `Case_Ignorable`.
    CaseIgnorable = 36,
}

impl Prop {
    /// Every property in table order.
    pub const ALL: [Prop; UCPROP_SIZE] = [
        Prop::Mn,
        Prop::Mc,
        Prop::Me,
        Prop::Nd,
        Prop::Nl,
        Prop::No,
        Prop::Zs,
        Prop::Zl,
        Prop::Zp,
        Prop::Os,
        Prop::Co,
        Prop::Cn,
        Prop::Lu,
        Prop::Ll,
        Prop::Lt,
        Prop::Lm,
        Prop::Lo,
        Prop::Sm,
        Prop::Sc,
        Prop::Sk,
        Prop::So,
        Prop::L,
        Prop::R,
        Prop::En,
        Prop::Es,
        Prop::Et,
        Prop::An,
        Prop::Cs,
        Prop::B,
        Prop::S,
        Prop::Ws,
        Prop::On,
        Prop::Al,
        Prop::C,
        Prop::P,
        Prop::Cased,
        Prop::CaseIgnorable,
    ];

    /// The inclusive code point ranges listed for this property, in ascending order.
    pub fn ranges(self) -> impl ExactSizeIterator<Item = (u32, u32)> + Clone {
        let n = self as usize;
        let start = usize::from(UCPROP_OFFSETS[n]);
        let end = usize::from(UCPROP_OFFSETS[n + 1]);
        let (pairs, rest) = UCPROP_RANGES[start..end].as_chunks::<2>();
        debug_assert!(rest.is_empty());
        pairs.iter().map(|&[first, last]| (first, last))
    }
}

/// Port of `prop_lookup()` (PHPSRC/ext/mbstring/php_unicode.c): binary search over the range
/// pairs of property `n`, keeping `m` on the start of a pair.
fn prop_lookup(code: u32, n: usize) -> bool {
    let mut l = i64::from(UCPROP_OFFSETS[n]);
    let mut r = i64::from(UCPROP_OFFSETS[n + 1]) - 1;
    while l <= r {
        // Determine a "mid" point and adjust to make sure the mid point is at the beginning
        // of a range pair.
        let mut m = (l + r) >> 1;
        m -= m & 1;
        if code > UCPROP_RANGES[(m + 1) as usize] {
            l = m + 2;
        } else if code < UCPROP_RANGES[m as usize] {
            r = m - 2;
        } else {
            return true;
        }
    }
    false
}

/// Returns whether `code` has property `prop`.
///
/// Port of `php_unicode_is_prop1()` (PHPSRC/ext/mbstring/php_unicode.c). Values that are not
/// code points (such as [`crate::utf8::BAD_INPUT`]) have no property.
#[inline]
pub fn is_prop(code: u32, prop: Prop) -> bool {
    prop_lookup(code, prop as usize)
}

/// Returns whether `code` has at least one of `props`, testing them in order.
///
/// Port of `php_unicode_is_prop()` (PHPSRC/ext/mbstring/php_unicode.c), whose variadic list
/// ends with `-1`.
pub fn is_any_prop(code: u32, props: &[Prop]) -> bool {
    props.iter().any(|&prop| prop_lookup(code, prop as usize))
}

/// Returns whether `code` is `Cased` (`php_unicode_is_cased()`, PHPSRC/ext/mbstring/php_unicode.h).
#[inline]
pub fn is_cased(code: u32) -> bool {
    is_prop(code, Prop::Cased)
}

/// Returns whether `code` is `Case_Ignorable` (`php_unicode_is_case_ignorable()`,
/// PHPSRC/ext/mbstring/php_unicode.h).
#[inline]
pub fn is_case_ignorable(code: u32) -> bool {
    is_prop(code, Prop::CaseIgnorable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_numbering_matches_the_table_order() {
        for (i, prop) in Prop::ALL.iter().enumerate() {
            assert_eq!(*prop as usize, i);
        }
    }

    #[test]
    fn binary_search_finds_every_range_boundary() {
        for prop in Prop::ALL {
            let ranges: Vec<_> = prop.ranges().collect();
            let contains = |code: u32| ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&code));
            for &(lo, hi) in &ranges {
                let probes = [
                    lo.wrapping_sub(1),
                    lo,
                    lo + (hi - lo) / 2,
                    hi,
                    hi.saturating_add(1),
                ];
                for code in probes {
                    assert_eq!(is_prop(code, prop), contains(code), "{code:#x} {prop:?}");
                }
            }
            for code in [0, 0x10_FFFF, 0x11_0000, crate::utf8::BAD_INPUT] {
                assert_eq!(is_prop(code, prop), contains(code), "{code:#x} {prop:?}");
            }
        }
    }

    #[test]
    fn ranges_are_sorted_and_disjoint() {
        for prop in Prop::ALL {
            let ranges: Vec<_> = prop.ranges().collect();
            assert!(ranges.iter().all(|(lo, hi)| lo <= hi), "{prop:?}");
            assert!(ranges.windows(2).all(|w| w[0].1 < w[1].0), "{prop:?}");
        }
        assert_eq!(Prop::Cn.ranges().len(), 0);
    }

    #[test]
    fn spot_checks() {
        assert!(is_cased(u32::from('a')) && is_cased(u32::from('Σ')) && is_cased(0x345));
        assert!(!is_cased(u32::from('1')) && !is_cased(u32::from('\'')));
        assert!(is_case_ignorable(u32::from('\'')) && is_case_ignorable(u32::from('.')));
        assert!(is_case_ignorable(0x345) && is_case_ignorable(0x2019));
        assert!(!is_case_ignorable(u32::from('-')) && !is_case_ignorable(u32::from('a')));
        assert!(is_prop(u32::from('A'), Prop::Lu) && is_prop(u32::from('a'), Prop::Ll));
        assert!(is_any_prop(u32::from('5'), &[Prop::Lu, Prop::Nd]));
        assert!(!is_any_prop(u32::from('5'), &[Prop::Lu, Prop::Ll]));
        assert!(!is_cased(crate::utf8::BAD_INPUT));
        assert!(!is_case_ignorable(crate::utf8::BAD_INPUT));
    }
}
