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
pub mod environment;
mod extension;
pub mod parser;
pub mod node;

pub use extension::Extension;
