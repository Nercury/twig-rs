//! Runs an oracle "twig" case with twig-rs and reports the result in the oracle's schema.
//!
//! The steps mirror `run_twig()` and `plain_environment()` of research/php-oracle (runner.php,
//! lib.php):
//!
//! 1. `setup`: `new Environment(new ArrayLoader($templates), array_merge(['cache' => false],
//!    $config))` (Twig's option handling as ported by `twig-test-support`), the globals, the
//!    `debug` / `string_loader` extensions, the `SandboxExtension` with the case's policy and
//!    global flag (and source policy), and the case's `lexer` options
//!    (`$twig->setLexer(new Lexer($twig, $lexer))`; [`EnvironmentOptions::lexer`] here).
//!    Diagnostics raised here are dropped: the oracle records only the phases below.
//! 2. `load`: every template in order (`load_all`, default true), then the main template.
//! 3. `data`: `data_json` decoded like `json_decode($json, true)`, or for `data_php` the typed tree
//!    the oracle recorded (`typed_data`), rebuilt by `twig-test-support` with the objects of
//!    [`Factory`]; the oracle's `state` (default timezone, `CoreExtension` settings) is applied and
//!    the diagnostics the data code raised in PHP (`data_diagnostics`, `data_deprecations`) are
//!    replayed in their place.
//! 4. `render`: `TemplateWrapper::render()` to bytes, with the oracle's "now" when its result has a
//!    `clock` (`gettimeofday()` taken right before PHP rendered).
//!
//! PHP diagnostics are collected (`"errors": "collect"`) or turned into `ErrorException`s
//! (`"throw"`), as the oracle's error handler does.
//!
//! The classes PHP knows in the plain environment are registered for `constant()` and `enum()`:
//! the internal classes `twig-test-support` implements (`SplObjectStorage`, ...) and the autoloadable
//! `Twig\Tests` enums always; the helper classes of Twig's `tests/IntegrationTest.php` when the case
//! sets `"test_classes": true` (the oracle then loads that file before the templates); and, after the
//! data phase, the class of every object the data contains (PHP declared them while the data code
//! ran).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Duration;

use twig::deprecation::IgnoreDeprecations;
use twig::escaper::DefaultStrategy;
use twig::extension::{DebugExtension, StringLoaderExtension};
use twig::lexer::LexerOptions;
use twig::loader::ArrayLoader;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, Notice, SandboxExtension, SecurityPolicy,
    SourcePolicyInterface,
};
use twig::source::Source;
use twig::value::{Array, ArrayKey, Collect, Level, PhpError, PhpString, Value};
use twig_date::FixedClock;
use twig_test_support::{Autoescape, PhpState, TypedNode};

use super::case::{Case, CaseData, ErrorsMode};
use super::factory::{Factory, apply_state};
use super::json::{Json, decode_php};
use super::result::{CaseResult, Diag, ErrorDesc, TwigErrorInfo};

/// The class of the error reported when a `data_php` case reaches the data phase without the
/// oracle's typed tree of its data.
pub const DATA_UNAVAILABLE: &str = "TwigFuzz\\DataUnavailable";
/// The class of the error reported when twig-rs panicked.
pub const PANIC: &str = "TwigFuzz\\Panic";
/// The class of the error reported when twig-rs did not finish in time.
pub const TIMEOUT: &str = "TwigFuzz\\Timeout";

/// The helper classes declared by Twig's `tests/IntegrationTest.php`.
const INTEGRATION_TEST_CLASSES: &[&str] = &[
    "Twig\\Tests\\TwigTestFoo",
    "Twig\\Tests\\MagicCallStub",
    "Twig\\Tests\\ToStringStub",
    "Twig\\Tests\\CountableStub",
    "Twig\\Tests\\IteratorAggregateStub",
    "Twig\\Tests\\SimpleIteratorForTesting",
];

