//! Port of `TWIG/tests/Extension/EscaperTest.php`.
//!
//! The upstream tests register escapers and safe classes with the deprecated `EscaperExtension::setEscaper()` /
//! `addSafeClass()` (legacy escapers receive the environment first), which twig-rs does not provide
//! (reference/deviations.md, "Deprecated escaper API"). The ports register them on the environment's escaper runtime
//! (`Environment::escaper_runtime()`, Twig's `getRuntime(EscaperRuntime::class)`), where the legacy API forwards them,
//! and keep the assertions: an environment's escapers reach its runtime only, they receive the charset passed to
//! `escape()` (the legacy escaper appends the environment's charset itself), and a runtime registered for
//! `Twig\Runtime\EscaperRuntime` (Twig's runtime loaders) is the one that receives the safe classes.
//!
//! Not ported: `testLastModified` (`getLastModified()` is the mtime of the PHP class file).

use std::sync::Arc;

use twig::extension::runtime::{EscaperCallback, EscaperRuntime};
use twig::runtime::{Environment, FactoryRuntimeLoader};
use twig::value::{Ignore, PhpString, Value, ops};

use super::support::{check_cases, export, loader};

/// `legacy_escaper(Environment $twig, $string, $charset)`: `$string.'**'.$charset.'**'.$twig->getCharset()`, with an
/// optional suffix (`legacy_escaper_again()` appends `**again`).
fn legacy_escaper(env_charset: String, suffix: &'static str) -> EscaperCallback {
    Arc::new(move |value, charset, diag| {
        let mut string = ops::concat(value, &Value::from("**"), diag)?;
        string = ops::concat(
            &Value::String(string),
            &Value::String(charset.clone()),
            diag,
        )?;
        string = ops::concat(
            &Value::String(string),
            &Value::from(format!("**{env_charset}{suffix}")),
            diag,
        )?;
        Ok(Value::String(string))
    })
}

/// `$twig->getRuntime(EscaperRuntime::class)->escape($string, $strategy, 'ISO-8859-1')`.
fn escape(env: &Environment, string: Value, strategy: &str) -> Result<Value, String> {
    env.escaper_runtime()
        .escape(
            string,
            strategy.as_bytes(),
            Some(b"ISO-8859-1"),
            false,
            &mut Ignore,
        )
        .map_err(|e| format!("{e:?}"))
}

fn provide_custom_escaper_cases() -> Vec<(String, (&'static str, Value, &'static str))> {
    vec![
        ("foo**ISO-8859-1**UTF-8", Value::from("foo"), "foo"),
        ("**ISO-8859-1**UTF-8", Value::Null, "foo"),
        ("42**ISO-8859-1**UTF-8", Value::Int(42), "foo"),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

fn custom_escaper(test: &str) {
    check_cases(
        test,
        provide_custom_escaper_cases(),
        |(expected, string, strategy)| {
            let twig = Environment::new(loader(&[]));
            twig.escaper_runtime()
                .set_escaper("foo", legacy_escaper(twig.charset(), ""));
            let escaped = escape(&twig, string.clone(), strategy)?;
            if escaped == Value::from(*expected) {
                Ok(())
            } else {
                Err(format!("expected {expected:?}, got {}", export(&escaped)))
            }
        },
    );
}

#[test]
fn test_custom_escaper() {
    custom_escaper("testCustomEscaper");
}

#[test]
fn test_custom_escaper_without_calling_set_escaper_runtime() {
    custom_escaper("testCustomEscaperWithoutCallingSetEscaperRuntime");
}

#[test]
fn test_custom_escapers_on_multiple_envs() {
    let env1 = Environment::new(loader(&[]));
    env1.escaper_runtime()
        .set_escaper("foo", legacy_escaper(env1.charset(), ""));
    let env2 = Environment::new(loader(&[]));
    env2.escaper_runtime()
        .set_escaper("foo", legacy_escaper(env2.charset(), "**again"));
    assert_eq!(
        escape(&env1, Value::from("foo"), "foo").unwrap(),
        Value::from("foo**ISO-8859-1**UTF-8")
    );
    assert_eq!(
        escape(&env2, Value::from("foo"), "foo").unwrap(),
        Value::from("foo**ISO-8859-1**UTF-8**again")
    );
}

#[test]
fn test_safe_classes_use_custom_runtime_loader() {
    let runtime = Arc::new(EscaperRuntime::default());
    let twig = Environment::new(loader(&[]));
    // new FactoryRuntimeLoader([EscaperRuntime::class => static fn () => $runtime])
    let factory = Arc::clone(&runtime);
    twig.add_runtime_loader(Arc::new(
        FactoryRuntimeLoader::new().factory("Twig\\Runtime\\EscaperRuntime", move || {
            Ok(Arc::clone(&factory))
        }),
    ));
    twig.escaper_runtime()
        .add_safe_class("ThisClassIsSafe", ["html"]);
    assert!(Arc::ptr_eq(&runtime, &twig.escaper_runtime()));
    assert!(
        runtime
            .safe_classes()
            .iter()
            .any(|(class, strategies)| class == "ThisClassIsSafe"
                && strategies == &[PhpString::from("html")]),
        "{:?}",
        runtime.safe_classes()
    );
}
