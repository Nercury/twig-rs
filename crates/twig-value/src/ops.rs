//! Arithmetic, bitwise, string and logical operators with PHP 8.5 semantics.
//!
//! reference/php/engine-semantics.md §6. Twig 3.31.1 compiles its operators to the PHP operators
//! (`+ - * / % **`, `~` → `.`, `b-and`/`b-or`/`b-xor` → `& | ^`, `not` → `!`, unary `-`/`+`), so
//! these functions are ports of the Zend engine's `*_function()`s (PHPSRC/Zend/zend_operators.c),
//! with the same operand conversion order, diagnostics and exceptions:
//!
//! * operands are converted left to right; a failing left operand throws before the right one is
//!   looked at (`"abc" + "1x"` throws without the right operand's warning);
//! * `TypeError: Unsupported operand types: %s %s %s` names the *original* operand types with
//!   [`zend_zval_type_name`] (`bool`, class names);
//! * `int` results overflow to `float`, computed from the original operands
//!   (`PHP_INT_MAX + 1` → `9.2233720368547758E+18`);
//! * unary minus and plus are compiled by PHP as `expr * -1` and `expr * 1`
//!   (`zend_compile_unary_pm`), so [`negate`] and [`unary_plus`] report `*` in their messages.
//!
//! Objects with a `do_operation` handler (GMP, `BCMath\Number`) do not exist here, so the
//! `ZEND_TRY_BINARY_OBJECT_OPERATION` steps are no-ops.

use std::fmt;

use crate::array::Array;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::math;
use crate::string::PhpString;
use crate::types::zend_zval_type_name;
use crate::value::{Number, Value};

/// A binary operator of the engine (the PHP operator a Twig operator compiles to).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `+` (`add_function`; array union for two arrays).
    Add,
    /// `-` (`sub_function`).
    Sub,
    /// `*` (`mul_function`).
    Mul,
    /// `/` (`div_function`).
    Div,
    /// `%` (`mod_function`).
    Mod,
    /// `**` (`pow_function`).
    Pow,
    /// `.` (`concat_function`; Twig `~`).
    Concat,
    /// `&` (`bitwise_and_function`; Twig `b-and`).
    BitAnd,
    /// `|` (`bitwise_or_function`; Twig `b-or`).
    BitOr,
    /// `^` (`bitwise_xor_function`; Twig `b-xor`).
    BitXor,
    /// `<<` (`shift_left_function`).
    ShiftLeft,
    /// `>>` (`shift_right_function`).
    ShiftRight,
}

impl BinaryOp {
    /// The operator as PHP spells it in `Unsupported operand types: %s %s %s`.
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::Pow => "**",
            BinaryOp::Concat => ".",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitOr => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::ShiftLeft => "<<",
            BinaryOp::ShiftRight => ">>",
        }
    }
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// Evaluates `a <op> b`.
pub fn binary_op(
    op: BinaryOp,
    a: &Value,
    b: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match op {
        BinaryOp::Add => add(a, b, diag),
        BinaryOp::Sub => sub(a, b, diag),
        BinaryOp::Mul => mul(a, b, diag),
        BinaryOp::Div => div(a, b, diag),
        BinaryOp::Mod => modulo(a, b, diag),
        BinaryOp::Pow => pow(a, b, diag),
        BinaryOp::Concat => concat(a, b, diag).map(Value::String),
        BinaryOp::BitAnd => bitwise_and(a, b, diag),
        BinaryOp::BitOr => bitwise_or(a, b, diag),
        BinaryOp::BitXor => bitwise_xor(a, b, diag),
        BinaryOp::ShiftLeft => shift_left(a, b, diag),
        BinaryOp::ShiftRight => shift_right(a, b, diag),
    }
}

/// Port of `zend_binop_error()`: `TypeError: Unsupported operand types: %s %s %s`.
pub fn binop_error(symbol: &str, a: &Value, b: &Value) -> PhpError {
    PhpError::type_error(format!(
        "Unsupported operand types: {} {} {}",
        zend_zval_type_name(a),
        symbol,
        zend_zval_type_name(b)
    ))
}

