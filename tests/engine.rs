//! The engine against Twig 3.31.1 on PHP 8.5.13: templates rendered by [`twig::runtime::Environment`] with an
//! `ArrayLoader`, compared with what the oracle rendered (output, error class and message with template name and
//! line, PHP diagnostics in order): the core VM topics, template composition (inheritance, traits, embed, include,
//! macros, the errors of composed templates), the sandbox, and the `api` topic (sequences of API calls).
//!
//! The cases are oracle batches in `tests/vectors/engine/<topic>.gen.json`, written by
//! `tests/vectors/engine/engine.gen.py`; the results are in `<topic>.json`. Every case runs the plain environment
//! (`new Environment(new ArrayLoader($templates), $config)`) with `autoescape` off and `optimizations` set to `0`
//! (the `optimized` topic uses Twig's default, `-1`), collecting diagnostics, or converting warnings to exceptions
//! (`"errors": "throw"`) like PHPUnit does in Twig's test suite. Object cases build their values with the PHP classes
//! at the top of their `data_php`; `tests/engine/objects.rs` has the same classes in Rust.
//!
//! Regenerate the batches and their results from the repository root (Git Bash on Windows needs
//! `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 python -I tests/vectors/engine/engine.gen.py --oracle [topic ...]
//! ```
//!
//! or one result file from its batch:
//!
//! ```text
//! MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php runner.php --pretty \
//!     < tests/vectors/engine/expressions.gen.json > tests/vectors/engine/expressions.json
//! ```

#[path = "common/mod.rs"]
mod common;
#[path = "engine/mod.rs"]
mod engine;

#[test]
fn output() {
    engine::run_topic("output");
}

#[test]
fn expressions() {
    engine::run_topic("expressions");
}

#[test]
fn variables() {
    engine::run_topic("variables");
}

#[test]
fn statements() {
    engine::run_topic("statements");
}

#[test]
fn errors() {
    engine::run_topic("errors");
}

#[test]
fn objects() {
    engine::run_topic("objects");
}

#[test]
fn lines() {
    engine::run_topic("lines");
}

#[test]
fn diagnostics() {
    engine::run_topic("diagnostics");
}

#[test]
fn optimized() {
    engine::run_topic("optimized");
}

#[test]
fn escape() {
    engine::run_topic("escape");
}

#[test]
fn inheritance() {
    engine::run_topic("inheritance");
}

#[test]
fn traits() {
    engine::run_topic("traits");
}

#[test]
fn embed() {
    engine::run_topic("embed");
}

#[test]
fn include() {
    engine::run_topic("include");
}

#[test]
fn macros() {
    engine::run_topic("macros");
}

#[test]
fn composition_errors() {
    engine::run_topic("composition_errors");
}

#[test]
fn sandbox() {
    engine::run_topic("sandbox");
}

#[test]
fn api() {
    engine::api::run_api_topic("api");
}

#[test]
fn parity() {
    engine::run_topic("parity");
}
