//! Helpers shared by the ports: environments over array loaders, contexts, a `capture()` function that records the
//! values a template passes to it, message helpers and deprecation collection.

use std::sync::{Arc, Mutex};

use twig::escaper::DefaultStrategy;
use twig::extension::runtime::CallableRuntime;
use twig::extension::{CallableParameter, FunctionOptions, PhpCallable, TwigFunction};
use twig::loader::ArrayLoader;
use twig::runtime::{DiagnosticsPolicy, Environment, EnvironmentOptions, Notice};
use twig::value::{Array, ArrayKey, Level, Value};
use twig::{Error, ErrorKind};

/// An array loader over `(name, source)` pairs.
pub fn loader(templates: &[(&str, &str)]) -> ArrayLoader {
    ArrayLoader::new(
        templates
            .iter()
            .map(|(n, c)| (n.to_string(), c.to_string())),
    )
}

/// `new Environment(new ArrayLoader($templates))` with Twig's defaults.
pub fn env(templates: &[(&str, &str)]) -> Environment {
    Environment::new(loader(templates))
}

/// `new Environment(new ArrayLoader($templates), $options)`.
pub fn env_with(templates: &[(&str, &str)], options: EnvironmentOptions) -> Environment {
    Environment::with_options(loader(templates), options)
}

/// `['strict_variables' => $strict]`.
pub fn strict(strict: bool) -> EnvironmentOptions {
    EnvironmentOptions {
        strict_variables: strict,
        ..Default::default()
    }
}

/// `['strict_variables' => $strict, 'autoescape' => false]`.
pub fn strict_raw(strict: bool) -> EnvironmentOptions {
    EnvironmentOptions {
        strict_variables: strict,
        autoescape: DefaultStrategy::Disabled,
        ..Default::default()
    }
}

/// `['debug' => true, 'cache' => false]` plus `strict_variables`.
pub fn debug(strict: bool) -> EnvironmentOptions {
    EnvironmentOptions {
        debug: true,
        strict_variables: strict,
        ..Default::default()
    }
}

/// A context from `(name, value)` pairs.
pub fn ctx(pairs: &[(&str, Value)]) -> Array {
    let mut array = Array::new();
    for (key, value) in pairs {
        array.insert(*key, value.clone());
    }
    array
}

/// The values a template passed to `capture()`, in call order.
#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<Vec<Value>>>);

impl Captured {
    /// The values captured since the last call, emptying the list.
    pub fn take(&self) -> Vec<Value> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    /// The single value captured since the last call.
    pub fn one(&self) -> Value {
        let mut values = self.take();
        assert_eq!(
            values.len(),
            1,
            "capture() was called {} times",
            values.len()
        );
        values.remove(0)
    }
}

/// Registers `capture(value)`: records the value, returns `''`.
pub fn add_capture(env: &Environment) -> Captured {
    let captured = Captured::default();
    let slot = captured.clone();
    let callable = PhpCallable::closure(vec![CallableParameter::required("value")]);
    CallableRuntime::new(move |call| {
        slot.0
            .lock()
            .unwrap()
            .push(call.arg(0).cloned().unwrap_or_default());
        Ok(Value::from(""))
    })
    .attach(&callable);
    env.add_function(TwigFunction::new(
        "capture",
        Some(callable),
        FunctionOptions::default(),
    ))
    .unwrap();
    captured
}

/// `var_export($value, true)`.
pub fn var_export(value: &Value) -> String {
    twig::value::dump::var_export(value, &mut twig::value::Ignore)
        .expect("var_export")
        .to_string_lossy()
        .into_owned()
}

/// `get_class($e).': '.$e->getMessage()`.
pub fn describe(error: &Error) -> String {
    format!("{}: {error}", error.class_name())
}

/// The class and message of the previous exception of an error (`$e->getPrevious()`), as `Class: message`.
pub fn previous(error: &Error) -> Option<String> {
    let previous = error.previous()?;
    if let Some(php) = previous.downcast_ref::<twig::value::PhpError>() {
        return Some(format!("{}: {}", php.class().name(), php.message_lossy()));
    }
    if let Some(twig) = previous.downcast_ref::<Error>() {
        return Some(describe(twig));
    }
    Some(previous.to_string())
}

