//! Sorting: ports of `zend_sort()` / `zend_insert_sort()` (PHPSRC/Zend/zend_sort.c), the bucket
//! sort of `zend_hash_sort_internal()` (PHPSRC/Zend/zend_hash.c) and the sort functions of
//! PHPSRC/ext/standard/array.c with their comparison functions and sort flags.
//!
//! * `zend_sort()` is a hybrid: insertion sort (with sorting networks for 2-5 elements) up to 16
//!   elements, otherwise a libc++-style quicksort with a median of 3 (median of 5 from 1024
//!   elements) that recurses into the smaller partition. PHP's comparison is not a total order
//!   across types, so results for mixed arrays depend on this exact sequence of comparisons.
//! * Sorting is stable since PHP 8.0: elements remember their original position and ties
//!   (comparison result `0`) are ordered by it (`stable_sort_fallback()`); the reverse variants
//!   negate the comparison but keep the fallback ascending.
//! * `asort()`-style functions keep keys and leave `nNextFreeElement` alone; `sort()`-style
//!   functions renumber to a list.

use std::cmp::Ordering;

use super::callback::Callback;
use crate::array::Array;
use crate::compare::{self, UNCOMPARABLE};
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::key::ArrayKey;
use crate::numeric;
use crate::string::PhpString;
use crate::value::Value;

/// `SORT_REGULAR`: compare with `zend_compare()` (PHP's `<=>`).
pub const SORT_REGULAR: i64 = 0;
/// `SORT_NUMERIC`: compare as floats (`zval_get_double()`).
pub const SORT_NUMERIC: i64 = 1;
/// `SORT_STRING`: compare as strings, bytewise (`zend_binary_strcmp()`).
pub const SORT_STRING: i64 = 2;
/// `SORT_LOCALE_STRING`: compare as strings with `strcoll()` (the oracle's `LC_COLLATE` is `C`,
/// so bytewise up to the first NUL byte).
pub const SORT_LOCALE_STRING: i64 = 5;
/// `SORT_NATURAL`: natural order (`strnatcmp_ex()`).
pub const SORT_NATURAL: i64 = 6;
/// `SORT_FLAG_CASE`: combined with `SORT_STRING` or `SORT_NATURAL` for ASCII case-insensitive
/// comparison.
pub const SORT_FLAG_CASE: i64 = 8;

/// A comparison function of the sort algorithm: only the sign of the result matters (`> 0` means
/// "out of order"); an `Err` aborts the sort.
type Cmp<'a, T> = dyn FnMut(&T, &T) -> PhpResult<i32> + 'a;

/// Port of `zend_sort_2()`.
fn sort2<T>(b: &mut [T], x: usize, y: usize, cmp: &mut Cmp<'_, T>) -> PhpResult<()> {
    if cmp(&b[x], &b[y])? > 0 {
        b.swap(x, y);
    }
    Ok(())
}

/// Port of `zend_sort_3()`.
fn sort3<T>(b: &mut [T], x: usize, y: usize, z: usize, cmp: &mut Cmp<'_, T>) -> PhpResult<()> {
    if !(cmp(&b[x], &b[y])? > 0) {
        if !(cmp(&b[y], &b[z])? > 0) {
            return Ok(());
        }
        b.swap(y, z);
        if cmp(&b[x], &b[y])? > 0 {
            b.swap(x, y);
        }
        return Ok(());
    }
    if !(cmp(&b[z], &b[y])? > 0) {
        b.swap(x, z);
        return Ok(());
    }
    b.swap(x, y);
    if cmp(&b[y], &b[z])? > 0 {
        b.swap(y, z);
    }
    Ok(())
}

/// Port of `zend_sort_4()`.
fn sort4<T>(
    b: &mut [T],
    x: usize,
    y: usize,
    z: usize,
    w: usize,
    cmp: &mut Cmp<'_, T>,
) -> PhpResult<()> {
    sort3(b, x, y, z, cmp)?;
    if cmp(&b[z], &b[w])? > 0 {
        b.swap(z, w);
        if cmp(&b[y], &b[z])? > 0 {
            b.swap(y, z);
            if cmp(&b[x], &b[y])? > 0 {
                b.swap(x, y);
            }
        }
    }
    Ok(())
}

/// Port of `zend_sort_5()`.
fn sort5<T>(
    b: &mut [T],
    x: usize,
    y: usize,
    z: usize,
    w: usize,
    v: usize,
    cmp: &mut Cmp<'_, T>,
) -> PhpResult<()> {
    sort4(b, x, y, z, w, cmp)?;
    if cmp(&b[w], &b[v])? > 0 {
        b.swap(w, v);
        if cmp(&b[z], &b[w])? > 0 {
            b.swap(z, w);
            if cmp(&b[y], &b[z])? > 0 {
                b.swap(y, z);
                if cmp(&b[x], &b[y])? > 0 {
                    b.swap(x, y);
                }
            }
        }
    }
    Ok(())
}

