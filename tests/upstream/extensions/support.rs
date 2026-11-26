//! Helpers shared by the ports: environments, contexts, PHP-like values, a `capture()` function that hands template
//! values back to the test, `assertSame()` and data-provider runs.

use std::sync::{Arc, Mutex};

use twig::escaper::DefaultStrategy;
use twig::extension::runtime::{Call, CallResult, CallableRuntime};
use twig::extension::{
    CallableParameter, FilterOptions, FunctionOptions, PhpCallable, TestOptions, TwigFilter,
    TwigFunction, TwigTest,
};
use twig::loader::ArrayLoader;
use twig::runtime::{Environment, EnvironmentOptions, Notice};
use twig::value::{Array, ArrayKey, Ignore, Value};

/// Twig's `new Environment($loader, ['autoescape' => false, ...])` options with `debug` (most ports use them).
pub fn options() -> EnvironmentOptions {
    EnvironmentOptions {
        autoescape: DefaultStrategy::Disabled,
        ..Default::default()
    }
}

/// `new ArrayLoader($templates)`.
pub fn loader(templates: &[(&str, &str)]) -> ArrayLoader {
    ArrayLoader::new(
        templates
            .iter()
            .map(|(name, source)| (name.to_string(), source.to_string())),
    )
}

/// `new Environment(new ArrayLoader($templates), $options)`.
pub fn env_with(templates: &[(&str, &str)], options: EnvironmentOptions) -> Environment {
    Environment::with_options(loader(templates), options)
}

/// A context array.
pub fn ctx(pairs: &[(&str, Value)]) -> Array {
    let mut array = Array::new();
    for (key, value) in pairs {
        array.insert(*key, value.clone());
    }
    array
}

/// A PHP list `[a, b, c]`.
pub fn list<I: IntoIterator<Item = Value>>(values: I) -> Value {
    Value::Array(Array::from_values(values))
}

/// A PHP list of strings.
pub fn strings(values: &[&str]) -> Value {
    list(values.iter().map(|s| Value::from(*s)))
}

/// A PHP list of ints.
pub fn ints<I: IntoIterator<Item = i64>>(values: I) -> Value {
    list(values.into_iter().map(Value::Int))
}

/// An array literal whose elements may have keys (`None` appends like `[$v]`).
pub fn array(entries: Vec<(Option<ArrayKey>, Value)>) -> Array {
    let mut array = Array::new();
    for (key, value) in entries {
        match key {
            Some(key) => {
                array.insert(key, value);
            }
            None => {
                array.push(value).expect("no next-index overflow");
            }
        }
    }
    array
}

/// A string key.
pub fn key(name: &str) -> Option<ArrayKey> {
    Some(ArrayKey::from(name))
}

/// `var_export()` of a value, for failure messages.
pub fn export(value: &Value) -> String {
    twig::value::dump::var_export(value, &mut Ignore)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|e| format!("<var_export failed: {e:?}>"))
}

/// PHPUnit's `assertSame()`: `===`.
pub fn same(expected: &Value, actual: &Value) -> Result<(), String> {
    if twig::value::compare::identical(expected, actual) {
        Ok(())
    } else {
        Err(format!(
            "expected {} but got {}",
            export(expected),
            export(actual)
        ))
    }
}

/// Values handed back by the `capture()` function of a template.
#[derive(Clone, Default)]
pub struct Capture {
    values: Arc<Mutex<Vec<Value>>>,
}

impl Capture {
    pub fn new() -> Self {
        Capture::default()
    }

    /// A `capture(value)` function recording its argument.
    pub fn function(&self) -> TwigFunction {
        let values = Arc::clone(&self.values);
        function(
            "capture",
            vec![CallableParameter::required("value")],
            move |call| {
                values
                    .lock()
                    .unwrap()
                    .push(call.arg(0).cloned().unwrap_or_default());
                Ok(Value::Null)
            },
        )
    }

    /// Registers `capture()` on an environment.
    pub fn register(&self, env: &Environment) {
        env.add_function(self.function())
            .expect("capture() registers");
    }

    pub fn take(&self) -> Vec<Value> {
        std::mem::take(&mut *self.values.lock().unwrap())
    }
}

