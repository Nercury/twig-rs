//! `sprintf()` and `vsprintf()`: port of `php_formatted_print()` and its helpers
//! (PHPSRC/ext/standard/formatted_print.c), reference/php/stdlib.md §5.1.
//!
//! Grammar of a conversion: `%` [argnum `N$`] [flags `-+ 0` or `'c`] [width or `*`[`N$`]]
//! [`.` [precision or `*`[`N$`]]] [`l`] specifier. The upstream quirks are kept: integer
//! padding with `0` turns into spaces when left-aligned (but not for `x`/`o`/`b`/floats/strings),
//! any explicit precision empties `%x`/`%o`/`%b` output, `NaN`/`INF` ignore the width and, with
//! `+` and `0` padding, lose their first letter (`+aN`), and a missing argument only skips the
//! conversion so the rest of the format is still scanned (and can still fail) before the
//! `ArgumentCountError`.

use super::{check_alloc, check_arg_count, conv};
use crate::array::Array;
use crate::convert;
use crate::diag::{Diagnostics, Ignore};
use crate::error::{PhpError, PhpResult};
use crate::float;
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::Value;

/// `ADJ_WIDTH`
const ADJ_WIDTH: u8 = 1;
/// `ADJ_PRECISION`
const ADJ_PRECISION: u8 = 2;
/// `FLOAT_PRECISION`: default precision of the float conversions.
const FLOAT_PRECISION: i32 = 6;
/// `MAX_FLOAT_PRECISION`
const MAX_FLOAT_PRECISION: i32 = 53;
/// C `INT_MAX`.
const INT_MAX: i64 = i32::MAX as i64;
/// `ARG_NUM_NEXT`: no `N$` given, take the next argument.
const ARG_NUM_NEXT: i32 = -1;

const HEXCHARS: &[u8; 16] = b"0123456789abcdef";
const HEXCHARS_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// `ALIGN_LEFT` / `ALIGN_RIGHT`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Right,
}

/// Port of `PHP_FUNCTION(sprintf)` (`sprintf(string $format, mixed ...$values): string`):
/// `$format` coerced to string (`null` deprecated, arrays rejected), then [`formatted_print`]
/// with `nb_additional_parameters = 1`. Twig's `format` filter is
/// `\sprintf($format ?? '', ...$values)`.
pub fn sprintf(
    format: &Value,
    values: &[Value],
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let format = param::parse_str(format, &Arg::new("sprintf", 1, "format"), diag)?;
    formatted_print("sprintf", &format, values, 1, diag)
}

/// [`sprintf`] called with an argument list (format first): `ArgumentCountError: sprintf()
/// expects at least 1 argument, 0 given` for an empty list.
pub fn sprintf_call(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    check_arg_count("sprintf", args, 1, None)?;
    sprintf(&args[0], &args[1..], diag).map(Value::String)
}

/// Port of `PHP_FUNCTION(vsprintf)` (`vsprintf(string $format, array $values): string`): the
/// array's values in order are the arguments; missing ones raise `ValueError: The arguments array
/// must contain %d items, %d given`.
pub fn vsprintf(
    format: &Value,
    values: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let format = param::parse_str(format, &Arg::new("vsprintf", 1, "format"), diag)?;
    let array: Array = param::parse_array(values, &Arg::new("vsprintf", 2, "values"))?;
    let values: Vec<Value> = array.values().cloned().collect();
    formatted_print("vsprintf", &format, &values, -1, diag)
}

/// [`vsprintf`] called with an argument list: exactly two arguments.
pub fn vsprintf_call(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    check_arg_count("vsprintf", args, 2, Some(2))?;
    vsprintf(&args[0], &args[1], diag).map(Value::String)
}

