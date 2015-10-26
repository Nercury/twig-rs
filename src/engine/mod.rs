use std::mem;
use std::fmt;
use std::collections::HashMap;
use environment::{ Environment, CompiledEnvironment };
use error::Result;
use tokens::Lexer;
use loader::Loader;
use nodes::parse;
use value::{ Value, ValueRef };
use instructions::compile;
use std::io::{ Read, Write };
use std::error::Error;
use little::{ Template, Function, Interpreter, Options, Parameter, BuildProcessor, LittleValue, Run };

impl LittleValue for Value { }

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

    pub fn get<'r, D: Into<ValueRef<'r>>>(&mut self, name: &'r str, data: D)
        -> Result<String>
    {
        let lexer = self.take_lexer();

        let compiled_template = try!(self.get_compiled_template(&lexer, name));

        let funs = HashMap::new();
        let mut i = Interpreter::new();
        let p = match i.build_processor(compiled_template, &funs) {
            Ok(p) => p,
            Err(e) => panic!("not implemented - handle build_processor error {:?}", e),
        };

        let mut res = String::new();
        let mut interpreter = p.run(Options::<Parameter, Value>::empty());
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
