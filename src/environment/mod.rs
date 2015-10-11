use std::collections::HashMap;
use std::collections::HashSet;

use extension::{ Extension, CoreExtension };
use operator::{ Operator, OperatorOptions };
use token_parser::{ TokenParser };

/// Project configuration container.
pub struct Environment {
    pub operators: Vec<Operator>,
    pub token_parsers: Vec<TokenParser>,
}

impl Environment {
    pub fn default() -> Environment {
        let mut staged = Environment {
            operators: Vec::new(),
            token_parsers: Vec::new(),
        };

        CoreExtension::apply(&mut staged);

        staged
    }

    pub fn init_all(self) -> CompiledEnvironment {
        CompiledEnvironment {
            lexing: LexingEnvironment::new(&self),
            parsing: ParsingEnvironment::new(&self),
        }
    }

    pub fn push_operators<I: IntoIterator<Item=Operator>>(&mut self, ops: I) {
        self.operators.extend(ops);
    }
}

pub struct LexingEnvironment {
    pub operators: HashSet<&'static str>,
}

impl LexingEnvironment {
    pub fn new(env: &Environment) -> LexingEnvironment {
        LexingEnvironment {
            operators: {
                env.operators.iter()
                    .map(|i| i.options.value)
                    .collect()
            },
        }
    }
}

pub struct ParsingEnvironment {
    pub operators: HashMap<&'static str, OperatorOptions>,
}

impl ParsingEnvironment {
    pub fn new(env: &Environment) -> ParsingEnvironment {
        ParsingEnvironment {
            operators: {
                env.operators.iter()
                    .map(|i| (i.options.value, i.options))
                    .collect()
            },
        }
    }
}

/// Project configuration container with all extensions applied.
pub struct CompiledEnvironment {
    pub lexing: LexingEnvironment,
    pub parsing: ParsingEnvironment,
}

impl CompiledEnvironment {

    pub fn default() -> CompiledEnvironment {
        Environment::default()
            .init_all()
    }
}
