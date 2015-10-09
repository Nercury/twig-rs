use std::collections::HashMap;

use extension::core::CoreExtension;
use operator::{ Operator };
use {
    Extension,
    Container,
};

/// Project configuration container.
pub struct Environment {
    pub operators: Vec<Operator>,
}

impl Environment {
    pub fn default() -> Environment {
        let mut staged = Environment {
            operators: Vec::new(),
        };

        CoreExtension::apply(&mut staged);

        staged
    }

    pub fn init(self) -> CompiledEnvironment {
        CompiledEnvironment {
            operators: {
                self.operators.iter()
                    .map(|i| (i.value(), *i))
                    .collect()
            },
        }
    }

    pub fn push_operators<L: Into<Container<Operator>>>(&mut self, ops: L) {
        let Container(items) = ops.into();
        self.operators.extend(items);
    }
}

/// Project configuration container with all extensions applied.
pub struct CompiledEnvironment {
    pub operators: HashMap<&'static str, Operator>,
}

impl CompiledEnvironment {

    pub fn default() -> CompiledEnvironment {
        Environment::default()
            .init()
    }
}