/// Port of `zend_insert_sort()` (PHPSRC/Zend/zend_sort.c): sorting networks for up to five
/// elements; otherwise straight insertion for the first six elements and, after that, a backwards
/// search in steps of two followed by a fix-up comparison.
///
/// `cmp(a, b) > 0` means `a` must come after `b`. The exact sequence of `cmp` calls matches
/// upstream, which matters for inconsistent comparators. An `Err` from `cmp` stops the sort and is
/// returned (the slice is then partially sorted).
pub fn zend_insert_sort<T>(
    base: &mut [T],
    cmp: &mut dyn FnMut(&T, &T) -> PhpResult<i32>,
) -> PhpResult<()> {
    let nmemb = base.len();
    match nmemb {
        0 | 1 => Ok(()),
        2 => sort2(base, 0, 1, cmp),
        3 => sort3(base, 0, 1, 2, cmp),
        4 => sort4(base, 0, 1, 2, 3, cmp),
        5 => sort5(base, 0, 1, 2, 3, 4, cmp),
        _ => {
            let start = 0;
            let end = nmemb;
            let sentry = 6;
            let mut i = start + 1;
            while i < sentry {
                let mut j = i - 1;
                if !(cmp(&base[j], &base[i])? > 0) {
                    i += 1;
                    continue;
                }
                while j != start {
                    j -= 1;
                    if !(cmp(&base[j], &base[i])? > 0) {
                        j += 1;
                        break;
                    }
                }
                let mut k = i;
                while k > j {
                    base.swap(k, k - 1);
                    k -= 1;
                }
                i += 1;
            }
            let mut i = sentry;
            while i < end {
                let mut j = i - 1;
                if !(cmp(&base[j], &base[i])? > 0) {
                    i += 1;
                    continue;
                }
                loop {
                    // j >= start + 3 on entry and j > start + 1 when looping, so this never
                    // goes below `start`.
                    j -= 2;
                    if !(cmp(&base[j], &base[i])? > 0) {
                        j += 1;
                        if !(cmp(&base[j], &base[i])? > 0) {
                            j += 1;
                        }
                        break;
                    }
                    if j == start {
                        break;
                    }
                    if j == start + 1 {
                        j -= 1;
                        if cmp(&base[i], &base[j])? > 0 {
                            j += 1;
                        }
                        break;
                    }
                }
                let mut k = i;
                while k > j {
                    base.swap(k, k - 1);
                    k -= 1;
                }
                i += 1;
            }
            Ok(())
        }
    }
}

/// Port of `zend_sort()` (PHPSRC/Zend/zend_sort.c, derived from libc++'s `std::sort`): insertion
/// sort ([`zend_insert_sort`]) for up to 16 elements; otherwise the median of the first, middle
/// and last element (of five elements from 1024 elements on) becomes the pivot, the range is
/// partitioned, the smaller part is sorted recursively and the larger one iteratively.
///
/// `cmp(a, b) > 0` means `a` must come after `b`; the exact sequence of calls matches upstream.
/// The algorithm is not stable by itself: PHP's sort functions make it stable by breaking ties on
/// the original position inside `cmp`. An `Err` from `cmp` stops the sort and is returned (the
/// slice is then partially sorted).
pub fn zend_sort<T>(
    base: &mut [T],
    cmp: &mut dyn FnMut(&T, &T) -> PhpResult<i32>,
) -> PhpResult<()> {
    let mut lo = 0;
    let mut nmemb = base.len();
    loop {
        if nmemb <= 16 {
            return zend_insert_sort(&mut base[lo..lo + nmemb], cmp);
        }
        let b = &mut base[lo..lo + nmemb];
        let end = nmemb;
        let offset = nmemb >> 1;
        let pivot = offset;
        if nmemb >> 10 != 0 {
            let delta = offset >> 1;
            sort5(b, 0, delta, pivot, pivot + delta, end - 1, cmp)?;
        } else {
            sort3(b, 0, pivot, end - 1, cmp)?;
        }
        b.swap(1, pivot);
        let pivot = 1;
        let mut i = pivot + 1;
        let mut j = end - 1;
        'partition: loop {
            while cmp(&b[pivot], &b[i])? > 0 {
                i += 1;
                if i == j {
                    break 'partition;
                }
            }
            j -= 1;
            if j == i {
                break 'partition;
            }
            while cmp(&b[j], &b[pivot])? > 0 {
                j -= 1;
                if j == i {
                    break 'partition;
                }
            }
            b.swap(i, j);
            i += 1;
            if i == j {
                break 'partition;
            }
        }
        b.swap(pivot, i - 1);
        if i - 1 < end - i {
            zend_sort(&mut b[..i - 1], cmp)?;
            lo += i;
            nmemb = end - i;
        } else {
            zend_sort(&mut b[i..], cmp)?;
            nmemb = i - 1;
        }
    }
}