/// `zendi_try_convert_scalar_to_number()` for both operands, op1 first; the binop error on failure.
fn numbers(
    symbol: &str,
    a: &Value,
    b: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<(Number, Number)> {
    let Some(na) = convert::try_to_number(a, diag)? else {
        return Err(binop_error(symbol, a, b));
    };
    let Some(nb) = convert::try_to_number(b, diag)? else {
        return Err(binop_error(symbol, a, b));
    };
    Ok((na, nb))
}

/// `zendi_try_get_long()` of one operand of `% << >> & | ^` (`convert_op1_op2_long`); the binop
/// error on failure.
fn long_operand(
    v: &Value,
    symbol: &str,
    a: &Value,
    b: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        _ => convert::try_get_long(v, diag)?.ok_or_else(|| binop_error(symbol, a, b)),
    }
}

/// Port of `fast_long_add_function()`: overflow gives `(double) a + (double) b`.
fn long_add(a: i64, b: i64) -> Value {
    match a.checked_add(b) {
        Some(r) => Value::Int(r),
        None => Value::Float(a as f64 + b as f64),
    }
}

/// Port of `fast_long_sub_function()`: overflow gives `(double) a - (double) b`.
fn long_sub(a: i64, b: i64) -> Value {
    match a.checked_sub(b) {
        Some(r) => Value::Int(r),
        None => Value::Float(a as f64 - b as f64),
    }
}

/// Port of `ZEND_SIGNED_MULTIPLY_LONG()` (PHPSRC/Zend/zend_multiply.h): overflow gives
/// `(double) a * (double) b`.
fn long_mul(a: i64, b: i64) -> Value {
    match a.checked_mul(b) {
        Some(r) => Value::Int(r),
        None => Value::Float(a as f64 * b as f64),
    }
}

/// Port of `add_function_array()`: the left array, then every element of the right array whose
/// key is absent (`zend_hash_merge(..., overwrite = 0)`), in the right array's order.
pub fn array_union(a: &Array, b: &Array) -> Array {
    let mut result = a.clone();
    for (k, v) in b.iter() {
        result.insert_if_absent(k.clone(), v.clone());
    }
    result
}

/// Port of `add_function_fast()`.
fn add_fast(a: &Value, b: &Value) -> Option<Value> {
    Some(match (a, b) {
        (Value::Int(x), Value::Int(y)) => long_add(*x, *y),
        (Value::Float(x), Value::Float(y)) => Value::Float(x + y),
        (Value::Int(x), Value::Float(y)) => Value::Float(*x as f64 + y),
        (Value::Float(x), Value::Int(y)) => Value::Float(x + *y as f64),
        (Value::Array(x), Value::Array(y)) => Value::Array(array_union(x, y)),
        _ => return None,
    })
}

fn add_numbers(a: Number, b: Number) -> Value {
    match (a, b) {
        (Number::Int(x), Number::Int(y)) => long_add(x, y),
        (x, y) => Value::Float(x.to_f64() + y.to_f64()),
    }
}

/// Port of `add_function()` (PHPSRC/Zend/zend_operators.c): `a + b`.
///
/// Numbers add with overflow to float; two arrays give the union ([`array_union`]); other operands
/// go through `zendi_try_convert_scalar_to_number()` (leading-numeric strings warn `A non-numeric
/// value encountered`; non-numeric strings, arrays and objects throw
/// `Unsupported operand types: string + int`).
pub fn add(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    if let Some(r) = add_fast(a, b) {
        return Ok(r);
    }
    let (na, nb) = numbers("+", a, b, diag)?;
    Ok(add_numbers(na, nb))
}

/// Port of `sub_function_fast()`.
fn sub_numbers(a: Number, b: Number) -> Value {
    match (a, b) {
        (Number::Int(x), Number::Int(y)) => long_sub(x, y),
        (Number::Float(x), Number::Float(y)) => Value::Float(x - y),
        (Number::Int(x), Number::Float(y)) => Value::Float(x as f64 - y),
        (Number::Float(x), Number::Int(y)) => Value::Float(x - y as f64),
    }
}