/// Port of `php_formatted_print()` (PHPSRC/ext/standard/formatted_print.c).
///
/// `function` names the caller for the `Requested precision` notice (`sprintf(): ...`).
/// `nb_additional_parameters` selects the missing-argument error like upstream: `-1` gives
/// `ValueError: The arguments array must contain %d items, %d given` (vsprintf), `n >= 0` gives
/// `ArgumentCountError: %d arguments are required, %d given` with both counts raised by `n`.
///
/// Errors (verbatim `ValueError`s): `Argument number specifier must be greater than zero and less
/// than 2147483647`, `Missing padding character`, `Width must be an integer`, `Width must be
/// between 0 and 2147483647`, `Precision must be an integer`, `Precision must be between -1 and
/// 2147483647`, `Precision must be between 0 and 2147483647`, `Precision -1 is only supported for
/// %g, %G, %h and %H`, `Missing format specifier at end of string`, `Unknown format specifier
/// "%c"` (cut at the specifier when it is a NUL byte, like the C string upstream builds).
///
/// Argument conversions are PHP's (`%s`: string conversion with `Array to string conversion`
/// and `__toString()`; `%d %u %c %o %x %X %b`: `(int)`; floats: `(float)`). When a `__toString()`
/// conversion throws, upstream keeps formatting with an empty string and throws the first
/// exception at the end; meanwhile no further `__toString()` runs and the diagnostics raised are
/// lost, because the engine does not call the (user) error handler while an exception is pending
/// (`zend_call_function()`). This port does the same: they never reach `diag`. An exception
/// returned by `diag` itself (an error handler that throws) is treated the same way: formatting
/// goes on and it is thrown at the end, or becomes the `previous` of a later error. (A direct call
/// `sprintf('%s', ...)` with a literal format is compiled to string interpolation by PHP 8.4+ and
/// behaves differently; Twig always calls `sprintf()` with spread arguments.)
pub fn formatted_print(
    function: &str,
    format: &[u8],
    args: &[Value],
    nb_additional_parameters: i32,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let mut f = Formatter {
        function,
        format,
        args,
        out: Vec::with_capacity(format.len().max(16)),
        pending: None,
        diag,
        ignore: Ignore,
    };
    let result = f.run(nb_additional_parameters);
    match (result, f.pending) {
        // An exception thrown while another one is pending gets it as `previous`
        // (zend_throw_exception_internal()).
        (Err(e), Some(pending)) if e.previous().is_none() => Err(e.with_previous(pending)),
        (Err(e), _) | (Ok(()), Some(e)) => Err(e),
        (Ok(()), None) => Ok(PhpString::from(f.out)),
    }
}

/// The state of one `php_formatted_print()` call.
struct Formatter<'a, 'd> {
    function: &'a str,
    format: &'a [u8],
    args: &'a [Value],
    out: Vec<u8>,
    /// An exception thrown by a `%s` conversion: upstream finishes the format first.
    pending: Option<PhpError>,
    diag: &'d mut dyn Diagnostics,
    /// Where diagnostics go while an exception is pending.
    ignore: Ignore,
}

impl Formatter<'_, '_> {
    /// Runs a conversion (or a diagnostic) against the caller's sink, or against nothing while an
    /// exception is pending (the user error handler is not called then). An exception from the
    /// sink (an error handler that throws) becomes the pending exception: upstream's handler
    /// returns into the conversion, which completes, and formatting goes on. The conversion is
    /// then redone without diagnostics for its result (scalar conversions have no other effects).
    fn with_sink<T>(&mut self, f: impl Fn(&mut dyn Diagnostics) -> PhpResult<T>) -> PhpResult<T> {
        if self.pending.is_none() {
            match f(&mut *self.diag) {
                Ok(v) => return Ok(v),
                Err(e) => self.pending = Some(e),
            }
        }
        f(&mut self.ignore)
    }

    /// `*format` of the NUL-terminated C string: the byte at `i`, `0` past the end.
    fn at(&self, i: usize) -> u8 {
        self.format.get(i).copied().unwrap_or(0)
    }