/// What running a case gave.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// A result in the oracle's schema.
    Ran(Box<CaseResult>),
    /// The case uses something the runner cannot reproduce (why).
    Unsupported(String),
}

impl Outcome {
    /// The result, if the case ran.
    pub fn result(&self) -> Option<&CaseResult> {
        match self {
            Outcome::Ran(result) => Some(result.as_ref()),
            Outcome::Unsupported(_) => None,
        }
    }
}

/// Runs `case` on the current thread. `oracle` is the oracle's result for the same case; only its
/// typed data (`data`, `state`, `data_diagnostics`, `data_deprecations`) is used.
pub fn run(case: &Case, oracle: Option<&CaseResult>) -> Outcome {
    match run_inner(case, oracle) {
        Ok(result) => Outcome::Ran(Box::new(result)),
        Err(Stop::Unsupported(reason)) => Outcome::Unsupported(reason),
    }
}

/// Runs `case` on a thread of its own with a large stack: a panic becomes a result in phase
/// `panic` (error class [`PANIC`]), and a run that takes longer than `timeout` a result in phase
/// `timeout` ([`TIMEOUT`]; the thread is left running).
pub fn run_guarded(case: &Case, oracle: Option<&CaseResult>, timeout: Duration) -> Outcome {
    let (sender, receiver) = std::sync::mpsc::channel();
    let owned_case = case.clone();
    let owned_oracle = oracle.cloned();
    let spawned = std::thread::Builder::new()
        .name(format!("twig-fuzz {}", case.id))
        .stack_size(256 << 20)
        .spawn(move || {
            let outcome =
                catch_unwind(AssertUnwindSafe(|| run(&owned_case, owned_oracle.as_ref())));
            let _ = sender.send(outcome);
        });
    let id = case.json.get("id").cloned().unwrap_or(Json::Null);
    if let Err(error) = spawned {
        return Outcome::Unsupported(format!("cannot spawn a thread: {error}"));
    }
    match receiver.recv_timeout(timeout) {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(payload)) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a non-string panic payload".to_owned());
            Outcome::Ran(Box::new(CaseResult::failure(
                id,
                "panic",
                plain_error(PANIC, message.into_bytes()),
            )))
        }
        Err(_) => Outcome::Ran(Box::new(CaseResult::failure(
            id,
            "timeout",
            plain_error(
                TIMEOUT,
                format!("twig-rs did not finish within {} ms", timeout.as_millis()).into_bytes(),
            ),
        ))),
    }
}

enum Stop {
    Unsupported(String),
}

fn unsupported<T>(reason: impl Into<String>) -> Result<T, Stop> {
    Err(Stop::Unsupported(reason.into()))
}

/// Accumulates the result while the phases run.
struct Recorder {
    id: Json,
    deprecations: Vec<String>,
    diagnostics: Vec<Diag>,
}

impl Recorder {
    fn take(&mut self, env: &Environment) {
        for notice in env.take_notices() {
            match notice {
                Notice::Php(diagnostic) => {
                    let message = diagnostic.message.to_string_lossy().into_owned();
                    if diagnostic.level == Level::UserDeprecated {
                        self.deprecations.push(message.clone());
                    }
                    self.diagnostics.push(Diag {
                        kind: level_name(diagnostic.level).to_owned(),
                        message,
                        silenced: false,
                    });
                }
                Notice::Deprecation(deprecation) => {
                    let message = deprecation.to_string();
                    self.deprecations.push(message.clone());
                    self.diagnostics.push(Diag {
                        kind: "E_USER_DEPRECATED".to_owned(),
                        message,
                        silenced: false,
                    });
                }
            }
        }
    }

    fn finish(self, mut result: CaseResult) -> CaseResult {
        result.deprecations = self.deprecations;
        result.diagnostics = self.diagnostics;
        result
    }

    fn fail(self, phase: &str, error: ErrorDesc) -> CaseResult {
        let result = CaseResult::failure(self.id.clone(), phase, error);
        self.finish(result)
    }
}