/// The value as a number when it already is one (the `*_function_fast()` type pairs).
fn as_number(v: &Value) -> Option<Number> {
    match v {
        Value::Int(i) => Some(Number::Int(*i)),
        Value::Float(d) => Some(Number::Float(*d)),
        _ => None,
    }
}

/// Port of `sub_function()`: `a - b`.
pub fn sub(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    if let (Some(x), Some(y)) = (as_number(a), as_number(b)) {
        return Ok(sub_numbers(x, y));
    }
    let (na, nb) = numbers("-", a, b, diag)?;
    Ok(sub_numbers(na, nb))
}

/// Port of `mul_function_fast()`.
fn mul_numbers(a: Number, b: Number) -> Value {
    match (a, b) {
        (Number::Int(x), Number::Int(y)) => long_mul(x, y),
        (Number::Float(x), Number::Float(y)) => Value::Float(x * y),
        (Number::Int(x), Number::Float(y)) => Value::Float(x as f64 * y),
        (Number::Float(x), Number::Int(y)) => Value::Float(x * y as f64),
    }
}

/// Port of `mul_function()`: `a * b`.
pub fn mul(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    if let (Some(x), Some(y)) = (as_number(a), as_number(b)) {
        return Ok(mul_numbers(x, y));
    }
    let (na, nb) = numbers("*", a, b, diag)?;
    Ok(mul_numbers(na, nb))
}

/// Unary minus `-a`: PHP compiles it to `a * -1` (`zend_compile_unary_pm()`), so `-"abc"` throws
/// `Unsupported operand types: string * int` and `-PHP_INT_MIN` is a float.
pub fn negate(a: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    mul(a, &Value::Int(-1), diag)
}

/// Unary plus `+a`: compiled to `a * 1` (`zend_compile_unary_pm()`).
pub fn unary_plus(a: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    mul(a, &Value::Int(1), diag)
}

/// `DivisionByZeroError: Division by zero`.
fn division_by_zero() -> PhpError {
    PhpError::division_by_zero("Division by zero")
}

/// Port of `div_function_base()` for two numbers: `None` is `DIV_BY_ZERO`.
fn div_numbers(a: Number, b: Number) -> Option<Value> {
    Some(match (a, b) {
        (Number::Int(x), Number::Int(y)) => {
            if y == 0 {
                return None;
            } else if y == -1 && x == i64::MIN {
                // Prevent overflow error/crash
                Value::Float(i64::MIN as f64 / -1.0)
            } else if x % y == 0 {
                // integer
                Value::Int(x / y)
            } else {
                Value::Float(x as f64 / y as f64)
            }
        }
        (Number::Float(x), Number::Float(y)) => {
            if y == 0.0 {
                return None;
            }
            Value::Float(x / y)
        }
        (Number::Float(x), Number::Int(y)) => {
            if y == 0 {
                return None;
            }
            Value::Float(x / y as f64)
        }
        (Number::Int(x), Number::Float(y)) => {
            if y == 0.0 {
                return None;
            }
            Value::Float(x as f64 / y)
        }
    })
}

/// Port of `div_function()`: `a / b`.
///
/// `int / int` is an int when exact, else a float; division by `0`, `0.0` or `-0.0` (also after
/// conversion: `"0"`, `null`, `false`) throws `DivisionByZeroError: Division by zero`, after the
/// operand conversion (so `"abc" / 0` is a `TypeError`).
pub fn div(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    if let (Some(x), Some(y)) = (as_number(a), as_number(b)) {
        return div_numbers(x, y).ok_or_else(division_by_zero);
    }
    let (na, nb) = numbers("/", a, b, diag)?;
    div_numbers(na, nb).ok_or_else(division_by_zero)
}

/// Twig's `//` operator (`FloorDivBinary`), which Twig compiles to `(int) floor(a / b)`: [`div`],
/// then PHP's `floor()` (ints become floats unchanged, floats are rounded down with
/// [`math::floor`]), then the `(int)` cast (out-of-range results wrap with `The float %s is not
/// representable as an int, cast occurred`).
pub fn floor_div(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    let floored = match div(a, b, diag)? {
        Value::Int(i) => i as f64,
        Value::Float(d) => math::floor(d),
        other => unreachable!("division yields a number, not {other:?}"),
    };
    Ok(Value::Int(convert::dval_to_lval(floored, diag)?))
}