    /// The main loop of `php_formatted_print()`.
    fn run(&mut self, nb_additional_parameters: i32) -> PhpResult<()> {
        let len = self.format.len();
        let argc = self.args.len().min(i32::MAX as usize) as i32;
        let mut p = 0usize;
        let mut currarg: i32 = 0;
        let mut max_missing_argnum: i32 = -1;

        while p < len {
            let mut expprec = false;

            match memchr::memchr(b'%', &self.format[p..]) {
                None => {
                    self.out.extend_from_slice(&self.format[p..]);
                    break;
                }
                Some(off) => {
                    self.out.extend_from_slice(&self.format[p..p + off]);
                    p += off;
                }
            }
            p += 1; // skip the '%'

            if self.at(p) == b'%' {
                self.out.push(b'%');
                p += 1;
                continue;
            }

            // starting a new format specifier, reset variables
            let mut alignment = Align::Right;
            let mut adjusting: u8 = 0;
            let mut padding = b' ';
            let mut always_sign = false;
            let width: i32;
            let precision: i32;
            let mut argnum: i32;

            if self.at(p).is_ascii_alphabetic() {
                width = 0;
                precision = 0;
                argnum = ARG_NUM_NEXT;
            } else {
                // first look for argnum
                argnum = self.get_argnum(&mut p)?;

                // after argnum comes modifiers
                loop {
                    let c = self.at(p);
                    if c == b' ' || c == b'0' {
                        padding = c;
                    } else if c == b'-' {
                        alignment = Align::Left;
                        // space padding, the default
                    } else if c == b'+' {
                        always_sign = true;
                    } else if c == b'\'' {
                        if len.saturating_sub(p) > 1 {
                            p += 1;
                            padding = self.at(p);
                        } else {
                            return Err(PhpError::value_error("Missing padding character"));
                        }
                    } else {
                        break;
                    }
                    p += 1;
                }

                // after modifiers comes width
                if self.at(p) == b'*' {
                    p += 1;
                    let mut width_argnum = self.get_argnum(&mut p)?;
                    if width_argnum == ARG_NUM_NEXT {
                        width_argnum = currarg;
                        currarg += 1;
                    }
                    if width_argnum >= argc {
                        max_missing_argnum = max_missing_argnum.max(width_argnum);
                        continue;
                    }
                    let Value::Int(w) = self.args[width_argnum as usize] else {
                        return Err(PhpError::value_error("Width must be an integer"));
                    };
                    if !(0..=INT_MAX).contains(&w) {
                        return Err(PhpError::value_error(format!(
                            "Width must be between 0 and {INT_MAX}"
                        )));
                    }
                    width = w as i32;
                    adjusting |= ADJ_WIDTH;
                } else if self.at(p).is_ascii_digit() {
                    width = self.getnumber(&mut p);
                    if width < 0 {
                        return Err(PhpError::value_error(format!(
                            "Width must be between 0 and {INT_MAX}"
                        )));
                    }
                    adjusting |= ADJ_WIDTH;
                } else {
                    width = 0;
                }

                // after width and argnum comes precision
                if self.at(p) == b'.' {
                    p += 1;
                    if self.at(p) == b'*' {
                        p += 1;
                        let mut prec_argnum = self.get_argnum(&mut p)?;
                        if prec_argnum == ARG_NUM_NEXT {
                            prec_argnum = currarg;
                            currarg += 1;
                        }
                        if prec_argnum >= argc {
                            max_missing_argnum = max_missing_argnum.max(prec_argnum);
                            continue;
                        }
                        let Value::Int(pr) = self.args[prec_argnum as usize] else {
                            return Err(PhpError::value_error("Precision must be an integer"));
                        };
                        if !(-1..=INT_MAX).contains(&pr) {
                            return Err(PhpError::value_error(format!(
                                "Precision must be between -1 and {INT_MAX}"
                            )));
                        }
                        precision = pr as i32;
                        adjusting |= ADJ_PRECISION;
                        expprec = true;
                    } else if self.at(p).is_ascii_digit() {
                        precision = self.getnumber(&mut p);
                        if precision < 0 {
                            return Err(PhpError::value_error(format!(
                                "Precision must be between 0 and {INT_MAX}"
                            )));
                        }
                        adjusting |= ADJ_PRECISION;
                        expprec = true;
                    } else {
                        precision = 0;
                        adjusting |= ADJ_PRECISION;
                    }
                } else {
                    precision = 0;
                }
            }

            if self.at(p) == b'l' {
                p += 1;
            }

            if argnum == ARG_NUM_NEXT {
                argnum = currarg;
                currarg += 1;
            }
            if argnum >= argc {
                max_missing_argnum = max_missing_argnum.max(argnum);
                continue;
            }

            let spec = self.at(p);
            if expprec && precision == -1 && !matches!(spec, b'g' | b'G' | b'h' | b'H') {
                return Err(PhpError::value_error(
                    "Precision -1 is only supported for %g, %G, %h and %H",
                ));
            }

            // now we expect to find a type specifier
            let args = self.args;
            let tmp = &args[argnum as usize];
            let width = width as usize;
            match spec {
                b's' => {
                    let s = self.tmp_string(tmp)?;
                    self.append_string(
                        &s,
                        width,
                        precision.max(0) as usize,
                        padding,
                        alignment,
                        false,
                        expprec,
                        false,
                    )?;
                }
                b'd' => {
                    let n = self.with_sink(|d| convert::to_long(tmp, d))?;
                    self.append_int(n, width, padding, alignment, always_sign)?;
                }
                b'u' => {
                    let n = self.with_sink(|d| convert::to_long(tmp, d))?;
                    self.append_uint(n as u64, width, padding, alignment)?;
                }
                b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'h' | b'H' => {
                    let d = self.with_sink(|d| convert::to_double(tmp, d))?;
                    self.append_double(
                        d,
                        width,
                        padding,
                        alignment,
                        precision,
                        adjusting,
                        spec,
                        always_sign,
                    )?;
                }
                b'c' => {
                    let n = self.with_sink(|d| convert::to_long(tmp, d))?;
                    check_alloc(self.out.len() + 1)?;
                    self.out.push(n as u8);
                }
                b'o' => {
                    let n = self.with_sink(|d| convert::to_long(tmp, d))?;
                    self.append_2n(n, width, padding, alignment, 3, HEXCHARS, expprec)?;
                }
                b'x' => {
                    let n = self.with_sink(|d| convert::to_long(tmp, d))?;
                    self.append_2n(n, width, padding, alignment, 4, HEXCHARS, expprec)?;
                }
                b'X' => {
                    let n = self.with_sink(|d| convert::to_long(tmp, d))?;
                    self.append_2n(n, width, padding, alignment, 4, HEXCHARS_UPPER, expprec)?;
                }
                b'b' => {
                    let n = self.with_sink(|d| convert::to_long(tmp, d))?;
                    self.append_2n(n, width, padding, alignment, 1, HEXCHARS, expprec)?;
                }
                b'%' => self.out.push(b'%'),
                0 if p >= len => {
                    return Err(PhpError::value_error(
                        "Missing format specifier at end of string",
                    ));
                }
                0 => {
                    // zend_value_error() builds a C string: the message stops at the NUL.
                    return Err(PhpError::value_error("Unknown format specifier \""));
                }
                c => {
                    return Err(PhpError::value_error(PhpString::concat(&[
                        b"Unknown format specifier \"",
                        &[c],
                        b"\"",
                    ])));
                }
            }
            p += 1;
        }

        if max_missing_argnum >= 0 {
            return Err(if nb_additional_parameters == -1 {
                PhpError::value_error(format!(
                    "The arguments array must contain {} items, {} given",
                    i64::from(max_missing_argnum) + 1,
                    argc
                ))
            } else {
                PhpError::argument_count_error(format!(
                    "{} arguments are required, {} given",
                    i64::from(max_missing_argnum) + i64::from(nb_additional_parameters) + 1,
                    i64::from(argc) + i64::from(nb_additional_parameters)
                ))
            });
        }
        Ok(())
    }

