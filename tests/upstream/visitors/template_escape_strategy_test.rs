//! Twig 3.31.1's `tests/TemplateEscapeStrategyTest.php`: `TemplateWrapper::getDefaultEscapeStrategy()`.

use twig::Source;
use twig::escaper::DefaultStrategy;
use twig::runtime::EnvironmentOptions;

use super::support::{Mismatches, Oracle, Outcome, ctx, env, s};
use crate::common::Json;

fn oracle() -> Oracle {
    Oracle::load("template_escape_strategy")
}

/// `['autoescape' => $autoescape, 'cache' => false]` (`None` for `false`).
fn autoescape(strategy: Option<&str>) -> EnvironmentOptions {
    EnvironmentOptions {
        autoescape: match strategy {
            Some(strategy) => DefaultStrategy::Strategy(strategy.to_owned()),
            None => DefaultStrategy::Disabled,
        },
        ..Default::default()
    }
}

/// The JSON of `string|false`.
fn strategy_of(json: &Json) -> Option<String> {
    json.as_str().map(str::to_owned)
}

/// `provideTemplates()`: expected strategy (`None` for `false`), template name, autoescape option.
const TEMPLATES: [(Option<&str>, &str, Option<&str>); 5] = [
    (Some("html"), "guessed_html.html.twig", Some("name")),
    (Some("js"), "guessed_js.js.twig", Some("name")),
    (None, "guessed_none.txt.twig", Some("name")),
    (Some("html"), "forced_html.js.twig", Some("html")),
    (None, "disabled.html.twig", None),
];

/// `testCompiledTemplatesExposeTheirStrategy()`.
#[test]
fn compiled_templates_expose_their_strategy() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, (expected, name, strategy)) in TEMPLATES.iter().enumerate() {
        let env = env(&[(name, "{{ value }}")], autoescape(*strategy));
        let actual = env
            .load(name)
            .unwrap()
            .default_escape_strategy()
            .map(str::to_owned);
        mismatches.check(name, &expected.map(str::to_owned), &actual);
        mismatches.check(
            name,
            &strategy_of(&oracle.value(&format!("compiled_templates_expose_their_strategy/{i}"))),
            &actual,
        );
    }
    mismatches.assert_none();
}

/// `testAnAutoescapeTagDoesNotChangeTheStrategyOfTheTemplate()` with `provideAutoescapeTags()`.
#[test]
fn an_autoescape_tag_does_not_change_the_strategy_of_the_template() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, (name, tag)) in [
        ("another_strategy.html.twig", "'js'"),
        ("no_escaping.html.twig", "false"),
    ]
    .into_iter()
    .enumerate()
    {
        let template = format!("{{% autoescape {tag} %}}{{{{ value }}}}{{% endautoescape %}}");
        let env = env(&[(name, &template)], autoescape(Some("name")));
        let actual = env
            .load(name)
            .unwrap()
            .default_escape_strategy()
            .map(str::to_owned);
        mismatches.check(name, &Some("html".to_owned()), &actual);
        mismatches.check(
            name,
            &strategy_of(&oracle.value(&format!(
                "an_autoescape_tag_does_not_change_the_strategy_of_the_template/{i}"
            ))),
            &actual,
        );
    }
    mismatches.assert_none();
}

/// `testTheStrategyDescribesTheBodyAndNotWhatTheTemplateRenders()`.
#[test]
fn the_strategy_describes_the_body_and_not_what_the_template_renders() {
    let env = env(
        &[
            ("inherits.html.twig", "{% extends 'inherited.txt.twig' %}"),
            ("inherited.txt.twig", "{{ value }}"),
        ],
        autoescape(Some("name")),
    );
    let template = env.load("inherits.html.twig").unwrap();
    assert_eq!(template.default_escape_strategy(), Some("html"));
    assert_eq!(
        template.render(ctx(vec![("value", s("<b>"))])).unwrap(),
        "<b>"
    );
    let expected =
        oracle().value("the_strategy_describes_the_body_and_not_what_the_template_renders");
    assert_eq!(expected.field("strategy").as_str(), Some("html"));
    assert_eq!(expected.field("output").as_str(), Some("<b>"));
}

/// `testEmbeddedTemplatesExposeTheStrategyOfTheirTemplate()`: the compiled template and its embedded template both
/// report the strategy of the file (`js`), not the one of the embedded parent's name; plus the render.
#[test]
fn embedded_templates_expose_the_strategy_of_their_template() {
    let env = env(
        &[
            (
                "index.js.twig",
                "{% embed 'layout.html.twig' %}{% block content %}{{ value }}{% endblock %}{% endembed %}",
            ),
            ("layout.html.twig", "{% block content %}{% endblock %}"),
        ],
        autoescape(Some("name")),
    );
    let code = env
        .loader()
        .get_source_context("index.js.twig")
        .unwrap()
        .code()
        .to_vec();
    let program = env
        .compile_source(&Source::new(code, "index.js.twig"))
        .unwrap();
    let mut strategies = vec![program.default_escape_strategy().map(str::to_owned)];
    strategies.extend(
        program
            .embedded()
            .iter()
            .map(|embedded| embedded.default_escape_strategy().map(str::to_owned)),
    );
    let expected = oracle().value("embedded_templates_expose_the_strategy_of_their_template");
    assert_eq!(
        i64::try_from(strategies.len()).unwrap(),
        expected.field("methods").as_i64().unwrap()
    );
    let js = usize::try_from(expected.field("js").as_i64().unwrap()).unwrap();
    assert_eq!(strategies, vec![Some("js".to_owned()); js]);
    assert_eq!(
        Outcome::of(env.render("index.js.twig", ctx(vec![("value", s("<b>'"))]))),
        Outcome::Output(expected.field("output").as_str().unwrap().to_owned())
    );
}