/// Port of `mod_function()`: `a % b`.
///
/// Both operands are converted with `zendi_try_get_long()` (floats truncate with `Implicit
/// conversion from float %s to int loses precision`, leading-numeric strings warn); a zero divisor
/// throws `DivisionByZeroError: Modulo by zero`; the result has the sign of the dividend.
pub fn modulo(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    let l1 = long_operand(a, "%", a, b, diag)?;
    let l2 = long_operand(b, "%", a, b, diag)?;
    if l2 == 0 {
        // modulus by zero
        return Err(PhpError::division_by_zero("Modulo by zero"));
    }
    if l2 == -1 {
        // Prevent overflow error/crash if op1==LONG_MIN
        return Ok(Value::Int(0));
    }
    Ok(Value::Int(l1 % l2))
}

/// Port of `safe_pow()`: the C library `pow()` ([`math::pow`]) with the PHP 8.4 deprecation
/// `Power of base 0 and negative exponent is deprecated`.
pub fn safe_pow(base: f64, exponent: f64, diag: &mut dyn Diagnostics) -> PhpResult<f64> {
    if base == 0.0 && exponent < 0.0 {
        diag.deprecated("Power of base 0 and negative exponent is deprecated")?;
    }
    Ok(math::pow(base, exponent))
}

/// Port of `pow_function_base()` for two numbers: `int ** int` with a non-negative exponent uses
/// exponentiation by squaring and falls back to floats on overflow (from the partial results,
/// exactly like upstream); every other combination is [`safe_pow`].
pub fn pow_numbers(a: Number, b: Number, diag: &mut dyn Diagnostics) -> PhpResult<Number> {
    Ok(match (a, b) {
        (Number::Int(base), Number::Int(exp)) => {
            if exp >= 0 {
                let (mut l1, mut l2, mut i) = (1i64, base, exp);
                if i == 0 {
                    return Ok(Number::Int(1));
                } else if l2 == 0 {
                    return Ok(Number::Int(0));
                }
                while i >= 1 {
                    if i % 2 != 0 {
                        i -= 1;
                        match l1.checked_mul(l2) {
                            Some(v) => l1 = v,
                            None => {
                                let dval = l1 as f64 * l2 as f64;
                                return Ok(Number::Float(
                                    dval * safe_pow(l2 as f64, i as f64, diag)?,
                                ));
                            }
                        }
                    } else {
                        i /= 2;
                        match l2.checked_mul(l2) {
                            Some(v) => l2 = v,
                            None => {
                                let dval = l2 as f64 * l2 as f64;
                                return Ok(Number::Float(
                                    l1 as f64 * safe_pow(dval, i as f64, diag)?,
                                ));
                            }
                        }
                    }
                }
                // i == 0
                Number::Int(l1)
            } else {
                Number::Float(safe_pow(base as f64, exp as f64, diag)?)
            }
        }
        (x, y) => Number::Float(safe_pow(x.to_f64(), y.to_f64(), diag)?),
    })
}

/// Port of `pow_function()`: `a ** b` (also PHP's `pow()` function).
pub fn pow(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    if let (Some(x), Some(y)) = (as_number(a), as_number(b)) {
        return pow_numbers(x, y, diag).map(Value::from);
    }
    let (na, nb) = numbers("**", a, b, diag)?;
    pow_numbers(na, nb, diag).map(Value::from)
}

/// `ZSTR_MAX_LEN` on 64-bit builds: `SIZE_MAX - ZEND_MM_ALIGNED_SIZE(_ZSTR_HEADER_SIZE + 1)`.
const ZSTR_MAX_LEN: usize = usize::MAX - 32;

/// The string operand of `.`: strings as they are, everything else through
/// `zval_try_get_string_func()` ([`convert::to_string`]).
fn string_operand(v: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    match v {
        Value::String(s) => Ok(s.clone()),
        _ => convert::to_string(v, diag),
    }
}