    /// Port of `php_sprintf_getnumber()`: `ZEND_STRTOL(*buffer, &endptr, 10)`, then `-1` when the
    /// number is negative or `>= INT_MAX`. Callers stand on a digit (or on the `$` of an empty
    /// argnum, where nothing is consumed and the result is 0), so strtol's whitespace and sign
    /// handling never applies; overflowing digit runs are consumed whole (strtol saturates).
    fn getnumber(&self, p: &mut usize) -> i32 {
        let mut num: i64 = 0;
        while self.at(*p).is_ascii_digit() {
            num = num
                .saturating_mul(10)
                .saturating_add(i64::from(self.at(*p) - b'0'));
            *p += 1;
        }
        if !(0..INT_MAX).contains(&num) {
            -1
        } else {
            num as i32
        }
    }

    /// Port of `php_sprintf_get_argnum()`: `N$` → `N - 1`, else [`ARG_NUM_NEXT`] without
    /// consuming anything.
    fn get_argnum(&self, p: &mut usize) -> PhpResult<i32> {
        let mut temppos = *p;
        while self.at(temppos).is_ascii_digit() {
            temppos += 1;
        }
        if self.at(temppos) != b'$' {
            return Ok(ARG_NUM_NEXT);
        }
        let argnum = self.getnumber(p);
        if argnum <= 0 {
            return Err(PhpError::value_error(format!(
                "Argument number specifier must be greater than zero and less than {INT_MAX}"
            )));
        }
        *p += 1; // skip the '$'
        Ok(argnum - 1)
    }

