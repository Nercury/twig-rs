//! Port of TWIG/tests/TemplateWrapperTest.php.

use twig::ErrorKind;
use twig::escaper::DefaultStrategy;
use twig::extension::runtime::CallableRuntime;
use twig::extension::{FunctionOptions, PhpCallable, TwigFunction};
use twig::runtime::EnvironmentOptions;
use twig::value::{Array, PhpError, ThrowableClass, Value};

use crate::support::*;

#[test]
fn test_unwrap_checks_the_environment() {
    let twig = env(&[("index", "content")]);
    let wrapper = twig.load("index").unwrap();
    assert_eq!(wrapper.unwrap(Some(&twig)).unwrap().name(), "index");

    let error = expect_error(wrapper.unwrap(Some(&env(&[]))), ErrorKind::Runtime);
    assert!(
        error
            .to_string()
            .contains(r#"can only be used with the "Twig\Environment" that created it"#),
        "{error}"
    );
    // the oracle's whole message
    assert_eq!(
        error.to_string(),
        r#"A "Twig\TemplateWrapper" can only be used with the "Twig\Environment" that created it."#
    );
}

/// The check of `testUnwrapChecksTheEnvironment` through a template: `Template::load()` unwraps a wrapper given to
/// `{% include %}` with the environment of the including template.
#[test]
fn unwrap_checks_the_environment_through_a_template() {
    let twig = env(&[("index", "content"), ("include", "{% include tpl %}")]);
    let wrapper = twig.load("index").unwrap();
    assert_eq!(
        twig.render("include", ctx(&[("tpl", wrapper.to_value())]))
            .unwrap(),
        "content"
    );
    let other = env(&[("include", "{% include tpl %}")]);
    let error = expect_error(
        other.render("include", ctx(&[("tpl", wrapper.to_value())])),
        ErrorKind::Runtime,
    );
    assert!(
        error
            .to_string()
            .contains(r#"can only be used with the "Twig\Environment" that created it"#),
        "{error}"
    );
}

#[test]
fn test_unwrap_can_be_called_without_arguments() {
    let twig = env(&[("index", "content")]);
    let wrapper = twig.load("index").unwrap();
    let (template, deprecations) = collect_deprecations(&twig, || wrapper.unwrap(None).unwrap());
    assert_eq!(template.name(), "index");
    assert_eq!(
        deprecations,
        [
            r#"Since twig/twig 3.30: Calling "Twig\TemplateWrapper::unwrap()" without arguments is deprecated, pass the Twig environment instead."#
        ]
    );
}

#[test]
fn test_get_default_escape_strategy() {
    let twig = env_with(
        &[("index.js.twig", "content")],
        EnvironmentOptions {
            autoescape: DefaultStrategy::Strategy("name".to_owned()),
            ..Default::default()
        },
    );
    assert_eq!(
        twig.load("index.js.twig")
            .unwrap()
            .default_escape_strategy(),
        Some("js")
    );
}

#[test]
fn test_has_get_blocks() {
    let twig = env(&[
        ("index", "{% block foo %}{% endblock %}"),
        (
            "index_with_use",
            "{% use \"imported\" %}{% block foo %}{% endblock %}",
        ),
        (
            "index_with_extends",
            "{% extends \"extended\" %}{% block foo %}{% endblock %}",
        ),
        ("imported", "{% block imported %}{% endblock %}"),
        ("extended", "{% block extended %}{% endblock %}"),
    ]);

    let wrapper = twig.load("index").unwrap();
    assert!(wrapper.has_block("foo", Array::new()).unwrap());
    assert!(!wrapper.has_block("bar", Array::new()).unwrap());
    assert_eq!(wrapper.block_names(Array::new()).unwrap(), ["foo"]);

    let wrapper = twig.load("index_with_use").unwrap();
    assert!(wrapper.has_block("foo", Array::new()).unwrap());
    assert!(wrapper.has_block("imported", Array::new()).unwrap());
    assert_eq!(
        wrapper.block_names(Array::new()).unwrap(),
        ["imported", "foo"]
    );

    let wrapper = twig.load("index_with_extends").unwrap();
    assert!(wrapper.has_block("foo", Array::new()).unwrap());
    assert!(wrapper.has_block("extended", Array::new()).unwrap());
    assert_eq!(
        wrapper.block_names(Array::new()).unwrap(),
        ["foo", "extended"]
    );
}

#[test]
fn test_block_introspection_includes_globals() {
    let twig = env(&[
        ("index", "{% extends layout %}"),
        ("global_parent", "{% block global %}{% endblock %}"),
        ("local_parent", "{% block local %}{% endblock %}"),
    ]);
    twig.add_global("layout", "global_parent").unwrap();

    let wrapper = twig.load("index").unwrap();
    assert!(wrapper.has_block("global", Array::new()).unwrap());
    assert!(!wrapper.has_block("local", Array::new()).unwrap());
    assert_eq!(wrapper.block_names(Array::new()).unwrap(), ["global"]);

    let context = ctx(&[("layout", Value::from("local_parent"))]);
    assert!(wrapper.has_block("local", context.clone()).unwrap());
    assert!(!wrapper.has_block("global", context.clone()).unwrap());
    assert_eq!(wrapper.block_names(context).unwrap(), ["local"]);
}

#[test]
fn test_stream_block_includes_globals() {
    let twig = env(&[
        ("index", "{% extends layout %}"),
        ("layout", "{% block foo %}{{ foo }}{{ bar }}{% endblock %}"),
    ]);
    twig.add_global("layout", "layout").unwrap();
    twig.add_global("bar", "BAR").unwrap();

    let mut streamed: Vec<u8> = Vec::new();
    twig.load("index")
        .unwrap()
        .stream_block("foo", ctx(&[("foo", Value::from("FOO"))]), &mut streamed)
        .unwrap();
    assert_eq!(streamed, b"FOOBAR");
}

fn boom() -> TwigFunction {
    // new TwigFunction('boom', static function (): never { throw new \DomainException('kaboom'); })
    let callable = PhpCallable::closure(vec![]);
    CallableRuntime::new(|_| Err(PhpError::new(ThrowableClass::DomainException, "kaboom").into()))
        .attach(&callable);
    TwigFunction::new("boom", Some(callable), FunctionOptions::default())
}

#[test]
fn test_dynamic_parent_failures_are_wrapped() {
    let data: [(&str, &str, Option<TwigFunction>, &str); 2] = [
        (
            "missing parent variable",
            "{% extends parent %}",
            None,
            "TypeError",
        ),
        (
            "exception thrown by parent expression",
            "{% extends boom() %}",
            Some(boom()),
            "DomainException",
        ),
    ];
    let mut failures = Failures::default();
    for (name, template, function, previous_class) in data {
        let twig = env(&[("index", template)]);
        if let Some(function) = function {
            twig.add_function(function).unwrap();
        }
        let entry_points: [(&str, twig::Result<String>); 2] = [
            (
                "block introspection",
                twig.load("index")
                    .and_then(|t| t.block_names(Array::new()))
                    .map(|names| names.join(",")),
            ),
            ("rendering", twig.render("index", Array::new())),
        ];
        for (entry_point, result) in entry_points {
            let label = format!("{name} during {entry_point}");
            match result {
                Ok(output) => failures.fail(format!("{label}: no error, got {output:?}")),
                Err(error) => failures.check(
                    &label,
                    (
                        error.kind(),
                        error.source_context().map(|s| s.name().to_owned()),
                        error.template_line(),
                        previous(&error).map(|p| p.split(':').next().unwrap_or("").to_owned()),
                    ),
                    (
                        ErrorKind::Runtime,
                        Some("index".to_owned()),
                        1,
                        Some(previous_class.to_owned()),
                    ),
                ),
            }
        }
    }
    failures.assert_none();
}

#[test]
fn test_dynamic_parent_twig_errors_are_not_wrapped() {
    let twig = env(&[("index", "{% extends parent %}")]);
    let error = expect_error(
        twig.load("index")
            .unwrap()
            .block_names(ctx(&[("parent", Value::from("missing"))])),
        ErrorKind::Loader,
    );
    assert_eq!(error.source_context().map(|s| s.name()), Some("index"));
    assert_eq!(error.template_line(), 1);
    assert!(error.previous().is_none());
}

#[test]
fn test_block_introspection_reports_the_extends_line_for_a_missing_parent() {
    let twig = env(&[("index", "\n\n{% extends 'missing' %}")]);
    let error = expect_error(
        twig.load("index").unwrap().has_block("foo", Array::new()),
        ErrorKind::Loader,
    );
    assert_eq!(error.raw_message(), "Template \"missing\" is not defined.");
    assert_eq!(error.template_line(), 3);
}

#[test]
fn test_render_block() {
    let twig = env(&[("index", "{% block foo %}{{ foo }}{{ bar }}{% endblock %}")]);
    twig.add_global("bar", "BAR").unwrap();
    let wrapper = twig.load("index").unwrap();
    assert_eq!(
        wrapper
            .render_block("foo", ctx(&[("foo", Value::from("FOO"))]))
            .unwrap(),
        "FOOBAR"
    );
}

#[test]
fn test_display_block() {
    let twig = env(&[("index", "{% block foo %}{{ foo }}{{ bar }}{% endblock %}")]);
    twig.add_global("bar", "BAR").unwrap();
    let wrapper = twig.load("index").unwrap();
    let mut out: Vec<u8> = Vec::new();
    wrapper
        .display_block("foo", ctx(&[("foo", Value::from("FOO"))]), &mut out)
        .unwrap();
    assert_eq!(out, b"FOOBAR");
}
