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

mod engine;

pub use engine::Engine;

/// Returns different output based on expected value.
pub trait Expect<V> {
    type Output;

    fn expect(&mut self, expected: V) -> Self::Output;
}
