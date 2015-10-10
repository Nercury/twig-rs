use std::collections::HashMap;

use extension::core::CoreExtension;
use operator::{ Operator, OperatorKind };
use {
    Extension,
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
                    .map(|i| (i.options.value, i.options.kind))
                    .collect()
            },
        }
    }

    pub fn push_operators<I: IntoIterator<Item=Operator>>(&mut self, ops: I) {
        self.operators.extend(ops);
    }
}

/// Project configuration container with all extensions applied.
pub struct CompiledEnvironment {
    pub operators: HashMap<&'static str, OperatorKind>,
}

impl CompiledEnvironment {

    pub fn default() -> CompiledEnvironment {
        Environment::default()
            .init()
    }
}