/// A `Bucket` during a sort: key, value and the original position (`Z_EXTRA(val)`).
struct Bucket {
    key: ArrayKey,
    val: Value,
    order: usize,
}

/// Port of `stable_sort_fallback()` (PHPSRC/ext/standard/array.c): ties keep the original order.
fn stable_sort_fallback(a: &Bucket, b: &Bucket) -> i32 {
    match a.order.cmp(&b.order) {
        Ordering::Greater => 1,
        Ordering::Less => -1,
        Ordering::Equal => 0,
    }
}

/// The entry point a sort function uses, which decides when a packed table becomes a hash.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    /// `zend_array_sort()`: converts packed tables to hashes before anything else.
    ArraySort,
    /// `zend_hash_sort()`: leaves tables with fewer than two elements alone.
    HashSort,
}

/// An array with the same entries as a hash table (`zend_hash_packed_to_hash()`), keeping
/// `nNextFreeElement`.
fn rebuild_hash(entries: impl Iterator<Item = (ArrayKey, Value)>, next_free: Option<i64>) -> Array {
    let mut out = Array::new_hash();
    for (k, v) in entries {
        out.insert(k, v);
    }
    out.set_next_free(next_free);
    out
}

/// Port of `zend_hash_sort_internal()` (PHPSRC/Zend/zend_hash.c) with the stable wrapper
/// (`RETURN_STABLE_SORT`): the buckets are sorted with [`zend_sort`]; `renumber` turns the result
/// into a list (`nNextFreeElement` = count), otherwise keys and `nNextFreeElement` are kept and
/// the table is a hash.
///
/// Known deviation: an emptied packed table that PHP turns into a hash in `zend_array_sort_ex()`
/// stays as it is (unobservable apart from `nNextFreeElement` corner cases).
fn hash_sort(
    array: &mut Array,
    renumber: bool,
    entry: Entry,
    cmp: &mut Cmp<'_, Bucket>,
) -> PhpResult<()> {
    let n = array.len();
    if !(n > 1) && !(renumber && n > 0) {
        // Doesn't require sorting; zend_array_sort_ex() has already converted a packed table.
        if entry == Entry::ArraySort && n == 1 {
            let next = array.next_free();
            let rebuilt = rebuild_hash(array.iter().map(|(k, v)| (k.clone(), v.clone())), next);
            *array = rebuilt;
        }
        return Ok(());
    }
    let mut buckets: Vec<Bucket> = array
        .iter()
        .enumerate()
        .map(|(order, (k, v))| Bucket {
            key: k.clone(),
            val: v.clone(),
            order,
        })
        .collect();
    zend_sort(&mut buckets, &mut |a: &Bucket, b: &Bucket| {
        let r = cmp(a, b)?;
        Ok(if r != 0 {
            r
        } else {
            stable_sort_fallback(a, b)
        })
    })?;
    *array = if renumber {
        Array::from_values(buckets.into_iter().map(|b| b.val))
    } else {
        rebuild_hash(
            buckets.into_iter().map(|b| (b.key, b.val)),
            array.next_free(),
        )
    };
    Ok(())
}

/// `ZEND_THREEWAY_COMPARE()` on doubles (NaN compares as `1`).
fn three_way(a: f64, b: f64) -> i32 {
    if a == b {
        0
    } else if a < b {
        -1
    } else {
        1
    }
}

