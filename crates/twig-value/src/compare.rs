//! Comparison operators: `==`, `!=`, `<`, `<=`, `>`, `>=`, `<=>` (loose, `zend_compare()`) and
//! `===`, `!==` (identity).
//!
//! reference/php/engine-semantics.md §7. Ports of PHPSRC/Zend/zend_operators.c (`zend_compare()`,
//! `compare_longs_to_string`, `zendi_smart_strcmp()`, `zendi_smart_streq()`,
//! `zend_compare_symbol_tables()`, `fast_equal_check_function()`), PHPSRC/Zend/zend_hash.c
//! (`zend_hash_compare()`) and PHPSRC/Zend/zend_object_handlers.c (`zend_std_compare_objects()`):
//!
//! * `a > b` is compiled by PHP as `b < a` and `a >= b` as `b <= a` (`zend_compile.c`), so
//!   [`greater`] and [`greater_or_equal`] evaluate `zend_compare(b, a)`; "uncomparable" pairs give
//!   `1` in both orders, which makes `<`, `<=`, `>`, `>=` and `==` all false.
//! * Numeric strings compare numerically, a number and a non-numeric string compare as strings
//!   (the number formatted with precision 14), `null`/`bool` operands compare as booleans except
//!   `null <=> string` (`""` only) and `null <=> object`.
//! * Arrays compare by count, then key by key in the left array's order (uncomparable when a key is
//!   missing); two arrays sharing one table are equal without looking at the elements (so
//!   `$m = $n = [NAN]` gives `$m == $n`).
//! * Objects: identity, then the class's `compare` handler ([`Object::compare`]: enums and closures
//!   are uncomparable), else `zend_std_compare_objects()`: different classes are uncomparable,
//!   otherwise properties compare in declaration order; an object against an `int`/`float` is
//!   treated as `1` with the notice `Object of class X could not be converted to int`.
//!
//! Exceptions raised during a comparison (`__toString()`, recursion, a throwing diagnostics sink)
//! abort it with `Err`.
//!
//! **Known deviation**: PHP switches from the per-slot comparison to the property-table
//! comparison once an object's property table has been materialised (by `var_dump()`, `foreach`,
//! `(array)`, a dynamic property, ...); this port uses the table comparison exactly when one of the
//! objects has dynamic properties. The two only differ for objects with uninitialised or unset
//! declared properties.
//!
//! [`Object::compare`]: crate::object::Object::compare

use std::borrow::Cow;
use std::cmp::Ordering;
use std::sync::Arc;

use crate::array::Array;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::float;
use crate::numeric;
use crate::object::{CastTarget, ObjectRef};
use crate::string::PhpString;
use crate::value::{Number, TypeTag, Value};

/// `ZEND_UNCOMPARABLE`: the result for pairs that have no order, in both directions.
pub const UNCOMPARABLE: i32 = 1;

/// A comparison operator of the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareOp {
    /// `==` (`ZEND_IS_EQUAL`).
    Equal,
    /// `!=` (`ZEND_IS_NOT_EQUAL`).
    NotEqual,
    /// `<` (`ZEND_IS_SMALLER`).
    Less,
    /// `<=` (`ZEND_IS_SMALLER_OR_EQUAL`).
    LessOrEqual,
    /// `>` (`ZEND_IS_SMALLER` with swapped operands).
    Greater,
    /// `>=` (`ZEND_IS_SMALLER_OR_EQUAL` with swapped operands).
    GreaterOrEqual,
    /// `<=>` (`ZEND_SPACESHIP`): `-1`, `0` or `1`.
    Spaceship,
    /// `===` (`ZEND_IS_IDENTICAL`).
    Identical,
    /// `!==` (`ZEND_IS_NOT_IDENTICAL`).
    NotIdentical,
}

impl CompareOp {
    /// The operator as PHP spells it.
    pub fn symbol(self) -> &'static str {
        match self {
            CompareOp::Equal => "==",
            CompareOp::NotEqual => "!=",
            CompareOp::Less => "<",
            CompareOp::LessOrEqual => "<=",
            CompareOp::Greater => ">",
            CompareOp::GreaterOrEqual => ">=",
            CompareOp::Spaceship => "<=>",
            CompareOp::Identical => "===",
            CompareOp::NotIdentical => "!==",
        }
    }
}

