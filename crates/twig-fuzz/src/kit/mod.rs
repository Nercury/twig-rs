//! The replay core: oracle cases and results, the twig-rs runner, the diff and the vector files.
//!
//! These modules refer to each other only through `super::`, so the repository's
//! `tests/fuzz_regressions.rs` can compile them into the `twig` package's own test (with
//! `#[path]`) without a dependency on this crate.

pub mod case;
pub mod diff;
pub mod factory;
pub mod json;
pub mod result;
pub mod run;
pub mod vectors;
