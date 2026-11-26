//! Twig 3.31.1's `tests/NodeVisitor/EscaperTest.php`.

use std::sync::Arc;

use twig::ast::{Arguments, Expr, NodeKind, NodeMut};
use twig::escaper::DefaultStrategy;
use twig::extension::{Extension, ExtensionSet, NodeVisitor};
use twig::lltl::Program;
use twig::runtime::EnvironmentOptions;
use twig::value::Array;
use twig::visitor::{Visit, VisitContext, VisitResult};
use twig::{ErrorKind, Result};

use super::support::{Mismatches, Oracle, Outcome, ctx, env, s};

fn oracle() -> Oracle {
    Oracle::load("escaper")
}

fn autoescape(strategy: Option<&str>) -> EnvironmentOptions {
    EnvironmentOptions {
        autoescape: match strategy {
            Some(strategy) => DefaultStrategy::Strategy(strategy.to_owned()),
            None => DefaultStrategy::Disabled,
        },
        ..Default::default()
    }
}

/// How many classes of a compiled file hold the escaper runtime (`private \Twig\Runtime\EscaperRuntime $escaper;`):
/// the template and its embedded templates whose escape filters use the template's escaper.
fn escaper_classes(program: &Program) -> i64 {
    i64::from(program.uses_template_escaper())
        + program
            .embedded()
            .iter()
            .map(|embedded| escaper_classes(embedded))
            .sum::<i64>()
}

/// `provideTemplates()`: name, autoescape option, template, escaper classes, output.
const TEMPLATES: [(&str, Option<&str>, &str, i64, &str); 6] = [
    (
        "autoescaped print",
        Some("html"),
        "{{ foo }}",
        1,
        "&lt;br&gt;",
    ),
    (
        "print marked as safe",
        Some("html"),
        "{{ foo|raw }}",
        0,
        "<br>",
    ),
    (
        "autoescaping disabled",
        None,
        "{{ foo }}{# not autoescaped #}",
        0,
        "<br>",
    ),
    (
        "explicit escape filter",
        None,
        "{{ foo|e }}",
        1,
        "&lt;br&gt;",
    ),
    (
        "autoescape tag",
        None,
        r#"{% autoescape "html" %}{{ foo }}{% endautoescape %}"#,
        1,
        "&lt;br&gt;",
    ),
    (
        "escaping in an embedded template only",
        None,
        r#"{% embed "embedded" %}{% block content %}{{ foo|e }}{% endblock %}{% endembed %}"#,
        1,
        "&lt;br&gt;",
    ),
];

/// `testTemplatesFetchTheEscaperOnlyWhenTheyEscape()`: the compiled code holds the escaper runtime only when the
/// template escapes (counted on `Program::uses_template_escaper()`), and the output.
#[test]
fn templates_fetch_the_escaper_only_when_they_escape() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, (name, strategy, template, classes, output)) in TEMPLATES.iter().enumerate() {
        let env = env(
            &[
                ("index", template),
                ("embedded", "{% block content %}{% endblock %}"),
            ],
            autoescape(*strategy),
        );
        let source = env.loader().get_source_context("index").unwrap();
        let program = env.compile_source(&source).unwrap();
        let expected = oracle.value(&format!(
            "templates_fetch_the_escaper_only_when_they_escape/{i}"
        ));
        mismatches.check(name, classes, &escaper_classes(&program));
        mismatches.check(
            name,
            &expected.field("escaper_classes").as_i64().unwrap(),
            &escaper_classes(&program),
        );
        let actual = Outcome::of(env.render("index", ctx(vec![("foo", s("<br>"))])));
        mismatches.check(name, &Outcome::Output((*output).to_owned()), &actual);
        mismatches.check(
            name,
            &Outcome::Output(expected.field("output").as_str().unwrap().to_owned()),
            &actual,
        );
    }
    mismatches.assert_none();
}

