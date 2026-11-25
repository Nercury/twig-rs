//! Twig template engine for Rust, aiming for exact compatibility with
//! [Twig 3.31.1 for PHP](https://twig.symfony.com/): identical output and identical error messages.
//!
//! The pipeline follows Twig's: the lexer ([`lexer`]) and the parser ([`parser`], [`token_parser`]) build a syntax
//! tree mirroring Twig's node classes ([`ast`]); node visitors check and transform it ([`visitor`]: correctness checks,
//! auto-escaping, the optimizer, the sandbox's checks); the code generator lowers it into a program of the low-level
//! template language ([`lltl`]) in Twig's compile order; the virtual machine ([`vm`]) runs it with PHP's value
//! semantics ([`value`], the `twig-value` crate). The [`runtime`] ties it together: the
//! [`Environment`](runtime::Environment) owns the extensions ([`extension`]: the core filters, functions, tests and
//! operators, escaping ([`escaper`]), the sandbox, debug and string-loader extensions, and the host's), the globals,
//! the loader ([`loader`]: array, filesystem and chain loaders) and the loaded templates; template composition
//! (inheritance, blocks, macros, `include`, `embed`, `use`) and the sandbox ([`runtime::sandbox`]) run there.
//!
//! ```
//! use twig::loader::ArrayLoader;
//! use twig::runtime::Environment;
//! use twig::value::{Array, Value};
//!
//! let env = Environment::new(ArrayLoader::new([(
//!     "index",
//!     "{% for user in users %}Hello {{ user.name|upper }}!{% endfor %}",
//! )]));
//! let mut user = Array::new();
//! user.insert("name", Value::from("fabien"));
//! let mut context = Array::new();
//! context.insert("users", Value::Array(Array::from_values([Value::Array(user)])));
//! assert_eq!(env.render("index", context)?, "Hello FABIEN!");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! Errors are [`Error`]s with Twig's messages, template names and lines: Twig's own classes ([`ErrorKind::Syntax`],
//! [`ErrorKind::Loader`], [`ErrorKind::Runtime`], the sandbox's), or the PHP throwable a non-Twig failure stands for
//! ([`ErrorKind::Php`]), which a template wraps like Twig does (`An exception has been thrown during the rendering of
//! a template ("...")`). Behaviour that deliberately differs from PHP is listed in `reference/deviations.md`.

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
/// The PHP value model (the `twig-value` crate): template contexts are [`value::Array`]s of [`value::Value`]s.
pub use twig_value as value;

/// The examples of `README.md`, compiled and run as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

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
        assert_send_sync::<crate::runtime::Environment>();
        assert_send_sync::<crate::runtime::TemplateWrapper>();
        assert_send_sync::<crate::lltl::Program>();
        assert_send_sync::<crate::vm::Exception>();
        assert_send_sync::<crate::extension::runtime::CallableRuntime>();
    }
}
