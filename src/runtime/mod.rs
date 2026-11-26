//! The runtime: [`Environment`], template instances and [`TemplateWrapper`], template composition (inheritance,
//! blocks, traits, embed, include, macros), the [`sandbox`], [`Markup`], attribute access and the other helpers
//! compiled templates call. See `plans/engine.md`.
//!
//! An [`Environment`] owns the extensions, the globals, the loader, the runtimes ([`RuntimeLoader`]) and the cache of
//! loaded templates; loading a template lexes, parses, visits and [compiles](crate::lltl) it once per environment,
//! and rendering runs the program on the [virtual machine](crate::vm). Host code reaches the composition of templates
//! through [`TemplateWrapper`], [`BlockChain`] and [`MacroNamespace`], and attribute access and Twig's core helpers
//! through the functions of [`CoreExtension`](crate::extension::CoreExtension) (see the
//! [guide](crate#templates-from-rust)).

pub(crate) mod attribute;
mod block_chain;
pub(crate) mod composition;
mod constants;
pub(crate) mod core;
mod core_extension;
mod environment;
pub(crate) mod functions;
pub(crate) mod macros;
mod markup;
mod regex;
mod runtime_loader;
pub mod sandbox;
mod template;
mod typed_property;

pub use attribute::ARRAY_LIKE_CLASSES;
pub use block_chain::{BlockChain, BlockChainTemplate};
pub use constants::Constants;
pub use core::{compare_values, ensure_traversable, in_filter, test_empty, to_array};
pub use environment::{
    DEFAULT_MAX_DEPTH, DiagnosticsPolicy, Environment, EnvironmentOptions, Notice, TemplateRef,
    default_iconv_backend,
};
pub use macros::{MacroNamespace, TwigMacro};
pub use markup::{MARKUP_CLASS, Markup};
pub use regex::PcreRegexValidator;
pub use runtime_loader::{FactoryRuntimeLoader, Runtime, RuntimeLoader};
pub use sandbox::{
    Sandbox, SandboxBridgeRuntime, SandboxExtension, SecurityChecker, SecurityError,
    SecurityErrorKind, SecurityPolicy, SecurityPolicyInterface, SourcePolicyInterface,
};
pub(crate) use template::{Blocks, TemplateObject};
pub use template::{Template, TemplateWrapper};
// The charset converters of `EnvironmentOptions::iconv_backend`: the trait, twig-value's clean-room converter and,
// with the `glibc-iconv` feature, twig-iconv's glibc one.
#[cfg(feature = "glibc-iconv")]
pub use twig_iconv::GlibcIconv;
pub use twig_value::builtins::string::{BasicIconv, IconvBackend};