/// Evaluates `a <op> b`: a `bool`, or an `int` for `<=>`.
pub fn compare_op(
    op: CompareOp,
    a: &Value,
    b: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    Ok(match op {
        CompareOp::Equal => Value::Bool(equals(a, b, diag)?),
        CompareOp::NotEqual => Value::Bool(!equals(a, b, diag)?),
        CompareOp::Less => Value::Bool(less(a, b, diag)?),
        CompareOp::LessOrEqual => Value::Bool(less_or_equal(a, b, diag)?),
        CompareOp::Greater => Value::Bool(greater(a, b, diag)?),
        CompareOp::GreaterOrEqual => Value::Bool(greater_or_equal(a, b, diag)?),
        CompareOp::Spaceship => Value::Int(spaceship(a, b, diag)?),
        CompareOp::Identical => Value::Bool(identical(a, b)),
        CompareOp::NotIdentical => Value::Bool(!identical(a, b)),
    })
}

/// Port of `zend_compare()`: the three-way loose comparison (`-1`, `0`, `1`) behind `==`, `<`,
/// `<=>`, `sort()`, `in_array()`, `max()`... Uncomparable pairs give [`UNCOMPARABLE`].
pub fn compare(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i32> {
    Comparer::new(diag).compare(a, b)
}

/// Port of `fast_equal_check_function()` / `ZEND_IS_EQUAL`: `a == b`.
#[inline]
pub fn equals(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x == y),
        (Value::Int(x), Value::Float(y)) => Ok(*x as f64 == *y),
        (Value::Float(x), Value::Float(y)) => Ok(x == y),
        (Value::Float(x), Value::Int(y)) => Ok(*x == *y as f64),
        (Value::String(x), Value::String(y)) => Ok(fast_equal_strings(x, y)),
        _ => Ok(compare(a, b, diag)? == 0),
    }
}

/// `a != b` (`ZEND_IS_NOT_EQUAL`).
#[inline]
pub fn not_equals(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    Ok(!equals(a, b, diag)?)
}

/// `a < b` (`ZEND_IS_SMALLER`: `zend_compare(a, b) < 0`).
#[inline]
pub fn less(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    if let (Value::Int(x), Value::Int(y)) = (a, b) {
        return Ok(x < y);
    }
    Ok(compare(a, b, diag)? < 0)
}

/// `a <= b` (`ZEND_IS_SMALLER_OR_EQUAL`: `zend_compare(a, b) <= 0`).
#[inline]
pub fn less_or_equal(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    if let (Value::Int(x), Value::Int(y)) = (a, b) {
        return Ok(x <= y);
    }
    Ok(compare(a, b, diag)? <= 0)
}

/// `a > b`, compiled by PHP as `b < a` (`zend_compare(b, a) < 0`).
#[inline]
pub fn greater(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    less(b, a, diag)
}

/// `a >= b`, compiled by PHP as `b <= a` (`zend_compare(b, a) <= 0`).
#[inline]
pub fn greater_or_equal(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    less_or_equal(b, a, diag)
}

/// `a <=> b` (`ZEND_SPACESHIP` → `compare_function()`).
pub fn spaceship(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
    Ok(i64::from(compare(a, b, diag)?))
}

/// Port of `zend_is_identical()`: `a === b` (same type and value; floats with `==`, arrays in
/// order with identical keys and values, objects by handle).
#[inline]
pub fn identical(a: &Value, b: &Value) -> bool {
    a.identical(b)
}

/// `ZEND_THREEWAY_COMPARE()` on doubles: NaN gives `1`.
fn three_way(a: f64, b: f64) -> i32 {
    if a == b {
        0
    } else if a < b {
        -1
    } else {
        1
    }
}

