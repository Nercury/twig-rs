//! Twig template engine for Rust, aiming for exact compatibility with
//! [Twig for PHP](https://twig.symfony.com/).
//!
//! At the moment only the lexer ([`tokens`]) and the parser ([`nodes`]) exist,
//! and both are incomplete.

pub mod environment;
pub mod error;
pub mod extension;
pub mod function;
pub mod loader;
pub mod nodes;
pub mod operator;
pub mod tokens;
pub mod value;

/// Returns different output based on expected value.
pub trait Expect<V> {
    type Output;

    fn expect(&mut self, expected: V) -> Self::Output;
}
