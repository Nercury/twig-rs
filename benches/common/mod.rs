//! The benchmark scenarios of `benches/scenarios/`: one directory per scenario with its templates (`*.twig`, rendered
//! from `index.twig`) and its context (`data.json`). `research/benchmarks/bench.php` runs the same scenarios on PHP
//! Twig.

use std::fs;
use std::path::{Path, PathBuf};

use twig::loader::ArrayLoader;
use twig::runtime::Environment;
use twig::value::{Array, Value};

/// A scenario: templates and context.
pub struct Scenario {
    /// The directory name.
    pub name: String,
    /// `(name, source)` of every template, sorted by name.
    pub templates: Vec<(String, String)>,
    /// The context, decoded like PHP's `json_decode($json, true)`.
    #[allow(dead_code, reason = "the compile benchmarks do not render")]
    pub context: Array,
}

impl Scenario {
    /// An environment with Twig's default options and the scenario's templates in an `ArrayLoader`; the date filters
    /// use UTC (`CoreExtension::setTimezone('UTC')`).
    pub fn environment(&self) -> Environment {
        let templates = self
            .templates
            .iter()
            .map(|(name, code)| (name.clone(), code.clone()));
        let env = Environment::new(ArrayLoader::new(templates));
        let mut diagnostics = Vec::new();
        env.core_settings()
            .set_timezone(env.date_context(), &Value::from("UTC"), &mut diagnostics)
            .expect("UTC is a timezone");
        env
    }
}

/// Every scenario, sorted by name.
pub fn scenarios() -> Vec<Scenario> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/scenarios");
    let mut dirs: Vec<PathBuf> = fs::read_dir(&root)
        .expect("benches/scenarios")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs.iter().map(|dir| load(dir)).collect()
}

fn load(dir: &Path) -> Scenario {
    let mut templates = Vec::new();
    let mut context = None;
    for entry in fs::read_dir(dir).expect("a scenario directory") {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a file name")
            .to_owned();
        let text = fs::read_to_string(&path).expect("a UTF-8 scenario file");
        if name == "data.json" {
            let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
            match to_value(&json) {
                Value::Array(array) => context = Some(array),
                _ => panic!("{}: the context must be a JSON object", path.display()),
            }
        } else if name.ends_with(".twig") {
            templates.push((name, text));
        }
    }
    templates.sort();
    Scenario {
        name: dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a directory name")
            .to_owned(),
        templates,
        context: context.unwrap_or_default(),
    }
}

/// PHP's `json_decode($json, true)`: objects become arrays with normalised keys (`"1"` becomes `1`), integers that
/// fit an `int` stay integers, every other number is a float.
fn to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::from(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::from(i),
            None => Value::from(n.as_f64().expect("a JSON number")),
        },
        serde_json::Value::String(s) => Value::from(s.as_str()),
        serde_json::Value::Array(items) => {
            Value::Array(Array::from_values(items.iter().map(to_value)))
        }
        serde_json::Value::Object(map) => {
            let mut array = Array::new();
            for (key, value) in map {
                array.insert(key.as_str(), to_value(value));
            }
            Value::Array(array)
        }
    }
}