fn three_way_long(a: i64, b: i64) -> i32 {
    match a.cmp(&b) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// `ZEND_NORMALIZE_BOOL()` of a double: `n ? (n < 0 ? -1 : 1) : 0` (NaN gives `1`).
fn normalize_double(n: f64) -> i32 {
    if n != 0.0 {
        if n < 0.0 { -1 } else { 1 }
    } else {
        0
    }
}

/// Port of `zend_binary_strcmp()`: `memcmp()` of the common prefix, then the lengths. Like glibc's
/// `memcmp()` the result is the difference of the first differing bytes (any sign/magnitude is
/// possible); callers that need `-1`/`0`/`1` normalise it.
pub fn binary_strcmp(s1: &[u8], s2: &[u8]) -> i32 {
    let n = s1.len().min(s2.len());
    for i in 0..n {
        if s1[i] != s2[i] {
            return i32::from(s1[i]) - i32::from(s2[i]);
        }
    }
    match s1.len().cmp(&s2.len()) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// `lval`/`dval` of an `is_numeric_string_ex()` result the way the C callers see them: the field
/// that was not written keeps its initial `0`.
fn numeric_parts(n: Number) -> (bool, i64, f64) {
    match n {
        Number::Int(l) => (false, l, 0.0),
        Number::Float(d) => (true, 0, d),
    }
}

/// Port of `zendi_smart_strcmp()`: `<=>` of two strings. Numeric strings (as a whole, surrounding
/// whitespace allowed) compare as numbers, with care for integers that overflowed `zend_long` and
/// for infinities; everything else compares bytewise.
pub fn smart_strcmp(s1: &[u8], s2: &[u8]) -> i32 {
    if let Some(n1) = numeric::is_numeric_string_ex(s1, false)
        && let Some(n2) = numeric::is_numeric_string_ex(s2, false)
    {
        let (double1, lval1, mut dval1) = numeric_parts(n1.number);
        let (double2, lval2, mut dval2) = numeric_parts(n2.number);
        let (oflow1, oflow2) = (i32::from(n1.overflow), i32::from(n2.overflow));
        if oflow1 != 0 && oflow1 == oflow2 && dval1 - dval2 == 0.0 {
            // both values are integers overflowed to the same side, and the double comparison
            // may have resulted in crucial accuracy lost
            return normalize_long(binary_strcmp(s1, s2));
        }
        if double1 || double2 {
            if !double1 {
                if oflow2 != 0 {
                    // 2nd operand is integer > LONG_MAX (oflow2==1) or < LONG_MIN (-1)
                    return -oflow2;
                }
                dval1 = lval1 as f64;
            } else if !double2 {
                if oflow1 != 0 {
                    return oflow1;
                }
                dval2 = lval2 as f64;
            } else if dval1 == dval2 && !dval1.is_finite() {
                // Both values overflowed and have the same sign, so a numeric comparison would
                // be inaccurate
                return normalize_long(binary_strcmp(s1, s2));
            }
            return normalize_double(dval1 - dval2);
        }
        // they both have to be long's
        return three_way_long(lval1, lval2);
    }
    normalize_long(binary_strcmp(s1, s2))
}

/// Port of `zendi_smart_streq()`: `==` of two strings (same structure as [`smart_strcmp`]).
pub fn smart_streq(s1: &[u8], s2: &[u8]) -> bool {
    if let Some(n1) = numeric::is_numeric_string_ex(s1, false)
        && let Some(n2) = numeric::is_numeric_string_ex(s2, false)
    {
        let (double1, lval1, mut dval1) = numeric_parts(n1.number);
        let (double2, lval2, mut dval2) = numeric_parts(n2.number);
        let (oflow1, oflow2) = (n1.overflow, n2.overflow);
        if oflow1 != 0 && oflow1 == oflow2 && dval1 - dval2 == 0.0 {
            // both values are integers overflown to the same side, and the double comparison
            // may have resulted in crucial accuracy lost
            return s1 == s2;
        }
        if double1 || double2 {
            if !double1 {
                if oflow2 != 0 {
                    // 2nd operand is integer > LONG_MAX (oflow2==1) or < LONG_MIN (-1)
                    return false;
                }
                dval1 = lval1 as f64;
            } else if !double2 {
                if oflow1 != 0 {
                    return false;
                }
                dval2 = lval2 as f64;
            } else if dval1 == dval2 && !dval1.is_finite() {
                // Both values overflowed and have the same sign, so a numeric comparison would
                // be inaccurate
                return s1 == s2;
            }
            return dval1 == dval2;
        }
        // they both have to be long's
        return lval1 == lval2;
    }
    s1 == s2
}

/// `ZEND_NORMALIZE_BOOL()` of an int.
fn normalize_long(n: i32) -> i32 {
    n.signum()
}

/// Port of `zend_fast_equal_strings()`: same string, else bytewise when either first byte is
/// above `'9'` (as a signed `char`: bytes `>= 0x80` are not), else [`smart_streq`].
fn fast_equal_strings(s1: &PhpString, s2: &PhpString) -> bool {
    if PhpString::ptr_eq(s1, s2) {
        return true;
    }
    // ZSTR_VAL(s)[0] of an empty string is its NUL terminator.
    let above_nine = |s: &PhpString| matches!(s.first(), Some(&c) if c > b'9' && c < 0x80);
    if above_nine(s1) || above_nine(s2) {
        s1.as_bytes() == s2.as_bytes()
    } else {
        smart_streq(s1, s2)
    }
}

/// Port of `compare_longs_to_string()` (`compare_long_to_string()`): `l <=> s`.
fn compare_long_to_string(l: i64, s: &[u8]) -> i32 {
    match numeric::is_numeric_string(s, false) {
        Some(Number::Int(sl)) => three_way_long(l, sl),
        Some(Number::Float(sd)) => three_way(l as f64, sd),
        None => normalize_long(binary_strcmp(l.to_string().as_bytes(), s)),
    }
}

/// Port of `compare_double_to_string()`: `d <=> s` for a non-NaN `d`; a non-numeric string is
/// compared with `d` formatted at precision 14 (`INF == "INF"`).
fn compare_double_to_string(d: f64, s: &[u8]) -> i32 {
    match numeric::is_numeric_string(s, false) {
        Some(Number::Int(sl)) => three_way(d, sl as f64),
        Some(Number::Float(sd)) => three_way(d, sd),
        None => normalize_long(binary_strcmp(float::double_to_string(d).as_bytes(), s)),
    }
}

/// What a recursion guard protects (`GC_PROTECT_RECURSION` flags).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Guard {
    /// An array table (`zend_hash_compare()` protects `ht1`).
    Table(usize),
    /// An object (`Z_PROTECT_RECURSION_P(o1)` in the slot comparison).
    Object(u64),
    /// An object's property table (the `zend_hash_compare()` of the table comparison).
    ObjectTable(u64),
}

/// `zend_compare()` with the recursion guards of one comparison.
struct Comparer<'d> {
    diag: &'d mut dyn Diagnostics,
    guards: Vec<Guard>,
}

