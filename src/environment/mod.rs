pub mod operators;

use std::collections::HashMap;

use extension::core::CoreExtension;
use {
    Extension,
    UnaryOperator,
    BinaryOperator,
    Container,
};

pub struct Environment {
    pub binary_operators: Vec<BinaryOperator>,
    pub unary_operators: Vec<UnaryOperator>,
}

impl Environment {
    pub fn default() -> Environment {
        let mut staged = Environment {
            binary_operators: Vec::new(),
            unary_operators: Vec::new(),
        };

        CoreExtension::apply(&mut staged);

        staged
    }

    pub fn init(self) -> CompiledEnvironment {
        CompiledEnvironment {
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

pub struct CompiledEnvironment {
    pub binary_operators: HashMap<&'static str, BinaryOperator>,
    pub unary_operators: HashMap<&'static str, UnaryOperator>,
}

impl CompiledEnvironment {

    pub fn default() -> CompiledEnvironment {
        Environment::default()
            .init()
    }
}
