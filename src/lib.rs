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
pub mod lexer;
mod error;
mod environment;
mod extension;
pub mod parser;
pub mod node;

pub use extension::Extension;
pub use environment::{ CompiledEnvironment, Environment };
pub use environment::operators::{ UnaryOperator, BinaryOperator, Associativity };

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
