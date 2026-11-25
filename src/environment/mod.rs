use std::collections::HashMap;
use std::collections::HashSet;

use crate::extension::{CoreExtension, Extension};
use crate::function::Function;
use crate::nodes::{TokenParser, TokenParserExtension};
use crate::operator::{Operator, OperatorKind, OperatorOptions};

/// Environment configuration.
pub struct Config {
    pub autoescape: String,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            autoescape: "html".into(),
        }
    }
}

impl Config {
    pub fn from_hashmap(map: HashMap<String, String>) -> Config {
        let default = Config::default();

        Config {
            autoescape: map.get("autoescape").cloned().unwrap_or(default.autoescape),
        }
    }
}

/// Project configuration container.
#[derive(Debug)]
pub struct Environment {
    pub operators: Vec<Operator>,
    pub token_parsers: Vec<TokenParser>,
    pub functions: Vec<Function>,
}

impl Environment {
    pub fn new(_config: Config) -> Environment {
        let mut staged = Environment {
            operators: Vec::new(),
            token_parsers: Vec::new(),
            functions: Vec::new(),
        };

        CoreExtension::apply(&mut staged);

        staged
    }

    pub fn init_all(self) -> CompiledEnvironment {
        CompiledEnvironment {
            lexing: LexingEnvironment {
                operators: {
                    self.operators
                        .iter()
                        .filter_map(|i| match i.options.kind {
                            OperatorKind::Unary { value, .. } => Some(value),
                            OperatorKind::Binary { value, .. } => Some(value),
                            OperatorKind::Other => None,
                        })
                        .collect()
                },
            },
            parsing: ParsingEnvironment {
                operators: {
                    self.operators
                        .into_iter()
                        .filter_map(|i| match i.options.kind {
                            OperatorKind::Unary { value, .. } => Some((value, i.options)),
                            OperatorKind::Binary { value, .. } => Some((value, i.options)),
                            OperatorKind::Other => None,
                        })
                        .collect()
                },
                handlers: {
                    self.token_parsers
                        .into_iter()
                        .map(|i| (i.tag, i.extension))
                        .collect()
                },
                functions: { self.functions.iter().map(|f| f.name).collect() },
            },
        }
    }

    pub fn push_operators<I: IntoIterator<Item = Operator>>(&mut self, ops: I) {
        self.operators.extend(ops);
    }

    pub fn push_token_parsers<I: IntoIterator<Item = TokenParser>>(&mut self, ops: I) {
        self.token_parsers.extend(ops);
    }

    pub fn push_functions<I: IntoIterator<Item = Function>>(&mut self, funs: I) {
        self.functions.extend(funs);
    }
}

impl Default for Environment {
    fn default() -> Environment {
        Environment::new(Config::default())
    }
}

pub struct LexingEnvironment {
    pub operators: HashSet<&'static str>,
}

pub struct ParsingEnvironment {
    pub operators: HashMap<&'static str, OperatorOptions>,
    pub handlers: HashMap<&'static str, Box<dyn TokenParserExtension>>,
    pub functions: HashSet<&'static str>,
}

/// Project configuration container with all extensions applied.
pub struct CompiledEnvironment {
    pub lexing: LexingEnvironment,
    pub parsing: ParsingEnvironment,
}

impl Default for CompiledEnvironment {
    fn default() -> CompiledEnvironment {
        Environment::default().init_all()
    }
}
