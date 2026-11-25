//! The misc callables against Twig 3.31.1 on PHP 8.5.13: `abs`, `round`, `number_format`, `date`, `date_modify`, the
//! `date` function, `constant`, `enum`, `enum_cases`, the inline core tests (`even`, `odd`, `divisible by`, `same as`,
//! `null`/`none`, `true`, `defined`, `constant`), `dump`, `template_from_string` and `random`, rendered by
//! [`twig::runtime::Environment`] and compared with what the oracle rendered (output, error class and message with
//! template name and line, PHP diagnostics in order).
//!
//! The cases are oracle batches in `tests/vectors/callables/misc/<topic>.gen.json`, written by
//! `tests/vectors/callables/misc/misc.gen.py`; the results are in `<topic>.json`. Every case renders `index.twig` in a
//! plain environment (Twig's defaults, autoescape off unless a case turns it on) and collects diagnostics. Values
//! JSON cannot express are tagged objects (`{"$date": [...]}`, see the generator) built here with the same PHP
//! classes as the `data_php` of the oracle (`TwigRs*`, below). The `settings` batch (the configuration of the
//! `CoreExtension`) runs in the unit tests of `src/extension/runtime/misc`.
//!
//! A case marked `pending` depends on work outside the misc callables (named in the marker); it may fail, and is
//! listed on stderr when it passes.
//!
//! Comparison rules for what twig-rs cannot reproduce (reference/deviations.md): object handles (`#N` in dumps) are
//! not compared, and PHP's `, called in <file> on line N` suffix of userland `TypeError`s and the ` in <file> on line
//! N` of `Too few arguments` messages are removed from the oracle's messages.
//!
//! Regenerate the batches and their results from the repository root (Git Bash on Windows needs
//! `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 python -I tests/vectors/callables/misc/misc.gen.py --oracle [topic ...]
//! ```

#[path = "common/mod.rs"]
mod common;

use std::sync::{Arc, LazyLock};

use twig::escaper::DefaultStrategy;
use twig::extension::{DebugExtension, StringLoaderExtension};
use twig::loader::ArrayLoader;
use twig::runtime::{DiagnosticsPolicy, Environment, EnvironmentOptions, Markup, Notice};
use twig::value::object::{
    ArrayObject, CallArgs, ClassBuilder, ClassRef, EnumBacking, Object, ObjectRef, PropertyStore,
    Visibility, builtin,
};
use twig::value::{
    Array, ArrayKey, Diagnostic, Diagnostics, Level, PhpError, PhpResult, PhpString, Value,
};
use twig_date::{DateObject, IntervalObject, TimeZoneObject, classes};

use common::{Json, base64_decode};

// ---- the PHP test classes of the batches ---------------------------------------------------------------------------

static IFACE: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::interface("TwigRsIface")
        .constant("IFACE", "iface")
        .build()
});

static CONSTS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsConsts")
        .implements(&IFACE)
        .constant("FOO", "foo")
        .constant("NUM", 3)
        .constant("ARR", Array::from_values([Value::Int(1), Value::Int(2)]))
        .constant_with_visibility("PRIV", "priv", Visibility::Private)
        .constant_with_visibility("PROT", "prot", Visibility::Protected)
        .build()
});

static PLAIN: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsPlain")
        .property("p", "pp")
        .build()
});

static STRING: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsString")
        .implements(&builtin::stringable())
        .method("__construct")
        .method("__toString")
        .build()
});

static SUIT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::enumeration("TwigRsSuit", None)
        .case("Hearts", None)
        .case("Spades", None)
        .build()
});

static STATUS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::enumeration("TwigRsStatus", Some(EnumBacking::String))
        .case("Active", Some(Value::from("active")))
        .case("Inactive", Some(Value::from("inactive")))
        .build()
});

static EMPTY: LazyLock<ClassRef> =
    LazyLock::new(|| ClassBuilder::enumeration("TwigRsEmpty", None).build());

/// An instance of a class with declared properties only (`TwigRsConsts`, `TwigRsPlain`).
struct Instance {
    class: ClassRef,
    properties: PropertyStore,
}

impl Instance {
    fn value(class: &ClassRef) -> Value {
        Value::Object(ObjectRef::new(Instance {
            class: class.clone(),
            properties: PropertyStore::new(class),
        }))
    }
}

impl Object for Instance {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }
}

/// `new TwigRsString($s)`: a `Stringable` object.
struct Stringy(PhpString);

impl Object for Stringy {
    fn class(&self) -> &ClassRef {
        &STRING
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__tostring" => Ok(Value::String(self.0.clone())),
            other => Err(PhpError::error(format!(
                "Call to undefined method TwigRsString::{other}()"
            ))),
        }
    }
}

// ---- values ----------------------------------------------------------------------------------------------------------

fn hex_decode(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
        .collect()
}

