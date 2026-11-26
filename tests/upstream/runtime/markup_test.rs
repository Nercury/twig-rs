//! Port of TWIG/tests/MarkupTest.php.

use twig::runtime::Markup;
use twig::value::object::{CallArgs, ObjectRef};
use twig::value::{Ignore, PhpString};

fn markup(content: &str, safe_strategies: Option<&[&str]>) -> ObjectRef {
    Markup::create(
        PhpString::from(content),
        Some("UTF-8".into()),
        safe_strategies.map(|s| s.iter().map(|s| PhpString::from(*s)).collect()),
    )
}

fn is_safe_for(markup: &ObjectRef, strategy: &str) -> bool {
    markup
        .downcast_ref::<Markup>()
        .expect("a Twig\\Markup")
        .is_safe_for(strategy.as_bytes())
}

#[test]
fn test_content_is_safe_for_all_strategies_by_default() {
    assert!(is_safe_for(&markup("<br />", None), "js"));
}

#[test]
fn test_content_is_safe_for_the_given_strategies() {
    let provide_safe_strategies: [(bool, &[&str], &str); 7] = [
        (true, &["all"], "js"),
        (true, &["html", "js"], "js"),
        (false, &["html", "js"], "css"),
        (true, &["html_attr"], "html"),
        (true, &["html_attr"], "html_attr_relaxed"),
        (false, &["html"], "html_attr"),
        (false, &[], "html"),
    ];
    for (safe, safe_strategies, strategy) in provide_safe_strategies {
        assert_eq!(
            is_safe_for(&markup("<br />", Some(safe_strategies)), strategy),
            safe,
            "{safe_strategies:?} {strategy}"
        );
    }
}

#[test]
fn test_with_content_keeps_the_safe_strategies() {
    let original = markup("<br />", Some(&["html"]));
    let markup = original
        .call_method("withContent", CallArgs::new().arg("<hr />"), &mut Ignore)
        .unwrap();
    let markup = markup.as_object().expect("a Twig\\Markup").clone();
    assert_eq!(markup.to_php_string(&mut Ignore).unwrap(), "<hr />");
    assert!(is_safe_for(&markup, "html"));
    assert!(!is_safe_for(&markup, "js"));
}
