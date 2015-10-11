use std::collections::HashMap;
use std::collections::HashSet;

use extension::{ Extension, CoreExtension };
use operator::{ Operator, OperatorOptions };
use token_parser::{ TokenParser, TokenParserExtension };

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
            lexing: LexingEnvironment {
                operators: {
                    self.operators.iter()
                        .map(|i| i.options.value)
                        .collect()
                },
            },
            parsing: ParsingEnvironment {
                operators: {
                    self.operators.into_iter()
                        .map(|i| (i.options.value, i.options))
                        .collect()
                },
                handlers: {
                    self.token_parsers.into_iter()
                        .map(|i| (i.tag, i.extension))
                        .collect()
                },
            },
        }
    }

    pub fn push_operators<I: IntoIterator<Item=Operator>>(&mut self, ops: I) {
        self.operators.extend(ops);
    }

    pub fn push_token_parsers<I: IntoIterator<Item=TokenParser>>(&mut self, ops: I) {
        self.token_parsers.extend(ops);
    }
}

pub struct LexingEnvironment {
    pub operators: HashSet<&'static str>,
}

pub struct ParsingEnvironment {
    pub operators: HashMap<&'static str, OperatorOptions>,
    pub handlers: HashMap<&'static str, Box<TokenParserExtension>>,
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
