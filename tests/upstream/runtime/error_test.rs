//! Port of TWIG/tests/ErrorTest.php (line attribution and error enrichment).
//!
//! PHP's `getFile()`/`getLine()` of a Twig error are the template path and line once the error has a filesystem
//! source (`Error::updateRepr()`); the ports check the source path and the template line instead. The filesystem
//! fixtures are copies of TWIG/tests/Fixtures/errors in `upstream/runtime/Fixtures/errors`.
//!
//! Not ported:
//! - `testErrorWithObjectFilename`: a `Source` named by an `SplFileInfo` object; twig-rs source names are strings.
//! - `testErrorWithoutLineAndContext` (32 data sets): a custom token parser returning a custom node that compiles to
//!   `throw new RuntimeError(...)` with or without `addDebugInfo()`; twig-rs has no code generation hook for nodes of
//!   extensions (missing API, see `reference/deviations.md`).
//! - The `getFile()` assertions on PHP files (`Parser.php`, `Environment.php`) of templates without a path.

use twig::ErrorKind;
use twig::extension::runtime::CallableRuntime;
use twig::extension::{FunctionOptions, PhpCallable, TwigFunction};
use twig::loader::FilesystemLoader;
use twig::runtime::{Environment, EnvironmentOptions};
use twig::value::{Array, Value};

use crate::classes::error_test_foo;
use crate::support::*;