    /// `zval_get_tmp_string()` with upstream's behaviour after an exception: the failed
    /// conversion yields `""`, later objects are not converted (no `__toString()` call, no new
    /// exception), and the first exception is kept for the end.
    fn tmp_string(&mut self, v: &Value) -> PhpResult<PhpString> {
        match v {
            Value::Object(o) => {
                if self.pending.is_some() {
                    return Ok(PhpString::empty());
                }
                match o.to_php_string(&mut *self.diag) {
                    Ok(s) => Ok(s),
                    Err(e) => {
                        self.pending = Some(e);
                        Ok(PhpString::empty())
                    }
                }
            }
            _ => self.with_sink(|d| convert::to_string(v, d)),
        }
    }

    /// Port of `php_sprintf_appendstring()`.
    #[allow(clippy::too_many_arguments)]
    fn append_string(
        &mut self,
        add: &[u8],
        min_width: usize,
        max_width: usize,
        padding: u8,
        alignment: Align,
        neg: bool,
        expprec: bool,
        always_sign: bool,
    ) -> PhpResult<()> {
        let mut copy_len = if expprec {
            max_width.min(add.len())
        } else {
            add.len()
        };
        let npad = min_width.saturating_sub(copy_len);
        let m_width = min_width.max(copy_len);
        let pos = self.out.len();

        if m_width > (INT_MAX as usize).saturating_sub(pos).saturating_sub(1) {
            // zend_error_noreturn(E_ERROR, ...): a fatal error upstream.
            return Err(PhpError::error(format!(
                "Field width {m_width} is too long"
            )));
        }
        check_alloc(pos + m_width)?;

        let mut add = add;
        if alignment == Align::Right {
            if (neg || always_sign) && padding == b'0' {
                self.out.push(if neg { b'-' } else { b'+' });
                add = &add[1..];
                copy_len -= 1;
            }
            self.out.extend(std::iter::repeat_n(padding, npad));
        }
        self.out.extend_from_slice(&add[..copy_len]);
        if alignment == Align::Left {
            self.out.extend(std::iter::repeat_n(padding, npad));
        }
        Ok(())
    }

