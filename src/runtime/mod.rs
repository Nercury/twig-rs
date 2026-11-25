//! The runtime: [`Environment`], template instances and [`TemplateWrapper`], template composition (inheritance,
//! blocks, traits, embed, include, macros), the [`sandbox`], [`Markup`], attribute access and the other helpers
//! compiled templates call. See `plans/engine.md`.
//!
//! An [`Environment`] owns the extensions, the globals, the loader and the cache of loaded templates; loading a
//! template lexes, parses, visits and [compiles](crate::lltl) it once per environment, and rendering runs the program
//! on the [virtual machine](crate::vm).

pub(crate) mod attribute;
pub(crate) mod composition;
mod constants;
pub(crate) mod core;
mod environment;
pub(crate) mod functions;
pub(crate) mod macros;
mod markup;
mod regex;
pub mod sandbox;
mod template;

pub use attribute::ARRAY_LIKE_CLASSES;
pub use constants::Constants;
pub use core::{compare_values, ensure_traversable, in_filter, test_empty, to_array};
pub use environment::{
    DEFAULT_MAX_DEPTH, DiagnosticsPolicy, Environment, EnvironmentOptions, Notice,
};
pub use markup::{MARKUP_CLASS, Markup};
pub use regex::PcreRegexValidator;
pub use sandbox::{
    Sandbox, SandboxBridgeRuntime, SandboxExtension, SecurityChecker, SecurityError,
    SecurityErrorKind, SecurityPolicy, SecurityPolicyInterface, SourcePolicyInterface,
};
pub(crate) use template::{Blocks, TemplateObject};
pub use template::{Template, TemplateWrapper};