/// `ZEND_THREEWAY_COMPARE()` on lengths.
fn three_way_len(a: usize, b: usize) -> i32 {
    match a.cmp(&b) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Port of `zend_binary_strcasecmp()` (PHPSRC/Zend/zend_operators.c): bytewise after ASCII
/// lowercasing (`zend_tolower_ascii`), then the lengths.
fn binary_strcasecmp(s1: &[u8], s2: &[u8]) -> i32 {
    for (a, b) in s1.iter().zip(s2) {
        let (c1, c2) = (
            i32::from(a.to_ascii_lowercase()),
            i32::from(b.to_ascii_lowercase()),
        );
        if c1 != c2 {
            return c1 - c2;
        }
    }
    three_way_len(s1.len(), s2.len())
}

/// `strcoll()` in the `C` locale: `strcmp()` of the C strings (up to the first NUL byte).
fn c_strcoll(s1: &[u8], s2: &[u8]) -> i32 {
    let c = |s: &[u8]| -> usize { memchr::memchr(0, s).unwrap_or(s.len()) };
    match s1[..c(s1)].cmp(&s2[..c(s2)]) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// C `isspace()` in the C locale.
fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// Port of `compare_right()` (PHPSRC/ext/standard/strnatcmp.c): two right-aligned digit runs; the
/// longer run wins, else the first differing digit.
fn compare_right(a: &[u8], ap: &mut usize, b: &[u8], bp: &mut usize) -> i32 {
    let mut bias = 0;
    loop {
        let a_digit = *ap < a.len() && a[*ap].is_ascii_digit();
        let b_digit = *bp < b.len() && b[*bp].is_ascii_digit();
        if !a_digit && !b_digit {
            return bias;
        } else if !a_digit {
            return -1;
        } else if !b_digit {
            return 1;
        } else if a[*ap] < b[*bp] {
            if bias == 0 {
                bias = -1;
            }
        } else if a[*ap] > b[*bp] && bias == 0 {
            bias = 1;
        }
        *ap += 1;
        *bp += 1;
    }
}

/// Port of `compare_left()` (PHPSRC/ext/standard/strnatcmp.c): two left-aligned digit runs (a
/// fraction or leading zeros); the first differing digit wins.
fn compare_left(a: &[u8], ap: &mut usize, b: &[u8], bp: &mut usize) -> i32 {
    loop {
        let a_digit = *ap < a.len() && a[*ap].is_ascii_digit();
        let b_digit = *bp < b.len() && b[*bp].is_ascii_digit();
        if !a_digit && !b_digit {
            return 0;
        } else if !a_digit {
            return -1;
        } else if !b_digit {
            return 1;
        } else if a[*ap] < b[*bp] {
            return -1;
        } else if a[*ap] > b[*bp] {
            return 1;
        }
        *ap += 1;
        *bp += 1;
    }
}

/// Port of `strnatcmp_ex()` (PHPSRC/ext/standard/strnatcmp.c, Martin Pool's natural order
/// comparison as modified for PHP): leading zeros of the first number are skipped, whitespace is
/// skipped, digit runs compare by value (left-aligned when one starts with `0`), everything else
/// bytewise (ASCII-uppercased with `case_insensitive`). Returns `-1`, `0` or `1`.
///
/// Like the C code, which reads the strings' NUL terminators, a byte past the end reads as `0`.
pub fn strnatcmp_ex(a: &[u8], b: &[u8], case_insensitive: bool) -> i32 {
    if a.is_empty() || b.is_empty() {
        return if a.len() == b.len() {
            0
        } else if a.len() > b.len() {
            1
        } else {
            -1
        };
    }
    let at = |s: &[u8], i: usize| -> u8 { s.get(i).copied().unwrap_or(0) };
    let (aend, bend) = (a.len(), b.len());
    let (mut ap, mut bp) = (0, 0);
    let (mut ca, mut cb) = (a[0], b[0]);

    // skip over leading zeros
    while ca == b'0' && ap + 1 < aend && a[ap + 1].is_ascii_digit() {
        ap += 1;
        ca = a[ap];
    }
    while cb == b'0' && bp + 1 < bend && b[bp + 1].is_ascii_digit() {
        bp += 1;
        cb = b[bp];
    }

    loop {
        // Skip consecutive whitespace
        while is_c_space(ca) {
            ap += 1;
            ca = at(a, ap);
        }
        while is_c_space(cb) {
            bp += 1;
            cb = at(b, bp);
        }

        // process run of digits
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let fractional = ca == b'0' || cb == b'0';
            let result = if fractional {
                compare_left(a, &mut ap, b, &mut bp)
            } else {
                compare_right(a, &mut ap, b, &mut bp)
            };
            if result != 0 {
                return result;
            } else if ap == aend && bp == bend {
                // End of the strings. Let caller sort them out.
                return 0;
            } else if ap == aend {
                return -1;
            } else if bp == bend {
                return 1;
            } else {
                // Keep on comparing from the current point.
                ca = a[ap];
                cb = b[bp];
            }
        }

        if case_insensitive {
            ca = ca.to_ascii_uppercase();
            cb = cb.to_ascii_uppercase();
        }

        match ca.cmp(&cb) {
            Ordering::Less => return -1,
            Ordering::Greater => return 1,
            Ordering::Equal => {}
        }

        ap += 1;
        bp += 1;
        if ap >= aend && bp >= bend {
            // The strings compare the same. Perhaps the caller will want to call strcmp to
            // break the tie.
            return 0;
        } else if ap >= aend {
            return -1;
        } else if bp >= bend {
            return 1;
        }

        ca = a[ap];
        cb = b[bp];
    }
}

/// The comparison selected by a sort flag value (`php_get_data_compare_func()` /
/// `php_get_key_compare_func()`: `flags & ~SORT_FLAG_CASE`, unknown values mean `SORT_REGULAR`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Regular,
    Numeric,
    String,
    StringCase,
    Natural,
    NaturalCase,
    Locale,
}

