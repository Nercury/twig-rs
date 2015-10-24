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
#[macro_use] extern crate log;

// Allow unused things in development so that real warnings are more visible.

#[allow(unused_variables, dead_code, unused_assignments)]
pub mod tokens;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod nodes;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod loader;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod error;
#[allow(unused_variables, dead_code, unused_assignments)]
mod environment;

#[allow(unused_variables, dead_code, unused_assignments)]
pub mod extension;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod operator;
#[allow(unused_variables, dead_code, unused_assignments)]
pub mod modules;

pub use environment::{ CompiledEnvironment, Environment, Config };

/// Returns different output based on expected value.
pub trait Expect<V> {
    type Output;

    fn expect(&mut self, expected: V) -> Self::Output;
}

/// Twig Engine.
///
/// Given the specified environment settings, converts templates
/// to output string.
pub struct Engine<L> {
    loader: L,
    env: environment::CompiledEnvironment,
    lexer: Option<tokens::Lexer>,
}

use std::mem;
use std::collections::HashMap;

impl<L: loader::Loader> Engine<L> {
    pub fn new(loader: L, env: environment::Environment) -> Engine<L> {
        let mut engine = Engine {
            loader: loader,
            env: env.init_all(),
            lexer: None,
        };

        engine.lexer = Some(tokens::Lexer::default(&engine.env.lexing));

        engine
    }

    pub fn get<'r>(&mut self, name: &'r str, data: HashMap<&'r str, &'r str>)
        -> error::Result<String>
    {
        let lexer = self.take_lexer();
        let source = try!(self.loader.get_source(name));
        let result = self.process_template(&lexer, &source);
        self.return_lexer(lexer);
        result
    }

    fn process_template(&self, lexer: &tokens::Lexer, template: &str) -> error::Result<String> {
        let mut tokens = lexer.tokens(template);
        let parser = nodes::Parser::new(
            &self.env.parsing, &mut tokens
        );

        Ok("".into())
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
