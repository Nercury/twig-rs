//! Tests of [`EscaperRuntime`]: a port of Twig's `EscaperRuntimeTest` for custom escapers, safe classes and markup
//! (TWIG/tests/Runtime/EscaperRuntimeTest.php; the string tables are in `tests/escaper.rs`), and the oracle batch
//! `tests/vectors/callables/strings/runtime.gen.json`: templates rendered with escapers and safe classes registered
//! on the environment's runtime.
//!
//! The vector cases configure the environment's runtime ([`Environment::escaper_runtime`], Twig's
//! `$twig->getRuntime(EscaperRuntime::class)`) like the case's `setup`, before the templates render.
//!
//! [`Environment::escaper_runtime`]: crate::runtime::Environment::escaper_runtime

use std::sync::{Arc, LazyLock};

use twig_value::object::{
    CallArgs, ClassBuilder, ClassRef, Object, ObjectRef, PropertyStore, undefined_method,
};
use twig_value::{Array, Diagnostic, Diagnostics, PhpResult, PhpString, Value};

use super::super::test_vectors::{markup, run_topic};
use super::{EscaperCallback, EscaperRuntime};

// ---- objects ------------------------------------------------------------------------------------------------------

/// A class with a constant `__toString()`.
struct Stringable {
    class: ClassRef,
    properties: PropertyStore,
    string: &'static str,
}

impl Stringable {
    fn value(class: &ClassRef, string: &'static str) -> Value {
        Value::Object(ObjectRef::new(Stringable {
            class: class.clone(),
            properties: PropertyStore::new(class),
            string,
        }))
    }
}

impl Object for Stringable {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__tostring" => Ok(Value::from(self.string)),
            _ => Err(undefined_method(&self.class, method)),
        }
    }
}

/// `interface Extension_SafeHtmlInterface {}`
static SAFE_HTML_INTERFACE: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::interface("Twig\\Tests\\Runtime\\Extension_SafeHtmlInterface").build()
});

/// `class Extension_TestClass implements Extension_SafeHtmlInterface { public function __toString() { return '<br
/// />'; } }`
static TEST_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\Runtime\\Extension_TestClass")
        .implements(&SAFE_HTML_INTERFACE)
        .method("__toString")
        .build()
});

fn test_object() -> Value {
    Stringable::value(&TEST_CLASS, "<br />")
}

/// `$string.'**'.$charset` (`Twig\Tests\Runtime\escaper()`, the `star` escaper of the vectors).
fn star_escaper() -> EscaperCallback {
    Arc::new(|value, charset, diag| {
        let string = twig_value::ops::concat(value, &Value::from("**"), diag)?;
        Ok(Value::String(twig_value::ops::concat(
            &Value::String(string),
            &Value::String(charset.clone()),
            diag,
        )?))
    })
}

fn escape(
    runtime: &EscaperRuntime,
    value: Value,
    strategy: &str,
    charset: Option<&str>,
    autoescape: bool,
) -> Value {
    let mut diag: Vec<Diagnostic> = Vec::new();
    runtime
        .escape(
            value,
            strategy.as_bytes(),
            charset.map(str::as_bytes),
            autoescape,
            &mut diag,
        )
        .unwrap_or_else(|error| panic!("escape failed: {}", error.message()))
}

fn string(value: Value) -> String {
    match value {
        Value::String(s) => s.to_string_lossy().into_owned(),
        Value::Object(o) => o
            .to_php_string(&mut Vec::<Diagnostic>::new())
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        other => panic!("not a string: {other:?}"),
    }
}

// ---- EscaperRuntimeTest --------------------------------------------------------------------------------------------

#[test]
fn unknown_custom_escaper() {
    let mut diag: Vec<Diagnostic> = Vec::new();
    let error = EscaperRuntime::default()
        .escape(Value::from("foo"), b"bar", None, false, &mut diag)
        .unwrap_err();
    let error = error.twig_error().expect("a RuntimeError");
    assert_eq!(error.kind(), crate::ErrorKind::Runtime);
    assert_eq!(
        error.raw_message(),
        "Invalid escaping strategy \"bar\" (valid ones: \"html\", \"js\", \"url\", \"css\", \"html_attr\", \"html_attr_relaxed\")."
    );
}

