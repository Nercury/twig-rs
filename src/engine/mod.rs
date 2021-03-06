use std::mem;
use std::fmt;
use std::collections::HashMap;
use environment::{ Environment, CompiledEnvironment };
use error::Result;
use tokens::Lexer;
use loader::Loader;
use nodes::parse;
use value::{ Value, HashKey };
use instructions::compile;
use std::io::{ Read, Write };
use std::error::Error;
use little::interpreter::{ Interpreter };
use little::{ Fingerprint, Sha1Hasher, IdentifyValue, Template, Function, LittleValue, Build, Execute };
use sha1::Sha1;
use std::result;

impl LittleValue for Value { }

struct FingerprintHasher {
    hasher: Sha1,
}

impl FingerprintHasher {
    fn new() -> FingerprintHasher {
        FingerprintHasher {
            hasher: Sha1::new()
        }
    }
}

impl Sha1Hasher for FingerprintHasher {
    /// Completes a round of hashing, producing the output hash generated.
    fn finish(&self) -> Fingerprint {
        let mut buf = [0;20];
        self.hasher.output(&mut buf);
        Fingerprint::new(buf)
    }

    /// Writes some data into this `Sha1Hasher`
    fn write(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
    }
}

impl IdentifyValue for Value {
    fn identify_value(&self) -> Option<Fingerprint> {
        let mut hasher = FingerprintHasher::new();
        match self.hash_value(&mut hasher) {
            Ok(_) => Some(hasher.finish()),
            Err(_) => None,
        }
    }

    fn hash_value<H: Sha1Hasher>(&self, hasher: &mut H) -> result::Result<(), ()> {
        match *self {
            Value::Null => {
                hasher.write(b"n");
            },
            Value::Int(ref v) => {
                hasher.write(b"i");
                hasher.write_i64(*v);
            },
            Value::Float(_) => return Err(()),
            Value::Str(ref v) => {
                hasher.write(b"s");
                hasher.write(v.as_bytes());
            },
            Value::Array(ref v) => {
                hasher.write(b"a");
                for i in v {
                    try!(i.hash_value(hasher));
                }
            },
            Value::Hash(ref v) => {
                hasher.write(b"h");
                for (k, v) in v {
                    match *k {
                        HashKey::Int(ref v) => {
                            hasher.write(b"i");
                            hasher.write_i64(*v);
                        },
                        HashKey::Str(ref v) => {
                            hasher.write(b"s");
                            hasher.write(v.as_bytes());
                        },
                    }
                    try!(v.hash_value(hasher));
                }
            },
            Value::Obj(_) | Value::Func(_) => return Err(()),
        };
        Ok(())
    }
}

impl Default for Value {
    fn default() -> Value {
        Value::Null
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Value::Int(ref v) => write!(f, "{}", v),
            Value::Float(v) => write!(f, "{}", v),
            Value::Str(ref v) => write!(f, "{}", v),
            _ => Ok(()),
        }
    }
}

/// Twig Engine.
///
/// Given the specified environment settings, converts templates
/// to output string.
pub struct Engine<L> {
    loader: L,
    env: CompiledEnvironment,
    lexer: Option<Lexer>,
    functions: HashMap<&'static str, Box<Function<Value>>>,
}

impl<L: Loader> Engine<L> {
    pub fn new(loader: L, env: Environment) -> Engine<L> {
        let mut engine = Engine {
            loader: loader,
            env: env.init_all(),
            lexer: None,
            functions: HashMap::new(),
        };

        engine.lexer = Some(Lexer::default(&engine.env.lexing));

        engine
    }

    pub fn get<D: Into<Value>>(&mut self, name: &str, data: D)
        -> Result<String>
    {
        let lexer = self.take_lexer();

        let compiled_template = try!(self.get_compiled_template(&lexer, name));

        let funs = HashMap::new();
        let mut i = Interpreter::new();
        let p = match i.build("", compiled_template, &funs) {
            Ok(p) => p,
            Err(e) => panic!("not implemented - handle build_processor error {:?}", e),
        };

        let mut res = String::new();
        let mut interpreter = p.execute(Value::Null);
        loop {
            match interpreter.read_to_string(&mut res) {
                Err(e) => {
                    match e.description() {
                        "interupt" => {
                            unreachable!("unimplemented interupt handling");
                        },
                        e => unreachable!("unimplemented other error {:?}", e),
                    };
                },
                Ok(_) => break,
            }
        }

        self.return_lexer(lexer);

        Ok(res)
    }

    fn get_compiled_template<'r>(&mut self, lexer: &'r Lexer, name: &'r str)
        -> Result<Template<Value>>
    {
        let source = try!(self.loader.get_source(name));
        let mut tokens = lexer.tokens(&source);
        let module = try!(parse(&self.env.parsing, &mut tokens));
        Ok(try!(compile((), &module)))
    }

    fn take_lexer(&mut self) -> Lexer {
        let mut ninja_lexer = None;
        mem::swap(&mut ninja_lexer, &mut self.lexer);

        match ninja_lexer {
            None => unreachable!("lexer is gone"),
            Some(lexer) => lexer,
        }
    }

    fn return_lexer(&mut self, lexer: Lexer) {
        let mut ninja_lexer = Some(lexer);
        mem::swap(&mut ninja_lexer, &mut self.lexer);
    }
}
