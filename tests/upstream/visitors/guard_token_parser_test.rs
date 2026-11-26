//! Twig 3.31.1's `tests/TokenParser/GuardTokenParserTest.php` through the environment (the parser-level port is in
//! tests/parser/token_parser_test.rs).

use std::sync::Arc;

use twig::value::Array;
use twig::{Error, Source};

use super::support::{Oracle, env, no_escape};

/// `testUndefinedHandlers()`: an undefined-function callback that throws a `SyntaxError` means that the guarded
/// function does not exist; the template compiles.
#[test]
fn undefined_handlers() {
    let env = env(&[], no_escape());
    env.register_undefined_function_callback(Arc::new(|_| Err(Error::syntax("boom."))));
    let compiled = env.compile_source(&Source::new("{% guard function boom %}{% endguard %}", ""));
    assert!(compiled.is_ok(), "{:?}", compiled.err());
    let expected = Oracle::load("token_parser").value("undefined_handlers");
    assert!(expected.field("parse_error").is_null());
}

/// Beyond the upstream test: the guard of a function the callback rejects renders its `else` branch.
#[test]
fn undefined_handlers_render_the_else_branch() {
    let env = env(
        &[(
            "index",
            "{% guard function boom %}yes{% else %}no{% endguard %}",
        )],
        no_escape(),
    );
    env.register_undefined_function_callback(Arc::new(|_| Err(Error::syntax("boom."))));
    let expected = Oracle::load("token_parser").value("undefined_handlers");
    assert_eq!(
        env.render("index", Array::new()).unwrap(),
        expected.field("render").as_str().unwrap()
    );
}