/// `Nesting level too deep - recursive dependency?`
fn nesting_error() -> PhpError {
    PhpError::error("Nesting level too deep - recursive dependency?")
}

impl<'d> Comparer<'d> {
    fn new(diag: &'d mut dyn Diagnostics) -> Self {
        Comparer {
            diag,
            guards: Vec::new(),
        }
    }

    fn protect(&mut self, g: Guard) -> PhpResult<()> {
        if self.guards.contains(&g) {
            return Err(nesting_error());
        }
        self.guards.push(g);
        Ok(())
    }

    fn unprotect(&mut self, g: Guard) {
        if let Some(pos) = self.guards.iter().rposition(|x| *x == g) {
            self.guards.remove(pos);
        }
    }

    /// Port of `zend_compare()`.
    fn compare(&mut self, op1: &Value, op2: &Value) -> PhpResult<i32> {
        let mut converted = false;
        let mut a: Cow<'_, Value> = Cow::Borrowed(op1);
        let mut b: Cow<'_, Value> = Cow::Borrowed(op2);
        loop {
            match (&*a, &*b) {
                (Value::Int(x), Value::Int(y)) => return Ok(three_way_long(*x, *y)),
                (Value::Float(x), Value::Int(y)) => return Ok(three_way(*x, *y as f64)),
                (Value::Int(x), Value::Float(y)) => return Ok(three_way(*x as f64, *y)),
                (Value::Float(x), Value::Float(y)) => return Ok(three_way(*x, *y)),
                (Value::Array(x), Value::Array(y)) => return self.compare_arrays(x, y),
                (Value::Null | Value::Bool(false), Value::Null | Value::Bool(false))
                | (Value::Bool(true), Value::Bool(true)) => return Ok(0),
                (Value::Null, Value::Bool(true)) => return Ok(-1),
                (Value::Bool(true), Value::Null) => return Ok(1),
                (Value::String(x), Value::String(y)) => {
                    if PhpString::ptr_eq(x, y) {
                        return Ok(0);
                    }
                    return Ok(smart_strcmp(x, y));
                }
                (Value::Null, Value::String(s)) => return Ok(if s.is_empty() { 0 } else { -1 }),
                (Value::String(s), Value::Null) => return Ok(if s.is_empty() { 0 } else { 1 }),
                (Value::Int(l), Value::String(s)) => return Ok(compare_long_to_string(*l, s)),
                (Value::String(s), Value::Int(l)) => return Ok(-compare_long_to_string(*l, s)),
                (Value::Float(d), Value::String(s)) => {
                    if d.is_nan() {
                        return Ok(1);
                    }
                    return Ok(compare_double_to_string(*d, s));
                }
                (Value::String(s), Value::Float(d)) => {
                    if d.is_nan() {
                        return Ok(1);
                    }
                    return Ok(-compare_double_to_string(*d, s));
                }
                (Value::Object(_), Value::Null) => return Ok(1),
                (Value::Null, Value::Object(_)) => return Ok(-1),
                (x, y) => {
                    if matches!(x, Value::Object(_)) || matches!(y, Value::Object(_)) {
                        return self.compare_object(x, y);
                    }
                    if !converted {
                        let nan = |v: &Value| matches!(v, Value::Float(d) if d.is_nan());
                        if nan(x) || nan(y) {
                            // NAN used be cast to TRUE so handle this manually for the time being
                            if x.type_tag() < TypeTag::True {
                                return Ok(-1);
                            } else if matches!(x, Value::Bool(true))
                                || matches!(y, Value::Bool(true))
                            {
                                return Ok(0);
                            } else if y.type_tag() < TypeTag::True {
                                return Ok(1);
                            } else if !matches!(x, Value::Float(_)) {
                                a = Cow::Owned(convert::to_number_silent(x, self.diag)?);
                                converted = true;
                            } else if !matches!(y, Value::Float(_)) {
                                b = Cow::Owned(convert::to_number_silent(y, self.diag)?);
                                converted = true;
                            }
                        } else if x.type_tag() < TypeTag::True {
                            return Ok(if convert::to_bool(y, self.diag)? {
                                -1
                            } else {
                                0
                            });
                        } else if matches!(x, Value::Bool(true)) {
                            return Ok(if convert::to_bool(y, self.diag)? {
                                0
                            } else {
                                1
                            });
                        } else if y.type_tag() < TypeTag::True {
                            return Ok(if convert::to_bool(x, self.diag)? {
                                1
                            } else {
                                0
                            });
                        } else if matches!(y, Value::Bool(true)) {
                            return Ok(if convert::to_bool(x, self.diag)? {
                                0
                            } else {
                                -1
                            });
                        } else {
                            let na = convert::to_number_silent(x, self.diag)?;
                            let nb = convert::to_number_silent(y, self.diag)?;
                            a = Cow::Owned(na);
                            b = Cow::Owned(nb);
                            converted = true;
                        }
                    } else if matches!(x, Value::Array(_)) {
                        return Ok(1);
                    } else if matches!(y, Value::Array(_)) {
                        return Ok(-1);
                    } else {
                        return Err(PhpError::error("Unsupported operand types"));
                    }
                }
            }
        }
    }

