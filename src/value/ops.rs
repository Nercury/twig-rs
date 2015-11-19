use std::i64;
use super::MAX_DEBUG_STRING_LENGTH;
use error::{ RuntimeResult, RuntimeError, CastTarget, CastError };

#[derive(Debug)]
pub enum ParseAsNumericResult {
    Int(i64),
    Float(f64),
}

pub fn float_to_int(value: f64) -> RuntimeResult<i64> {
    if value.is_nan() {
        return Err(RuntimeError::ImpossibleCast {
            target: CastTarget::Int,
            reason: CastError::FloatNotANumber(value),
        });
    }
    if value.is_infinite() {
        return Err(RuntimeError::ImpossibleCast {
            target: CastTarget::Int,
            reason: CastError::FloatIsInfinite(value),
        });
    }
    if !double_fits_long(value) {
        return Err(RuntimeError::ImpossibleCast {
            target: CastTarget::Int,
            reason: CastError::FloatRange(value),
        });
    }
    Ok(value as i64)
}

/// Try to convert string to a numeric value (either int or float).
pub fn parse_as_numeric(value: &str) -> RuntimeResult<ParseAsNumericResult> {
    /*
    [Reference implementation](https://github.com/php/php-src/blob/2dd32fe489ebee719bd5eaff497689e1c3a88e95/Zend/zend_operators.c#L2753).

    This is actually a mini-parser. Therefore it is implemented as such here.
    */

    if value.is_empty() {
        return Err(RuntimeError::ImpossibleCast {
            target: CastTarget::Number,
            reason: CastError::StringEmpty
        });
    }

    match parse_as_float_or_int(value) {
        Ok(v) => Ok(v),
        Err(t) => Err(RuntimeError::ImpossibleCast {
            target: t,
            reason: CastError::StringNotNumerical(to_string_limited(value)),
        })
    }
}

fn parse_as_float_or_int(value: &str) -> Result<ParseAsNumericResult, CastTarget> {
    const MAX_LENGTH_OF_LONG: usize = 20;

    /// Parsing state.
    enum State {
        /// At the begining, we skip any whitespace.
        Whitespace,
        /// Next, will skip any leading zeros.
        LeadingZeros { starts_at: usize },
        /// We know it is float and where it starts.
        Float { starts_at: usize },
        /// It can still be a number and we know where it should start.
        MaybeNumber { starts_at: usize },
        /// At the end, skip whitespace, has to be int.
        MaybeInt { starts_at: usize, ends_at: usize },
    }

    let mut neg = false;
    let mut state = State::Whitespace;

    for (i, c) in value.chars().enumerate() {
        match state {
            State::Whitespace => if !c.is_whitespace() {
                match c {
                    '.' | 'i' | 'N' => state = State::Float { starts_at: i },
                    '-' => {
                        neg = true;
                        state = State::LeadingZeros { starts_at: i + 1 };
                    },
                    '+' => state = State::LeadingZeros { starts_at: i + 1 },
                    '0' => state = State::LeadingZeros { starts_at: i },
                    n if n.is_digit(10) => state = State::MaybeNumber { starts_at: i },
                    _ => return Err(CastTarget::Number),
                }
            },
            State::LeadingZeros { starts_at } => match c {
                '.' => state = State::Float { starts_at: i },
                'i' => state = State::Float { starts_at: starts_at },
                c if c.is_whitespace() => state = State::MaybeInt { starts_at: i-1, ends_at: i },
                n if n.is_digit(10) => state = State::MaybeNumber { starts_at: i },
                _ => return Err(CastTarget::Number),
            },
            State::MaybeNumber { starts_at } => match c {
                '.' => state = State::Float { starts_at: starts_at },
                c if c.is_whitespace() => state = State::MaybeInt { starts_at: starts_at, ends_at: i },
                n if n.is_digit(10) => {
                    if i - starts_at >= MAX_LENGTH_OF_LONG {
                        state = State::Float { starts_at: starts_at };
                    }
                },
                _ => return Err(CastTarget::Number),
            },
            State::Float { .. } => break,
            State::MaybeInt { .. } => if !c.is_whitespace() {
                return Err(CastTarget::Number);
            }
        };
    }

    match state {
        State::MaybeNumber { starts_at } => Ok(ParseAsNumericResult::Int(
            match value[starts_at..].parse() {
                Ok(v) => if neg { 0 - v } else { v },
                Err(_) => return Err(CastTarget::Int),
            }
        )),
        State::MaybeInt { starts_at, ends_at } => Ok(ParseAsNumericResult::Int(
            match value[starts_at..ends_at].parse() {
                Ok(v) => if neg { 0 - v } else { v },
                Err(_) => return Err(CastTarget::Int),
            }
        )),
        State::Float { starts_at } => Ok(ParseAsNumericResult::Float(
            match value[starts_at..].trim_right().parse() {
                Ok(v) => if neg { 0.0 - v } else { v },
                Err(_) => return Err(CastTarget::Float),
            }
        )),
        State::LeadingZeros { .. } => Ok(ParseAsNumericResult::Int(0)),
        _ => Err(CastTarget::Number),
    }
}