/// A `$date` spec: `[class, time, zone|null]`.
fn date(env: &Environment, spec: &Json) -> Value {
    let parts = spec.as_array();
    let ctx = env.date_context();
    let class = match parts[0].as_str() {
        Some("DateTimeImmutable") => classes::datetime_immutable(),
        _ => classes::datetime(),
    };
    let zone = parts[2]
        .as_str()
        .map(|zone| TimeZoneObject::construct(ctx, zone.as_bytes()).expect("timezone"));
    let time = parts[1].as_str().expect("time");
    Value::Object(DateObject::construct(ctx, class, time.as_bytes(), zone.as_ref()).expect("date"))
}

/// A value of a case: JSON as `json_decode($json, true)` reads it, or a tagged object (see the generator).
fn value(env: &Environment, json: &Json) -> Value {
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
        Json::Array(items) => Value::Array(Array::from_values(
            items.iter().map(|item| value(env, item)),
        )),
        Json::Object(fields) if fields.len() == 1 && fields[0].0.starts_with('$') => {
            let (tag, arg) = (&fields[0].0, &fields[0].1);
            let ctx = env.date_context();
            match tag.as_str() {
                "$date" => date(env, arg),
                "$tz" => Value::Object(
                    TimeZoneObject::construct(ctx, arg.as_str().expect("zone").as_bytes())
                        .expect("timezone"),
                ),
                "$interval" => Value::Object(
                    IntervalObject::construct(ctx, arg.as_str().expect("interval").as_bytes())
                        .expect("interval"),
                ),
                "$diff" => {
                    let parts = arg.as_array();
                    let (a, b) = (date(env, &parts[0]), date(env, &parts[1]));
                    let Value::Object(a) = a else { unreachable!() };
                    a.call_method(
                        "diff",
                        CallArgs::from(vec![b]),
                        &mut Vec::<Diagnostic>::new(),
                    )
                    .expect("diff")
                }
                "$float" => Value::Float(match arg.as_str() {
                    Some("NAN") => f64::NAN,
                    Some("INF") => f64::INFINITY,
                    Some("-INF") => f64::NEG_INFINITY,
                    _ => -0.0,
                }),
                "$enum" => {
                    let parts = arg.as_array();
                    let (class, case) = (
                        parts[0].as_str().expect("enum"),
                        parts[1].as_str().expect("case"),
                    );
                    let class = env
                        .with_constants(|constants| constants.class(class).cloned())
                        .expect("registered enum");
                    Value::Object(class.enum_case(case).expect("enum case"))
                }
                "$object" => match arg.as_str() {
                    Some("TwigRsConsts") => Instance::value(&CONSTS),
                    Some("TwigRsPlain") => Instance::value(&PLAIN),
                    other => panic!("unknown test class {other:?}"),
                },
                "$string" => Value::Object(ObjectRef::new(Stringy(PhpString::from(
                    arg.as_str().expect("string"),
                )))),
                "$markup" => {
                    Markup::new_value(PhpString::from(arg.as_str().expect("markup")), "UTF-8")
                }
                "$arrayobject" => {
                    let Value::Array(array) = value(env, arg) else {
                        panic!("ArrayObject of an array")
                    };
                    Value::Object(ArrayObject::new(array).into_ref())
                }
                "$bytes" => Value::String(PhpString::from(hex_decode(arg.as_str().expect("hex")))),
                other => panic!("unknown value tag {other}"),
            }
        }
        Json::Object(fields) => {
            let mut array = Array::new();
            for (key, field) in fields {
                array.insert(
                    ArrayKey::from_string(PhpString::from(key.as_str())),
                    value(env, field),
                );
            }
            Value::Array(array)
        }
    }
}

// ---- running a case ----------------------------------------------------------------------------------------------

/// The environment of a case: `new Environment(new ArrayLoader($templates), $config)` plus its extensions, globals,
/// PHP default timezone and the test classes.
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
            "debug" => options.debug = matches!(value, Json::Bool(true)),
            "charset" => options.charset = value.as_str().expect("charset").to_owned(),
            other => panic!("unsupported config option {other}"),
        }
    }
    options.diagnostics = DiagnosticsPolicy::Collect;
    let templates: Vec<(String, Vec<u8>)> = case
        .field("templates")
        .as_object()
        .iter()
        .map(|(name, code)| (name.clone(), code.as_bytes()))
        .collect();
    let env = Environment::with_options(ArrayLoader::new(templates), options);
    for extension in case
        .get("extensions")
        .map(Json::as_array)
        .unwrap_or_default()
    {
        match extension.as_str() {
            Some("debug") => env
                .add_extension(Arc::new(DebugExtension::new()))
                .expect("debug extension"),
            Some("string_loader") => env
                .add_extension(Arc::new(StringLoaderExtension::new()))
                .expect("string loader extension"),
            other => panic!("unsupported extension {other:?}"),
        }
    }
    if let Some(globals) = case.get("globals") {
        for (name, global) in globals.as_object() {
            env.add_global(name, value(&env, global)).expect("global");
        }
    }
    if let Some(zone) = case.get("timezone").and_then(Json::as_str) {
        assert!(
            env.date_context()
                .default_timezone_set(zone.as_bytes(), &mut Vec::<Diagnostic>::new())
                .expect("timezone"),
            "valid timezone {zone}"
        );
    }
    for class in [
        &*IFACE, &*CONSTS, &*PLAIN, &*STRING, &*SUIT, &*STATUS, &*EMPTY,
    ] {
        env.add_class(class.clone()).expect("test class");
    }
    env
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

