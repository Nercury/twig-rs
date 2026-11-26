//! Ports of Twig 3.31.1's behaviour-level PHPUnit tests of the runtime (reference/twig/test-suite.md §6):
//! `TemplateTest.php`, `TemplateWrapperTest.php`, `EnvironmentTest.php`, `ErrorTest.php`, the behavioural parts of
//! `EnvironmentOptionsHashTest.php`, `CallMacroTest.php`, `TwigMacroTest.php`, `MarkupTest.php` and `SourceTest.php`.
//! `BlockChainTest.php` is not ported: twig-rs has no `Twig\BlockChain` (see `upstream/runtime/block_chain_test.rs`).
//!
//! Test names are the upstream method names in snake case; data providers keep their data sets and order. Each
//! module lists the upstream methods it does not port and why. Tests that fail because twig-rs behaves differently
//! from PHP are marked `#[ignore = "twig-rs mismatch: ..."]` after the expectation was checked with the oracle.
//!
//! Beyond the ports, oracle sweeps widen the subjects of these tests: the getAttribute() matrix (`get_attribute_sweep`),
//! Stringable and odd keys (`stringable_keys`), `TwigMacro::callLegacy()` argument binding (`macro_calls`), the
//! `TemplateWrapper` block API (`block_api`) and error line attribution (`error_lines`). They use the PHP test classes
//! of TemplateTest.php/ErrorTest.php (Rust versions in `upstream/runtime/classes.rs`).
//!
//! Oracle vectors live in `tests/vectors/upstream/runtime/`; each `.gen.php` says how to regenerate its `.json`.

#[path = "upstream/runtime/classes.rs"]
mod classes;
#[path = "upstream/runtime/support.rs"]
mod support;

#[path = "upstream/runtime/block_api.rs"]
mod block_api;
#[path = "upstream/runtime/block_chain_test.rs"]
mod block_chain_test;
#[path = "upstream/runtime/call_macro_test.rs"]
mod call_macro_test;
#[path = "upstream/runtime/environment_options_hash_test.rs"]
mod environment_options_hash_test;
#[path = "upstream/runtime/environment_test.rs"]
mod environment_test;
#[path = "upstream/runtime/error_lines.rs"]
mod error_lines;
#[path = "upstream/runtime/error_test.rs"]
mod error_test;
#[path = "upstream/runtime/get_attribute_sweep.rs"]
mod get_attribute_sweep;
#[path = "upstream/runtime/macro_calls.rs"]
mod macro_calls;
#[path = "upstream/runtime/markup_test.rs"]
mod markup_test;
#[path = "upstream/runtime/source_test.rs"]
mod source_test;
#[path = "upstream/runtime/stringable_keys.rs"]
mod stringable_keys;
#[path = "upstream/runtime/template_test.rs"]
mod template_test;
#[path = "upstream/runtime/template_wrapper_test.rs"]
mod template_wrapper_test;
#[path = "upstream/runtime/twig_macro_test.rs"]
mod twig_macro_test;
