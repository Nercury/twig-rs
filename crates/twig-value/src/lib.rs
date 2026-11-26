//! PHP-exact value model (zval semantics) and the PHP engine conversions Twig relies on.
//!
//! Behaviour mirrors **PHP 8.5.13** (64-bit, default ini: `precision=14`,
//! `serialize_precision=-1`); every ported function names its php-src origin (`PHPSRC/...`), and
//! the facts are collected in `reference/php/engine-semantics.md` and `reference/php/stdlib.md` of
//! the repository. Oracle-generated vectors under `tests/vectors/` pin the behaviour.
//!
//! # Map of the crate
//!
//! | module | contents |
//! |---|---|
//! | [`Value`], [`TypeTag`], [`Number`] | the zval: `null`, `bool`, `int`, `float`, byte `string`, `array`, `object` |
//! | [`PhpString`] | immutable, reference-counted byte strings (not necessarily UTF-8) |
//! | [`Array`], [`ArrayKey`], [`KeyRef`], [`KeyContext`] | ordered hash maps with PHP key normalisation, `nNextFreeElement`, copy-on-write |
//! | [`dim`] | `$c[$k]`, `$c[$k] ?? …`, `isset()`/`empty()` on offsets for every container type, string offsets, `count()` |
//! | [`object`] | the [`Object`](object::Object) protocol, [`ObjectRef`] handles, class entries, [`ObjectBuilder`](object::ObjectBuilder), `stdClass`, `Closure`, enums, `ArrayObject`, `ArrayIterator` |
//! | [`PhpError`], [`ThrowableClass`] | thrown PHP exceptions (class + verbatim message) |
//! | [`Diagnostics`], [`Diagnostic`], [`Level`] | the sink for warnings, notices and deprecations; [`Collect`], [`Ignore`], [`Throw`] |
//! | [`convert`] | truthiness and the `(int)`, `(float)`, `(string)`, `(array)`, `(object)` casts, operand conversions, float→int rules |
//! | [`numeric`] | numeric strings (`_is_numeric_string_ex`) and `zend_strtod` |
//! | [`float`] | float formatting: `precision` (echo) and `serialize_precision` (`var_dump`, `var_export`, `json_encode`) |
//! | [`param`] | coercive-mode parameter passing: internal functions (`zend_parse_arg_*_weak`) and userland typed parameters (`zend_check_type`) |
//! | [`types`] | type names for messages (`zend_zval_type_name`, `zend_zval_value_name`, `get_debug_type`, `gettype`) |
//! | [`ops`] | arithmetic, bitwise, concatenation and logical operators (`+ - * / % ** . & \| ^ ~ << >> ! xor`, Twig's `//`) |
//! | [`compare`] | `== != < <= > >= <=>` (`zend_compare`) and `=== !==` |
//! | [`dump`] | `var_dump()`, `var_export()`, `print_r()` |
//! | [`math`] | the C library math PHP exposes, bit-exact: `pow()` (glibc 2.41 FMA build), `floor`, `ceil`, `trunc`, `fmod`, `fma` |
//! | [`builtins`] | the PHP builtin functions Twig calls (`format`, `string`, `array`) |
//! | `serde` | (feature `serde`) Rust data into values through serde: `to_value`, `to_array` |
//!
//! Rust data converts into values with `From` ([`Value::from`], [`Array::from`]: scalars, strings, options,
//! sequences and maps, see [`value`]), or through serde with the `serde` feature.
//!
//! # Conventions for code built on this crate
//!
//! * **Errors and diagnostics.** Operations that throw in PHP return `Err(PhpError)`; non-fatal
//!   diagnostics are reported to a `&mut dyn Diagnostics` parameter, in PHP's order, and an `Err`
//!   returned by the sink aborts the operation (mirrors `if (EG(exception)) return;`).
//! * **Bytes.** Strings and messages are bytes ([`PhpString`]). Use [`PhpString::c_str`] where PHP
//!   prints a string with C `%s` (stops at NUL).
//! * **Identity vs equality.** `PartialEq` on [`Value`]/[`Array`] is PHP `===`. Loose comparison
//!   (`==`, `<=>`) is [`compare`]; object comparison goes through
//!   [`Object::compare`](object::Object::compare).
//! * **Copy-on-write.** [`Array`] clones are O(1); mutate through `&mut Array` (separates shared
//!   storage with `zend_array_dup()` rules). [`Array::ptr_eq`] exposes PHP's "same HashTable"
//!   shortcut used by `==`/`===` on arrays.
//! * **Objects.** Operations on [`ObjectRef`] behave as if called from outside the object's class
//!   (Twig's `CoreExtension` scope). New host classes implement [`object::Object`] (see the module
//!   documentation of [`object`]), or are assembled with [`object::ObjectBuilder`].
//! * **Thread safety.** All value types are `Send + Sync`.
//!
//! # Building on this crate
//!
//! Each entry names the building block that already reproduces a PHP sub-step, so a port only has
//! to add the remaining algorithm (in upstream order).
//!
//! ## Engine operations (the VM)
//!
//! * Operators: [`ops::binary_op`] with [`ops::BinaryOp`] (or [`ops::add`], [`ops::concat`], ...),
//!   [`ops::negate`] / [`ops::unary_plus`] for unary `-`/`+` (PHP compiles them to `* -1` / `* 1`),
//!   [`ops::bitwise_not`], [`ops::boolean_not`], [`ops::boolean_xor`], and [`ops::floor_div`] for
//!   Twig's `//`. `and`/`or` short-circuit in the VM with [`convert::to_bool`].
//! * Comparison: [`compare::compare_op`] with [`compare::CompareOp`], or [`compare::equals`],
//!   [`compare::less`], [`compare::greater`] (evaluated as `b < a`, like PHP), [`compare::spaceship`],
//!   [`compare::identical`]; [`compare::compare`] is `zend_compare()` for builtins that sort or
//!   search loosely (`sort`, `in_array`, `max`, ...).
//! * Output: `{{ x }}` is [`convert::to_string`]; Twig's `dump()` is [`dump::var_dump`].
//! * Dimension and property access: [`dim::read`] / [`dim::isset`] / [`dim::empty`],
//!   [`ArrayKey::from_value`] for writes, [`ObjectRef::read_property`] & co. for `->`.
//!
//! ## Numbers in builtins
//!
//! * `pow()` is [`ops::pow`] (same as `**`); C library math is [`math::pow`], [`math::floor`],
//!   [`math::ceil`], [`math::fmod`] (`round()` needs `pow` only beyond 22 decimal places).
//! * Float output: [`float::double_to_string`] (precision 14), [`float::format_serialize`]
//!   (`%.*H`), [`float::format_var_export`], [`float::format_json`].
//!
//! ## Builtins (strings, format, arrays)
//!
//! * Arguments: describe each parameter with [`param::Arg`] (function, position, name, arginfo
//!   type) and parse with [`param::parse_long`], [`param::parse_str`], [`param::parse_number`], …;
//!   use [`param::Arg::type_error`] / [`param::Arg::value_error`] for the remaining messages and
//!   [`object::CallArgs::bind`] for arity/named-argument errors ([`object::CallArgs::unpack`] with
//!   an [`object::CalleeParams`] when `...` is unpacked into a known callee). Twig's own typed PHP
//!   methods (userland functions): [`param::verify_user_arg`] / [`param::verify_user_return`] with a
//!   [`param::DeclaredType`].
//! * Strings are [`PhpString`] bytes; string conversion of arguments is [`convert::to_string`]
//!   (floats at precision 14, `Array to string conversion`, `__toString()`).
//! * Arrays: build results with [`Array::new`] (`array_init`), [`Array::with_capacity`]
//!   (`array_init_size`) or [`Array::new_hash`] (mixed init) so that later appends see PHP's
//!   `nNextFreeElement`; recompute it with [`Array::set_next_free`] where upstream does
//!   (`array_pop`, `array_shift`, `array_splice`); look up without allocating via
//!   [`Array::get_ref`] / [`Array::get_sym`]; `array_is_list` is [`Array::is_list`];
//!   `count()` is [`dim::count`].
//! * `Traversable` arguments: [`object::traverse`] (PHP's call order), [`object::iterator_to_array`],
//!   [`object::iterator_count`]; callables: [`ObjectRef::invoke`] (`Closure`, `__invoke`).
//! * Every new PHP behaviour gets an oracle vector file under `tests/vectors/` (see the existing
//!   generators for the typed-JSON format).

pub mod array;
pub mod builtins;
pub mod compare;
pub mod convert;
pub mod diag;
pub mod dim;
pub mod dump;
pub mod error;
pub mod float;
pub mod key;
pub mod math;
pub mod numeric;
pub mod object;
pub mod ops;
pub mod param;
#[cfg(feature = "serde")]
pub mod serde;
pub mod string;
pub mod types;
pub mod value;

#[cfg(feature = "serde")]
pub use self::serde::{to_array, to_value};
pub use array::Array;
pub use diag::{Collect, Diagnostic, Diagnostics, FnDiagnostics, Ignore, Level, Throw};
pub use error::{PhpError, PhpResult, ThrowableClass};
pub use key::{ArrayKey, KeyContext, KeyRef};
pub use object::{FetchMode, ObjectRef};
pub use string::PhpString;
pub use value::{Number, TypeTag, Value};

/// The example of `README.md`, compiled and run as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn values_are_send_and_sync() {
        assert_send_sync::<Value>();
        assert_send_sync::<Array>();
        assert_send_sync::<PhpString>();
        assert_send_sync::<ObjectRef>();
        assert_send_sync::<PhpError>();
    }
}