pub fn to_string_limited(v: &str) -> String {
    if v.len() > MAX_DEBUG_STRING_LENGTH {
        [&v[..MAX_DEBUG_STRING_LENGTH], "..."].concat()
    } else {
        v.into()
    }
}

pub fn double_fits_long(v: f64) -> bool {
    v > i64::MAX as f64 || v < i64::MIN as f64
}

#[cfg(test)]
mod tests {
    use std::f64;
    use super::*;

    #[test]
    fn parses_string_to_float() {
        let cases = vec![
            (".", 0.0),
            (" . ", 0.0),
            (" .0 ", 0.0),
            (" .00 ", 0.0),
            ("0.0", 0.0),
            ("000.0", 0.0),
            (" 000.0", 0.0),
            (".01", 0.01),
            ("-.01", -0.01),
            (" -.01 ", -0.01),
            (" -.01e+3 ", -0.01e+3),
            ("inf", f64::INFINITY),
            ("-inf", f64::NEG_INFINITY),
            ("NaN", f64::NAN),
            (" inf ", f64::INFINITY),
            (" -inf ", f64::NEG_INFINITY),
            (" NaN ", f64::NAN),
            ("2.01", 2.01),
            ("002.01", 2.01),
            (" 002.01", 2.01),
        ];

        for (input, expected) in cases {
            match parse_as_numeric(input) {
                Ok(ParseAsNumericResult::Float(v)) => assert_floats_equal(v, expected),
                Ok(ParseAsNumericResult::Int(v)) => panic!("expected float when parsing {:?} case, got {:?}", input, v),
                Err(e) => panic!("failed to parse {:?}, error: {:?}", input, e),
            }
        }
    }

    #[test]
    fn parses_string_to_integer() {
        let cases = vec![
            ("0", 0),
            (" 0 ", 0),
            (" 000 ", 0),
            (" 000", 0),
            ("000 ", 0),
            ("22", 22),
            (" 22 ", 22),
            (" 22", 22),
            ("22 ", 22),
            ("001", 1),
            (" 001 ", 1),
            (" 001", 1),
            ("001 ", 1),
            ("-001", -1),
            ("  -001  ", -1),
            ("  -001", -1),
            ("-001 ", -1),
            ("145354534", 145354534),
        ];

        for (input, expected) in cases {
            match parse_as_numeric(input) {
                Ok(ParseAsNumericResult::Int(v)) => assert_eq!(v, expected),
                Ok(ParseAsNumericResult::Float(v)) => panic!("expected int when parsing {:?} case, got {:?}", input, v),
                Err(e) => panic!("failed to parse {:?}, error: {:?}", input, e),
            }
        }
    }

    #[test]
    fn fails_parsing_invalid_string_to_float() {
        let cases = vec![
            (".k", r#"Nonnumerical string ".k" is not a float"#),
            ("12 12", r#"Nonnumerical string "12 12" is not a number"#),
            ("0inf", r#"Nonnumerical string "0inf" is not a float"#),
            ("-0inf", r#"Nonnumerical string "-0inf" is not a number"#),
            ("0-inf", r#"Nonnumerical string "0-inf" is not a number"#),
        ];

        for (input, expected) in cases {
            match parse_as_numeric(input) {
                Err(e) => assert_eq!(&format!("{}", e), expected),
                Ok(v) => panic!("expected error {:?} when parsing {:?}, received {:?}", expected, input, v),
            }
        }
    }

    fn assert_floats_equal(a: f64, b: f64) {
        if a.is_nan() && b.is_nan() {
            return;
        }

        if a.is_infinite() && b.is_infinite() {
            if a.is_sign_positive() == b.is_sign_positive() {
                return;
            }
        }

        const e: f64 = 0.0000001;
        assert!(a > b - e && a < b + e, format!("expected floats {:?} and {:?} to be equal", a, b));
    }
}
