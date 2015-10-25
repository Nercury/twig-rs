use std::mem;
use std::collections::HashMap;
use environment::{ Environment, CompiledEnvironment };
use error::Result;
use tokens::Lexer;
use loader::Loader;
use nodes::parse;
use value::{ Value, ValueRef };
use instructions::compile;
use little::{ Instruction, Function };

/// Twig Engine.
///
/// Given the specified environment settings, converts templates
/// to output string.
pub struct Engine<'e, L> {
    loader: L,
    env: CompiledEnvironment,
    lexer: Option<Lexer>,
    functions: HashMap<&'static str, &'e Function<Value>>,
}

impl<'e, L: Loader> Engine<'e, L> {
    pub fn new<'r>(loader: L, env: Environment) -> Engine<'r, L> {
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

        let instructions = try!(self.get_instructions(&lexer, name));

        self.return_lexer(lexer);

        Ok("".into())
    }

    fn get_instructions<'r>(&mut self, lexer: &'r Lexer, name: &'r str)
        -> Result<Vec<Instruction>>
    {
        let source = try!(self.loader.get_source(name));
        let mut tokens = lexer.tokens(&source);
        let module = try!(parse(&self.env.parsing, &mut tokens));
        Ok({
            let mut instructions = Vec::new();
            try!(compile((), &module, &mut instructions));
            instructions
        })
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
