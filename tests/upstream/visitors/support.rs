//! Shared helpers: environments built like the upstream tests build them, PHP values, and the oracle vectors.

#![allow(dead_code)]

use std::collections::HashMap;

use twig::Error;
use twig::escaper::DefaultStrategy;
use twig::loader::ArrayLoader;
use twig::runtime::{DiagnosticsPolicy, Environment, EnvironmentOptions, Notice};
use twig::value::object::StdClass;
use twig::value::{Array, Value};

use crate::common::Json;

/// `new Environment(new ArrayLoader($templates), $options)`.
pub fn env(templates: &[(&str, &str)], options: EnvironmentOptions) -> Environment {
    Environment::with_options(
        ArrayLoader::new(
            templates
                .iter()
                .map(|(name, code)| (name.to_string(), code.to_string())),
        ),
        options,
    )
}

/// Twig's default options (`new Environment($loader)`).
pub fn defaults() -> EnvironmentOptions {
    EnvironmentOptions::default()
}

/// `['cache' => false, 'autoescape' => false]`.
pub fn no_escape() -> EnvironmentOptions {
    EnvironmentOptions {
        autoescape: DefaultStrategy::Disabled,
        ..Default::default()
    }
}

/// `['strict_variables' => true]`.
pub fn strict() -> EnvironmentOptions {
    EnvironmentOptions {
        strict_variables: true,
        ..Default::default()
    }
}

/// A context array.
pub fn ctx(pairs: Vec<(&str, Value)>) -> Array {
    let mut array = Array::new();
    for (key, value) in pairs {
        array.insert(key, value);
    }
    array
}

/// `(object) ['name' => value, ...]`.
pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(StdClass::with_properties(pairs).into_ref())
}

/// A PHP string.
pub fn s(value: &str) -> Value {
    Value::from(value)
}

/// The results of an oracle batch (`tests/vectors/upstream/visitors/<topic>.json`) by case id.
pub struct Oracle {
    topic: &'static str,
    results: HashMap<String, Json>,
}

impl Oracle {
    pub fn load(topic: &'static str) -> Oracle {
        let results = Json::load_vectors(&format!("upstream/visitors/{topic}.json"));
        let results = results
            .as_array()
            .iter()
            .map(|result| {
                let id = result.field("id").as_str().expect("a case id").to_owned();
                (id, result.clone())
            })
            .collect();
        Oracle { topic, results }
    }

    pub fn result(&self, id: &str) -> &Json {
        self.results
            .get(id)
            .unwrap_or_else(|| panic!("no case {id:?} in the {} vectors", self.topic))
    }

    /// What a Twig-kind case produced: its output, or the class and message of its exception.
    pub fn outcome(&self, id: &str) -> Outcome {
        let result = self.result(id);
        if matches!(result.get("ok"), Some(Json::Bool(true))) {
            Outcome::Output(result.field("output").as_lossy_string())
        } else {
            let error = result.field("error");
            Outcome::Error {
                class: error.field("class").as_str().unwrap().to_owned(),
                message: error.field("message").as_lossy_string(),
            }
        }
    }

    /// The JSON-encoded value a PHP-kind case returned.
    pub fn value(&self, id: &str) -> Json {
        let result = self.result(id);
        assert!(
            matches!(result.get("ok"), Some(Json::Bool(true))),
            "the oracle case {id:?} failed: {result:?}"
        );
        Json::parse(result.field("json").as_str().expect("a JSON string")).unwrap()
    }
}

/// A render with what it reported: the outcome, the Twig deprecations and the PHP warnings and notices (messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub outcome: Outcome,
    pub deprecations: Vec<String>,
    pub warnings: Vec<String>,
}

/// The options with the diagnostics collected (`DiagnosticsPolicy::Collect`), for [`render_full`].
pub fn collecting(options: EnvironmentOptions) -> EnvironmentOptions {
    EnvironmentOptions {
        diagnostics: DiagnosticsPolicy::Collect,
        ..options
    }
}

