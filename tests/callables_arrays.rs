//! The array and iteration callables (`batch`, `column`, `cycle`, `filter`, `find`, `first`, `invoke`, `join`, `keys`,
//! `last`, `length`, `map`, `max`, `merge`, `min`, `random`, `range`, `reduce`, `reverse`, `shuffle`, `slice`, `sort`,
//! `split` and the `empty`, `iterable`, `mapping`, `sequence` tests) against Twig 3.31.1 on PHP 8.5.13: templates
//! rendered by [`twig::runtime::Environment`] compared with what the oracle rendered (output, error class and message
//! with template name and line, PHP diagnostics and deprecations in order).
//!
//! The cases are oracle batches in `tests/vectors/callables/arrays/<topic>.gen.json`, written by
//! `tests/vectors/callables/arrays/arrays.gen.py`; the results are in `<topic>.json`. Every case runs the plain
//! environment (`new Environment(new ArrayLoader($templates), $config)`) with `autoescape` off, optionally with
//! another `charset` (topic `charset`) or a `SandboxExtension` and its policy (topic `sandbox`). Values are printed
//! with `json_encode(1024)`; the harness gives `json_encode` a runtime calling
//! `twig::value::builtins::format::json_encode`, so these vectors do not depend on the string group. Object cases
//! build their values in PHP with the classes at the top of their `data_php`; this file has the same classes in Rust
//! and builds the objects from the case's `objects` specs. The seeded `random` topic is checked by the unit tests of
//! `src/extension/runtime/arrays.rs`.
//!
//! Regenerate the batches and their results from the repository root (Git Bash on Windows needs
//! `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 python -I tests/vectors/callables/arrays/arrays.gen.py --oracle [topic ...]
//! ```

#[path = "common/mod.rs"]
mod common;

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use common::Json;
use twig::ast::ConstantValue;
use twig::escaper::DefaultStrategy;
use twig::extension::runtime::CallableRuntime;
use twig::extension::{CallableParameter, FilterOptions, PhpCallable, TwigFilter};
use twig::loader::ArrayLoader;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, Markup, Notice, SandboxExtension,
    SecurityError, SecurityPolicy,
};
use twig::value::object::{
    ArrayIterator, ArrayObject, CallArgs, ClassBuilder, ClassRef, FunctionKind, Object,
    ObjectIterator, ObjectRef, Param, PropertyInfo, PropertyStore, StdClass, Visibility, builtin,
};
use twig::value::{
    Array, ArrayKey, Diagnostics, FetchMode, Level, PhpError, PhpResult, PhpString, Value,
    builtins, compare, dim,
};

// ---- vectors -----------------------------------------------------------------------------------------------------

/// A JSON value as `json_decode($json, true)` decodes it: objects become arrays (keys normalised), integers that fit
/// stay ints, other numbers are floats.
fn json_to_value(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(text) => {
            if !text.contains(['.', 'e', 'E'])
                && let Ok(i) = text.parse::<i64>()
            {
                return Value::Int(i);
            }
            Value::Float(text.parse::<f64>().expect("a JSON number"))
        }
        Json::String(s) => Value::String(PhpString::from(s.as_str())),
        Json::Array(items) => Value::Array(Array::from_values(items.iter().map(json_to_value))),
        Json::Object(fields) => {
            let mut array = Array::new();
            for (key, value) in fields {
                array.insert(
                    ArrayKey::from_string(PhpString::from(key.as_str())),
                    json_to_value(value),
                );
            }
            Value::Array(array)
        }
    }
}

fn json_array(json: &Json) -> Array {
    match json_to_value(json) {
        Value::Array(array) => array,
        _ => Array::new(),
    }
}

/// The `[key, value]` pairs of a generator or iterator spec.
fn pairs(spec: &Json) -> Vec<(Value, Value)> {
    spec.field("pairs")
        .as_array()
        .iter()
        .map(|pair| {
            let pair = pair.as_array();
            (json_to_value(&pair[0]), json_to_value(&pair[1]))
        })
        .collect()
}

