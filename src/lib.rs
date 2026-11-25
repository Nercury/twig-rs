//! Twig template engine for Rust, aiming for exact compatibility with
//! [Twig 3.31.1 for PHP](https://twig.symfony.com/): identical output and identical error messages.
//!
//! At the moment the front end exists: the error model ([`Error`]), template sources ([`Source`]), deprecation
//! reporting ([`deprecation`]), the lexer ([`lexer`]), the syntax tree ([`ast`]), the extension registry
//! ([`extension`]), the parser with the complete expression grammar ([`parser`]), every core tag
//! ([`token_parser`]) and the node traverser with the correctness checks ([`visitor`]). The code generator, the
//! virtual machine and the runtime come next.
//!
//! ```
//! use twig::deprecation::IgnoreDeprecations;
//! use twig::extension::ExtensionSet;
//! use twig::parser::ParseContext;
//! use twig::Source;
//!
//! let context = ParseContext::new(ExtensionSet::with_core_extensions());
//! let module = context.parse_source(
//!     &Source::new("{% for user in users %}Hello {{ user.name|upper }}!{% endfor %}", "index"),
//!     &mut IgnoreDeprecations,
//! )?;
//! assert!(module.as_module().is_some());
//! # Ok::<(), twig::Error>(())
//! ```

pub mod ast;
pub mod deprecation;
pub mod error;
pub mod escaper;
pub mod extension;
pub mod lexer;
pub mod lltl;
pub mod loader;
pub mod parser;
pub mod runtime;
pub mod source;
pub mod token_parser;
pub mod visitor;
pub mod vm;

pub use error::{Error, ErrorKind, Result};
pub use source::Source;

#[cfg(test)]
mod tests {
    /// One environment (and its lexer) must be usable from many threads.
    #[test]
    fn public_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<crate::Error>();
        assert_send_sync::<crate::Source>();
        assert_send_sync::<crate::lexer::Lexer>();
        assert_send_sync::<crate::lexer::TokenStream>();
        assert_send_sync::<crate::deprecation::Deprecation>();
        assert_send_sync::<crate::ast::Node>();
        assert_send_sync::<crate::ast::Expr>();
        assert_send_sync::<crate::extension::ExtensionSet>();
        assert_send_sync::<crate::parser::ParseContext>();
    }
}
