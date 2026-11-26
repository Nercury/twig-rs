//! Ports of Twig 3.31.1's behaviour-level PHPUnit tests of the extensions and their runtimes
//! (`reference/twig/test-suite.md` §6):
//!
//! | upstream file | port |
//! |---|---|
//! | `Extension/CoreTest.php` | `upstream/extensions/core_test.rs` |
//! | `Extension/SandboxTest.php` | `upstream/extensions/sandbox_test.rs` |
//! | `Extension/EscaperTest.php` | `upstream/extensions/escaper_test.rs` |
//! | `Extension/StringLoaderExtensionTest.php` | `upstream/extensions/string_loader_extension_test.rs` |
//! | `DeprecatedCallableInfoTest.php` | `upstream/extensions/deprecated_callable_info_test.rs` |
//! | `Runtime/EscaperRuntimeTest.php` | `upstream/extensions/runtime/escaper_runtime_test.rs` |
//! | `Util/CallableArgumentsExtractorTest.php` | `upstream/extensions/util/callable_arguments_extractor_test.rs` |
//! | `Util/DeprecationCollectorTest.php` | `upstream/extensions/util/deprecation_collector_test.rs` |
//!
//! Test functions keep the upstream names in snake case and data providers stay functions returning the cases (a
//! data-provider test reports every failing case at once). Expected values are the upstream assertions, which pass on
//! the oracle (`./oracle.sh suite all`, `research/php-oracle`); where twig-rs differs, the case was checked again
//! with the oracle and the test is `#[ignore = "twig-rs mismatch: ..."]` (`cargo test --test upstream_extensions --
//! --ignored` runs them).
//!
//! Beyond the upstream assertions, the results are compared with oracle vectors in `tests/vectors/upstream/extensions/`
//! (each generator's header says how to regenerate it):
//!
//! - `sandbox.json` (`sandbox.gen.php`): every scenario of `SandboxTest.php`, run with the test's own classes and
//!   `getEnvironment()`: the output or the exception class, message and previous class, and every deprecation;
//!   `sandbox_test.rs` compares each render with it;
//! - `messages.json` (`messages.gen.json`): the messages and deprecations behind the exception-class-only assertions of
//!   `CoreTest.php`, `EscaperRuntimeTest.php` and `CallableArgumentsExtractorTest.php`;
//! - `differential.json` (`differential.gen.php`): `CoreExtension::compare()` over a matrix of values, `EscaperRuntime::
//!   escape()` over values x strategies x charsets x autoescape, argument binding of host callables with various PHP
//!   signatures, and the `CoreTest.php` helpers through templates with edge inputs (`differential.rs`).
//!
//! Not ported (no observable behaviour through twig-rs's API):
//!
//! - `CustomExtensionTest.php` (`testGetInvalidOperators`): the legacy `ExtensionInterface::getOperators()` is not
//!   supported by twig-rs (reference/deviations.md, "Legacy node classes and APIs").
//! - `Extension/DebugExtensionTest.php`: does not exist in Twig 3.31.1.
//! - `ContainerRuntimeLoaderTest.php`, `FactoryRuntimeLoaderTest.php`: twig-rs has no runtime loaders
//!   (`RuntimeLoaderInterface`, `FactoryRuntimeLoader`, `ContainerRuntimeLoader`); runtimes are registered with
//!   `Environment::add_runtime()` instead (PSR-11 containers are PHP infrastructure).
//! - `Util/CallableParametersTest.php`: `CallableParameters::isStringCoercionSafe()` reflects PHP types; host
//!   callables declare `CallableParameter::string_coercion_safe()` instead (reference/deviations.md, "Parameter types in
//!   the sandbox").
//!
//! The tests skipped inside the ported files are listed in each module's documentation.

#[path = "upstream/extensions/util/callable_arguments_extractor_test.rs"]
mod callable_arguments_extractor_test;
#[path = "upstream/extensions/classes.rs"]
mod classes;
mod common;
#[path = "upstream/extensions/core_test.rs"]
mod core_test;
#[path = "upstream/extensions/deprecated_callable_info_test.rs"]
mod deprecated_callable_info_test;
#[path = "upstream/extensions/util/deprecation_collector_test.rs"]
mod deprecation_collector_test;
#[path = "upstream/extensions/differential.rs"]
mod differential;
#[path = "upstream/extensions/runtime/escaper_runtime_test.rs"]
mod escaper_runtime_test;
#[path = "upstream/extensions/escaper_test.rs"]
mod escaper_test;
#[path = "upstream/extensions/sandbox_test.rs"]
mod sandbox_test;
#[path = "upstream/extensions/string_loader_extension_test.rs"]
mod string_loader_extension_test;
#[path = "upstream/extensions/support.rs"]
mod support;