/// `json_encode` with PHP's internal function: `json_encode(mixed $value, int $flags = 0, int $depth = 512)`.
fn json_encode_filter() -> TwigFilter {
    let callable = PhpCallable::function(
        "json_encode",
        vec![
            CallableParameter::required("value"),
            CallableParameter::optional("flags", ConstantValue::Int(0)),
            CallableParameter::optional("depth", ConstantValue::Int(512)),
        ],
    );
    CallableRuntime::new(|call| {
        let args = call.bind(
            "json_encode",
            &[
                Param::required("value"),
                Param::optional("flags", 0),
                Param::optional("depth", 512),
            ],
            FunctionKind::Internal,
        )?;
        Ok(builtins::format::json_encode(
            &args[0],
            Some(&args[1]),
            Some(&args[2]),
            call.diagnostics(),
        )?)
    })
    .attach(&callable);
    TwigFilter::new("json_encode", Some(callable), FilterOptions::default())
}

/// The environment a case asks for (`"env": "plain"`): Twig's defaults plus the case's config.
fn environment(case: &Json) -> Environment {
    let mut options = EnvironmentOptions::default();
    for (key, value) in case.field("config").as_object() {
        match key.as_str() {
            "autoescape" => {
                options.autoescape = match value {
                    Json::Bool(false) => DefaultStrategy::Disabled,
                    Json::String(s) => DefaultStrategy::Strategy(s.clone()),
                    other => panic!("unsupported autoescape {other:?}"),
                }
            }
            "strict_variables" => options.strict_variables = matches!(value, Json::Bool(true)),
            "charset" => options.charset = value.as_str().expect("charset").to_owned(),
            other => panic!("unsupported config option {other}"),
        }
    }
    options.diagnostics = match case.get("errors").and_then(Json::as_str) {
        Some("throw") => DiagnosticsPolicy::Error,
        _ => DiagnosticsPolicy::Collect,
    };
    let templates: Vec<(String, Vec<u8>)> = case
        .field("templates")
        .as_object()
        .iter()
        .map(|(name, code)| (name.clone(), code.as_bytes()))
        .collect();
    let env = Environment::with_options(ArrayLoader::new(templates), options);
    env.add_filter(json_encode_filter())
        .expect("json_encode can be registered");
    if let Some(sandbox) = case.get("sandbox") {
        // new SecurityPolicy($tags, $filters, $methods, $properties, $functions)
        let policy = SecurityPolicy::new();
        let names = |list: &Json| -> Vec<String> {
            list.as_array()
                .iter()
                .map(|name| name.as_str().expect("a name").to_owned())
                .collect()
        };
        let per_class = |map: &Json| -> Vec<(String, Vec<String>)> {
            map.as_object()
                .iter()
                .map(|(class, list)| (class.clone(), names(list)))
                .collect()
        };
        let lists = sandbox.field("policy");
        policy.set_allowed_tags(names(lists.field("tags")));
        policy.set_allowed_filters(names(lists.field("filters")));
        policy.set_allowed_methods(per_class(lists.field("methods")));
        policy.set_allowed_properties(per_class(lists.field("properties")));
        policy.set_allowed_functions(names(lists.field("functions")));
        let global = matches!(sandbox.field("global"), Json::Bool(true));
        env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), global)))
            .expect("the sandbox can be registered");
    }
    env
}

/// The context of a case: `data_json`, or `data_rust` plus the objects of its specs.
fn context(case: &Json) -> Array {
    let mut context = case
        .get("data_json")
        .or_else(|| case.get("data_rust"))
        .map(json_array)
        .unwrap_or_default();
    if let Some(objects) = case.get("objects") {
        for (name, spec) in objects.as_object() {
            context.insert(name.as_str(), build(spec));
        }
    }
    context
}

/// The PHP error type name of a diagnostic level, as the oracle reports it.
fn level_name(level: Level) -> &'static str {
    match level {
        Level::Warning => "E_WARNING",
        Level::Notice => "E_NOTICE",
        Level::Deprecated => "E_DEPRECATED",
        Level::UserDeprecated => "E_USER_DEPRECATED",
    }
}

