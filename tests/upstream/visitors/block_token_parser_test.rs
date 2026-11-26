//! The rendering parts of Twig 3.31.1's `tests/TokenParser/BlockTokenParserTest.php` (the documentation comments
//! attached to the parsed nodes are checked in tests/parser/token_parser_test.rs).

use twig::ErrorKind;
use twig::escaper::DefaultStrategy;
use twig::runtime::EnvironmentOptions;
use twig::value::Array;

use super::support::{Mismatches, Oracle, Outcome, defaults, env};

fn oracle() -> Oracle {
    Oracle::load("token_parser")
}

/// `provideDocumentationWhitespaceControl()`: name, template, expected output.
const DOCUMENTATION_WHITESPACE_CONTROL: [(&str, &str, &str); 4] = [
    (
        "opening whitespace trim marker",
        "A \n{##- The main content #}\n{% block content %}B{% endblock %}",
        "AB",
    ),
    (
        "opening line whitespace trim marker",
        "A \n  {##~ The main content #}\n{% block content %}B{% endblock %}",
        "A \nB",
    ),
    (
        "closing whitespace trim marker",
        "A{## The main content -##}\n  {% block content %}B{% endblock %}",
        "AB",
    ),
    (
        "closing line whitespace trim marker",
        "A{## The main content ~##}  \n{% block content %}B{% endblock %}",
        "A\nB",
    ),
];

/// `testDocumentationWhitespaceControl()`: `$env->render('index')` with `['autoescape' => false, 'optimizations' =>
/// 0]`.
#[test]
fn documentation_whitespace_control() {
    let oracle = oracle();
    let mut mismatches = Mismatches::default();
    for (i, (name, template, expected)) in DOCUMENTATION_WHITESPACE_CONTROL.iter().enumerate() {
        let options = EnvironmentOptions {
            autoescape: DefaultStrategy::Disabled,
            optimizations: 0,
            ..Default::default()
        };
        let actual =
            Outcome::of(env(&[("index", template)], options).render("index", Array::new()));
        mismatches.check(name, &Outcome::Output((*expected).to_owned()), &actual);
        mismatches.check(
            name,
            &oracle.outcome(&format!("documentation_whitespace_control/{i}")),
            &actual,
        );
    }
    mismatches.assert_none();
}

/// `testShortcutAssignmentNamedDocsRemainsValid()`: `docs="..."` in the block shortcut is an assignment.
#[test]
fn shortcut_assignment_named_docs_remains_valid() {
    let env = env(
        &[("index", r#"{% block title docs="The page title" %}"#)],
        defaults(),
    );
    let actual = env.render("index", Array::new());
    assert_eq!(actual.as_deref().ok(), Some("The page title"));
    assert_eq!(
        oracle().outcome("shortcut_assignment_named_docs_remains_valid"),
        Outcome::of(actual)
    );
}

/// `testDocumentationCommentRequiresALineBreak()`: a `SyntaxError` (the message from the oracle).
#[test]
fn documentation_comment_requires_a_line_break() {
    let env = env(
        &[("index", "{% block ## Description %}Hello{% endblock %}")],
        defaults(),
    );
    let actual = env.render("index", Array::new());
    assert_eq!(actual.as_ref().unwrap_err().kind(), ErrorKind::Syntax);
    assert_eq!(
        oracle().outcome("documentation_comment_requires_a_line_break"),
        Outcome::of(actual)
    );
}