#[test]
fn custom_escaper() {
    for (expected, value, strategy, charset) in [
        (
            "foo**ISO-8859-1",
            Value::from("foo"),
            "foo",
            Some("ISO-8859-1"),
        ),
        ("**ISO-8859-1", Value::Null, "foo", Some("ISO-8859-1")),
        ("42**UTF-8", Value::Int(42), "foo", None),
    ] {
        let runtime = EscaperRuntime::default();
        runtime.set_escaper("foo", star_escaper());
        assert_eq!(
            string(escape(&runtime, value, strategy, charset, false)),
            expected
        );
    }
}

#[test]
fn custom_escapers_are_listed_in_registration_order() {
    let runtime = EscaperRuntime::new("UTF-8");
    runtime.set_escaper("b", star_escaper());
    runtime.set_escaper("a", star_escaper());
    runtime.set_escaper("b", star_escaper());
    let names: Vec<PhpString> = runtime
        .escapers()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, [PhpString::from("b"), PhpString::from("a")]);
    let mut diag: Vec<Diagnostic> = Vec::new();
    let error = runtime
        .escape(Value::from("x"), b"nope", None, false, &mut diag)
        .unwrap_err();
    assert!(error.message().ends_with(
        "(valid ones: \"html\", \"js\", \"url\", \"css\", \"html_attr\", \"html_attr_relaxed\", \"b\", \"a\")."
    ));
    // built-in strategies cannot be replaced
    runtime.set_escaper("html", star_escaper());
    assert_eq!(
        string(escape(&runtime, Value::from("<"), "html", None, false)),
        "&lt;"
    );
}

#[test]
fn object_escaping() {
    for (escaped_html, escaped_js, class, strategies) in [
        (
            "&lt;br /&gt;",
            "<br />",
            "\\Twig\\Tests\\Runtime\\Extension_TestClass",
            "js",
        ),
        (
            "<br />",
            "\\u003Cbr\\u0020\\/\\u003E",
            "\\Twig\\Tests\\Runtime\\Extension_TestClass",
            "html",
        ),
        (
            "&lt;br /&gt;",
            "<br />",
            "\\Twig\\Tests\\Runtime\\Extension_SafeHtmlInterface",
            "js",
        ),
        (
            "<br />",
            "<br />",
            "\\Twig\\Tests\\Runtime\\Extension_SafeHtmlInterface",
            "all",
        ),
    ] {
        let runtime = EscaperRuntime::default();
        runtime.set_safe_classes([(class, [strategies])]);
        assert_eq!(
            string(escape(&runtime, test_object(), "html", None, true)),
            escaped_html
        );
        assert_eq!(
            string(escape(&runtime, test_object(), "js", None, true)),
            escaped_js
        );
    }
}

#[test]
fn safe_classes_are_safe_for_implied_strategies() {
    for (escaped, declared, strategy) in [
        (false, "html_attr", "html"),
        (false, "html_attr", "html_attr_relaxed"),
        (false, "html_attr_relaxed", "html"),
        (true, "html", "html_attr"),
        (true, "html", "html_attr_relaxed"),
        (true, "html_attr_relaxed", "html_attr"),
        (true, "html_attr", "js"),
    ] {
        let runtime = EscaperRuntime::default();
        runtime.add_safe_class("Twig\\Tests\\Runtime\\Extension_TestClass", [declared]);
        let output = string(escape(&runtime, test_object(), strategy, None, true));
        assert_eq!(output != "<br />", escaped, "{declared} / {strategy}");
    }
}

#[test]
fn inherited_safe_classes_are_safe_for_implied_strategies() {
    let runtime = EscaperRuntime::default();
    runtime.add_safe_class(
        "Twig\\Tests\\Runtime\\Extension_SafeHtmlInterface",
        ["html_attr"],
    );
    assert_eq!(
        string(escape(&runtime, test_object(), "html", None, true)),
        "<br />"
    );
    // the class inherited the interface's strategies and is cached
    let classes = runtime.safe_classes();
    assert_eq!(
        classes,
        [
            (
                "Twig\\Tests\\Runtime\\Extension_SafeHtmlInterface".to_owned(),
                vec![PhpString::from("html_attr")]
            ),
            (
                "Twig\\Tests\\Runtime\\Extension_TestClass".to_owned(),
                vec![PhpString::from("html_attr")]
            ),
        ]
    );
}