    /// The object branch of `zend_compare()`.
    fn compare_object(&mut self, op1: &Value, op2: &Value) -> PhpResult<i32> {
        let (object, other, object_is_left) = match (op1, op2) {
            (Value::Object(o), other) => (o, other, true),
            (other, Value::Object(o)) => (o, other, false),
            _ => unreachable!("compare_object() needs an object operand"),
        };
        match other {
            Value::Object(o) if ObjectRef::ptr_eq(object, o) => return Ok(0),
            Value::Bool(_) => {
                return match object.cast(CastTarget::Bool, self.diag)? {
                    None => Ok(if object_is_left { 1 } else { -1 }),
                    Some(casted) => {
                        if object_is_left {
                            self.compare(&casted, other)
                        } else {
                            self.compare(other, &casted)
                        }
                    }
                };
            }
            _ => {}
        }
        // Z_OBJ_HANDLER_P(object, compare)(op1, op2)
        let handler = object.object().compare(
            object,
            other,
            object_is_left,
            &mut |x: &Value, y: &Value| self.compare(x, y),
        );
        match handler {
            Some(r) => r,
            None => self.std_compare_objects(op1, op2),
        }
    }

    /// Port of `zend_std_compare_objects()`.
    fn std_compare_objects(&mut self, o1: &Value, o2: &Value) -> PhpResult<i32> {
        let (obj1, obj2) = match (o1, o2) {
            (Value::Object(x), Value::Object(y)) => (x, y),
            _ => return self.compare_object_to_value(o1, o2),
        };
        if ObjectRef::ptr_eq(obj1, obj2) {
            return Ok(0); // the same object
        }
        if !Arc::ptr_eq(obj1.class(), obj2.class()) {
            return Ok(UNCOMPARABLE); // different classes
        }
        let store1 = obj1.object().property_store();
        let store2 = obj2.object().property_store();
        let dynamic =
            |s: Option<&crate::object::PropertyStore>| s.is_some_and(|s| s.dynamic_len() > 0);
        if !dynamic(store1) && !dynamic(store2) {
            let slots1 = store1.map(|s| s.slot_values()).unwrap_or_default();
            let slots2 = store2.map(|s| s.slot_values()).unwrap_or_default();
            if slots1.is_empty() {
                return Ok(0);
            }
            // It's enough to protect only one of the objects.
            let guard = Guard::Object(obj1.id());
            self.protect(guard)?;
            let mut ret = 0;
            for (i, p1) in slots1.iter().enumerate() {
                let p2 = slots2.get(i).and_then(Option::as_ref);
                match (p1, p2) {
                    (Some(p1), Some(p2)) => {
                        let r = self.compare(p1, p2);
                        match r {
                            Ok(0) => {}
                            other => {
                                self.unprotect(guard);
                                return other;
                            }
                        }
                    }
                    (None, None) => {}
                    _ => {
                        ret = 1;
                        break;
                    }
                }
            }
            self.unprotect(guard);
            return Ok(ret);
        }
        // zend_compare_symbol_tables(zend_std_get_properties_ex(zobj1), ...(zobj2))
        let slots1 = store1.map(|s| s.slot_values()).unwrap_or_default();
        let slots2 = store2.map(|s| s.slot_values()).unwrap_or_default();
        let dyn1 = store1.map(|s| s.dynamic_entries()).unwrap_or_default();
        let count1 = slots1.len() + dyn1.len();
        let count2 = slots2.len() + store2.map_or(0, |s| s.dynamic_len());
        if count1 != count2 {
            return Ok(if count1 > count2 { 1 } else { -1 });
        }
        let guard = Guard::ObjectTable(obj1.id());
        self.protect(guard)?;
        let result = self.compare_property_tables(&slots1, &slots2, &dyn1, store2);
        self.unprotect(guard);
        result
    }

