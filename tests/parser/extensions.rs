//! The registry lifecycle of `Twig\ExtensionSet` (TWIG/src/ExtensionSet.php) and `Twig\Extension\StagingExtension`:
//! registration errors, lazy initialization, replacement of same-name callables, lookup order.

use std::any::Any;
use std::sync::Arc;

use twig::ErrorKind;
use twig::extension::{
    CoreExtension, Extension, ExtensionSet, FilterOptions, FunctionOptions, PhpCallable,
    TestOptions, TwigFilter, TwigFunction, TwigTest,
};

struct Named(&'static str, Vec<TwigFilter>);

impl Extension for Named {
    fn class_name(&self) -> &str {
        self.0
    }

    fn filters(&self) -> Vec<TwigFilter> {
        self.1.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn filter(name: &str) -> TwigFilter {
    TwigFilter::new(
        name,
        Some(PhpCallable::closure(Vec::new())),
        FilterOptions::default(),
    )
}

#[test]
fn extensions_are_keyed_by_class() {
    let mut set = ExtensionSet::with_core_extensions();
    let error = set
        .add_extension(Arc::new(CoreExtension::new()))
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "Unable to register extension \"Twig\\Extension\\CoreExtension\" as it is already registered."
    );
    assert!(set.has_extension("\\Twig\\Extension\\CoreExtension"));
    assert!(set.extension::<CoreExtension>().is_some());
    let error = set.get_extension("Acme\\Nope").err().unwrap();
    assert_eq!(error.kind(), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "The \"Acme\\Nope\" extension is not enabled."
    );
}

#[test]
fn nothing_can_be_added_after_initialization() {
    let mut set = ExtensionSet::with_core_extensions();
    assert!(!set.is_initialized());
    set.get_filter(b"upper").unwrap();
    assert!(set.is_initialized());
    let messages = [
        set.add_extension(Arc::new(Named("Acme\\Ext", Vec::new())))
            .unwrap_err()
            .to_string(),
        set.add_filter(filter("f")).unwrap_err().to_string(),
        set.add_function(TwigFunction::new("f", None, FunctionOptions::default()))
            .unwrap_err()
            .to_string(),
        set.add_test(TwigTest::new("t", None, TestOptions::default()))
            .unwrap_err()
            .to_string(),
    ];
    assert_eq!(
        messages,
        [
            "Unable to register extension \"Acme\\Ext\" as extensions have already been initialized.",
            "Unable to add filter \"f\" as extensions have already been initialized.",
            "Unable to add function \"f\" as extensions have already been initialized.",
            "Unable to add test \"t\" as extensions have already been initialized.",
        ]
    );
}

#[test]
fn the_staging_area_rejects_duplicates() {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_filter(filter("f")).unwrap();
    assert_eq!(
        set.add_filter(filter("f")).unwrap_err().to_string(),
        "Filter \"f\" is already registered."
    );
    set.add_test(TwigTest::new("t", None, TestOptions::default()))
        .unwrap();
    assert_eq!(
        set.add_test(TwigTest::new("t", None, TestOptions::default()))
            .unwrap_err()
            .to_string(),
        "Test \"t\" is already registered."
    );
}

#[test]
fn later_definitions_replace_earlier_ones_in_place() {
    let mut set = ExtensionSet::new();
    set.add_extension(Arc::new(Named(
        "A",
        vec![filter("a"), filter("b"), filter("c")],
    )))
    .unwrap();
    let replacement = filter("a");
    set.add_extension(Arc::new(Named("B", vec![filter("d"), replacement.clone()])))
        .unwrap();
    // staging comes last
    set.add_filter(filter("b")).unwrap();
    let names: Vec<&[u8]> = set.filters().map(|(name, _)| name).collect();
    assert_eq!(names, [b"a".as_slice(), b"b", b"c", b"d"]);
    assert!(set.get_filter(b"a").unwrap().unwrap().ptr_eq(&replacement));
}

#[test]
fn lookups_try_exact_names_then_patterns_then_callbacks() {
    let mut set = ExtensionSet::new();
    set.add_extension(Arc::new(Named(
        "A",
        vec![filter("*_path"), filter("x_path"), filter("*")],
    )))
    .unwrap();
    set.register_undefined_filter_callback(Arc::new(|name: &[u8]| {
        Ok((name == b"from_callback").then(|| filter("from_callback")))
    }));
    assert_eq!(
        set.get_filter(b"x_path").unwrap().unwrap().dynamic_name(),
        b"x_path"
    );
    let dynamic = set.get_filter(b"y_path").unwrap().unwrap();
    assert_eq!(dynamic.name(), b"y_path");
    assert_eq!(dynamic.dynamic_name(), b"*_path");
    assert_eq!(dynamic.arguments(), [b"y".to_vec()]);
    // `*` matches anything, so the callback is never reached for names matching a pattern
    assert_eq!(
        set.get_filter(b"from_callback")
            .unwrap()
            .unwrap()
            .dynamic_name(),
        b"*"
    );

    let mut set = ExtensionSet::new();
    set.register_undefined_function_callback(Arc::new(|name: &[u8]| {
        Ok((name == b"cb").then(|| TwigFunction::new("cb", None, FunctionOptions::default())))
    }));
    assert!(set.get_function(b"cb").unwrap().is_some());
    assert!(set.get_function(b"other").unwrap().is_none());
}

/// Callbacks run in registration order; the first one that finds something wins, and an error (Twig's exception)
/// stops the lookup.
#[test]
fn undefined_callbacks_can_fail() {
    let mut set = ExtensionSet::new();
    set.register_undefined_test_callback(Arc::new(|name: &[u8]| {
        if name == b"boom" {
            return Err(twig::Error::syntax("This test is undefined in the tests."));
        }
        Ok(None)
    }));
    set.register_undefined_test_callback(Arc::new(|name: &[u8]| {
        Ok(Some(TwigTest::new(
            name.to_vec(),
            None,
            TestOptions::default(),
        )))
    }));
    assert_eq!(
        set.get_test(b"boom").unwrap_err().to_string(),
        "This test is undefined in the tests."
    );
    assert_eq!(set.get_test(b"other").unwrap().unwrap().name(), b"other");
}

#[test]
fn the_lexer_uses_the_registered_operators() {
    let set = ExtensionSet::with_core_extensions();
    let lexer = set.lexer(Default::default());
    let mut deprecations = Vec::new();
    let stream = lexer
        .tokenize(
            &twig::Source::new("{{ a starts with b }}", "index"),
            &mut deprecations,
        )
        .unwrap();
    assert!(
        stream
            .tokens()
            .iter()
            .any(|t| t.value().as_bytes() == Some(b"starts with"))
    );
    // without the core extension there are no operators at all
    let lexer = ExtensionSet::new().lexer(Default::default());
    let stream = lexer
        .tokenize(
            &twig::Source::new("{{ a starts with b }}", "index"),
            &mut deprecations,
        )
        .unwrap();
    assert!(
        !stream
            .tokens()
            .iter()
            .any(|t| t.value().as_bytes() == Some(b"starts with"))
    );
}
