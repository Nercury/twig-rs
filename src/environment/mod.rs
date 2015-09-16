use std::cmp::Ordering;
use std::convert::From;
use std::collections::HashMap;

use extension::core::CoreExtension;
use Extension;

pub struct Container<T>(Vec<T>);

impl<T: Clone, I: Into<T>, C: IntoIterator<Item=I>> From<C> for Container<T> {
    fn from(source: C) -> Container<T> {
        Container(
            source.into_iter()
                .map(|i| i.into().clone())
                .collect()
        )
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

#[derive(Copy, Clone)]
pub struct UnaryOperator {
    chars: &'static str,
    precedence: u16,
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
    chars: &'static str,
    precedence: u16,
    associativity: Associativity,
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

pub struct StagedEnvironment {
    pub binary_operators: Vec<BinaryOperator>,
    pub unary_operators: Vec<UnaryOperator>,
}

impl StagedEnvironment {
    pub fn default() -> StagedEnvironment {
        let mut staged = StagedEnvironment {
            binary_operators: Vec::new(),
            unary_operators: Vec::new(),
        };

        CoreExtension::apply(&mut staged);

        staged
    }

    pub fn init(self) -> Environment {
        Environment {
            binary_operators: {
                self.binary_operators.iter()
                    .map(|i| (i.chars, *i))
                    .collect()
            },
            unary_operators: {
                self.unary_operators.iter()
                    .map(|i| (i.chars, *i))
                    .collect()
            }
        }
    }

    pub fn push_binary_operators<L: Into<Container<BinaryOperator>>>(&mut self, ops: L) {
        let Container(items) = ops.into();
        self.binary_operators.extend(items);
    }

    pub fn push_unary_operators<L: Into<Container<UnaryOperator>>>(&mut self, ops: L) {
        let Container(items) = ops.into();
        self.unary_operators.extend(items);
    }
}

pub struct Environment {
    pub binary_operators: HashMap<&'static str, BinaryOperator>,
    pub unary_operators: HashMap<&'static str, UnaryOperator>,
}

impl Environment {

    pub fn default() -> Environment {
        StagedEnvironment::default()
            .init()
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
