/*!
<a href="https://github.com/Nercury/twig-rs">
    <img style="position: absolute; top: 0; left: 0; border: 0;" src="https://s3.amazonaws.com/github/ribbons/forkme_left_green_007200.png" alt="Fork me on GitHub">
</a>
<style>.sidebar { margin-top: 53px }</style>
*/

/**
 * This module is part of twig-rs.
 *
 * (c) 2015 Rust Twig Team
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

extern crate regex;
extern crate uuid;
extern crate little;
#[macro_use] extern crate log;

// Allow unused things in development so that real warnings are more visible.

#[allow(unused_variables, dead_code, unused_assignments)]
pub mod tokens;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod nodes;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod instructions;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod loader;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod error;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod environment;

#[allow(unused_variables, dead_code, unused_assignments)]
pub mod extension;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod operator;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod value;

/// Returns different output based on expected value.
pub trait Expect<V> {
    type Output;

    fn expect(&mut self, expected: V) -> Self::Output;
}

use std::mem;
use std::collections::HashMap;
use nodes::parse;
use instructions::compile;
use little::{ Instruction, Function };

/// Twig Engine.
///
/// Given the specified environment settings, converts templates
/// to output string.
pub struct Engine<'e, L> {
    loader: L,
    env: environment::CompiledEnvironment,
    lexer: Option<tokens::Lexer>,
    functions: HashMap<&'static str, &'e Function<value::Value>>,
}

impl<'e, L: loader::Loader> Engine<'e, L> {
    pub fn new<'r>(loader: L, env: environment::Environment) -> Engine<'r, L> {
        let mut engine = Engine {
            loader: loader,
            env: env.init_all(),
            lexer: None,
            functions: HashMap::new(),
        };

        engine.lexer = Some(tokens::Lexer::default(&engine.env.lexing));

        engine
    }

    pub fn get<'r, D: Into<value::ValueRef<'r>>>(&mut self, name: &'r str, data: D)
        -> error::Result<String>
    {
        let lexer = self.take_lexer();

        let instructions = try!(self.get_instructions(&lexer, name));

        self.return_lexer(lexer);

        Ok("".into())
    }

    fn get_instructions<'r>(&mut self, lexer: &'r tokens::Lexer, name: &'r str)
        -> error::Result<Vec<Instruction>>
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

    fn take_lexer(&mut self) -> tokens::Lexer {
        let mut ninja_lexer = None;
        mem::swap(&mut ninja_lexer, &mut self.lexer);

        match ninja_lexer {
            None => unreachable!("lexer is gone"),
            Some(lexer) => lexer,
        }
    }

    fn return_lexer(&mut self, lexer: tokens::Lexer) {
        let mut ninja_lexer = Some(lexer);
        mem::swap(&mut ninja_lexer, &mut self.lexer);
    }
}
