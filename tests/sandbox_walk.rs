//! The array walk of the sandbox (Twig 3.31.1, TWIG/src/Sandbox/SecurityChecker.php): `ensureToStringAllowed()` hands
//! an array to `ensureToStringAllowedForArray()` before it checks `isSandboxed()`, and `ensureSpreadAllowed()` walks
//! its materialised operand either way, so with the SandboxExtension registered every element of the arrays a template
//! converts to strings is converted to bool (`if (!$v)`) even when nothing is sandboxed. PHP 8.5 warns `unexpected NAN
//! value was coerced to bool` for a NAN element; everything else of the walk (the policy, `Traversable`s) needs the
//! sandbox.
//!
//! Every expected output, diagnostic and error was checked with the oracle (`twig-oracle:3.31.1`, plain environments
//! with the SandboxExtension, the data built as here; the source policy counts with a PHP class counting its
//! `enableSandbox()` calls). The node kinds whose children the sandbox visitor wraps are covered by the vectors of
//! `tests/vectors/fuzz/expressions/round5-sandbox-nan-bool.json` (P43 of round 5).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use twig::deprecation::Deprecation;
use twig::escaper::DefaultStrategy;
use twig::loader::ArrayLoader;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, SandboxExtension, SecurityError,
    SecurityErrorKind, SecurityPolicy, SourcePolicyInterface,
};
use twig::value::object::{ArrayIterator, ArrayObject, ObjectBuilder};
use twig::value::{Array, Value};
use twig::{ErrorKind, Source};

const NAN_TO_BOOL: &str = "Warning: unexpected NAN value was coerced to bool";

/// How the SandboxExtension of [`env`] sandboxes templates.
enum Mode {
    /// Registered, nothing sandboxed (the policy allows the `length` filter, for `{% sandbox %}`).
    Off,
    /// Every template sandboxed; the policy allows these filters and functions.
    Global(&'static [&'static str], &'static [&'static str]),
}

fn env(templates: &[(&str, &str)], mode: Mode) -> Environment {
    let env = Environment::with_options(
        ArrayLoader::new(
            templates
                .iter()
                .map(|(name, source)| (name.to_string(), source.to_string())),
        ),
        EnvironmentOptions {
            autoescape: DefaultStrategy::Disabled,
            ..Default::default()
        },
    );
    let policy = SecurityPolicy::new();
    let global = match mode {
        Mode::Off => {
            policy.set_allowed_filters(["length"]);
            false
        }
        Mode::Global(filters, functions) => {
            policy.set_allowed_filters(filters.iter().copied());
            policy.set_allowed_functions(functions.iter().copied());
            true
        }
    };
    env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), global)))
        .unwrap();
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    env
}

fn list(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(Array::from_values(values))
}

fn iterator(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Object(ArrayIterator::new(Array::from_values(values)).into_ref())
}

fn aggregate(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Object(ArrayObject::new(Array::from_values(values)).into_ref())
}

/// `new \Twig\Tests\ToStringStub('a')` (TWIG/tests/IntegrationTest.php).
fn stringable() -> Value {
    Value::Object(
        ObjectBuilder::new("Twig\\Tests\\ToStringStub")
            .stringable(|_, _| Ok("a"))
            .build(),
    )
}

fn nan() -> Value {
    Value::Float(f64::NAN)
}

/// Renders `index.twig` with `x`: the output or the error message, and the notices.
fn render(env: &Environment, x: Value) -> (String, Vec<String>) {
    let mut context = Array::new();
    context.insert("x", x);
    let result = match env.render("index.twig", context) {
        Ok(output) => output,
        Err(error) => format!("error: {error}"),
    };
    let notices = env.take_notices().iter().map(ToString::to_string).collect();
    (result, notices)
}

fn warnings(n: usize) -> Vec<String> {
    vec![NAN_TO_BOOL.to_owned(); n]
}

