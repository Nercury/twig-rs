extern crate twig;

use super::support;

#[test]
#[should_panic(
    expected = r#"Arguments must be separated by a comma at line 1"#
)]
fn attribute_call_does_not_support_named_arguments() {
    support::maybe_parsed(r#"{{ foo.bar(name="Foo") }}"#).unwrap();
}
