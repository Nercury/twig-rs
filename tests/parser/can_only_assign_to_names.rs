extern crate twig;

use super::support;

#[test]
fn test_can_only_assign_to_names() {
    for template in get_failing_tests_for_assignment() {
        match support::maybe_parsed(template) {
            Ok(_) => panic!("expected {:?} to produce error", template),
            Err(e) => { println!("tmp {} produces {:?}", template, e); },
        }
    }
}

fn get_failing_tests_for_assignment<'r>() -> Vec<&'static str> {
    vec![
        r#"{% set false = "foo" %}"#,
        r#"{% set true = "foo" %}"#,
        r#"{% set none = "foo" %}"#,
        r#"{% set 3 = "foo" %}"#,
        r#"{% set 1 + 2 = "foo" %}"#,
        r#"{% set "bar" = "foo" %}"#,
        r#"{% set %}{% endset %}"#,
    ]
}