    /// The `zend_hash_compare_impl()` loop over two property tables of the same class: declared
    /// slots (`IS_INDIRECT`, possibly `IS_UNDEF`) then dynamic properties looked up by name.
    fn compare_property_tables(
        &mut self,
        slots1: &[Option<Value>],
        slots2: &[Option<Value>],
        dyn1: &[(PhpString, Value)],
        store2: Option<&crate::object::PropertyStore>,
    ) -> PhpResult<i32> {
        for (i, p1) in slots1.iter().enumerate() {
            let p2 = slots2.get(i).and_then(Option::as_ref);
            match (p1, p2) {
                (None, Some(_)) => return Ok(-1),
                (Some(_), None) => return Ok(1),
                (None, None) => {}
                (Some(p1), Some(p2)) => {
                    let r = self.compare(p1, p2)?;
                    if r != 0 {
                        return Ok(r);
                    }
                }
            }
        }
        for (name, v1) in dyn1 {
            let Some(v2) = store2.and_then(|s| s.dynamic_value(name)) else {
                return Ok(1);
            };
            let r = self.compare(v1, &v2)?;
            if r != 0 {
                return Ok(r);
            }
        }
        Ok(0)
    }

    /// The "object and non-object" part of `zend_std_compare_objects()`: the object is cast to the
    /// other operand's type (only `__toString()` can succeed); a failed cast to `int`/`float`
    /// notices `Object of class %s could not be converted to %s` and uses `1`; any other failure
    /// makes the object the greater operand.
    fn compare_object_to_value(&mut self, o1: &Value, o2: &Value) -> PhpResult<i32> {
        let (object, value, object_lhs) = match (o1, o2) {
            (Value::Object(o), v) => (o, v, true),
            (v, Value::Object(o)) => (o, v, false),
            _ => unreachable!("an object operand"),
        };
        let target = match value {
            Value::Int(_) => Some(CastTarget::Long),
            Value::Float(_) => Some(CastTarget::Double),
            Value::String(_) => Some(CastTarget::String),
            // IS_ARRAY: no cast handler supports it. null and bool are handled by zend_compare().
            _ => None,
        };
        let casted = match target {
            Some(t) => object.cast(t, self.diag)?,
            None => None,
        };
        let casted = match casted {
            Some(c) => c,
            None => match target {
                Some(t @ (CastTarget::Long | CastTarget::Double)) => {
                    self.diag.notice(format!(
                        "Object of class {} could not be converted to {}",
                        object.class().display_name(),
                        t.type_name()
                    ))?;
                    if t == CastTarget::Long {
                        Value::Int(1)
                    } else {
                        Value::Float(1.0)
                    }
                }
                _ => return Ok(if object_lhs { 1 } else { -1 }),
            },
        };
        if object_lhs {
            self.compare(&casted, value)
        } else {
            self.compare(value, &casted)
        }
    }

