//! Port of `TWIG/tests/Util/DeprecationCollectorTest.php`.
//!
//! twig-rs has no `Twig\Util\DeprecationCollector` (nor the `Environment::tokenize()`/`parse()` it calls); the port
//! does what `collect()` does with the public API: every template is compiled (`compile_source()`, which parses
//! first: Twig triggers callable deprecations while parsing) with the environment collecting deprecations, syntax
//! errors are ignored, and the `E_USER_DEPRECATED` messages are returned in order.

use twig::Source;
use twig::extension::{DeprecatedCallableInfo, FunctionOptions, PhpCallable, TwigFunction};
use twig::runtime::{DiagnosticsPolicy, Environment};

use crate::support::{deprecations, loader};

/// `DeprecationCollector::collect()` over `[name => contents]`.
fn collect(twig: &Environment, templates: &[(&str, &str)]) -> Vec<String> {
    twig.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    let _ = twig.take_notices();
    for (name, contents) in templates {
        // ignore templates containing syntax errors
        let _ = twig.compile_source(&Source::new(*contents, *name));
    }
    deprecations(twig)
}

#[test]
fn test_collect() {
    let twig = Environment::new(loader(&[]));
    twig.add_function(TwigFunction::new(
        "deprec",
        Some(PhpCallable::method(
            "Twig\\Tests\\Util\\DeprecationCollectorTest",
            "deprec",
            vec![],
        )),
        FunctionOptions {
            deprecation_info: Some(DeprecatedCallableInfo::new("foo/bar", "1.1")),
            ..Default::default()
        },
    ))
    .unwrap();
    let deprecations = collect(
        &twig,
        &[
            ("ok.twig", "{{ foo }}"),
            ("deprec.twig", "{{ deprec(\"foo\") }}"),
        ],
    );
    assert_eq!(
        deprecations,
        ["Since foo/bar 1.1: Twig Function \"deprec\" is deprecated in deprec.twig at line 1."]
    );
}