/// Port of `concat_function()`: `a . b` (Twig `~`).
///
/// Operands are converted left to right with PHP string conversion (floats with precision 14,
/// arrays → `"Array"` with `Array to string conversion`, objects through `__toString()` or
/// `Error: Object of class X could not be converted to string`).
pub fn concat(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let s1 = string_operand(a, diag)?;
    let s2 = string_operand(b, diag)?;
    if s1.is_empty() {
        return Ok(s2);
    }
    if s2.is_empty() {
        return Ok(s1);
    }
    if s1.len() > ZSTR_MAX_LEN - s2.len() {
        return Err(PhpError::error("String size overflow"));
    }
    Ok(PhpString::concat(&[&s1, &s2]))
}

/// The bytewise `|`, `&`, `^` of two strings (`bitwise_*_function()`): `|` keeps the length of
/// the longer string (its tail is copied), `&` and `^` the length of the shorter one.
fn bytewise(s1: &[u8], s2: &[u8], keep_longer: bool, f: impl Fn(u8, u8) -> u8) -> PhpString {
    let (longer, shorter) = if s1.len() >= s2.len() {
        (s1, s2)
    } else {
        (s2, s1)
    };
    let mut out: Vec<u8> = longer.iter().zip(shorter).map(|(&l, &s)| f(l, s)).collect();
    if keep_longer {
        out.extend_from_slice(&longer[shorter.len()..]);
    }
    PhpString::from(out)
}

/// Port of `bitwise_or_function()`: `a | b` (Twig `b-or`). Two strings are or-ed bytewise;
/// otherwise both operands go through `zendi_try_get_long()`.
pub fn bitwise_or(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    if let (Value::Int(x), Value::Int(y)) = (a, b) {
        return Ok(Value::Int(x | y));
    }
    if let (Value::String(x), Value::String(y)) = (a, b) {
        return Ok(Value::String(bytewise(x, y, true, |l, s| l | s)));
    }
    let l1 = long_operand(a, "|", a, b, diag)?;
    let l2 = long_operand(b, "|", a, b, diag)?;
    Ok(Value::Int(l1 | l2))
}

/// Port of `bitwise_and_function()`: `a & b` (Twig `b-and`).
pub fn bitwise_and(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    if let (Value::Int(x), Value::Int(y)) = (a, b) {
        return Ok(Value::Int(x & y));
    }
    if let (Value::String(x), Value::String(y)) = (a, b) {
        return Ok(Value::String(bytewise(x, y, false, |l, s| l & s)));
    }
    let l1 = long_operand(a, "&", a, b, diag)?;
    let l2 = long_operand(b, "&", a, b, diag)?;
    Ok(Value::Int(l1 & l2))
}

/// Port of `bitwise_xor_function()`: `a ^ b` (Twig `b-xor`).
pub fn bitwise_xor(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    if let (Value::Int(x), Value::Int(y)) = (a, b) {
        return Ok(Value::Int(x ^ y));
    }
    if let (Value::String(x), Value::String(y)) = (a, b) {
        return Ok(Value::String(bytewise(x, y, false, |l, s| l ^ s)));
    }
    let l1 = long_operand(a, "^", a, b, diag)?;
    let l2 = long_operand(b, "^", a, b, diag)?;
    Ok(Value::Int(l1 ^ l2))
}

/// Port of `bitwise_not_function()`: `~a`. Ints are complemented, floats first converted with
/// `zend_dval_to_lval_safe()`, strings complemented bytewise; anything else throws
/// `TypeError: Cannot perform bitwise not on %s` ([`zend_zval_value_name`](crate::types::zend_zval_value_name)).
pub fn bitwise_not(a: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    match a {
        Value::Int(i) => Ok(Value::Int(!i)),
        Value::Float(d) => Ok(Value::Int(!convert::dval_to_lval_safe(*d, diag)?)),
        Value::String(s) => Ok(Value::String(PhpString::from(
            s.iter().map(|b| !b).collect::<Vec<u8>>(),
        ))),
        _ => Err(PhpError::type_error(format!(
            "Cannot perform bitwise not on {}",
            crate::types::zend_zval_value_name(a)
        ))),
    }
}

/// `ArithmeticError: Bit shift by negative number`.
fn negative_shift() -> PhpError {
    PhpError::arithmetic_error("Bit shift by negative number")
}