#[test]
fn arrays_are_walked_outside_the_sandbox() {
    let length = env(&[("index.twig", "{{ x|length }}")], Mode::Off);
    assert_eq!(render(&length, list([nan()])), ("1".into(), warnings(1)));
    // one warning per NAN at any depth; falsy elements (`0`, `[]`) are skipped
    let nested = {
        let mut c = Array::new();
        c.insert("c", list([nan(), list([nan()])]));
        let mut b = Array::new();
        b.insert("b", Value::Array(c));
        let mut x = Array::new();
        x.insert("a", Value::Array(b));
        for value in [Value::Int(0), nan(), list([])] {
            x.push(value).unwrap();
        }
        Value::Array(x)
    };
    assert_eq!(render(&length, nested), ("4".into(), warnings(3)));
    // both operands of `==` are wrapped (and the same array is equal to itself)
    let equal = env(&[("index.twig", "{{ x == x ? 'y' : 'n' }}")], Mode::Off);
    assert_eq!(render(&equal, list([nan()])), ("y".into(), warnings(2)));
}

#[test]
fn warnings_of_the_walk_become_exceptions() {
    let env = env(&[("index.twig", "{{ x|length }}")], Mode::Off);
    env.set_diagnostics_policy(DiagnosticsPolicy::Error);
    let (result, notices) = render(&env, list([Value::Int(1), nan()]));
    assert_eq!(
        result,
        "error: An exception has been thrown during the rendering of a template (\"unexpected NAN value was coerced to bool\") in \"index.twig\" at line 1."
    );
    assert_eq!(notices, warnings(1));
}

#[test]
fn traversables_are_walked_only_in_the_sandbox() {
    let off = env(&[("index.twig", "{{ x|length }}")], Mode::Off);
    assert_eq!(render(&off, iterator([nan()])), ("1".into(), vec![]));
    assert_eq!(
        render(&off, aggregate([list([nan()])])),
        ("1".into(), vec![])
    );
    assert_eq!(
        render(&off, list([iterator([nan()]), nan()])),
        ("2".into(), warnings(1))
    );

    let sandboxed = env(
        &[("index.twig", "{{ x|length }}")],
        Mode::Global(&["length"], &[]),
    );
    // an iterator is materialised and walked like an array
    assert_eq!(
        render(&sandboxed, iterator([nan()])),
        ("1".into(), warnings(1))
    );
    assert_eq!(
        render(&sandboxed, list([iterator([nan()])])),
        ("1".into(), warnings(1))
    );
    // `foreach` over an IteratorAggregate hands each element on without a bool conversion; nested arrays are walked
    assert_eq!(render(&sandboxed, aggregate([nan()])), ("1".into(), vec![]));
    assert_eq!(
        render(&sandboxed, aggregate([list([nan()])])),
        ("1".into(), warnings(1))
    );
}

#[test]
fn the_policy_applies_only_in_the_sandbox() {
    let off = env(&[("index.twig", "{{ x|length }}")], Mode::Off);
    assert_eq!(
        render(&off, list([stringable(), nan()])),
        ("2".into(), warnings(1))
    );

    let sandboxed = env(
        &[("index.twig", "{{ x|length }}")],
        Mode::Global(&["length"], &[]),
    );
    let mut context = Array::new();
    context.insert("x", list([nan(), stringable()]));
    let error = sandboxed.render("index.twig", context).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SecurityNotAllowedMethod);
    assert_eq!(
        SecurityError::of(&error).map(SecurityError::kind),
        Some(&SecurityErrorKind::Method {
            class: "Twig\\Tests\\ToStringStub".to_owned(),
            method: "__tostring".to_owned(),
        })
    );
    assert_eq!(
        error.to_string(),
        "Calling \"__tostring\" method on a \"Twig\\Tests\\ToStringStub\" object is not allowed in \"index.twig\" at line 1."
    );
    let notices: Vec<String> = sandboxed
        .take_notices()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(notices, warnings(1));
}

