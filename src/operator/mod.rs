use std::cmp::Ordering;
use std::fmt;
use value::TwigValue;
use error::{ RuntimeError, RuntimeResult };

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum OperatorKind {
    /// Single argument operator, i.e negation.
    Unary { value: &'static str },
    /// Two argument operator, i.e sum.
    Binary { value: &'static str, associativity: Associativity },
    /// Any operator handled by extension (i.e. the "=" operator).
    Other,
}

impl OperatorKind {
    pub fn new_binary(value: &'static str, associativity: Associativity) -> OperatorKind {
        OperatorKind::Binary { value: value, associativity: associativity }
    }

    pub fn new_binary_left(value: &'static str) -> OperatorKind {
        OperatorKind::Binary { value: value, associativity: Associativity::Left }
    }

    pub fn new_binary_right(value: &'static str) -> OperatorKind {
        OperatorKind::Binary { value: value, associativity: Associativity::Right }
    }

    pub fn new_unary(value: &'static str) -> OperatorKind {
        OperatorKind::Unary { value: value }
    }

    pub fn new_other() -> OperatorKind {
        OperatorKind::Other
    }
}

#[derive(Debug, Copy, Clone)]
pub struct OperatorOptions {
    pub precedence: Option<u16>,
    pub kind: OperatorKind,
}

impl OperatorOptions {

    pub fn new_binary(chars: &'static str, precedence: u16, associativity: Associativity) -> OperatorOptions {
        OperatorOptions {
            precedence: Some(precedence),
            kind: OperatorKind::new_binary(chars, associativity),
        }
    }

    pub fn new_binary_left(chars: &'static str, precedence: u16) -> OperatorOptions {
        OperatorOptions::new_binary(chars, precedence, Associativity::Left)
    }

    pub fn new_binary_right(chars: &'static str, precedence: u16) -> OperatorOptions {
        OperatorOptions::new_binary(chars, precedence, Associativity::Right)
    }

    pub fn new_unary(chars: &'static str, precedence: u16) -> OperatorOptions {
        OperatorOptions {
            precedence: Some(precedence),
            kind: OperatorKind::new_unary(chars),
        }
    }

    pub fn new_other() -> OperatorOptions {
        OperatorOptions {
            precedence: None,
            kind: OperatorKind::new_other(),
        }
    }
}

pub struct Operator {
    pub options: OperatorOptions,
    pub callable: Box<
        for<'e> Fn(&'e [TwigValue]) -> RuntimeResult<TwigValue>
    >,
}

impl fmt::Debug for Operator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({:?}, callable)", self.options)
    }
}

impl Operator {

    pub fn new_binary<F: 'static>(
        chars: &'static str,
        precedence: u16,
        associativity: Associativity,
        callable: F
    )
        -> Operator
    where
        F: for<'e> Fn(&'e TwigValue, &'e TwigValue) -> RuntimeResult<TwigValue>
    {
        Operator {
            options: OperatorOptions::new_binary(chars, precedence, associativity),
            callable: Box::new(move |args| {
                if args.len() != 2 {
                    return Err(
                        RuntimeError::InvalidArgumentCount {
                            expected: 2,
                            found: args.len()
                        }
                    )
                }

                callable(
                    unsafe { args.get_unchecked(0) },
                    unsafe { args.get_unchecked(1) }
                )
            }),
        }
    }

    pub fn new_binary_left<F: 'static>(
        chars: &'static str,
        precedence: u16,
        callable: F
    )
        -> Operator
    where
        F: for<'e> Fn(&'e TwigValue, &'e TwigValue) -> RuntimeResult<TwigValue>
    {
        Operator::new_binary(
            chars,
            precedence,
            Associativity::Left,
            callable
        )
    }

    pub fn new_binary_right<F: 'static>(
        chars: &'static str,
        precedence: u16,
        callable: F
    )
        -> Operator
    where
        F: for<'e> Fn(&'e TwigValue, &'e TwigValue) -> RuntimeResult<TwigValue>
    {
        Operator::new_binary(
            chars,
            precedence,
            Associativity::Right,
            callable
        )
    }

    pub fn new_unary<F: 'static>(
        chars: &'static str,
        precedence: u16,
        callable: F
    )
        -> Operator
    where
        F: for<'e> Fn(&'e TwigValue) -> RuntimeResult<TwigValue>
    {
        Operator {
            options: OperatorOptions::new_unary(chars, precedence),
            callable: Box::new(move |args| {
                if args.len() != 1 {
                    return Err(
                        RuntimeError::InvalidArgumentCount {
                            expected: 1,
                            found: args.len()
                        }
                    )
                }

                callable(
                    unsafe { args.get_unchecked(0) }
                )
            }),
        }
    }
}

/// Operator associativity.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Associativity {
    Left,
    Right,
}

impl PartialOrd for Associativity {
    fn partial_cmp(&self, other: &Associativity) -> Option<Ordering> {
        match (*self, *other) {
            (Associativity::Left, Associativity::Right) => Some(Ordering::Less),
            (Associativity::Right, Associativity::Left) => Some(Ordering::Greater),
            _ => Some(Ordering::Equal),
        }
    }
}

impl Ord for Associativity {
    fn cmp(&self, other: &Associativity) -> Ordering {
        match (*self, *other) {
            (Associativity::Left, Associativity::Right) => Ordering::Less,
            (Associativity::Right, Associativity::Left) => Ordering::Greater,
            _ => Ordering::Equal,
        }
    }
}

#[cfg(test)]
mod test {
    use super::Associativity;

    #[test]
    fn associativity_left_should_be_less_than_right() {
        assert!(Associativity::Left < Associativity::Right);
    }

    #[test]
    fn associativity_right_should_be_greater_than_left() {
        assert!(Associativity::Right > Associativity::Left);
    }

    #[test]
    fn associativity_right_should_be_equal_to_right() {
        assert!(Associativity::Right == Associativity::Right);
    }
}