    /// Port of `zend_compare_symbol_tables()` / `zend_hash_compare(ht1, ht2, ..., ordered = 0)`.
    fn compare_arrays(&mut self, ht1: &Array, ht2: &Array) -> PhpResult<i32> {
        if Array::ptr_eq(ht1, ht2) {
            return Ok(0);
        }
        // It's enough to protect only one of the arrays.
        let guard = Guard::Table(ht1.table_id());
        self.protect(guard)?;
        let result = self.compare_arrays_impl(ht1, ht2);
        self.unprotect(guard);
        result
    }

    /// `zend_hash_compare_impl(..., ordered = 0)`.
    fn compare_arrays_impl(&mut self, ht1: &Array, ht2: &Array) -> PhpResult<i32> {
        if ht1.len() != ht2.len() {
            return Ok(if ht1.len() > ht2.len() { 1 } else { -1 });
        }
        for (key, v1) in ht1.iter() {
            let Some(v2) = ht2.get(key) else {
                return Ok(1);
            };
            let r = self.compare(v1, v2)?;
            if r != 0 {
                return Ok(r);
            }
        }
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::StdClass;

    fn cmp(a: impl Into<Value>, b: impl Into<Value>) -> i32 {
        compare(&a.into(), &b.into(), &mut Vec::new()).unwrap()
    }

    fn eq(a: impl Into<Value>, b: impl Into<Value>) -> bool {
        equals(&a.into(), &b.into(), &mut Vec::new()).unwrap()
    }

    #[test]
    fn scalars() {
        assert_eq!(cmp(0, ""), 1);
        assert_eq!(cmp(0, "a"), -1);
        assert!(!eq(0, "a"));
        assert!(eq(1, " 1"));
        assert!(eq(1, "1 "));
        assert!(!eq(1, "1abc"));
        assert!(eq(i64::MAX, "9223372036854775808"));
        assert!(eq(f64::INFINITY, "INF"));
        assert_eq!(cmp(1.5, "abc"), -1);
        assert_eq!(cmp(f64::NAN, 0), 1);
        assert_eq!(cmp(0, f64::NAN), 1);
        assert!(eq(Value::Null, false));
        assert!(!eq(Value::Null, "0"));
        assert!(eq(false, "0"));
        assert_eq!(cmp(Value::Null, -1), -1);
        assert!(eq(f64::NAN, true));
        assert_eq!(cmp(f64::NAN, Value::Array(Array::new())), -1);
        assert_eq!(cmp(Value::Array(Array::new()), f64::NAN), 1);
        assert_eq!(cmp(Value::Array(Array::new()), 0), 1);
        assert!(!eq(Value::Array(Array::new()), 0));
        assert!(eq(Value::Array(Array::new()), false));
    }

    #[test]
    fn strings() {
        assert_eq!(
            smart_strcmp(b"9223372036854775807", b"9223372036854775808"),
            -1
        );
        assert_eq!(
            smart_strcmp(b"9223372036854775808", b"9223372036854775809"),
            -1
        );
        assert_eq!(
            smart_strcmp(b"9223372036854775808", b"9.2233720368547758E+18"),
            0
        );
        assert_eq!(smart_strcmp(b"1e1000", b"2e1000"), -1);
        assert_eq!(smart_strcmp(b"a", b"B"), 1);
        assert_eq!(smart_strcmp(b"10", b"9a"), -1);
        assert_eq!(smart_strcmp(b"1\0", b"1"), 1);
        assert!(smart_streq(b"1e3", b"1000"));
        assert!(!smart_streq(b"abc", b"ABC"));
        assert!(eq("  1", "1  "));
        assert!(eq("1e-1000", "0"));
        assert_eq!(binary_strcmp(b"a", b"c"), -2);
    }

    #[test]
    fn arrays_and_objects() {
        let nan_arr = Value::Array(Array::from_values([Value::Float(f64::NAN)]));
        assert!(eq(nan_arr.clone(), nan_arr.clone()));
        let other = Value::Array(Array::from_values([Value::Float(f64::NAN)]));
        assert!(!eq(nan_arr, other));
        let a: Array = [("a", Value::Int(1))].into_iter().collect();
        let b: Array = [("b", Value::Int(1))].into_iter().collect();
        assert_eq!(cmp(a.clone(), b.clone()), 1);
        assert_eq!(cmp(b, a), 1);
        let o1 = Value::Object(StdClass::new().into_ref());
        let o2 = Value::Object(StdClass::new().into_ref());
        assert!(eq(o1.clone(), o2.clone()));
        let mut diag = Vec::new();
        assert!(equals(&o1, &Value::Int(1), &mut diag).unwrap());
        assert_eq!(diag.len(), 1);
        assert_eq!(
            diag[0].message,
            "Object of class stdClass could not be converted to int"
        );
        assert!(eq(o1.clone(), true));
        assert!(!eq(o1.clone(), Value::Null));
        assert_eq!(cmp(o1.clone(), "x"), 1);
        assert_eq!(cmp("x", o1.clone()), -1);
        let o3 = Value::Object(StdClass::with_properties([("a", Value::Int(1))]).into_ref());
        assert_eq!(cmp(o1.clone(), o3.clone()), -1);
        assert_eq!(cmp(o3, o1), 1);
    }

    #[test]
    fn recursion() {
        let a = StdClass::new().into_ref();
        a.write_property(b"self", Value::Object(a.clone()), &mut Vec::new())
            .unwrap();
        let b = StdClass::new().into_ref();
        b.write_property(b"self", Value::Object(b.clone()), &mut Vec::new())
            .unwrap();
        let err = compare(&Value::Object(a), &Value::Object(b), &mut Vec::new()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Error: Nesting level too deep - recursive dependency?"
        );
    }
}
