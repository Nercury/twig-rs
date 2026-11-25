//! Array builtins (reference/php/stdlib.md §9.4, §10, §12): sorting (`zend_sort`), `range()`,
//! `min()`/`max()`, the array functions Twig's runtime calls, and PHP's seeded MT19937 randomness.
//!
//! Every function ports its php-src counterpart (`PHPSRC/ext/standard/array.c`,
//! `PHPSRC/Zend/zend_sort.c`, `PHPSRC/Zend/zend_hash.c`, `PHPSRC/ext/random/`,
//! `PHPSRC/ext/spl/spl_iterators.c`) in upstream order, with PHP's coercive-mode argument parsing
//! and verbatim `TypeError`/`ValueError`/`ArgumentCountError` texts. Oracle vectors under
//! `tests/vectors/array/` pin the behaviour.
//!
//! # Map
//!
//! | area | functions |
//! |---|---|
//! | sorting | [`zend_sort`], [`zend_insert_sort`], [`sort`], [`rsort`], [`usort`], [`asort`], [`arsort`], [`uasort`], [`ksort`], [`krsort`], [`uksort`], [`natsort`], [`natcasesort`], [`strnatcmp_ex`], the `SORT_*` flags |
//! | ranges | [`range`] (also Twig's `a..b`), [`range_call`] |
//! | extremes | [`min`], [`max`] (regular calls), [`min_frameless`], [`max_frameless`] (direct two-argument calls) |
//! | arrays | [`array_merge`], [`array_chunk`], [`array_column`], [`array_slice`], [`array_keys`], [`array_values`], [`array_reverse`], [`array_pad`], [`array_fill_keys`], [`array_flip`], [`array_intersect_key`], [`array_diff_key`], [`array_key_first`], [`array_key_last`], [`array_is_list`], [`current`] |
//! | searching | [`in_array`], [`array_search`], [`array_key_exists`] |
//! | callbacks | [`array_map`], [`array_filter`], [`array_reduce`], with [`Callback`] and [`object_callback`] |
//! | iteration | [`iterator_to_array`], [`iterator_count`] (arrays and `Traversable` objects); `count()` is [`dim::count`](crate::dim::count) |
//! | randomness | [`Mt19937`], [`mt_rand`], [`mt_rand_range`], [`mt_rand_call`], [`mt_srand`], [`shuffle`], [`array_rand`], [`array_pick_keys`], [`random_int`] |
//!
//! # Conventions
//!
//! * Functions that Twig (or compiled templates) call with template values take `&Value`
//!   arguments and parse them like the internal function does (`Z_PARAM_*`), so the PHP messages
//!   come out verbatim: `array_slice(): Argument #2 ($offset) must be of type int, string given`,
//!   `array_slice(): Passing null to parameter #4 ($preserve_keys) of type bool is deprecated`.
//!   Optional parameters are `Option<&Value>`; `None` means "not passed" (the default applies),
//!   `Some(&Value::Null)` is an explicit `null`.
//! * The sort functions work in place on `&mut Array` like PHP's by-reference parameter; on an
//!   exception the array is left unchanged (PHP leaves it partially sorted, which Twig never
//!   observes because its `sort` filter sorts a local copy).
//! * Callbacks (`usort()`'s comparator, `array_filter()`'s predicate, ...) are [`Callback`]s:
//!   Rust closures receiving the positional arguments and the diagnostics sink. An exception they
//!   return aborts the builtin immediately, like PHP's `if (EG(exception))` checks.
//! * Random functions take the generator explicitly ([`Mt19937`]); PHP keeps one per request.
//!
//! # Known deviations
//!
//! * **Hash layout.** [`Array`](crate::Array) does not expose PHP's packed/hash layout or the
//!   tombstones `unset()` leaves (`nNumUsed`). So `array_rand($a, 1)` on an array with removed
//!   elements can draw a different key than PHP (PHP samples table slots), and the "return the
//!   input as it is" shortcuts of `array_merge()`/`array_values()` treat every list as packed.
//!   Templates cannot remove array elements, so only host data can hit this.
//! * **Enum cases in sorts.** `SORT_REGULAR` orders two distinct enum cases by object address in
//!   PHP (to group equal cases), which depends on the heap. The stand-in orders by descending
//!   [`ObjectRef::id`](crate::ObjectRef::id), which reproduces PHP's usual order for cases created
//!   in declaration order (`enum_cases('Suit')|sort`); equal cases always group.
//! * **Nested user sorts.** PHP keeps the "Returning bool from comparison function" flag in a
//!   global that a nested `usort()` call resets; here each call has its own flag, so a comparator
//!   that itself sorts can see one deprecation fewer.
//! * **Exceptions during a sort** leave the array unchanged (PHP finishes the sort with the
//!   original-order fallback and keeps the result; Twig's `sort` filter discards it).
//! * **`memory_limit`.** `range()` reproduces PHP's fatal error for the default 128M limit as an
//!   `Error` (see [`range`]); other memory exhaustion is not modelled.
//! * **Unseeded randomness.** [`Mt19937::from_entropy`] and `mt_srand(null)` are seeded from the
//!   standard library's keyed hasher instead of the CSPRNG; [`random_int`] takes its 64-bit draws
//!   from the caller.

#![warn(missing_docs)]

mod callback;
mod functions;
mod minmax;
mod random;
mod range;
mod sort;

pub use callback::{Callback, object_callback};
pub use functions::{
    ARRAY_FILTER_USE_BOTH, ARRAY_FILTER_USE_KEY, array_chunk, array_column, array_diff_key,
    array_fill_keys, array_filter, array_flip, array_intersect_key, array_is_list,
    array_key_exists, array_key_first, array_key_last, array_keys, array_map, array_merge,
    array_pad, array_reduce, array_reverse, array_search, array_slice, array_values, current,
    in_array, iterator_count, iterator_to_array,
};
pub use minmax::{max, max_frameless, min, min_frameless};
pub use random::{
    MT_RAND_MAX, MT_RAND_MT19937, MT_RAND_PHP, Mt19937, MtMode, array_pick_keys, array_rand,
    mt_rand, mt_rand_call, mt_rand_range, mt_srand, random_int, shuffle,
};
pub use range::{range, range_call};
pub use sort::{
    SORT_FLAG_CASE, SORT_LOCALE_STRING, SORT_NATURAL, SORT_NUMERIC, SORT_REGULAR, SORT_STRING,
    arsort, asort, krsort, ksort, natcasesort, natsort, rsort, sort, strnatcmp_ex, uasort, uksort,
    usort, zend_insert_sort, zend_sort,
};

/// `HT_MAX_SIZE` on 64-bit builds (PHPSRC/Zend/zend_types.h): the largest number of elements a
/// PHP array can hold. `range()`, `array_merge()` and `array_pad()` check against it.
pub const HT_MAX_SIZE: u32 = 0x4000_0000;