fn attach(
    parameters: Vec<CallableParameter>,
    run: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static,
) -> PhpCallable {
    let callable = PhpCallable::closure(parameters);
    CallableRuntime::new(run).attach(&callable);
    callable
}

/// A host function (a PHP closure with the given parameters).
pub fn function(
    name: &str,
    parameters: Vec<CallableParameter>,
    run: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static,
) -> TwigFunction {
    function_with(name, parameters, FunctionOptions::default(), run)
}

/// A host function with options.
pub fn function_with(
    name: &str,
    parameters: Vec<CallableParameter>,
    options: FunctionOptions,
    run: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static,
) -> TwigFunction {
    TwigFunction::new(name, Some(attach(parameters, run)), options)
}

/// A host filter (a PHP closure with the given parameters; the first one receives the filtered value).
pub fn filter_with(
    name: &str,
    parameters: Vec<CallableParameter>,
    options: FilterOptions,
    run: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static,
) -> TwigFilter {
    TwigFilter::new(name, Some(attach(parameters, run)), options)
}

/// A host test (a PHP closure with the given parameters; the first one receives the tested value).
pub fn test_with(
    name: &str,
    parameters: Vec<CallableParameter>,
    options: TestOptions,
    run: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static,
) -> TwigTest {
    TwigTest::new(name, Some(attach(parameters, run)), options)
}

/// The deprecations an environment recorded (policy `Collect` or `Error`), as `trigger_deprecation()` prints them.
pub fn deprecations(env: &Environment) -> Vec<String> {
    env.take_notices()
        .into_iter()
        .filter_map(|notice| match notice {
            Notice::Deprecation(deprecation) => Some(deprecation.to_string()),
            Notice::Php(diagnostic) => match diagnostic.level {
                twig::value::Level::Deprecated | twig::value::Level::UserDeprecated => {
                    Some(diagnostic.message.to_string_lossy().into_owned())
                }
                _ => None,
            },
        })
        .collect()
}

/// Symfony's `expectDeprecation()`: the deprecation was triggered.
pub fn expect_deprecation(deprecations: &[String], expected: &str) {
    assert!(
        deprecations.iter().any(|d| d == expected),
        "expected the deprecation\n  {expected}\namong\n  {deprecations:#?}"
    );
}

thread_local! {
    /// The id of the scenario running on this thread (`<upstream test>/<case>`), for oracle vectors keyed by it.
    static SCENARIO: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Sets (or clears) the id of the scenario running on this thread.
pub fn set_scenario(id: Option<String>) {
    SCENARIO.with(|scenario| *scenario.borrow_mut() = id);
}

/// The id of the scenario running on this thread: the one set by [`set_scenario`] or [`check_cases`], else the
/// upstream name of the running test (`testFooBar` for the Rust test `module::test_foo_bar`).
pub fn scenario_id() -> Option<String> {
    if let Some(id) = SCENARIO.with(|scenario| scenario.borrow().clone()) {
        return Some(id);
    }
    upstream_test_name()
}

/// `testFooBar` for the running Rust test `module::test_foo_bar`.
pub fn upstream_test_name() -> Option<String> {
    let thread = std::thread::current();
    let name = thread.name()?.rsplit("::").next()?.to_owned();
    let mut camel = String::new();
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            camel.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            camel.push(c);
        }
    }
    camel.starts_with("test").then_some(camel)
}

/// Runs every case of a data provider and reports all failures at once.
pub fn check_cases<C>(
    test: &str,
    cases: impl IntoIterator<Item = (String, C)>,
    run: impl Fn(&C) -> Result<(), String>,
) {
    let mut failures = Vec::new();
    let mut count = 0;
    for (name, case) in cases {
        count += 1;
        set_scenario(Some(format!("{test}/{name}")));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&case)))
            .unwrap_or_else(|panic| {
                Err(format!(
                    "panicked: {}",
                    panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default()
                ))
            });
        if let Err(message) = result {
            failures.push(format!("{test} [{name}]: {message}"));
        }
    }
    set_scenario(None);
    assert!(count > 0, "{test}: no cases ran");
    assert!(
        failures.is_empty(),
        "{} of {count} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