/// `#123` object handles become `#N`.
fn without_handles(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        out.push(bytes[i]);
        if bytes[i] == b'#' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            out.push(b'N');
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    out
}

/// Removes what PHP appends to messages about its own files: `, called in <file> on line N` (userland `TypeError`s)
/// and ` in <file> on line N` after `passed` (`Too few arguments`).
fn without_php_locations(message: &str) -> String {
    let mut message = message.to_owned();
    while let Some(start) = message.find(", called in ") {
        let Some(line) = message[start..].find(" on line ") else {
            break;
        };
        let mut end = start + line + " on line ".len();
        end += message[end..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        message.replace_range(start..end, "");
    }
    if let Some(start) = message.find(" passed in ")
        && let Some(line) = message[start..].find(" on line ")
    {
        let from = start + " passed".len();
        let mut end = start + line + " on line ".len();
        end += message[end..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        message.replace_range(from..end, "");
    }
    message
}

/// Runs one case; returns a description of the mismatch, if any.
fn run_case(case: &Json, expected: &Json) -> Option<String> {
    let env = environment(case);
    let mut context = Array::new();
    if let Some(data) = case.get("data") {
        for (name, spec) in data.as_object() {
            context.insert(name.as_str(), value(&env, spec));
        }
    }
    let result = env
        .load("index.twig")
        .and_then(|template| template.render_bytes(context));
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
            let want = match expected.get("output_b64").and_then(Json::as_str) {
                Some(b64) => base64_decode(b64),
                None => expected.field("output").as_bytes(),
            };
            if without_handles(output) != without_handles(&want) {
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
            let message = without_php_locations(&want.field("message").as_lossy_string());
            let got = format!("{}: {}", error.kind().class_name(), error);
            let wanted = format!("{class}: {message}");
            if got != wanted {
                problems.push(format!("error {got:?}, expected {wanted:?}"));
            }
        }
        (Ok(output), _) => problems.push(format!(
            "rendered {:?}, expected error {:?}",
            String::from_utf8_lossy(output),
            expected
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Json::as_str)
                .unwrap_or("?")
        )),
        (Err(error), _) => problems.push(format!(
            "error {}: {error}, expected output {:?}",
            error.kind().class_name(),
            expected.get("output").and_then(Json::as_str).unwrap_or("?")
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

/// Runs every case of a topic and fails with the list of mismatches (pending cases may fail).
fn run_topic(topic: &str) {
    let cases = Json::load_vectors(&format!("callables/misc/{topic}.gen.json"));
    let results = Json::load_vectors(&format!("callables/misc/{topic}.json"));
    let cases = cases.field("cases").as_array();
    let results = results.as_array();
    assert_eq!(
        cases.len(),
        results.len(),
        "{topic}: cases and results differ in number"
    );
    let mut failures = Vec::new();
    let mut pending_passing = Vec::new();
    for (case, expected) in cases.iter().zip(results) {
        let id = case.field("id").as_str().unwrap_or_default();
        assert_eq!(expected.field("id").as_str(), Some(id), "result order");
        let problem = run_case(case, expected);
        match (problem, case.get("pending")) {
            (Some(problem), None) => {
                let templates: Vec<String> = case
                    .field("templates")
                    .as_object()
                    .iter()
                    .map(|(name, code)| format!("{name}: {:?}", code.as_lossy_string()))
                    .collect();
                failures.push(format!("{id} [{}]\n    {problem}", templates.join(", ")));
            }
            (None, Some(reason)) => {
                pending_passing.push(format!("{id} ({})", reason.as_lossy_string()))
            }
            _ => {}
        }
    }
    if !pending_passing.is_empty() {
        eprintln!(
            "{topic}: pending cases that pass now:\n  {}",
            pending_passing.join("\n  ")
        );
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
fn numbers() {
    run_topic("numbers");
}

#[test]
fn dates() {
    run_topic("dates");
}

#[test]
fn constants() {
    run_topic("constants");
}

#[test]
fn tests() {
    run_topic("tests");
}

#[test]
fn debug() {
    run_topic("debug");
}

#[test]
fn string_loader() {
    run_topic("string_loader");
}

#[test]
fn random() {
    run_topic("random");
}