fn kind(flags: i64) -> Kind {
    let case = flags & SORT_FLAG_CASE != 0;
    match flags & !SORT_FLAG_CASE {
        SORT_NUMERIC => Kind::Numeric,
        SORT_STRING if case => Kind::StringCase,
        SORT_STRING => Kind::String,
        SORT_NATURAL if case => Kind::NaturalCase,
        SORT_NATURAL => Kind::Natural,
        SORT_LOCALE_STRING => Kind::Locale,
        _ => Kind::Regular,
    }
}

/// Port of `php_array_data_compare_unstable_i()`: `zend_compare()` plus the enum rule of
/// `array_unique()`/sorting: when the right operand is an enum case and the pair is uncomparable,
/// two enum cases are ordered by object address and a non-enum left operand sorts first (`-1`).
///
/// Addresses depend on PHP's heap state and cannot be reproduced. Observed with PHP 8.5.13: case
/// objects are created on first access, recycled heap slots usually give a later case the lower
/// address, and sorting `Suit::cases()` (Twig's `enum_cases()`) of `enum Suit { case Hearts; case
/// Spades; case Clubs; }` yields Clubs, Spades, Hearts, the reverse declaration order (other
/// allocation histories give other orders). Cases here exist from class creation in declaration
/// order, so the stand-in orders by *descending* object id, reproducing that common order.
fn data_compare_regular(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i32> {
    let result = compare::compare(a, b, diag)?;
    if let Value::Object(rhs) = b
        && result == UNCOMPARABLE
        && rhs.class().is_enum()
    {
        if let Value::Object(lhs) = a
            && lhs.class().is_enum()
        {
            // Order doesn't matter, we just need to group the same enum values
            return Ok(match lhs.id().cmp(&rhs.id()) {
                Ordering::Equal => 0,
                // created later: lower address
                Ordering::Greater => -1,
                Ordering::Less => 1,
            });
        }
        // Shift enums to the end of the array
        return Ok(-1);
    }
    Ok(result)
}

/// Ports of `string_compare_function()` / `string_case_compare_function()`
/// (PHPSRC/Zend/zend_operators.c): strings as they are, other values through string conversion
/// (`zval_get_tmp_string()`, with its warnings and `__toString()` calls), then bytewise.
fn string_compare(
    a: &Value,
    b: &Value,
    case_insensitive: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i32> {
    let cmp = |x: &[u8], y: &[u8]| {
        if case_insensitive {
            binary_strcasecmp(x, y)
        } else {
            compare::binary_strcmp(x, y)
        }
    };
    if let (Value::String(x), Value::String(y)) = (a, b) {
        if PhpString::ptr_eq(x, y) {
            return Ok(0);
        }
        return Ok(cmp(x, y));
    }
    let s1 = convert::to_string(a, diag)?;
    let s2 = convert::to_string(b, diag)?;
    Ok(cmp(&s1, &s2))
}

/// The data comparison of a sort flag (`php_array_data_compare_*_unstable_i()`).
fn data_compare(kind: Kind, a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i32> {
    match kind {
        Kind::Regular => data_compare_regular(a, b, diag),
        // numeric_compare_function()
        Kind::Numeric => {
            let d1 = convert::to_double(a, diag)?;
            let d2 = convert::to_double(b, diag)?;
            Ok(three_way(d1, d2))
        }
        Kind::String => string_compare(a, b, false, diag),
        Kind::StringCase => string_compare(a, b, true, diag),
        // php_array_natural_general_compare()
        Kind::Natural | Kind::NaturalCase => {
            let s1 = convert::to_string(a, diag)?;
            let s2 = convert::to_string(b, diag)?;
            Ok(strnatcmp_ex(&s1, &s2, kind == Kind::NaturalCase))
        }
        // string_locale_compare_function()
        Kind::Locale => {
            let s1 = convert::to_string(a, diag)?;
            let s2 = convert::to_string(b, diag)?;
            Ok(c_strcoll(&s1, &s2))
        }
    }
}

/// A key as the string the key comparisons print it to (`zend_print_long_to_buf()` for ints).
fn key_bytes(k: &ArrayKey) -> PhpString {
    k.to_php_string()
}

/// A key as a double for `SORT_NUMERIC` (`zend_strtod()` of string keys).
fn key_double(k: &ArrayKey) -> f64 {
    match k {
        ArrayKey::Int(h) => *h as f64,
        ArrayKey::Str(s) => numeric::strtod(s).0,
    }
}

/// The key comparison of a sort flag (`php_array_key_compare_*_unstable_i()` and
/// `php_array_key_compare_string_natural_general()`).
fn key_compare(
    kind: Kind,
    a: &ArrayKey,
    b: &ArrayKey,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i32> {
    Ok(match kind {
        Kind::Regular => match (a, b) {
            (ArrayKey::Int(x), ArrayKey::Int(y)) => {
                if x > y {
                    1
                } else {
                    -1
                }
            }
            (ArrayKey::Str(x), ArrayKey::Str(y)) => compare::smart_strcmp(x, y),
            _ => compare::compare(&a.to_value(), &b.to_value(), diag)?,
        },
        Kind::Numeric => match (a, b) {
            (ArrayKey::Int(x), ArrayKey::Int(y)) => {
                if x > y {
                    1
                } else {
                    -1
                }
            }
            _ => three_way(key_double(a), key_double(b)),
        },
        Kind::String => compare::binary_strcmp(&key_bytes(a), &key_bytes(b)),
        Kind::StringCase => binary_strcasecmp(&key_bytes(a), &key_bytes(b)),
        Kind::Natural | Kind::NaturalCase => {
            strnatcmp_ex(&key_bytes(a), &key_bytes(b), kind == Kind::NaturalCase)
        }
        Kind::Locale => c_strcoll(&key_bytes(a), &key_bytes(b)),
    })
}

/// The reversed key comparison: `* -1`, except the natural variants, which swap the operands
/// (`php_array_reverse_key_compare_string_natural*()`).
fn reverse_key_compare(
    kind: Kind,
    a: &ArrayKey,
    b: &ArrayKey,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i32> {
    match kind {
        Kind::Natural | Kind::NaturalCase => key_compare(kind, b, a, diag),
        _ => Ok(-key_compare(kind, a, b, diag)?),
    }
}

/// `ZEND_NORMALIZE_BOOL()`.
fn normalize(n: i64) -> i32 {
    match n.cmp(&0) {
        Ordering::Greater => 1,
        Ordering::Less => -1,
        Ordering::Equal => 0,
    }
}

/// Port of `php_get_long()` (PHPSRC/ext/standard/array.c): ints as they are, everything else
/// through `zval_get_long()`, so floats are truncated silently (`0.5` is a tie), out-of-range
/// floats warn and wrap, numeric prefixes count, objects give `1` with a notice.
fn php_get_long(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        other => convert::to_long(other, diag),
    }
}

/// Port of `php_array_user_compare_unstable()` / `php_array_user_key_compare_unstable()`: calls the
/// comparator with `(a, b)`; a `bool` result raises the deprecation once per sort call and `false`
/// retries with `(b, a)` and negates; the result is normalised to `-1`/`0`/`1`.
fn user_compare(
    function: &str,
    callback: &mut Callback<'_>,
    a: &Value,
    b: &Value,
    deprecation_thrown: &mut bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i32> {
    let retval = callback(&[a.clone(), b.clone()], diag)?;
    if let Value::Bool(result) = retval {
        if !*deprecation_thrown {
            diag.deprecated(format!(
                "{function}(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero"
            ))?;
            *deprecation_thrown = true;
        }
        if !result {
            // Retry with swapped operands.
            let retval = callback(&[b.clone(), a.clone()], diag)?;
            let ret = php_get_long(&retval, diag)?;
            return Ok(-normalize(ret));
        }
    }
    let ret = php_get_long(&retval, diag)?;
    Ok(normalize(ret))
}

/// Port of `php_usort()` (PHPSRC/ext/standard/array.c): an empty array is left alone; otherwise
/// the (copied) array is sorted with the user comparator through `zend_array_sort()`. The
/// deprecation flag is reset per call, like `PHP_ARRAY_CMP_FUNC_BACKUP()`.
fn php_usort(
    function: &str,
    array: &mut Array,
    callback: &mut Callback<'_>,
    renumber: bool,
    by_key: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    if array.is_empty() {
        return Ok(());
    }
    let mut deprecation_thrown = false;
    hash_sort(
        array,
        renumber,
        Entry::ArraySort,
        &mut |x: &Bucket, y: &Bucket| {
            if by_key {
                user_compare(
                    function,
                    callback,
                    &x.key.to_value(),
                    &y.key.to_value(),
                    &mut deprecation_thrown,
                    diag,
                )
            } else {
                user_compare(
                    function,
                    callback,
                    &x.val,
                    &y.val,
                    &mut deprecation_thrown,
                    diag,
                )
            }
        },
    )
}

/// Port of `sort($array, $flags)` (PHPSRC/ext/standard/array.c): sorts the values and renumbers
/// the keys (`0, 1, …`). `flags` is one of the `SORT_*` constants (unknown values mean
/// [`SORT_REGULAR`]); ties keep their original order.
pub fn sort(array: &mut Array, flags: i64, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    let kind = kind(flags);
    hash_sort(
        array,
        true,
        Entry::ArraySort,
        &mut |a: &Bucket, b: &Bucket| data_compare(kind, &a.val, &b.val, diag),
    )
}

/// Port of `rsort($array, $flags)`: [`sort`] in descending order (the comparison negated; ties
/// still keep their original order).
pub fn rsort(array: &mut Array, flags: i64, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    let kind = kind(flags);
    hash_sort(
        array,
        true,
        Entry::ArraySort,
        &mut |a: &Bucket, b: &Bucket| Ok(-data_compare(kind, &a.val, &b.val, diag)?),
    )
}

/// Port of `usort($array, $callback)`: sorts the values with a user comparator and renumbers the
/// keys. The comparator's result goes through `zval_get_long()` (floats truncate silently: a
/// comparator returning `$a - $b` on `[1.5, 1.2]` reports a tie); returning a `bool` raises
/// `usort(): Returning bool from comparison function is deprecated, return an integer less than,
/// equal to, or greater than zero` once per call and `false` triggers a second call with swapped
/// arguments.
pub fn usort(
    array: &mut Array,
    callback: &mut Callback<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    php_usort("usort", array, callback, true, false, diag)
}

/// Port of `asort($array, $flags)`: sorts the values and keeps the keys (Twig's `sort` filter
/// without an arrow). `nNextFreeElement` is kept.
pub fn asort(array: &mut Array, flags: i64, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    let kind = kind(flags);
    hash_sort(
        array,
        false,
        Entry::ArraySort,
        &mut |a: &Bucket, b: &Bucket| data_compare(kind, &a.val, &b.val, diag),
    )
}

/// Port of `arsort($array, $flags)`: [`asort`] in descending order.
pub fn arsort(array: &mut Array, flags: i64, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    let kind = kind(flags);
    hash_sort(
        array,
        false,
        Entry::ArraySort,
        &mut |a: &Bucket, b: &Bucket| Ok(-data_compare(kind, &a.val, &b.val, diag)?),
    )
}

/// Port of `uasort($array, $callback)`: [`usort`] keeping the keys (Twig's `sort` filter with an
/// arrow).
pub fn uasort(
    array: &mut Array,
    callback: &mut Callback<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    php_usort("uasort", array, callback, false, false, diag)
}

/// Port of `ksort($array, $flags)`: sorts by key. With [`SORT_REGULAR`] int keys compare
/// numerically, string keys with PHP's smart string comparison and mixed pairs with
/// `zend_compare()`.
pub fn ksort(array: &mut Array, flags: i64, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    let kind = kind(flags);
    hash_sort(
        array,
        false,
        Entry::HashSort,
        &mut |a: &Bucket, b: &Bucket| key_compare(kind, &a.key, &b.key, diag),
    )
}

/// Port of `krsort($array, $flags)`: [`ksort`] in descending order.
pub fn krsort(array: &mut Array, flags: i64, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    let kind = kind(flags);
    hash_sort(
        array,
        false,
        Entry::HashSort,
        &mut |a: &Bucket, b: &Bucket| reverse_key_compare(kind, &a.key, &b.key, diag),
    )
}

/// Port of `uksort($array, $callback)`: sorts by key with a user comparator that receives the keys
/// (ints or strings); otherwise like [`uasort`].
pub fn uksort(
    array: &mut Array,
    callback: &mut Callback<'_>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    php_usort("uksort", array, callback, false, true, diag)
}

/// Port of `natsort($array)`: sorts the values in natural order ([`strnatcmp_ex`] of their string
/// forms), keeping the keys.
pub fn natsort(array: &mut Array, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    asort(array, SORT_NATURAL, diag)
}

/// Port of `natcasesort($array)`: [`natsort`] ignoring ASCII case.
pub fn natcasesort(array: &mut Array, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    asort(array, SORT_NATURAL | SORT_FLAG_CASE, diag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(a: &Array) -> Vec<i64> {
        a.values().map(|v| v.as_int().unwrap()).collect()
    }

    #[test]
    fn zend_sort_sorts_and_reports_errors() {
        for n in [0usize, 1, 2, 3, 5, 6, 7, 16, 17, 100, 1023, 1024, 3000] {
            let mut v: Vec<i64> = (0..n as i64).map(|i| (i * 7919) % 1000).collect();
            zend_sort(&mut v, &mut |a: &i64, b: &i64| Ok(a.cmp(b) as i32)).unwrap();
            assert!(v.windows(2).all(|w| w[0] <= w[1]), "n = {n}");
        }
        let mut v: Vec<i64> = (0..40).rev().collect();
        let mut calls = 0;
        let err = zend_sort(&mut v, &mut |a: &i64, b: &i64| {
            calls += 1;
            if calls == 10 {
                Err(crate::PhpError::exception("stop"))
            } else {
                Ok(a.cmp(b) as i32)
            }
        })
        .unwrap_err();
        assert_eq!(err.message(), "stop");
        assert_eq!(calls, 10);
    }

    #[test]
    fn stable_and_keyed() {
        let mut d = Vec::new();
        let mut a: Array = vec![
            ("x", Value::Int(2)),
            ("y", Value::Int(1)),
            ("z", Value::Int(2)),
            ("w", Value::Int(1)),
        ]
        .into_iter()
        .collect();
        asort(&mut a, SORT_REGULAR, &mut d).unwrap();
        let keys: Vec<String> = a.keys().map(|k| k.to_string()).collect();
        assert_eq!(keys, ["y", "w", "x", "z"]);
        arsort(&mut a, SORT_REGULAR, &mut d).unwrap();
        let keys: Vec<String> = a.keys().map(|k| k.to_string()).collect();
        assert_eq!(keys, ["x", "z", "y", "w"]);
        let mut l: Array = [3, 1, 2].into_iter().map(Value::from).collect();
        rsort(&mut l, SORT_REGULAR, &mut d).unwrap();
        assert_eq!(ints(&l), [3, 2, 1]);
        assert!(l.is_list());
        assert!(d.is_empty());
    }

    #[test]
    fn enum_cases_order() {
        use crate::object::ClassBuilder;
        // PHP 8.5.13 (oracle, fresh process): enum Suit: string { case Hearts = 'H'; case Spades
        // = 'S'; case Clubs = 'C'; } $a = [Suit::Clubs, Suit::Hearts, Suit::Spades, Suit::Hearts];
        // asort($a); gives keys 0, 2, 1, 3 (Clubs, Spades, Hearts, Hearts); sorting
        // array_reverse(Suit::cases()) gives Clubs, Spades, Hearts too.
        let suit = ClassBuilder::enumeration("Suit", Some(crate::object::EnumBacking::String))
            .case("Hearts", Some(Value::from("H")))
            .case("Spades", Some(Value::from("S")))
            .case("Clubs", Some(Value::from("C")))
            .build();
        let case = |n: &str| Value::Object(suit.enum_case(n).unwrap());
        let mut a: Array = vec![
            case("Clubs"),
            case("Hearts"),
            case("Spades"),
            case("Hearts"),
        ]
        .into_iter()
        .collect();
        let mut d = Vec::new();
        asort(&mut a, SORT_REGULAR, &mut d).unwrap();
        let keys: Vec<i64> = a.keys().map(|k| k.as_int().unwrap()).collect();
        assert_eq!(keys, [0, 2, 1, 3]);
        // a non-enum left operand sorts before an enum case
        let mut b: Array = vec![case("Hearts"), Value::Int(1), Value::from("a")]
            .into_iter()
            .collect();
        sort(&mut b, SORT_REGULAR, &mut d).unwrap();
        assert!(matches!(b.get_int(2), Some(Value::Object(_))));
        assert!(d.is_empty());
    }

    #[test]
    fn natural_order() {
        let c = |a: &str, b: &str| strnatcmp_ex(a.as_bytes(), b.as_bytes(), false);
        assert_eq!(c("img12", "img10"), 1);
        assert_eq!(c("img2", "img10"), -1);
        assert_eq!(c("a  b", "a b"), 0);
        // Verified with PHP 8.5.13 (the oracle vectors cover many more pairs).
        assert_eq!(c("0.5", "0.25"), -1);
        assert_eq!(c("", "a"), -1);
        assert_eq!(c("007", "7"), 0);
        assert_eq!(c("a ", "a"), 1);
        assert_eq!(c("x1 ", "x1\t"), 1);
        assert_eq!(strnatcmp_ex(b"ABC", b"abd", true), -1);
    }
}
