use std::cmp::Ordering;

#[derive(Copy, Clone)]
pub enum Operator {
    /// Single argument operator, i.e negation.
    Unary {
        value: &'static str,
        precedence: u16
    },
    /// Two argument operator, i.e sum.
    Binary {
        value: &'static str,
        precedence: u16,
        associativity: Associativity,
    }
}

impl<'s> Into<Operator> for &'s Operator {
    fn into(self) -> Operator {
        self.clone()
    }
}

impl Operator {
    pub fn value(&self) -> &'static str {
        match *self {
            Operator::Binary { value, .. } => value,
            Operator::Unary { value, .. } => value,
        }
    }

    pub fn new_binary_left(chars: &'static str, precedence: u16) -> Operator {
        Operator::Binary {
            value: chars,
            precedence: precedence,
            associativity: Associativity::Left,
        }
    }

    pub fn new_binary_right(chars: &'static str, precedence: u16) -> Operator {
        Operator::Binary {
            value: chars,
            precedence: precedence,
            associativity: Associativity::Right,
        }
    }

    pub fn new_unary(chars: &'static str, precedence: u16) -> Operator {
        Operator::Unary {
            value: chars,
            precedence: precedence,
        }
    }
}

/// Operator associativity.
#[derive(Copy, Clone, Eq, PartialEq)]
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