/// Runs one case; returns a description of the mismatch, if any.
fn run_case(case: &Json, expected: &Json) -> Option<String> {
    let env = environment(case);
    let result = env
        .load("index.twig")
        .and_then(|template| template.render_bytes(context(case)));
    let notices: Vec<(String, String)> = env
        .take_notices()
        .into_iter()
        .map(|notice| match notice {
            Notice::Php(diagnostic) => (
                level_name(diagnostic.level).to_owned(),
                diagnostic.message.to_string_lossy().into_owned(),
            ),
            Notice::Deprecation(deprecation) => {
                ("E_USER_DEPRECATED".to_owned(), deprecation.to_string())
            }
        })
        .collect();
    let mut problems = Vec::new();
    match (&result, expected.field("ok")) {
        (Ok(output), Json::Bool(true)) => {
            // output that is not UTF-8 comes with '?' substitutions and a base64 copy
            let want = match expected.get("output_b64").and_then(Json::as_str) {
                Some(b64) => common::base64_decode(b64),
                None => expected.field("output").as_bytes(),
            };
            if *output != want {
                problems.push(format!(
                    "output {:?}, expected {:?}",
                    String::from_utf8_lossy(output),
                    String::from_utf8_lossy(&want)
                ));
            }
        }
        (Err(error), Json::Bool(false)) => {
            let want = expected.field("error");
            let class = want.field("class").as_str().unwrap_or_default();
            let message = want.field("message").as_lossy_string();
            let class_name = SecurityError::of(error)
                .map_or_else(|| error.kind().class_name(), SecurityError::class_name);
            let got = format!("{class_name}: {error}");
            let wanted = format!("{class}: {message}");
            if got != wanted {
                problems.push(format!("error {got:?}, expected {wanted:?}"));
            }
        }
        (Ok(output), _) => problems.push(format!(
            "rendered {:?}, expected error {}",
            String::from_utf8_lossy(output),
            expected
                .get("error")
                .and_then(|e| e.get("message"))
                .map(Json::as_lossy_string)
                .unwrap_or_default()
        )),
        (Err(error), _) => problems.push(format!(
            "error {}: {error}, expected output {:?}",
            error.kind().class_name(),
            expected
                .get("output")
                .map(Json::as_lossy_string)
                .unwrap_or_default()
        )),
    }
    let want_notices: Vec<(String, String)> = expected
        .get("diagnostics")
        .map(|d| {
            d.as_array()
                .iter()
                .map(|n| {
                    (
                        n.field("type").as_str().unwrap_or_default().to_owned(),
                        n.field("message").as_lossy_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if notices != want_notices {
        problems.push(format!(
            "diagnostics {notices:?}, expected {want_notices:?}"
        ));
    }
    if problems.is_empty() {
        None
    } else {
        Some(problems.join("; "))
    }
}

/// Runs every case of a topic and fails with the list of mismatches.
fn run_topic(topic: &str) {
    let cases = Json::load_vectors(&format!("callables/arrays/{topic}.gen.json"));
    let results = Json::load_vectors(&format!("callables/arrays/{topic}.json"));
    let cases = cases.field("cases").as_array();
    let results = results.as_array();
    assert_eq!(
        cases.len(),
        results.len(),
        "{topic}: cases and results differ in number"
    );
    let mut failures = Vec::new();
    for (case, expected) in cases.iter().zip(results) {
        let id = case.field("id").as_str().unwrap_or_default();
        assert_eq!(expected.field("id").as_str(), Some(id), "result order");
        if let Some(problem) = run_case(case, expected) {
            let template = case
                .field("templates")
                .field("index.twig")
                .as_lossy_string();
            failures.push(format!("{id} {template:?}\n    {problem}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{topic}: {} of {} cases differ from the oracle:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn filters() {
    run_topic("filters");
}

#[test]
fn functions() {
    run_topic("functions");
}

#[test]
fn tests() {
    run_topic("tests");
}

#[test]
fn objects() {
    run_topic("objects");
}

#[test]
fn errors() {
    run_topic("errors");
}

#[test]
fn sandbox() {
    run_topic("sandbox");
}

#[test]
fn charset() {
    run_topic("charset");
}

// ---- deviations and properties the oracle cannot pin -------------------------------------------------------------

fn render(template: &str, context: Array) -> Result<String, twig::Error> {
    let options = EnvironmentOptions {
        autoescape: DefaultStrategy::Disabled,
        ..EnvironmentOptions::default()
    };
    let env = Environment::with_options(ArrayLoader::new([("index", template)]), options);
    env.render("index", context)
}

/// PHP adds `, called in <compiled template> on line N` (or ` in <file> on line N`) to the errors of userland
/// functions; twig-rs has no PHP files to name (reference/deviations.md).
#[test]
fn userland_argument_errors_have_no_php_location() {
    let error = render("{{ 'x'|invoke }}", Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Twig\\Extension\\CoreExtension::invoke(): \
         Argument #1 ($arrow) must be of type Closure, string given\") in \"index\" at line 1."
    );
    let error = render("{{ [1]|split('') }}", Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Twig\\Extension\\CoreExtension::\
         splitIntoCharacters(): Argument #1 ($string) must be of type string, array given\") in \"index\" at line 1."
    );
    let error = render("{{ [1]|map(...[]) }}", Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Too few arguments to function \
         Twig\\Extension\\CoreExtension::map(), 3 passed and exactly 4 expected\") in \"index\" at line 1."
    );
}

/// Unseeded randomness: `random` and `shuffle` pick among the right values.
#[test]
fn random_draws_stay_in_range() {
    for _ in 0..50 {
        let out = render(
            "{{ random(3) }}|{{ random(-2) }}|{{ random(5, 7) }}|{{ random(['a', 'b']) }}|{{ random('xé') }}|{{ random() >= 0 ? 'ok' }}",
            Array::new(),
        )
        .unwrap();
        let parts: Vec<&str> = out.split('|').collect();
        assert!(["0", "1", "2", "3"].contains(&parts[0]), "{out}");
        assert!(["-2", "-1", "0"].contains(&parts[1]), "{out}");
        assert!(["5", "6", "7"].contains(&parts[2]), "{out}");
        assert!(["a", "b"].contains(&parts[3]), "{out}");
        assert!(["x", "é"].contains(&parts[4]), "{out}");
        assert_eq!(parts[5], "ok");
        let shuffled = render(
            "{{ (1..10)|shuffle|sort|join(',') }}|{{ 'abcé'|shuffle|length }}",
            Array::new(),
        )
        .unwrap();
        assert_eq!(shuffled, "1,2,3,4,5,6,7,8,9,10|4");
    }
}

// ---- PHP test classes ---------------------------------------------------------------------------------------------

fn property(
    name: &str,
    visibility: Visibility,
    default: Option<Value>,
    class: &str,
) -> PropertyInfo {
    PropertyInfo {
        name: PhpString::from(name),
        visibility,
        is_static: false,
        is_readonly: false,
        type_name: None,
        default,
        declaring_class: class.into(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// `Generator` for `twig_rs_gen($pairs)` (`foreach ($pairs as $p) { yield $p[0] => $p[1]; }`): the engine's state
/// machine of PHPSRC/Zend/zend_generators.c (the body starts on first use, `rewind()` only at the first yield, a
/// finished generator cannot be traversed again).
struct Generator {
    class: ClassRef,
    state: Mutex<GeneratorState>,
}

struct GeneratorState {
    pairs: Vec<(Value, Value)>,
    /// The index of the next pair the body yields.
    next: usize,
    /// The body ran to its end (`execute_data == NULL`).
    finished: bool,
    /// The current pair (`None` before the first yield and after the end).
    current: Option<(Value, Value)>,
    at_first_yield: bool,
}

static GENERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    let mut b = ClassBuilder::class("Generator")
        .final_class()
        .implements(&builtin::iterator());
    for m in [
        "rewind",
        "valid",
        "current",
        "key",
        "next",
        "send",
        "throw",
        "getReturn",
    ] {
        b = b.method(m);
    }
    b.build()
});

impl Generator {
    fn create(pairs: Vec<(Value, Value)>) -> ObjectRef {
        ObjectRef::new(Generator {
            class: GENERATOR.clone(),
            state: Mutex::new(GeneratorState {
                pairs,
                next: 0,
                finished: false,
                current: None,
                at_first_yield: false,
            }),
        })
    }

    /// `zend_generator_resume()`: runs the body to its next yield.
    fn resume(state: &mut GeneratorState) {
        if state.finished {
            return;
        }
        state.at_first_yield = false;
        match state.pairs.get(state.next).cloned() {
            Some(pair) => {
                state.next += 1;
                state.current = Some(pair);
            }
            None => {
                state.current = None;
                state.finished = true;
            }
        }
    }

    /// `zend_generator_ensure_initialized()`.
    fn ensure_initialized(state: &mut GeneratorState) {
        if state.current.is_none() && !state.finished {
            Self::resume(state);
            state.at_first_yield = true;
        }
    }

    fn rewind(&self) -> PhpResult<()> {
        let mut state = lock(&self.state);
        Self::ensure_initialized(&mut state);
        if !state.at_first_yield {
            return Err(PhpError::exception(
                "Cannot rewind a generator that was already run",
            ));
        }
        Ok(())
    }

    fn valid(&self) -> bool {
        let mut state = lock(&self.state);
        Self::ensure_initialized(&mut state);
        !state.finished
    }

    fn current(&self) -> Value {
        let mut state = lock(&self.state);
        Self::ensure_initialized(&mut state);
        state
            .current
            .as_ref()
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    fn key(&self) -> Value {
        let mut state = lock(&self.state);
        Self::ensure_initialized(&mut state);
        state
            .current
            .as_ref()
            .map(|(k, _)| k.clone())
            .unwrap_or_default()
    }

    fn next(&self) {
        let mut state = lock(&self.state);
        Self::ensure_initialized(&mut state);
        Self::resume(&mut state);
    }
}

impl Object for Generator {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let none = |name: &str| {
            args.bind(&format!("Generator::{name}"), &[], FunctionKind::Internal)
                .map(|_| ())
        };
        match method {
            "rewind" => {
                none("rewind")?;
                self.rewind().map(|_| Value::Null)
            }
            "valid" => {
                none("valid")?;
                Ok(Value::Bool(self.valid()))
            }
            "current" => {
                none("current")?;
                Ok(self.current())
            }
            "key" => {
                none("key")?;
                Ok(self.key())
            }
            "next" => {
                none("next")?;
                self.next();
                Ok(Value::Null)
            }
            _ => Err(twig::value::object::undefined_method(&self.class, method)),
        }
    }

    /// `zend_generator_get_iterator()`.
    fn get_iterator(
        &self,
        this: &ObjectRef,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Box<dyn ObjectIterator>>> {
        if lock(&self.state).finished {
            return Err(PhpError::exception(
                "Cannot traverse an already closed generator",
            ));
        }
        Ok(Some(Box::new(GeneratorIterator {
            generator: this.clone(),
        })))
    }
}

/// `zend_generator_iterator_functions`.
struct GeneratorIterator {
    generator: ObjectRef,
}

impl GeneratorIterator {
    fn generator(&self) -> &Generator {
        self.generator
            .downcast_ref::<Generator>()
            .expect("a generator")
    }
}

impl ObjectIterator for GeneratorIterator {
    fn rewind(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.generator().rewind()
    }

    fn valid(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        Ok(self.generator().valid())
    }

    fn current(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        Ok(self.generator().current())
    }

    fn key(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        Ok(self.generator().key())
    }

    fn next(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.generator().next();
        Ok(())
    }
}

// TwigRsLogIter: an Iterator over pairs logging every call in its public `$log`.
static LOG_ITER: LazyLock<ClassRef> = LazyLock::new(|| {
    let c = "TwigRsLogIter";
    ClassBuilder::class(c)
        .implements(&builtin::iterator())
        .property_info(property("i", Visibility::Private, Some(Value::Int(0)), c))
        .property_info(property(
            "log",
            Visibility::Public,
            Some(Value::Array(Array::new())),
            c,
        ))
        .property_info(property("pairs", Visibility::Private, None, c))
        .method("__construct")
        .method("current")
        .method("key")
        .method("next")
        .method("rewind")
        .method("valid")
        .build()
});

struct LogIter {
    properties: PropertyStore,
    pairs: Vec<(Value, Value)>,
    position: Mutex<usize>,
}

impl LogIter {
    fn log(&self, what: &str) {
        let mut log = match self.properties.get(b"log") {
            Some(Value::Array(a)) => a,
            _ => Array::new(),
        };
        let _ = log.push(Value::from(what));
        self.properties.set(b"log", Value::Array(log));
    }
}

impl Object for LogIter {
    fn class(&self) -> &ClassRef {
        &LOG_ITER
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let mut i = lock(&self.position);
        match method {
            "current" => {
                self.log("current");
                match self.pairs.get(*i) {
                    Some((_, v)) => Ok(v.clone()),
                    None => {
                        diag.warn(format!("Undefined array key {}", *i))?;
                        diag.warn("Trying to access array offset on null")?;
                        Ok(Value::Null)
                    }
                }
            }
            "key" => {
                self.log("key");
                Ok(self
                    .pairs
                    .get(*i)
                    .map(|(k, _)| k.clone())
                    .unwrap_or_default())
            }
            "next" => {
                self.log("next");
                *i += 1;
                self.properties.set(b"i", Value::Int(*i as i64));
                Ok(Value::Null)
            }
            "rewind" => {
                self.log("rewind");
                *i = 0;
                self.properties.set(b"i", Value::Int(0));
                Ok(Value::Null)
            }
            "valid" => {
                self.log("valid");
                Ok(Value::Bool(*i < self.pairs.len()))
            }
            _ => Err(twig::value::object::undefined_method(&LOG_ITER, method)),
        }
    }
}

/// The array-holding classes: `TwigRsAgg`, `TwigRsAggGen`, `TwigRsCountAccess`, `TwigRsAccess`, `TwigRsAccessAgg`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Holder {
    Agg,
    AggGen,
    CountAccess,
    Access,
    AccessAgg,
}

fn holder_class(kind: Holder) -> ClassRef {
    static CLASSES: LazyLock<[ClassRef; 5]> = LazyLock::new(|| {
        let access = |b: ClassBuilder| {
            b.implements(&builtin::array_access())
                .method("offsetExists")
                .method("offsetGet")
                .method("offsetSet")
                .method("offsetUnset")
        };
        let data = |c: &str| property("data", Visibility::Private, None, c);
        [
            ClassBuilder::class("TwigRsAgg")
                .implements(&builtin::iterator_aggregate())
                .property_info(data("TwigRsAgg"))
                .method("__construct")
                .method("getIterator")
                .build(),
            ClassBuilder::class("TwigRsAggGen")
                .implements(&builtin::iterator_aggregate())
                .property_info(property("pairs", Visibility::Private, None, "TwigRsAggGen"))
                .method("__construct")
                .method("getIterator")
                .build(),
            access(
                ClassBuilder::class("TwigRsCountAccess")
                    .implements(&builtin::countable())
                    .property_info(data("TwigRsCountAccess"))
                    .method("__construct"),
            )
            .method("count")
            .build(),
            access(
                ClassBuilder::class("TwigRsAccess")
                    .property_info(data("TwigRsAccess"))
                    .method("__construct"),
            )
            .build(),
            access(
                ClassBuilder::class("TwigRsAccessAgg")
                    .implements(&builtin::iterator_aggregate())
                    .property_info(data("TwigRsAccessAgg"))
                    .method("__construct"),
            )
            .method("getIterator")
            .build(),
        ]
    });
    CLASSES[kind as usize].clone()
}

struct ArrayHolder {
    kind: Holder,
    class: ClassRef,
    data: Array,
    pairs: Vec<(Value, Value)>,
}

impl Object for ArrayHolder {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let data = Value::Array(self.data.clone());
        let offset = args.get(0).cloned().unwrap_or_default();
        let access = matches!(
            self.kind,
            Holder::CountAccess | Holder::Access | Holder::AccessAgg
        );
        match method {
            "getiterator" if self.kind == Holder::AggGen => {
                Ok(Value::Object(Generator::create(self.pairs.clone())))
            }
            "getiterator" if matches!(self.kind, Holder::Agg | Holder::AccessAgg) => Ok(
                Value::Object(ArrayIterator::new(self.data.clone()).into_ref()),
            ),
            // return isset($this->data[$o]);
            "offsetexists" if access => Ok(Value::Bool(dim::isset(&data, &offset, diag)?)),
            // return $this->data[$o];
            "offsetget" if access => dim::read(&data, &offset, FetchMode::Read, diag),
            "offsetset" | "offsetunset" if access => Ok(Value::Null),
            "count" if self.kind == Holder::CountAccess => Ok(Value::Int(self.data.len() as i64)),
            _ => Err(twig::value::object::undefined_method(&self.class, method)),
        }
    }
}

/// `TwigRsStr` (`__toString()`), `TwigRsCountable`, `TwigRsPlain`, `TwigRsInvoke`, `TwigRsCmp`.
enum Small {
    Str(PhpString),
    Countable(i64),
    Plain,
    Invoke,
    Cmp,
}

static STR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsStr")
        .property_info(property("s", Visibility::Private, None, "TwigRsStr"))
        .method("__construct")
        .method("__toString")
        .build()
});
static COUNTABLE: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsCountable")
        .implements(&builtin::countable())
        .property_info(property("n", Visibility::Private, None, "TwigRsCountable"))
        .method("__construct")
        .method("count")
        .build()
});
static PLAIN: LazyLock<ClassRef> = LazyLock::new(|| ClassBuilder::class("TwigRsPlain").build());
static INVOKE: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsInvoke")
        .method("__invoke")
        .build()
});
static CMP: LazyLock<ClassRef> =
    LazyLock::new(|| ClassBuilder::class("TwigRsCmp").method("__invoke").build());

struct SmallObject {
    kind: Small,
}

impl Object for SmallObject {
    fn class(&self) -> &ClassRef {
        match self.kind {
            Small::Str(_) => &STR,
            Small::Countable(_) => &COUNTABLE,
            Small::Plain => &PLAIN,
            Small::Invoke => &INVOKE,
            Small::Cmp => &CMP,
        }
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match (&self.kind, method) {
            (Small::Str(s), "__tostring") => Ok(Value::String(s.clone())),
            (Small::Countable(n), "count") => Ok(Value::Int(*n)),
            // public function __invoke(...$args) { return json_encode($args); }
            (Small::Invoke, "__invoke") => {
                let bound = args.bind(
                    "TwigRsInvoke::__invoke",
                    &[Param::variadic("args")],
                    FunctionKind::User,
                )?;
                builtins::format::json_encode(&bound[0], None, None, diag)
            }
            // public function __invoke($a, $b) { return $b <=> $a; }
            (Small::Cmp, "__invoke") => {
                let bound = args.bind(
                    "TwigRsCmp::__invoke",
                    &[Param::required("a"), Param::required("b")],
                    FunctionKind::User,
                )?;
                Ok(Value::Int(compare::spaceship(&bound[1], &bound[0], diag)?))
            }
            _ => Err(twig::value::object::undefined_method(self.class(), method)),
        }
    }
}

/// The object (or value) of a case's object spec, built like the PHP expression of the generator.
fn build(spec: &Json) -> Value {
    let kind = spec.field("kind").as_str().expect("an object kind");
    let small = |kind: Small| Value::Object(ObjectRef::new(SmallObject { kind }));
    let holder = |kind: Holder| {
        Value::Object(ObjectRef::new(ArrayHolder {
            kind,
            class: holder_class(kind),
            data: spec.get("data").map(json_array).unwrap_or_default(),
            pairs: if spec.get("pairs").is_some() {
                pairs(spec)
            } else {
                Vec::new()
            },
        }))
    };
    match kind {
        "gen" => Value::Object(Generator::create(pairs(spec))),
        "log_iter" => Value::Object(ObjectRef::new(LogIter {
            properties: PropertyStore::new(&LOG_ITER),
            pairs: pairs(spec),
            position: Mutex::new(0),
        })),
        "agg" => holder(Holder::Agg),
        "agg_gen" => holder(Holder::AggGen),
        "count_access" => holder(Holder::CountAccess),
        "access" => holder(Holder::Access),
        "access_agg" => holder(Holder::AccessAgg),
        "array_object" => {
            Value::Object(ArrayObject::new(json_array(spec.field("data"))).into_ref())
        }
        "array_iterator" => {
            Value::Object(ArrayIterator::new(json_array(spec.field("data"))).into_ref())
        }
        "str" => small(Small::Str(PhpString::from(spec.field("value").as_bytes()))),
        "markup" => Value::Object(Markup::create(
            PhpString::from(spec.field("value").as_bytes()),
            Some("UTF-8".into()),
            None,
        )),
        "std" => Value::Object(
            StdClass::with_properties(
                spec.field("data")
                    .as_object()
                    .iter()
                    .map(|(k, v)| (PhpString::from(k.as_str()), json_to_value(v))),
            )
            .into_ref(),
        ),
        "countable" => small(Small::Countable(spec.field("n").as_i64().expect("n"))),
        "plain" => small(Small::Plain),
        "invoke" => small(Small::Invoke),
        "cmp" => small(Small::Cmp),
        "list" => Value::Array(Array::from_values(
            spec.field("items").as_array().iter().map(build),
        )),
        "bytes" => {
            let hex = spec.field("hex").as_str().expect("hex");
            Value::String(PhpString::from(
                (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
                    .collect::<Vec<u8>>(),
            ))
        }
        other => panic!("unknown object kind {other}"),
    }
}
