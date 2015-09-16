use std::cmp::Ordering;

#[derive(Copy, Clone)]
pub struct UnaryOperator {
    pub chars: &'static str,
    pub precedence: u16,
}

impl<'s> Into<UnaryOperator> for &'s UnaryOperator {
    fn into(self) -> UnaryOperator {
        self.clone()
    }
}

impl<'s> Into<UnaryOperator> for &'s (&'static str, u16) {
    fn into(self) -> UnaryOperator {
        let &(chars, p) = self;
        UnaryOperator::new(chars, p)
    }
}

impl UnaryOperator {
    pub fn new(chars: &'static str, precedence: u16) -> UnaryOperator {
        UnaryOperator {
            chars: chars,
            precedence: precedence,
        }
    }
}

#[derive(Copy, Clone)]
pub struct BinaryOperator {
    pub chars: &'static str,
    pub precedence: u16,
    pub associativity: Associativity,
}

impl<'s> Into<BinaryOperator> for &'s BinaryOperator {
    fn into(self) -> BinaryOperator {
        self.clone()
    }
}

impl<'s> Into<BinaryOperator> for &'s (&'static str, u16, Associativity) {
    fn into(self) -> BinaryOperator {
        let &(chars, p, a) = self;
        BinaryOperator {
            chars: chars,
            precedence: p,
            associativity: a,
        }
    }
}

impl BinaryOperator {
    pub fn new_left(chars: &'static str, precedence: u16) -> BinaryOperator {
        BinaryOperator {
            chars: chars,
            precedence: precedence,
            associativity: Associativity::Left,
        }
    }

    pub fn new_right(chars: &'static str, precedence: u16) -> BinaryOperator {
        BinaryOperator {
            chars: chars,
            precedence: precedence,
            associativity: Associativity::Right,
        }
    }
}

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
