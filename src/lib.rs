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

mod token;
mod lexer;
mod error;
mod environment;
mod extension;

pub mod node;
pub mod parser;

pub use error::{ Error, Result };
pub use extension::Extension;
pub use lexer::{ Lexer };
pub use lexer::iter::TokenIter;
pub use environment::{ CompiledEnvironment, Environment };
pub use environment::operators::{ UnaryOperator, BinaryOperator, Associativity };
pub use token::Token;
pub use token::Value as TokenValue;

pub struct Container<T>(Vec<T>);

impl<T: Clone, I: Into<T>, C: IntoIterator<Item=I>> From<C> for Container<T> {
    fn from(source: C) -> Container<T> {
        Container(
            source.into_iter()
                .map(|i| i.into().clone())
                .collect()
        )
    }
}

/// Returns different output based on expected value.
pub trait Expect<V> {
    type Output;

    fn expect(&mut self, expected: V) -> Self::Output;
}