fn run_inner(case: &Case, oracle: Option<&CaseResult>) -> Result<CaseResult, Stop> {
    if let Some(reason) = case.unsupported_reason() {
        return unsupported(reason);
    }
    let mut recorder = Recorder {
        id: case.json.get("id").cloned().unwrap_or(Json::Null),
        deprecations: Vec::new(),
        diagnostics: Vec::new(),
    };

    // ---- setup ------------------------------------------------------------------------------
    let options = match options(case)? {
        Ok(options) => options,
        Err(error) => return Ok(recorder.fail("setup", describe_php(&error, 0))),
    };
    let env = Environment::with_options(ArrayLoader::new(case.templates.clone()), options);
    if let Some(timezone) = case.field("timezone") {
        let name = match timezone {
            Json::String(s) => s.clone(),
            other => other.to_compact(),
        };
        let mut ignored = Collect::new();
        // date_default_timezone_set() runs before the oracle installs its error handler
        let _ = env
            .date_context()
            .default_timezone_set(name.as_bytes(), &mut ignored);
    }
    register_classes(&env, case.field("test_classes").is_some_and(Json::truthy));
    if let Err(error) = setup(case, &env)? {
        return Ok(recorder.fail("setup", error));
    }
    let _ = env.take_notices();

    // ---- load -------------------------------------------------------------------------------
    let loaded = (|| {
        if case.load_all {
            for (name, _) in &case.templates {
                env.load(name)?;
            }
        }
        env.load(&case.main)
    })();
    recorder.take(&env);
    let wrapper = match loaded {
        Ok(wrapper) => wrapper,
        Err(error) => return Ok(recorder.fail("load", describe(&error, 0))),
    };

    // ---- data -------------------------------------------------------------------------------
    let context = match &case.data {
        CaseData::None => Value::Array(Array::new()),
        CaseData::Json(json) => decode_php(json),
        CaseData::Php(_) => match oracle.and_then(|o| o.extra("data")) {
            None => {
                let reason = match oracle {
                    None => "no oracle result was given",
                    Some(_) => "the oracle result has no typed data (run it with typed_data)",
                };
                return Ok(recorder.fail(
                    "data",
                    plain_error(
                        DATA_UNAVAILABLE,
                        format!("data_php cannot run in twig-rs: {reason}").into_bytes(),
                    ),
                ));
            }
            Some(tree) => {
                let oracle = oracle.expect("the tree comes from the oracle result");
                php_data(&env, tree, oracle, &mut recorder)?
            }
        },
    };
    let context = match context {
        Value::Array(array) => array,
        other => {
            let error = plain_error(
                "InvalidArgumentException",
                format!(
                    "The data must evaluate to an array, got \"{}\".",
                    debug_type(&other)
                )
                .into_bytes(),
            );
            return Ok(recorder.fail("data", error));
        }
    };

    // ---- render -----------------------------------------------------------------------------
    // the oracle's "now" (`clock`: gettimeofday() right before its render)
    if let Some(clock) = oracle.and_then(|o| o.extra("clock"))
        && let [sec, usec] = clock.items()
        && let (Some(sec), Some(usec)) = (sec.as_i64(), usec.as_i64())
    {
        env.date_context().set_clock(FixedClock::new(sec, usec));
    }
    let rendered = wrapper.render_bytes(context);
    recorder.take(&env);
    Ok(match rendered {
        Ok(output) => {
            let result = CaseResult::success(recorder.id.clone(), output);
            recorder.finish(result)
        }
        Err(error) => recorder.fail("render", describe(&error, 0)),
    })
}

