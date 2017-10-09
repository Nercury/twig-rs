use std::collections::HashMap;
use std::collections::HashSet;

use extension::{ Extension, CoreExtension };
use operator::{ Operator, OperatorKind, OperatorOptions };
use function::{ Function, Callable, FunctionCompiler };
use nodes::{ TokenParser, TokenParserExtension };

/// Environment configuration.
pub struct Config {
    pub autoescape: String,
}

impl Config {
    pub fn default() -> Config {
        Config {
            autoescape: "html".into()
        }
    }

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

    pub fn new(config: Config) -> Environment {
        let mut staged = Environment {
            operators: Vec::new(),
            token_parsers: Vec::new(),
            functions: Vec::new(),
        };

        CoreExtension::apply(&mut staged);

        staged
    }

    pub fn default() -> Environment {
        Environment::new(Config::default())
    }

    pub fn init_all(self) -> CompiledEnvironment {
        let function_names = self.functions.iter()
            .map(|f| f.name)
            .collect();

        let mut runtime_functions = HashMap::new();
        let mut function_compilers = HashMap::new();

        for function in self.functions {
            match function.callable {
                Callable::Static(compiler) => {
                    function_compilers.insert(function.name, compiler);
                },
                Callable::Dynamic(fun) => {
                    runtime_functions.insert(function.name, fun);
                },
            }
        }

        CompiledEnvironment {
            lexing: LexingEnvironment {
                operators: {
                    self.operators.iter()
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
                    self.operators.into_iter()
                        .filter_map(|i| match i.options.kind {
                            OperatorKind::Unary { value, .. } => Some((value, i.options)),
                            OperatorKind::Binary { value, .. } => Some((value, i.options)),
                            OperatorKind::Other => None,
                        })
                        .collect()
                },
                handlers: {
                    self.token_parsers.into_iter()
                        .map(|i| (i.tag, i.extension))
                        .collect()
                },
                functions: function_names
            },
            compiling: CompilingEnvironment {
                function_compilers: function_compilers
            }
        }
    }

    pub fn push_operators<I: IntoIterator<Item=Operator>>(&mut self, ops: I) {
        self.operators.extend(ops);
    }

    pub fn push_token_parsers<I: IntoIterator<Item=TokenParser>>(&mut self, ops: I) {
        self.token_parsers.extend(ops);
    }

    pub fn push_functions<I: IntoIterator<Item=Function>>(&mut self, funs: I) {
        self.functions.extend(funs);
    }
}

pub struct LexingEnvironment {
    pub operators: HashSet<&'static str>,
}

pub struct ParsingEnvironment {
    pub operators: HashMap<&'static str, OperatorOptions>,
    pub handlers: HashMap<&'static str, Box<TokenParserExtension>>,
    pub functions: HashSet<&'static str>,
}

pub struct CompilingEnvironment {
    pub function_compilers: HashMap<&'static str, FunctionCompiler>,
}

/// Project configuration container with all extensions applied.
pub struct CompiledEnvironment {
    pub lexing: LexingEnvironment,
    pub parsing: ParsingEnvironment,
    pub compiling: CompilingEnvironment,
}

impl CompiledEnvironment {

    pub fn default() -> CompiledEnvironment {
        Environment::default()
            .init_all()
    }
}