    /// Port of `php_sprintf_appendint()`.
    fn append_int(
        &mut self,
        number: i64,
        width: usize,
        padding: u8,
        alignment: Align,
        always_sign: bool,
    ) -> PhpResult<()> {
        let neg = number < 0;
        // Can't right-pad 0's on integers
        let padding = if alignment == Align::Left && padding == b'0' {
            b' '
        } else {
            padding
        };
        let mut numbuf = Vec::with_capacity(21);
        if neg {
            numbuf.push(b'-');
        } else if always_sign {
            numbuf.push(b'+');
        }
        numbuf.extend_from_slice(number.unsigned_abs().to_string().as_bytes());
        self.append_string(
            &numbuf,
            width,
            0,
            padding,
            alignment,
            neg,
            false,
            always_sign,
        )
    }

    /// Port of `php_sprintf_appenduint()`.
    fn append_uint(
        &mut self,
        number: u64,
        width: usize,
        padding: u8,
        alignment: Align,
    ) -> PhpResult<()> {
        // Can't right-pad 0's on integers
        let padding = if alignment == Align::Left && padding == b'0' {
            b' '
        } else {
            padding
        };
        let numbuf = number.to_string();
        self.append_string(
            numbuf.as_bytes(),
            width,
            0,
            padding,
            alignment,
            false,
            false,
            false,
        )
    }

    /// Port of `php_sprintf_appenddouble()`.
    #[allow(clippy::too_many_arguments)]
    fn append_double(
        &mut self,
        number: f64,
        width: usize,
        padding: u8,
        alignment: Align,
        mut precision: i32,
        adjust: u8,
        fmt: u8,
        always_sign: bool,
    ) -> PhpResult<()> {
        if adjust & ADJ_PRECISION == 0 {
            precision = FLOAT_PRECISION;
        } else if precision > MAX_FLOAT_PRECISION {
            let message = format!(
                "{}(): Requested precision of {precision} digits was truncated to PHP maximum of {MAX_FLOAT_PRECISION} digits",
                self.function
            );
            self.with_sink(|d| d.notice(message.clone()))?;
            precision = MAX_FLOAT_PRECISION;
        }

        if number.is_nan() {
            let is_negative = number < 0.0;
            return self.append_string(
                b"NaN",
                3,
                0,
                padding,
                alignment,
                is_negative,
                false,
                always_sign,
            );
        }

        if number.is_infinite() {
            let is_negative = number < 0.0;
            let s: &[u8] = if is_negative { b"-INF" } else { b"INF" };
            return self.append_string(
                s,
                s.len(),
                0,
                padding,
                alignment,
                is_negative,
                false,
                always_sign,
            );
        }

        let mut s: Vec<u8> = Vec::with_capacity(32);
        let is_negative;
        match fmt {
            b'e' | b'E' | b'f' | b'F' => {
                // The C locale's decimal point for 'f' (LCONV_DECIMAL_POINT), '.' otherwise.
                let (digits, neg) = conv::conv_fp(
                    if fmt == b'f' { b'F' } else { fmt },
                    number,
                    false,
                    precision,
                    b'.',
                );
                is_negative = neg;
                if is_negative {
                    s.push(b'-');
                } else if always_sign {
                    s.push(b'+');
                }
                s.extend_from_slice(&digits);
            }
            _ => {
                // 'g', 'G', 'h', 'H'
                if precision == 0 {
                    precision = 1;
                }
                // The C locale's decimal point for 'g'/'G', '.' for 'h'/'H'.
                let exp_char = if fmt == b'G' || fmt == b'H' {
                    b'E'
                } else {
                    b'e'
                };
                let text = float::zend_gcvt(number, precision, b'.', exp_char);
                is_negative = text.starts_with('-');
                if !is_negative && always_sign {
                    s.push(b'+');
                }
                s.extend_from_slice(text.as_bytes());
            }
        }

        self.append_string(
            &s,
            width,
            0,
            padding,
            alignment,
            is_negative,
            false,
            always_sign,
        )
    }