/// Renders `name` and collects what the render reported (the environment must use [`collecting`] options).
pub fn render_full(env: &Environment, name: &str, context: Array) -> Rendered {
    let outcome = Outcome::of(env.render(name, context));
    let mut deprecations = Vec::new();
    let mut warnings = Vec::new();
    for notice in env.take_notices() {
        match notice {
            Notice::Deprecation(deprecation) => deprecations.push(deprecation.to_string()),
            Notice::Php(diagnostic) => {
                warnings.push(diagnostic.message.to_string_lossy().into_owned());
            }
        }
    }
    Rendered {
        outcome,
        deprecations,
        warnings,
    }
}

impl Oracle {
    /// What a Twig-kind case produced and reported (PHP warnings and notices without the deprecations, which the
    /// oracle lists as `E_USER_DEPRECATED` diagnostics too).
    pub fn rendered(&self, id: &str) -> Rendered {
        let result = self.result(id);
        let strings = |field: &str| -> Vec<String> {
            result
                .get(field)
                .map(|list| list.as_array().iter().map(Json::as_lossy_string).collect())
                .unwrap_or_default()
        };
        let warnings = result
            .get("diagnostics")
            .map(|list| {
                list.as_array()
                    .iter()
                    .filter(|d| {
                        !matches!(
                            d.field("type").as_str(),
                            Some("E_USER_DEPRECATED" | "E_DEPRECATED")
                        )
                    })
                    .map(|d| d.field("message").as_lossy_string())
                    .collect()
            })
            .unwrap_or_default();
        Rendered {
            outcome: self.outcome(id),
            deprecations: strings("deprecations"),
            warnings,
        }
    }
}

/// The output of a render, or the class and message of the exception it threw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Output(String),
    Error { class: String, message: String },
}

impl Outcome {
    pub fn of(result: Result<String, Error>) -> Outcome {
        match result {
            Ok(output) => Outcome::Output(output),
            Err(error) => Outcome::Error {
                class: error.class_name().to_owned(),
                message: error.to_string(),
            },
        }
    }
}

/// Collects the mismatches of the cases of a test and fails with all of them at the end.
#[derive(Default)]
pub struct Mismatches(Vec<String>);

impl Mismatches {
    pub fn check<T: PartialEq + std::fmt::Debug>(&mut self, case: &str, expected: &T, actual: &T) {
        if expected != actual {
            self.0.push(format!(
                "{case}\n    expected: {expected:?}\n    actual:   {actual:?}"
            ));
        }
    }

    pub fn push(&mut self, message: String) {
        self.0.push(message);
    }

    pub fn assert_none(self) {
        assert!(
            self.0.is_empty(),
            "{} mismatching case(s):\n{}",
            self.0.len(),
            self.0.join("\n")
        );
    }
}

/// A result of the generator's `$try()`: `{"value": ...}` or `{"error": {"class", "message"}}`.
pub fn tried(json: &Json) -> Outcome {
    match json.get("error") {
        Some(error) => Outcome::Error {
            class: error.field("class").as_str().unwrap().to_owned(),
            message: error.field("message").as_lossy_string(),
        },
        None => Outcome::Output(json.field("value").as_lossy_string()),
    }
}

/// `new SecurityPolicy(allowedTags: $tags, allowedFilters: $filters, allowedFunctions: $functions)`, strict or not.
pub fn policy(
    tags: &[&str],
    filters: &[&str],
    functions: &[&str],
    strict: bool,
) -> std::sync::Arc<twig::runtime::SecurityPolicy> {
    let policy = twig::runtime::SecurityPolicy::new();
    policy.set_allowed_tags(tags.iter().copied());
    policy.set_allowed_filters(filters.iter().copied());
    policy.set_allowed_functions(functions.iter().copied());
    policy.set_strict(strict);
    std::sync::Arc::new(policy)
}