/// Runs `f` with the deprecations of `env` collected (`set_error_handler()` keeping `E_USER_DEPRECATED`, as the
/// `collectDeprecations()` helpers of the PHPUnit tests do), and returns them as `trigger_deprecation()` messages.
pub fn collect_deprecations<R>(env: &Environment, f: impl FnOnce() -> R) -> (R, Vec<String>) {
    let policy = env.diagnostics_policy();
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    let _ = env.take_notices();
    let result = f();
    let deprecations = env
        .take_notices()
        .into_iter()
        .filter_map(|notice| match notice {
            Notice::Deprecation(deprecation) => Some(deprecation.to_string()),
            Notice::Php(_) => None,
        })
        .collect();
    env.set_diagnostics_policy(policy);
    (result, deprecations)
}

/// Asserts that a result is an error of the given kind and returns it.
#[track_caller]
pub fn expect_error<T: std::fmt::Debug>(result: twig::Result<T>, kind: ErrorKind) -> Error {
    match result {
        Ok(value) => panic!("expected a {kind:?} error, got {value:?}"),
        Err(error) => {
            assert_eq!(error.kind(), kind, "unexpected error: {}", describe(&error));
            error
        }
    }
}

/// Collects the failures of a data provider loop, so one run reports every failing data set.
#[derive(Default)]
pub struct Failures(Vec<String>);

impl Failures {
    /// Records a failure when `actual != expected`.
    pub fn check<T: PartialEq + std::fmt::Debug>(&mut self, label: &str, actual: T, expected: T) {
        if actual != expected {
            self.0.push(format!(
                "{label}\n    actual:   {actual:?}\n    expected: {expected:?}"
            ));
        }
    }

    /// Records a failure message.
    pub fn fail(&mut self, message: String) {
        self.0.push(message);
    }

    /// Panics with every recorded failure.
    #[track_caller]
    pub fn assert_none(self) {
        assert!(
            self.0.is_empty(),
            "{} failing data set(s):\n{}",
            self.0.len(),
            self.0.join("\n")
        );
    }
}

/// The `describe()` of the sweep generators: `var_export()` for scalars, `object(Class)` for objects, arrays
/// recursively.
pub fn describe_value(value: &Value) -> String {
    match value {
        Value::Object(object) => format!("object({})", object.class_name()),
        Value::Array(array) => {
            let parts: Vec<String> = array
                .iter()
                .map(|(key, item)| {
                    let key = match key {
                        ArrayKey::Int(i) => Value::Int(*i),
                        ArrayKey::Str(s) => Value::String(s.clone()),
                    };
                    format!("{}=>{}", var_export(&key), describe_value(item))
                })
                .collect();
            format!("array({})", parts.join(","))
        }
        other => var_export(other),
    }
}

/// The diagnostics recorded by `env` (policy `Collect`) as the sweep generators print them: `Warning: ...`,
/// `Deprecated: ...`, `UserDeprecated: ...`.
pub fn notices(env: &Environment) -> Vec<String> {
    env.take_notices()
        .into_iter()
        .map(|notice| match notice {
            Notice::Php(diagnostic) => match diagnostic.level {
                Level::UserDeprecated => format!("UserDeprecated: {}", diagnostic.message),
                _ => diagnostic.to_string(),
            },
            Notice::Deprecation(deprecation) => format!("UserDeprecated: {deprecation}"),
        })
        .collect()
}

/// Removes the locations PHP appends to userland `TypeError`/`ArgumentCountError` messages (`, called in <file> on line
/// N`, `passed in <file> on line N`), which twig-rs leaves out (reference/deviations.md, "Userland TypeError suffix").
pub fn strip_php_location(text: &str) -> String {
    let mut text = text.to_owned();
    for (marker, replacement) in [(", called in /", ""), (" passed in /", " passed")] {
        while let Some(start) = text.find(marker) {
            let Some(line) = text[start..].find(" on line ") else {
                break;
            };
            let digits = start + line + " on line ".len();
            let end = digits
                + text[digits..]
                    .bytes()
                    .take_while(u8::is_ascii_digit)
                    .count();
            text.replace_range(start..end, replacement);
        }
    }
    text
}