/// The environment options of the case: `array_merge(['cache' => false], $config)` through Twig's
/// option handling. The inner `Err` is the PHP exception of the constructor (phase `setup`).
fn options(case: &Case) -> Result<Result<EnvironmentOptions, PhpError>, Stop> {
    let mut merged = Array::new();
    merged.insert("cache", false);
    match case.field("config") {
        None => {}
        Some(json @ (Json::Object(_) | Json::Array(_))) => {
            let Value::Array(config) = decode_php(json) else {
                return unsupported("config is not an array");
            };
            for (key, value) in config.iter() {
                match key {
                    // array_merge(): int keys are appended
                    ArrayKey::Int(_) => {
                        let _ = merged.push(value.clone());
                    }
                    ArrayKey::Str(_) => {
                        merged.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        Some(other) => return unsupported(format!("config is not an object: {other:?}")),
    }
    let mut ignored = Collect::new();
    let resolved = match twig_test_support::EnvironmentOptions::from_options(&merged, &mut ignored)
    {
        Ok(resolved) => resolved,
        Err(error) => return Ok(Err(error)),
    };
    let autoescape = match &resolved.autoescape {
        Autoescape::Disabled => DefaultStrategy::Disabled,
        Autoescape::Strategy(name) => {
            DefaultStrategy::Strategy(name.to_string_lossy().into_owned())
        }
        Autoescape::FileExtension => DefaultStrategy::Strategy("name".to_owned()),
        Autoescape::Callable(value) => {
            return unsupported(format!(
                "callable autoescape option {value:?} (PHP callables cannot be reproduced)"
            ));
        }
    };
    Ok(Ok(EnvironmentOptions {
        debug: resolved.debug,
        charset: resolved.charset.to_string_lossy().into_owned(),
        strict_variables: resolved.strict_variables,
        autoescape,
        auto_reload: Some(resolved.auto_reload),
        optimizations: resolved.optimizations,
        use_yield: resolved.use_yield,
        lexer: lexer_options(case)?,
        diagnostics: match case.errors {
            ErrorsMode::Collect => DiagnosticsPolicy::Collect,
            ErrorsMode::Throw => DiagnosticsPolicy::Error,
        },
        ..EnvironmentOptions::default()
    }))
}

/// The case's `lexer` options: `new \Twig\Lexer($twig, $lexer)` merges them over Twig's defaults
/// with `array_merge()`, so a key left out keeps its default and a key the lexer does not know is
/// ignored. Values must be strings, the delimiters (`tag_comment`, `tag_block`, `tag_variable`,
/// `interpolation`) pairs of strings. `whitespace_line_chars` is the literal set of characters
/// twig-rs takes (Twig's is the body of a PCRE character class: the same unless it uses escapes
/// or class syntax, `reference/deviations.md`).
fn lexer_options(case: &Case) -> Result<LexerOptions, Stop> {
    let mut options = LexerOptions::default();
    let Some(json) = case.field("lexer") else {
        return Ok(options);
    };
    let Json::Object(fields) = json else {
        return unsupported(format!("lexer options are not an object: {json:?}"));
    };
    let string = |key: &str, value: &Json| -> Result<Vec<u8>, Stop> {
        match value {
            Json::String(s) => Ok(s.as_bytes().to_vec()),
            other => unsupported(format!("lexer option {key} is not a string: {other:?}")),
        }
    };
    let pair = |key: &str, value: &Json| -> Result<[Vec<u8>; 2], Stop> {
        match (value, value.items()) {
            (Json::Array(_), [open, close]) => Ok([string(key, open)?, string(key, close)?]),
            _ => unsupported(format!(
                "lexer option {key} is not a pair of strings: {value:?}"
            )),
        }
    };
    // (a key given twice: json_decode() keeps the last one)
    for (key, value) in fields {
        match key.as_str() {
            "tag_comment" => options.tag_comment = pair(key, value)?,
            "tag_block" => options.tag_block = pair(key, value)?,
            "tag_variable" => options.tag_variable = pair(key, value)?,
            "interpolation" => options.interpolation = pair(key, value)?,
            "whitespace_trim" => options.whitespace_trim = string(key, value)?,
            "whitespace_line_trim" => options.whitespace_line_trim = string(key, value)?,
            "whitespace_line_chars" => options.whitespace_line_chars = string(key, value)?,
            _ => {}
        }
    }
    Ok(options)
}

/// Registers the classes PHP has in the plain environment (see the module documentation).
fn register_classes(env: &Environment, test_classes: bool) {
    for class in twig_test_support::classes::fixture_class_table().iter() {
        let name = class.name().to_string();
        if !test_classes && INTEGRATION_TEST_CLASSES.contains(&name.as_str()) {
            continue;
        }
        if env.with_constants(|constants| constants.class(&name).is_none()) {
            let _ = env.add_class(class.clone());
        }
    }
}

/// Globals, extensions and the sandbox, in `plain_environment()`'s order. The inner `Err` is the
/// exception PHP throws (phase `setup`).
fn setup(case: &Case, env: &Environment) -> Result<Result<(), ErrorDesc>, Stop> {
    match case.field("globals") {
        None => {}
        Some(globals @ (Json::Object(_) | Json::Array(_))) => {
            let Value::Array(globals) = decode_php(globals) else {
                return unsupported("globals are not an array");
            };
            for (key, value) in globals.iter() {
                let name = match key {
                    ArrayKey::Int(i) => i.to_string(),
                    ArrayKey::Str(s) => s.to_string_lossy().into_owned(),
                };
                if let Err(error) = env.add_global(&name, value.clone()) {
                    return Ok(Err(describe(&error, 0)));
                }
            }
        }
        Some(other) => return unsupported(format!("globals are not an object: {other:?}")),
    }
    let extensions: Vec<&Json> = match case.field("extensions") {
        None => Vec::new(),
        Some(Json::Array(items)) => items.iter().collect(),
        Some(Json::Object(fields)) => fields.iter().map(|(_, v)| v).collect(),
        Some(other) => return unsupported(format!("extensions are not a list: {other:?}")),
    };
    let mut wants_sandbox = false;
    for name in &extensions {
        let registered = match name.as_str() {
            Some("debug") => env.add_extension(Arc::new(DebugExtension::new())),
            Some("string_loader") => env.add_extension(Arc::new(StringLoaderExtension::new())),
            Some("sandbox") => {
                wants_sandbox = true;
                Ok(())
            }
            Some(other) => {
                return Ok(Err(unknown_extension(other)));
            }
            None => match name {
                Json::Number(n) => return Ok(Err(unknown_extension(n))),
                other => return unsupported(format!("extension name {other:?}")),
            },
        };
        if let Err(error) = registered {
            return Ok(Err(describe(&error, 0)));
        }
    }
    let sandbox = case.field("sandbox");
    if sandbox.is_some() || wants_sandbox {
        let empty = Json::Object(Vec::new());
        let sandbox = sandbox.unwrap_or(&empty);
        if !matches!(sandbox, Json::Object(_)) {
            return unsupported(format!("sandbox is not an object: {sandbox:?}"));
        }
        let policy = policy(sandbox.get_non_null("policy").unwrap_or(&empty))?;
        let global = sandbox.get_non_null("global").is_some_and(Json::truthy);
        let extension = match sandbox.get_non_null("source_policy") {
            None => SandboxExtension::new(Arc::new(policy), global),
            Some(Json::Array(items)) => {
                let mut names = Vec::new();
                for item in items {
                    match item {
                        Json::String(name) => names.push(name.clone()),
                        other => return unsupported(format!("source_policy name {other:?}")),
                    }
                }
                // (a setup notice: the oracle does not record it)
                SandboxExtension::with_source_policy(
                    Arc::new(policy),
                    global,
                    Arc::new(NamedSources(names)),
                    &mut IgnoreDeprecations,
                )
            }
            Some(other) => return unsupported(format!("source_policy is not a list: {other:?}")),
        };
        if let Err(error) = env.add_extension(Arc::new(extension)) {
            return Ok(Err(describe(&error, 0)));
        }
    }
    Ok(Ok(()))
}

/// The case's source policy (`"sandbox": {"source_policy": [names]}`): the templates of these names
/// run sandboxed, like the anonymous `SourcePolicyInterface` of research/php-oracle lib.php
/// (`in_array($source->getName(), $names, true)`).
struct NamedSources(Vec<String>);

impl SourcePolicyInterface for NamedSources {
    fn enable_sandbox(&self, source: &Source) -> bool {
        self.0.iter().any(|name| name == source.name())
    }
}

fn unknown_extension(name: &str) -> ErrorDesc {
    plain_error(
        "InvalidArgumentException",
        format!("Unknown extension \"{name}\" (supported: debug, string_loader, sandbox).")
            .into_bytes(),
    )
}

/// `new SecurityPolicy($p['tags'] ?? [], $p['filters'] ?? [], $p['methods'] ?? [],
/// $p['properties'] ?? [], $p['functions'] ?? [], $p['tests'] ?? [])` and `setStrict($p['strict'])`.
fn policy(json: &Json) -> Result<SecurityPolicy, Stop> {
    if !matches!(json, Json::Object(_)) {
        return unsupported(format!("sandbox policy is not an object: {json:?}"));
    }
    let names = |key: &str| -> Result<Vec<String>, Stop> {
        let mut out = Vec::new();
        let items: Vec<&Json> = match json.get_non_null(key) {
            None => Vec::new(),
            Some(Json::Array(items)) => items.iter().collect(),
            Some(Json::Object(fields)) => fields.iter().map(|(_, v)| v).collect(),
            Some(other) => return unsupported(format!("policy {key} is not a list: {other:?}")),
        };
        for item in items {
            // in_array($name, $allowed, true): a non-string entry never matches a name
            if let Json::String(s) = item {
                out.push(s.clone());
            }
        }
        Ok(out)
    };
    let per_class = |key: &str| -> Result<Vec<(String, Vec<String>)>, Stop> {
        let mut out = Vec::new();
        for (class, members) in json
            .get_non_null(key)
            .map(Json::entries)
            .unwrap_or_default()
        {
            let members = match members {
                Json::String(s) => vec![s.clone()],
                Json::Array(items) => {
                    let mut names = Vec::new();
                    for item in items {
                        match item.as_str() {
                            Some(s) => names.push(s.to_owned()),
                            None => {
                                return unsupported(format!("policy {key}: member {item:?}"));
                            }
                        }
                    }
                    names
                }
                other => return unsupported(format!("policy {key}: members {other:?}")),
            };
            out.push((class.clone(), members));
        }
        Ok(out)
    };
    let policy = SecurityPolicy::new();
    policy.set_allowed_tags(names("tags")?);
    policy.set_allowed_filters(names("filters")?);
    policy.set_allowed_methods(per_class("methods")?);
    policy.set_allowed_properties(per_class("properties")?);
    policy.set_allowed_functions(names("functions")?);
    policy.set_allowed_tests(names("tests")?);
    if json.get_non_null("strict").is_some_and(Json::truthy) {
        policy.set_strict(true);
    }
    Ok(policy)
}

/// The context of a `data_php` case from the oracle's typed tree.
fn php_data(
    env: &Environment,
    tree: &Json,
    oracle: &CaseResult,
    recorder: &mut Recorder,
) -> Result<Value, Stop> {
    let node: TypedNode = match tree.to_serde().and_then(|v| {
        serde_json::from_value::<TypedNode>(v).map_err(|e| format!("invalid typed tree: {e}"))
    }) {
        Ok(node) => node,
        Err(error) => return unsupported(error),
    };
    let conversion = match twig_test_support::convert(&node, &Factory::with_tree(env, tree)) {
        Ok(conversion) => conversion,
        Err(error) => return unsupported(format!("the data cannot be rebuilt: {error}")),
    };
    // the classes the data code declared exist from now on
    for value in conversion.objects.values() {
        if let Value::Object(object) = value {
            let class = object.class().clone();
            let name = class.name().to_string();
            if env.with_constants(|constants| constants.class(&name).is_none()) {
                let _ = env.add_class(class);
            }
        }
    }
    if let Some(state) = oracle.extra("state") {
        let state: PhpState = match state
            .to_serde()
            .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
        {
            Ok(state) => state,
            Err(error) => return unsupported(format!("invalid state: {error}")),
        };
        if let Err(error) = apply_state(env, &state) {
            return unsupported(error);
        }
    }
    // what the data code raised in PHP, in its place
    for diag in oracle
        .extra("data_diagnostics")
        .map(Json::items)
        .unwrap_or_default()
    {
        let diag = CaseResult::from_json(&Json::Object(vec![(
            "diagnostics".to_owned(),
            Json::Array(vec![diag.clone()]),
        )]))
        .diagnostics;
        recorder.diagnostics.extend(diag);
    }
    for deprecation in oracle
        .extra("data_deprecations")
        .map(Json::items)
        .unwrap_or_default()
    {
        recorder
            .deprecations
            .push(deprecation.as_str().unwrap_or_default().to_owned());
    }
    let _ = env.take_notices();
    Ok(conversion.value)
}

/// `get_debug_type()` of a non-array context.
fn debug_type(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(_) => "bool".to_owned(),
        Value::Int(_) => "int".to_owned(),
        Value::Float(_) => "float".to_owned(),
        Value::String(_) => "string".to_owned(),
        Value::Array(_) => "array".to_owned(),
        Value::Object(object) => object.class().name().to_string(),
    }
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

/// An error of a class that is not a Twig error.
fn plain_error(class: &str, message: Vec<u8>) -> ErrorDesc {
    ErrorDesc {
        class: class.to_owned(),
        message,
        twig: None,
        code: None,
        previous: None,
    }
}

/// `describe_throwable()` (lib.php) of a twig-rs error: Twig errors with their template name, line
/// and column, others with their PHP class; `previous` up to 5 levels deep.
pub fn describe(error: &twig::Error, depth: usize) -> ErrorDesc {
    let class = error.class_name().to_owned();
    if error.kind().is_twig_error() {
        let column = error.template_column().and_then(|c| i64::try_from(c).ok());
        ErrorDesc {
            class,
            message: error.message().into_bytes(),
            twig: Some(TwigErrorInfo {
                raw_message: error.raw_message().as_bytes().to_vec(),
                template_name: error.source_context().map(|s| s.name().to_owned()),
                template_line: error.template_line(),
                template_column: column,
            }),
            code: None,
            previous: if depth < 5 {
                error
                    .previous()
                    .map(|p| Box::new(describe_dyn(p, depth + 1)))
            } else {
                None
            },
        }
    } else {
        match error.php_error() {
            Some(php) => describe_php(php, depth),
            None => ErrorDesc {
                class,
                message: error.raw_message().as_bytes().to_vec(),
                twig: None,
                code: None,
                previous: if depth < 5 {
                    error
                        .previous()
                        .map(|p| Box::new(describe_dyn(p, depth + 1)))
                } else {
                    None
                },
            },
        }
    }
}

fn describe_php(error: &PhpError, depth: usize) -> ErrorDesc {
    ErrorDesc {
        class: error.class().name().to_owned(),
        message: PhpString::as_bytes(error.message()).to_vec(),
        twig: None,
        code: None,
        previous: if depth < 5 {
            error
                .previous()
                .map(|p| Box::new(describe_php(p, depth + 1)))
        } else {
            None
        },
    }
}

fn describe_dyn(
    error: &(dyn std::error::Error + Send + Sync + 'static),
    depth: usize,
) -> ErrorDesc {
    if let Some(twig) = error.downcast_ref::<twig::Error>() {
        return describe(twig, depth);
    }
    if let Some(php) = error.downcast_ref::<PhpError>() {
        return describe_php(php, depth);
    }
    plain_error("Exception", error.to_string().into_bytes())
}
