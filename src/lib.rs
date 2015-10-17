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

pub mod tokens;
mod lexer;
mod error;
mod environment;

pub mod extension;
pub mod node;
pub mod parser;
pub mod operator;
pub mod value;
pub mod runtime;
pub mod token_parser;

pub use error::{ Error, Result };
pub use lexer::{ Lexer };
pub use lexer::iter::TokenIter;
pub use environment::{ CompiledEnvironment, Environment };

/// Returns different output based on expected value.
pub trait Expect<V> {
    type Output;

    fn expect(&mut self, expected: V) -> Self::Output;
}