#[test]
fn a_class_met_before_its_interface_was_declared_safe_stays_unsafe() {
    // `safeClasses[$c]` is computed once per class, the first time one of its objects is auto-escaped
    let runtime = EscaperRuntime::default();
    assert_eq!(
        string(escape(&runtime, test_object(), "html", None, true)),
        "&lt;br /&gt;"
    );
    runtime.add_safe_class(
        "Twig\\Tests\\Runtime\\Extension_SafeHtmlInterface",
        ["html"],
    );
    assert_eq!(
        string(escape(&runtime, test_object(), "html", None, true)),
        "&lt;br /&gt;"
    );
    runtime.add_safe_class("Twig\\Tests\\Runtime\\Extension_TestClass", ["html"]);
    assert_eq!(
        string(escape(&runtime, test_object(), "html", None, true)),
        "<br />"
    );
}

#[test]
fn markup_is_only_safe_for_its_own_strategies() {
    for (expected, safe, strategy) in [
        ("<br />", None, "js"),
        ("<br />", Some(&["all"][..]), "js"),
        ("<br />", Some(&["html"][..]), "html"),
        ("\\u003Cbr\\u0020\\/\\u003E", Some(&["html"][..]), "js"),
        ("<br />", Some(&["html", "js"][..]), "js"),
        ("<br />", Some(&["html_attr"][..]), "html"),
        ("&lt;br /&gt;", Some(&[][..]), "html"),
    ] {
        let runtime = EscaperRuntime::default();
        assert_eq!(
            string(escape(
                &runtime,
                markup("<br />", safe),
                strategy,
                None,
                true
            )),
            expected
        );
    }
}

#[test]
fn markup_strategies_take_precedence_over_safe_classes() {
    let runtime = EscaperRuntime::default();
    runtime.add_safe_class("Twig\\Markup", ["html"]);
    assert_eq!(
        string(escape(
            &runtime,
            markup("<br />", Some(&["js"])),
            "html",
            None,
            true
        )),
        "&lt;br /&gt;"
    );
}

#[test]
fn markup_is_escaped_when_the_developer_calls_the_escape_filter() {
    assert_eq!(
        string(escape(
            &EscaperRuntime::default(),
            markup("<br />", Some(&["html"])),
            "html",
            None,
            false
        )),
        "&lt;br /&gt;"
    );
}

#[test]
fn values_that_are_not_strings() {
    let runtime = EscaperRuntime::new("UTF-8");
    assert_eq!(
        escape(&runtime, Value::Int(5), "html", None, false),
        Value::Int(5)
    );
    assert_eq!(escape(&runtime, Value::Null, "js", None, true), Value::Null);
    let array = Value::Array(Array::from_values([Value::from("<")]));
    assert_eq!(escape(&runtime, array.clone(), "url", None, false), array);
    assert_eq!(
        escape(&runtime, Value::from(""), "nope", None, false),
        Value::from("")
    );
    // a failed iconv() returns false
    let mut diag: Vec<Diagnostic> = Vec::new();
    assert_eq!(
        runtime
            .escape(
                Value::from("<"),
                b"html",
                Some(b"x-unknown"),
                false,
                &mut diag
            )
            .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(diag.len(), 2);
}

// ---- the oracle batch of the runtime topic ------------------------------------------------------------------------

/// The custom escapers of `PHP_ESCAPERS` in strings.gen.py.
fn vector_escaper(name: &str) -> EscaperCallback {
    match name {
        "star" => star_escaper(),
        // fn ($s, $c) => get_debug_type($s)
        "type" => Arc::new(|value, _charset, _diag| {
            Ok(Value::from(
                twig_value::types::get_debug_type(value).into_owned(),
            ))
        }),
        // fn ($s, $c) => [$s]
        "wrap" => {
            Arc::new(|value, _charset, _diag| Ok(Value::Array(Array::from_values([value.clone()]))))
        }
        other => panic!("unknown escaper {other}"),
    }
}

#[test]
fn runtime_vectors() {
    run_topic("runtime", &|env, case| {
        // `$runtime = $twig->getRuntime(EscaperRuntime::class)`
        let runtime = env.escaper_runtime();
        let setup = case.field("setup");
        if let Some(classes) = setup.get("safe_classes") {
            for entry in classes.as_array() {
                let [class, strategies] = entry.as_array() else {
                    panic!("bad safe class entry");
                };
                runtime.add_safe_class(
                    class.as_str().expect("class name"),
                    strategies
                        .as_array()
                        .iter()
                        .map(|s| PhpString::from(s.as_str().expect("strategy"))),
                );
            }
        }
        if let Some(escapers) = setup.get("escapers") {
            for name in escapers.as_array() {
                let name = name.as_str().expect("escaper name");
                runtime.set_escaper(name, vector_escaper(name));
            }
        }
    });
}
