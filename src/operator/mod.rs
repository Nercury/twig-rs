use std::cmp::Ordering;
use value::TwigValue;
use runtime;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum OperatorKind {
    /// Single argument operator, i.e negation.
    Unary,
    /// Two argument operator, i.e sum.
    Binary(Associativity)
}

impl OperatorKind {
    pub fn new_binary_left() -> OperatorKind {
        OperatorKind::Binary(Associativity::Left)
    }

    pub fn new_binary_right() -> OperatorKind {
        OperatorKind::Binary(Associativity::Right)
    }

    pub fn new_unary() -> OperatorKind {
        OperatorKind::Unary
    }
}

#[derive(Copy, Clone)]
pub struct OperatorOptions {
    pub value: &'static str,
    pub precedence: u16,
    pub kind: OperatorKind,
}

impl OperatorOptions {

    pub fn new_binary_left(chars: &'static str, precedence: u16) -> OperatorOptions {
        OperatorOptions {
            value: chars,
            precedence: precedence,
            kind: OperatorKind::new_binary_left(),
        }
    }

    pub fn new_binary_right(chars: &'static str, precedence: u16) -> OperatorOptions {
        OperatorOptions {
            value: chars,
            precedence: precedence,
            kind: OperatorKind::new_binary_right(),
        }
    }

    pub fn new_unary(chars: &'static str, precedence: u16) -> OperatorOptions {
        OperatorOptions {
            value: chars,
            precedence: precedence,
            kind: OperatorKind::new_unary(),
        }
    }
}

pub struct Operator {
    pub options: OperatorOptions,
    pub callable: Box<
        for<'e, 'r> Fn(&'e [TwigValue<'r>]) -> runtime::Result<TwigValue<'r>>
    >,
}

impl Operator {

    pub fn new_binary_left<'r>(chars: &'static str, precedence: u16) -> Operator {
        Operator {
            options: OperatorOptions::new_binary_left(chars, precedence),
            callable: Box::new(
                |_: &[TwigValue]| Err(runtime::Error::new(runtime::ErrorMessage::Poop))
            ),
        }
    }

    pub fn new_binary_right(chars: &'static str, precedence: u16) -> Operator {
        Operator {
            options: OperatorOptions::new_binary_right(chars, precedence),
            callable: Box::new(
                |_| Err(runtime::Error::new(runtime::ErrorMessage::Poop))
            ),
        }
    }

    pub fn new_unary(chars: &'static str, precedence: u16) -> Operator {
        Operator {
            options: OperatorOptions::new_unary(chars, precedence),
            callable: Box::new(
                |_| Err(runtime::Error::new(runtime::ErrorMessage::Poop))
            ),
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