/// Port of `shift_left_function()`: `a << b`. Counts of 64 or more give 0; negative counts throw
/// `ArithmeticError: Bit shift by negative number`; the shift is done on the unsigned bits.
pub fn shift_left(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    let l1 = long_operand(a, "<<", a, b, diag)?;
    let l2 = long_operand(b, "<<", a, b, diag)?;
    // prevent wrapping quirkiness on some processors where << 64 + x == << x
    if (l2 as u64) >= 64 {
        if l2 > 0 {
            return Ok(Value::Int(0));
        }
        return Err(negative_shift());
    }
    // Perform shift on unsigned numbers to get well-defined wrap behavior.
    Ok(Value::Int(((l1 as u64) << l2) as i64))
}

/// Port of `shift_right_function()`: `a >> b` (arithmetic shift). Counts of 64 or more give 0 or
/// -1 by sign; negative counts throw `ArithmeticError: Bit shift by negative number`.
pub fn shift_right(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    let l1 = long_operand(a, ">>", a, b, diag)?;
    let l2 = long_operand(b, ">>", a, b, diag)?;
    // prevent wrapping quirkiness on some processors where >> 64 + x == >> x
    if (l2 as u64) >= 64 {
        if l2 > 0 {
            return Ok(Value::Int(if l1 < 0 { -1 } else { 0 }));
        }
        return Err(negative_shift());
    }
    Ok(Value::Int(l1 >> l2))
}

/// Port of `boolean_not_function()`: `!a` (Twig `not`), PHP truthiness ([`convert::to_bool`]; NaN
/// warns `unexpected NAN value was coerced to bool`).
pub fn boolean_not(a: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    Ok(!convert::to_bool(a, diag)?)
}