/// `testCompilationErrorInsideAnAutoescapeTagDoesNotAffectTheNextTemplate()`, with the compilation error twig-rs
/// can raise from the escaper visitor inside an `autoescape` tag (Twig's test uses an `is_safe_callback` that
/// throws, which twig-rs callbacks cannot): `{% autoescape null %}` makes `needEscaping()` fail with the `TypeError`
/// of its `string|bool` return type, which `compileSource()` does not wrap (it is a PHP `\Error`). The templates
/// compiled next are escaped with their own strategy.
#[test]
fn compilation_error_inside_an_autoescape_tag_does_not_affect_the_next_template() {
    let env = env(
        &[
            (
                "broken.html",
                "{% autoescape false %}{% autoescape null %}{{ foo|nl2br }}{% endautoescape %}{% endautoescape %}",
            ),
            ("index.html", "{{ foo }}"),
            ("index.txt", "{{ foo }}"),
        ],
        autoescape(Some("name")),
    );
    let error = env.load("broken.html").unwrap_err();
    let expected = oracle().value(
        "compilation_error_inside_an_autoescape_tag_does_not_affect_the_next_template/null_strategy",
    );
    let php_error = expected.field("error");
    assert_eq!(error.kind(), ErrorKind::Php);
    assert_eq!(error.class_name(), "TypeError");
    assert_eq!(
        error.class_name(),
        php_error.field("class").as_str().unwrap()
    );
    assert_eq!(
        error.to_string(),
        "Twig\\NodeVisitor\\EscaperNodeVisitor::needEscaping(): Return value must be of type string|bool, null returned"
    );
    assert_eq!(
        error.to_string(),
        php_error.field("message").as_str().unwrap()
    );
    assert!(error.previous().is_none());
    assert!(php_error.get("previous").is_none());

    assert_eq!(
        env.render("index.txt", ctx(vec![("foo", s("<br>"))]))
            .unwrap(),
        "<br>"
    );
    assert_eq!(
        env.render("index.html", ctx(vec![("foo", s("<br>"))]))
            .unwrap(),
        "&lt;br&gt;"
    );
    assert_eq!(expected.field("index.txt").as_str(), Some("<br>"));
    assert_eq!(expected.field("index.html").as_str(), Some("&lt;br&gt;"));
}

/// The node visitor of `testEscapeFilterAddedByALaterVisitorStillEscapes()` (priority 10): every print gets an
/// `escape('html')` filter of the class the `escape` filter declares.
struct EscapeLater;

struct EscapeLaterPass;

impl NodeVisitor for EscapeLater {
    fn priority(&self) -> i64 {
        10
    }

    fn start<'a>(&'a self, _extensions: &'a ExtensionSet) -> Box<dyn Visit + 'a> {
        Box::new(EscapeLaterPass)
    }
}

impl Visit for EscapeLaterPass {
    fn enter(&mut self, _node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<()> {
        Ok(())
    }

    fn leave(&mut self, node: NodeMut<'_>, cx: &mut VisitContext<'_>) -> Result<VisitResult> {
        let NodeMut::Node(node) = node else {
            return Ok(VisitResult::Keep);
        };
        let NodeKind::Print(print) = &mut node.kind else {
            return Ok(VisitResult::Keep);
        };
        let filter = cx
            .extensions()
            .get_filter(b"escape")?
            .expect("the escape filter");
        let mut arguments = Arguments::new();
        arguments.push(Expr::constant("html", 1));
        let expr = std::mem::replace(&mut print.expr, Expr::constant("", 1));
        print.expr = Expr::filter(expr, filter, arguments, 1)?;
        node.line = 1;
        Ok(VisitResult::Keep)
    }
}

struct EscapeLaterExtension;

impl Extension for EscapeLaterExtension {
    fn class_name(&self) -> &str {
        "EscapeLaterExtension"
    }

    fn node_visitors(&self) -> Vec<Arc<dyn NodeVisitor>> {
        vec![Arc::new(EscapeLater)]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `testEscapeFilterAddedByALaterVisitorStillEscapes()`.
#[test]
fn escape_filter_added_by_a_later_visitor_still_escapes() {
    let env = env(&[("index", r#"{{ "<br>" }}"#)], autoescape(None));
    env.add_extension(Arc::new(EscapeLaterExtension)).unwrap();
    let actual = env.render("index", Array::new());
    assert_eq!(actual.as_deref().ok(), Some("&lt;br&gt;"), "{actual:?}");
    let expected = oracle().value("escape_filter_added_by_a_later_visitor_still_escapes");
    assert_eq!(actual.unwrap(), expected.as_str().unwrap());
}