/// `__DIR__.'/Fixtures/errors'` (relative to the port).
fn fixtures(path: &str) -> String {
    format!(
        "{}/tests/upstream/runtime/Fixtures/errors{path}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// PHP's `realpath()` of a fixture (on Windows without the `\\?\` prefix, as PHP prints it).
fn realpath(path: &str) -> String {
    let canonical = std::fs::canonicalize(path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .to_string_lossy()
        .into_owned();
    canonical
        .strip_prefix(r"\\?\")
        .map(str::to_owned)
        .unwrap_or(canonical)
}

fn filesystem_env(path: &str, options: EnvironmentOptions) -> Environment {
    Environment::with_options(
        FilesystemLoader::new([path.to_owned()]).expect("the fixtures directory"),
        options,
    )
}

#[test]
fn test_twig_error_is_enriched_without_being_wrapped() {
    let twig = env_with(&[("index", "foo\n{{ fail() }}")], debug(false));
    let callable = PhpCallable::closure(vec![]);
    CallableRuntime::new(|_| Err(twig::Error::runtime("Runtime error.").into())).attach(&callable);
    twig.add_function(TwigFunction::new(
        "fail",
        Some(callable),
        FunctionOptions::default(),
    ))
    .unwrap();

    let error = expect_error(twig.render("index", Array::new()), ErrorKind::Runtime);
    // `assertSame($error, $e)`: the error thrown by the function, not a wrapper
    assert_eq!(error.raw_message(), "Runtime error.");
    assert!(error.previous().is_none());
    assert_eq!(error.template_line(), 2);
    assert_eq!(error.source_context().map(|s| s.name()), Some("index"));
}

const INDEX_HTML: &str = "{% extends 'base.html' %}\n{% block content %}\n    {{ foo.bar }}\n{% endblock %}\n{% block foo %}\n    {{ foo.bar }}\n{% endblock %}";

fn array_loader_env() -> Environment {
    env_with(
        &[
            ("base.html", "{% block content %}{% endblock %}"),
            ("index.html", INDEX_HTML),
        ],
        debug(true),
    )
}

#[test]
fn test_twig_exception_guess_with_missing_var_and_array_loader() {
    let twig = array_loader_env();
    let template = twig.load("index.html").unwrap();
    let error = expect_error(template.render(Array::new()), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "Variable \"foo\" does not exist in \"index.html\" at line 3."
    );
    assert_eq!(error.template_line(), 3);
    assert_eq!(error.source_context().map(|s| s.name()), Some("index.html"));
}

#[test]
fn test_twig_exception_guess_with_exception_and_array_loader() {
    let twig = array_loader_env();
    let template = twig.load("index.html").unwrap();
    let error = expect_error(
        template.render(ctx(&[("foo", error_test_foo())])),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Runtime error...\") in \"index.html\" at line 3."
    );
    assert_eq!(error.template_line(), 3);
    assert_eq!(error.source_context().map(|s| s.name()), Some("index.html"));
}

#[test]
fn test_twig_exception_guess_with_missing_var_and_filesystem_loader() {
    let twig = filesystem_env(&fixtures(""), debug(true));
    let template = twig.load("index.html").unwrap();
    let error = expect_error(template.render(Array::new()), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "Variable \"foo\" does not exist in \"index.html\" at line 3."
    );
    assert_eq!(error.template_line(), 3);
    assert_eq!(error.source_context().map(|s| s.name()), Some("index.html"));
    // `$e->getLine()` / `$e->getFile()`
    assert_eq!(
        error.source_context().map(|s| s.path().to_owned()),
        Some(realpath(&fixtures("/index.html")))
    );
}

#[test]
fn test_twig_exception_guess_with_exception_and_filesystem_loader() {
    let twig = filesystem_env(&fixtures(""), debug(true));
    let template = twig.load("index.html").unwrap();
    let error = expect_error(
        template.render(ctx(&[("foo", error_test_foo())])),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Runtime error...\") in \"index.html\" at line 3."
    );
    assert_eq!(error.template_line(), 3);
    assert_eq!(error.source_context().map(|s| s.name()), Some("index.html"));
    assert_eq!(
        error.source_context().map(|s| s.path().to_owned()),
        Some(realpath(&fixtures("/index.html")))
    );
}

#[test]
fn test_twig_exception_guess_with_filesystem_loader_outside_debug_mode() {
    let twig = filesystem_env(&fixtures(""), strict(true));
    let template = twig.load("index.html").unwrap();
    let error = expect_error(
        template.render(ctx(&[("foo", error_test_foo())])),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Runtime error...\") in \"index.html\" at line 3."
    );
    assert_eq!(error.template_line(), 3);
    assert_eq!(
        error.source_context().map(|s| s.path().to_owned()),
        Some(realpath(&fixtures("/index.html")))
    );
}

/// Templates by name.
type Templates = Vec<(&'static str, &'static str)>;

/// Twig: `ErrorTest::getErroredTemplates()`: the templates, the name and the line of the failing template.
fn get_errored_templates() -> Vec<(Templates, &'static str, i64)> {
    vec![
        // error occurs in a template
        (
            vec![("index", "\n\n{{ foo.bar }}\n\n\n{{ 'foo' }}")],
            "index",
            3,
        ),
        // error occurs in an included template
        (
            vec![
                ("index", "{% include 'partial' %}"),
                ("partial", "{{ foo.bar }}"),
            ],
            "partial",
            1,
        ),
        // error occurs in a parent block when called via parent()
        (
            vec![
                (
                    "index",
                    "{% extends 'base' %}\n                    {% block content %}\n                        {{ parent() }}\n                    {% endblock %}",
                ),
                ("base", "{% block content %}{{ foo.bar }}{% endblock %}"),
            ],
            "base",
            1,
        ),
        // error occurs in a block from the child
        (
            vec![
                (
                    "index",
                    "{% extends 'base' %}\n                    {% block content %}\n                        {{ foo.bar }}\n                    {% endblock %}\n                    {% block foo %}\n                        {{ foo.bar }}\n                    {% endblock %}",
                ),
                ("base", "{% block content %}{% endblock %}"),
            ],
            "index",
            3,
        ),
        // error occurs in an embed tag
        (
            vec![
                (
                    "index",
                    "\n                    {% embed 'base' %}\n                    {% endembed %}",
                ),
                ("base", "{% block foo %}{{ foo.bar }}{% endblock %}"),
            ],
            "base",
            1,
        ),
        // error occurs in an overridden block from an embed tag
        (
            vec![
                (
                    "index",
                    "\n                    {% embed 'base' %}\n                        {% block foo %}\n                            {{ foo.bar }}\n                        {% endblock %}\n                    {% endembed %}",
                ),
                ("base", "{% block foo %}{% endblock %}"),
            ],
            "index",
            4,
        ),
    ]
}

#[test]
fn test_twig_exception_adds_file_and_line() {
    let mut failures = Failures::default();
    for (i, (templates, name, line)) in get_errored_templates().into_iter().enumerate() {
        let twig = env_with(&templates, debug(true));
        let template = twig.load("index").unwrap();
        for (context, message) in [
            (
                Array::new(),
                format!("Variable \"foo\" does not exist in \"{name}\" at line {line}."),
            ),
            (
                ctx(&[("foo", error_test_foo())]),
                format!(
                    "An exception has been thrown during the rendering of a template (\"Runtime error...\") in \"{name}\" at line {line}."
                ),
            ),
        ] {
            match template.render(context) {
                Ok(output) => failures.fail(format!("#{i}: rendered {output:?}")),
                Err(error) => failures.check(
                    &format!("#{i}"),
                    (
                        error.kind(),
                        error.to_string(),
                        error.template_line(),
                        error.source_context().map(|s| s.name().to_owned()),
                    ),
                    (ErrorKind::Runtime, message, line, Some(name.to_owned())),
                ),
            }
        }
    }
    failures.assert_none();
}

fn check_null_filter(name: &str, template: &str, expected: &str) {
    let twig = env_with(&[(name, template)], debug(false));
    let loaded = twig.load(name).unwrap();
    let items = Value::Array(Array::from_values([1, 2, 3, 4].map(Value::Int)));
    let output = loaded.render(ctx(&[("variable", items)])).unwrap();
    assert_eq!(output.trim(), expected);

    let error = expect_error(
        loaded.render(ctx(&[("variable", Value::Null)])),
        ErrorKind::Runtime,
    );
    assert_eq!(error.template_line(), 2, "{error}");
    assert_eq!(error.source_context().map(|s| s.name()), Some(name));
}

#[test]
fn test_twig_array_filter_throws_runtime_exceptions() {
    check_null_filter(
        "filter-null.html",
        "{# Argument 1 passed to IteratorIterator::__construct() must implement interface Traversable, null given: #}\n{% for n in variable|filter(x => x > 3) %}\n    This list contains {{n}}.\n{% endfor %}",
        "This list contains 4.",
    );
}

#[test]
fn test_twig_array_map_throws_runtime_exceptions() {
    check_null_filter(
        "map-null.html",
        "{# We expect a runtime error if `variable` is not traversable #}\n{% for n in variable|map(x => x * 3) %}\n    {{- n -}}\n{% endfor %}",
        "36912",
    );
}

#[test]
fn test_twig_array_reduce_throws_runtime_exceptions() {
    check_null_filter(
        "reduce-null.html",
        "{# We expect a runtime error if `variable` is not traversable #}\n{{ variable|reduce((carry, x) => carry + x) }}",
        "10",
    );
}

#[test]
fn test_twig_exception_update_file_and_line_together() {
    let twig = env_with(&[("index", "\n\n\n\n{{ foo() }}")], debug(false));
    let error = expect_error(
        twig.load("index").and_then(|t| t.render(Array::new())),
        ErrorKind::Syntax,
    );
    assert_eq!(
        error.to_string(),
        "Unknown \"foo\" function in \"index\" at line 5."
    );
    assert_eq!(error.template_line(), 5);
}

const INCLUDE_TWIG: &str = "\n\n\n{% extends 'invalid.twig' %}";

#[test]
fn test_error_from_array_loader() {
    let twig = env_with(
        &[
            ("index.twig", "{% include \"include.twig\" %}"),
            ("include.twig", INCLUDE_TWIG),
        ],
        debug(false),
    );
    let error = expect_error(twig.render("index.twig", Array::new()), ErrorKind::Loader);
    assert_eq!(
        error.raw_message(),
        "Template \"invalid.twig\" is not defined."
    );
    assert_eq!(error.template_line(), 4);
    let source = error.source_context().expect("a source");
    assert_eq!(source.name(), "include.twig");
    assert_eq!(source.code(), INCLUDE_TWIG.as_bytes());
}

/// Not an upstream test: found while porting `EnvironmentTest::testCompiledTemplatesKeepTheirErrorWhenTheLoader...`.
/// A runtime error gets the source of the compiled template (`ModuleNode::compileGetSourceContext()`), whose code is
/// `''` unless the template was compiled with `debug`, and so does `TemplateWrapper::getSourceContext()`; syntax
/// errors keep the loader's source. VERIFIED with the oracle (`$e->getSourceContext()->getCode()`,
/// `$twig->load('index')->getSourceContext()->getCode()`).
#[test]
#[ignore = "twig-rs mismatch: runtime errors and wrappers outside debug mode carry the template code ('' in PHP)"]
fn runtime_errors_carry_the_template_code_only_in_debug_mode() {
    for debug in [false, true] {
        let options = EnvironmentOptions {
            debug,
            strict_variables: true,
            ..Default::default()
        };
        let twig = env_with(&[("index", "a\n{{ foo.bar }}")], options.clone());
        let error = expect_error(twig.render("index", Array::new()), ErrorKind::Runtime);
        let expected: &[u8] = if debug { b"a\n{{ foo.bar }}" } else { b"" };
        assert_eq!(
            error.source_context().map(|s| s.code().to_vec()),
            Some(expected.to_vec()),
            "debug={debug}"
        );
        assert_eq!(
            twig.load("index").unwrap().source_context().code(),
            expected,
            "debug={debug}"
        );
        let twig = env_with(&[("index", "a\n{{ foo( }}")], options);
        let error = expect_error(twig.render("index", Array::new()), ErrorKind::Syntax);
        assert_eq!(
            error.source_context().map(|s| s.code().to_vec()),
            Some(b"a\n{{ foo( }}".to_vec()),
            "debug={debug}"
        );
    }
}

#[test]
fn test_error_from_filesystem_loader() {
    let dir = fixtures("/extends");
    let twig = filesystem_env(&dir, debug(false));
    let include = std::fs::read(format!("{dir}/include.twig")).unwrap();
    let error = expect_error(twig.render("index.twig", Array::new()), ErrorKind::Loader);
    assert!(
        error
            .raw_message()
            .contains("Unable to find template \"invalid.twig\""),
        "{error}"
    );
    assert_eq!(error.template_line(), 4);
    let source = error.source_context().expect("a source");
    assert_eq!(source.name(), "include.twig");
    assert_eq!(source.code(), include.as_slice());
}