    /// Port of `php_sprintf_append2n()`: base 2^`n` digits of the bit pattern; with `expprec`
    /// (any explicit precision) nothing but the padding is written.
    #[allow(clippy::too_many_arguments)]
    fn append_2n(
        &mut self,
        number: i64,
        width: usize,
        padding: u8,
        alignment: Align,
        n: u32,
        chartable: &[u8; 16],
        expprec: bool,
    ) -> PhpResult<()> {
        let andbits = (1u64 << n) - 1;
        let mut num = number as u64;
        let mut numbuf = Vec::with_capacity(64);
        loop {
            numbuf.push(chartable[(num & andbits) as usize]);
            num >>= n;
            if num == 0 {
                break;
            }
        }
        numbuf.reverse();
        self.append_string(&numbuf, width, 0, padding, alignment, false, expprec, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{Diagnostic, Level};

    fn sp(format: &str, args: &[Value]) -> String {
        let mut d: Vec<Diagnostic> = Vec::new();
        let r = formatted_print("sprintf", format.as_bytes(), args, 1, &mut d).unwrap();
        String::from_utf8(r.to_vec()).unwrap()
    }

    fn err(format: &[u8], args: &[Value]) -> (String, String) {
        let mut d: Vec<Diagnostic> = Vec::new();
        let e = formatted_print("sprintf", format, args, 1, &mut d).unwrap_err();
        (e.class().name().to_string(), e.message().to_string())
    }

    #[test]
    fn basics() {
        let i = Value::Int;
        let f = Value::Float;
        assert_eq!(
            sp(
                "%d|%5d|%-5d|%05d|%+d|%+05d",
                &[i(42), i(42), i(42), i(-42), i(5), i(5)]
            ),
            "42|   42|42   |-0042|+5|+0005"
        );
        assert_eq!(sp("%-05d|%-05x", &[i(42), i(255)]), "42   |ff000");
        assert_eq!(sp("%u", &[i(-1)]), "18446744073709551615");
        assert_eq!(
            sp(
                "%x|%X|%o|%b|%.2x|%5.2x|",
                &[i(255), i(255), i(8), i(5), i(255), i(255)]
            ),
            "ff|FF|10|101||     |"
        );
        assert_eq!(sp("%c%5c", &[i(65), i(321)]), "AA");
        assert_eq!(
            sp("%e|%.0e|%e", &[f(1234.5678), f(12345.0), f(0.0)]),
            "1.234568e+3|1e+4|0.000000e+0"
        );
        assert_eq!(
            sp(
                "%.1f|%.0f|%.0f|%05.1f",
                &[f(0.25), f(2.5), f(3.5), f(-2.34)]
            ),
            "0.2|2|4|-02.3"
        );
        assert_eq!(
            sp("%g|%g|%G", &[f(1e20), f(0.00001234), f(1e-10)]),
            "1.0e+20|1.234e-5|1.0E-10"
        );
        assert_eq!(
            sp(
                "%s|%.3s|%5s|%-5s|%'*5s|%-05s",
                &[
                    Value::from("abcdef"),
                    Value::from("abcdef"),
                    Value::from("ab"),
                    Value::from("ab"),
                    Value::from("ab"),
                    Value::from("ab"),
                ]
            ),
            "abcdef|abc|   ab|ab   |***ab|ab000"
        );
        assert_eq!(
            sp("%2$s %1$s %%", &[Value::from("a"), Value::from("b")]),
            "b a %"
        );
        assert_eq!(
            sp(
                "%+05f|%+05f|%010f|%-012f",
                &[f(f64::NAN), f(f64::INFINITY), f(f64::NEG_INFINITY), f(1.5)]
            ),
            "+aN|+NF|-INF|1.5000000000"
        );
        assert_eq!(
            sp("%f|%e|%g|%F", &[f(-0.0), f(-0.0), f(-0.0), f(-0.0)]),
            "0.000000|0.000000e+0|-0|0.000000"
        );
        assert_eq!(
            sp("%*d|%-*d|%.*f", &[i(5), i(1), i(3), i(2), i(2), f(1.005)]),
            "    1|2  |1.00"
        );
    }

    #[test]
    fn errors() {
        let one = [Value::Int(1)];
        assert_eq!(
            err(b"%", &[]),
            (
                "ArgumentCountError".into(),
                "2 arguments are required, 1 given".into()
            )
        );
        assert_eq!(
            err(b"%", &one).1,
            "Missing format specifier at end of string"
        );
        assert_eq!(err(b"%*d %z", &[]).1, "3 arguments are required, 1 given");
        assert_eq!(err(b"%z", &one).1, "Unknown format specifier \"z\"");
        assert_eq!(err(b"abc\0def%\0", &one).1, "Unknown format specifier \"");
        assert_eq!(
            err(b"%2147483647d", &one).1,
            "Width must be between 0 and 2147483647"
        );
        assert_eq!(
            err(b"%0$s", &one).1,
            "Argument number specifier must be greater than zero and less than 2147483647"
        );
        assert_eq!(err(b"%'", &one).1, "Missing padding character");
        assert_eq!(
            err(b"%*d", &[Value::from("5"), Value::Int(1)]).1,
            "Width must be an integer"
        );
        assert_eq!(
            err(b"%.*d", &[Value::Int(-1), Value::Int(1)]).1,
            "Precision -1 is only supported for %g, %G, %h and %H"
        );
        let mut d: Vec<Diagnostic> = Vec::new();
        let e = formatted_print("vsprintf", b"%s %s", &one, -1, &mut d).unwrap_err();
        assert_eq!(e.class().name(), "ValueError");
        assert_eq!(
            e.message(),
            "The arguments array must contain 2 items, 1 given"
        );
    }

    #[test]
    fn precision_notice() {
        let mut d: Vec<Diagnostic> = Vec::new();
        let r = formatted_print("vsprintf", b"%.60f", &[Value::Float(1.5)], -1, &mut d).unwrap();
        assert_eq!(r.len(), 55);
        assert_eq!(
            d,
            vec![Diagnostic::new(
                Level::Notice,
                "vsprintf(): Requested precision of 60 digits was truncated to PHP maximum of 53 digits"
            )]
        );
    }

    #[test]
    fn throwing_error_handler() {
        // Verified with set_error_handler(fn () => throw new ErrorException($msg)): the handler's
        // exception stays pending while formatting goes on, and a later error chains it.
        let call = |format: &[u8], args: &[Value]| {
            let mut sink = crate::diag::Throw::error_exception();
            let e = formatted_print("sprintf", format, args, 1, &mut sink).unwrap_err();
            let prev = e
                .previous()
                .map(|p| format!("{}: {}", p.class(), p.message()));
            (format!("{}: {}", e.class(), e.message()), prev)
        };
        let array = Value::Array(crate::array::Array::from_values([Value::Int(1)]));
        let object = Value::Object(crate::object::StdClass::new().into_ref());
        assert_eq!(
            call(b"%s %z", std::slice::from_ref(&array)),
            (
                "ArgumentCountError: 3 arguments are required, 2 given".to_string(),
                Some("ErrorException: Array to string conversion".to_string())
            )
        );
        assert_eq!(
            call(b"%s|%s", &[array.clone(), object]),
            (
                "ErrorException: Array to string conversion".to_string(),
                None
            )
        );
        assert_eq!(
            call(b"%.60f %s", &[Value::Float(1.5), array]),
            (
                "ErrorException: sprintf(): Requested precision of 60 digits was truncated to PHP maximum of 53 digits".to_string(),
                None
            )
        );
    }

    #[test]
    fn huge_widths() {
        // PHP: "Fatal error: Field width 2147483646 is too long" (checked before allocating).
        let e = err(b"x%2147483646s", &[Value::from("")]);
        assert_eq!(
            e,
            ("Error".into(), "Field width 2147483646 is too long".into())
        );
        // Within INT_MAX but beyond PHP's default memory_limit: refused, not allocated.
        let e = err(b"%200000000s", &[Value::from("")]);
        assert_eq!(e.0, "Error");
        assert!(e.1.starts_with("Allowed memory size of 134217728 bytes exhausted"));
    }
}
