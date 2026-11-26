//! Port of `TWIG/tests/Extension/StringLoaderExtensionTest.php`: `CoreExtension::include()` of
//! `StringLoaderExtension::templateFromString()` is what `{{ include(template_from_string(...)) }}` compiles to.

use std::sync::Arc;

use twig::extension::StringLoaderExtension;
use twig::runtime::Environment;
use twig::value::Array;

use super::support::loader;

#[test]
fn test_include_with_template_string_and_no_sandbox() {
    let twig = Environment::new(loader(&[(
        "index",
        "{{ include(template_from_string('something')) }}",
    )]));
    twig.add_extension(Arc::new(StringLoaderExtension::new()))
        .unwrap();
    assert_eq!(twig.render("index", Array::new()).unwrap(), "something");
}