/// Port of `boolean_xor_function()`: `a xor b` (Twig `xor`); both operands are evaluated.
pub fn boolean_xor(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    let x = convert::to_bool(a, diag)?;
    let y = convert::to_bool(b, diag)?;
    Ok(x ^ y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{Diagnostic, Level};

    fn run(
        f: impl Fn(&Value, &Value, &mut dyn Diagnostics) -> PhpResult<Value>,
        a: impl Into<Value>,
        b: impl Into<Value>,
    ) -> (Result<Value, String>, Vec<Diagnostic>) {
        let mut diag = Vec::new();
        let r = f(&a.into(), &b.into(), &mut diag).map_err(|e| e.to_string());
        (r, diag)
    }

    #[test]
    fn arithmetic() {
        assert_eq!(
            run(add, i64::MAX, 1).0,
            Ok(Value::Float(9.223372036854776e18))
        );
        assert_eq!(
            run(sub, i64::MIN, 1).0,
            Ok(Value::Float(-9.223372036854776e18))
        );
        assert_eq!(
            run(mul, i64::MAX, 2).0,
            Ok(Value::Float(1.8446744073709552e19))
        );
        assert_eq!(run(add, "1", "1").0, Ok(Value::Int(2)));
        assert_eq!(run(add, "1.0", 1).0, Ok(Value::Float(2.0)));
        let (r, d) = run(add, "12abc", 1);
        assert_eq!(r, Ok(Value::Int(13)));
        assert_eq!(
            d,
            vec![Diagnostic::new(
                Level::Warning,
                "A non-numeric value encountered"
            )]
        );
        assert_eq!(
            run(add, "abc", 1).0,
            Err("TypeError: Unsupported operand types: string + int".into())
        );
        let (r, d) = run(add, "1x", "abc");
        assert_eq!(
            r,
            Err("TypeError: Unsupported operand types: string + string".into())
        );
        assert_eq!(d.len(), 1);
        let (r, d) = run(add, "abc", "1x");
        assert!(r.is_err());
        assert!(d.is_empty());
        assert_eq!(run(div, 6, 3).0, Ok(Value::Int(2)));
        assert_eq!(run(div, 7, 2).0, Ok(Value::Float(3.5)));
        assert_eq!(
            run(div, i64::MIN, -1).0,
            Ok(Value::Float(9.223372036854776e18))
        );
        assert_eq!(
            run(div, 1, -0.0).0,
            Err("DivisionByZeroError: Division by zero".into())
        );
        assert_eq!(
            run(div, "abc", 0).0,
            Err("TypeError: Unsupported operand types: string / int".into())
        );
        assert_eq!(run(modulo, -7, 3).0, Ok(Value::Int(-1)));
        assert_eq!(run(modulo, 7, -3).0, Ok(Value::Int(1)));
        assert_eq!(run(modulo, i64::MIN, -1).0, Ok(Value::Int(0)));
        assert_eq!(
            run(modulo, 7, Value::Null).0,
            Err("DivisionByZeroError: Modulo by zero".into())
        );
        assert_eq!(
            run(|a, _, d| negate(a, d), "abc", Value::Null).0,
            Err("TypeError: Unsupported operand types: string * int".into())
        );
        assert_eq!(
            run(|a, _, d| negate(a, d), i64::MIN, 0).0,
            Ok(Value::Float(9.223372036854776e18))
        );
        let neg_zero = negate(&Value::Float(0.0), &mut Vec::new()).unwrap();
        assert!(matches!(neg_zero, Value::Float(f) if f == 0.0 && f.is_sign_negative()));
    }

    #[test]
    fn powers() {
        assert_eq!(run(pow, 2, 62).0, Ok(Value::Int(4611686018427387904)));
        assert_eq!(run(pow, 2, 63).0, Ok(Value::Float(9.223372036854776e18)));
        assert_eq!(run(pow, -2, 63).0, Ok(Value::Int(i64::MIN)));
        assert_eq!(run(pow, 7, 22).0, Ok(Value::Int(3909821048582988049)));
        assert_eq!(run(pow, 3, 40).0, Ok(Value::Float(1.2157665459056929e19)));
        assert_eq!(run(pow, -1, i64::MAX).0, Ok(Value::Int(-1)));
        assert_eq!(run(pow, 1, -1).0, Ok(Value::Float(1.0)));
        assert_eq!(run(pow, 0, 0).0, Ok(Value::Int(1)));
        let (r, d) = run(pow, 0, -1);
        assert_eq!(r, Ok(Value::Float(f64::INFINITY)));
        assert_eq!(
            d,
            vec![Diagnostic::new(
                Level::Deprecated,
                "Power of base 0 and negative exponent is deprecated"
            )]
        );
        assert_eq!(
            run(pow, Value::Array(Array::new()), 2).0,
            Err("TypeError: Unsupported operand types: array ** int".into())
        );
    }

    #[test]
    fn bitwise_and_strings() {
        assert_eq!(run(bitwise_or, "5", "3").0, Ok(Value::from("7")));
        assert_eq!(run(bitwise_and, "abc", "  ").0, Ok(Value::from("  ")));
        assert_eq!(run(bitwise_xor, "12", "3").0, Ok(Value::from(&b"\x02"[..])));
        assert_eq!(run(bitwise_or, "ab", "c").0, Ok(Value::from("cb")));
        assert_eq!(run(shift_left, 1, 63).0, Ok(Value::Int(i64::MIN)));
        assert_eq!(run(shift_left, 1, 64).0, Ok(Value::Int(0)));
        assert_eq!(run(shift_right, -8, 100).0, Ok(Value::Int(-1)));
        assert_eq!(
            run(shift_right, 1, -1).0,
            Err("ArithmeticError: Bit shift by negative number".into())
        );
        assert_eq!(
            run(|a, _, d| bitwise_not(a, d), true, 0).0,
            Err("TypeError: Cannot perform bitwise not on true".into())
        );
        assert_eq!(
            run(|a, _, d| bitwise_not(a, d), "\x00", 0).0,
            Ok(Value::from(&b"\xff"[..]))
        );
        assert_eq!(run(concat_v, 1.5, "").0, Ok(Value::from("1.5")));
        assert_eq!(run(concat_v, -0.0, "x").0, Ok(Value::from("-0x")));
    }

    fn concat_v(a: &Value, b: &Value, d: &mut dyn Diagnostics) -> PhpResult<Value> {
        concat(a, b, d).map(Value::String)
    }
}