#[test]
fn spread_operands_are_walked_outside_the_sandbox() {
    let x = || iterator([Value::Int(1), nan()]);
    let off = env(&[("index.twig", "{{ max(...x) }}")], Mode::Off);
    assert_eq!(render(&off, x()), ("1".into(), warnings(1)));
    let sandboxed = env(
        &[("index.twig", "{{ max(...x) }}")],
        Mode::Global(&[], &["max"]),
    );
    assert_eq!(render(&sandboxed, x()), ("1".into(), warnings(1)));
}

#[test]
fn the_walk_runs_on_both_sides_of_a_sandbox_tag() {
    let env = env(
        &[
            (
                "index.twig",
                "{{ x|length }}{% sandbox %}{% include 'inner.twig' %}{% endsandbox %}",
            ),
            ("inner.twig", "{{ x|length }}"),
        ],
        Mode::Off,
    );
    let mut expected = vec![
        "Deprecated: Since twig/twig 3.15: The \"sandbox\" tag is deprecated in \"index.twig\" at line 1."
            .to_owned(),
    ];
    expected.extend(warnings(2));
    assert_eq!(render(&env, list([nan()])), ("11".into(), expected));
}

/// Counts the calls of `enableSandbox()`, never sandboxing.
#[derive(Default)]
struct CountingSourcePolicy(AtomicUsize);

impl SourcePolicyInterface for CountingSourcePolicy {
    fn enable_sandbox(&self, _source: &Source) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst);
        false
    }
}

/// `doEnsureToStringAllowed()` checks `isSandboxed($source)` for the value it is given unless it is an array, so the
/// walk asks a source policy once per truthy element that is not an array, at any depth (an object too), and once for
/// a value that is no array; the counts include the `checkSecurity()` of the template and the print of the filter's or
/// function's result.
#[test]
fn the_source_policy_is_asked_for_every_truthy_element() {
    let policy = Arc::new(CountingSourcePolicy::default());
    let env = Environment::with_options(
        ArrayLoader::new([
            ("print", "{{ x }}"),
            ("length", "{{ x|length }}"),
            ("spread", "{{ max(0, 0, ...x) }}"),
        ]),
        EnvironmentOptions {
            autoescape: DefaultStrategy::Disabled,
            ..Default::default()
        },
    );
    let mut deprecations: Vec<Deprecation> = Vec::new();
    env.add_extension(Arc::new(SandboxExtension::with_source_policy(
        Arc::new(SecurityPolicy::new()),
        false,
        policy.clone(),
        &mut deprecations,
    )))
    .unwrap();
    let values = || {
        [
            Some(list([])),
            Some(list([
                Value::Int(1),
                Value::Int(0),
                Value::from("a"),
                list([Value::Int(2), list([Value::Int(3)])]),
                Value::Null,
                list([]),
                aggregate([Value::Int(1), Value::Int(2)]),
            ])),
            Some(Value::from("abc")),
            Some(iterator([Value::Int(1), Value::Int(2)])),
        ]
    };
    let mut counts = Vec::new();
    for (name, skip) in [("print", None), ("length", None), ("spread", Some(2))] {
        // compiled and loaded first
        let mut context = Array::new();
        context.insert("x", list([]));
        let _ = env.render(name, context);
        for (i, x) in values().into_iter().enumerate() {
            let Some(x) = x.filter(|_| skip != Some(i)) else {
                continue;
            };
            let before = policy.0.load(Ordering::SeqCst);
            let mut context = Array::new();
            context.insert("x", x);
            let _ = env.render(name, context);
            counts.push((
                format!("{name}/{i}"),
                policy.0.load(Ordering::SeqCst) - before,
            ));
        }
    }
    let expected = [
        ("print/0", 1),
        ("print/1", 6),
        ("print/2", 2),
        ("print/3", 2),
        ("length/0", 2),
        ("length/1", 7),
        ("length/2", 3),
        ("length/3", 3),
        ("spread/0", 2),
        ("spread/1", 7),
        ("spread/3", 4),
    ];
    assert_eq!(
        counts,
        expected.map(|(case, n)| (case.to_owned(), n)).to_vec()
    );
}
