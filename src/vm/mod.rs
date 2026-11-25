//! The virtual machine that runs [LLTL programs](crate::lltl).
//!
//! A render runs units ([`crate::lltl::Unit`]) on frames: each frame holds the unit's `$context` (a copy-on-write PHP
//! array, copied exactly where PHP copies it: block calls, includes, macros, `with`; shared by capture bodies; snapshot
//! by arrow functions), the operand stack, the compiler variables (`$_vN`), the active `foreach` iterators and the
//! argument lists of calls being built. Output goes to a [`Sink`] through a stack of captures
//! (`CoreExtension::captureOutput()`).
//!
//! Everything a render shares lives in one owned state: the environment, the stack of active units (which becomes the
//! trace of an exception, for Twig's line guessing) and the routing of PHP diagnostics to the environment's policy.
//! That state is also the [`twig_value::Diagnostics`] sink of every value-level operation, so a template closure
//! called back from `twig-value` code (a sort comparator, a host object method) reaches the running VM through
//! [`twig_value::Diagnostics::host`].
//!
//! Errors are [`Exception`]s: Twig errors or other PHP throwables. The template that handles one
//! (`Template::handleException()`) gives a Twig error its source and guessed line, and wraps anything else in
//! `An exception has been thrown during the rendering of a template ("%s").`. Recursion is bounded by the
//! environment's `max_depth` option (a documented deviation: PHP has no such limit).

mod arrow;
mod exception;
mod frame;
mod handler;
mod interp;
mod output;
mod state;

pub use exception::Exception;
pub use output::{IoSink, Sink};

pub(crate) use arrow::invoke_value;
pub(crate) use frame::{ArgList, Frame};
pub(crate) use handler::{guess, handle_exception};
pub(crate) use interp::run_unit;
pub(crate) use output::Output;
pub(crate) use state::VmState;
