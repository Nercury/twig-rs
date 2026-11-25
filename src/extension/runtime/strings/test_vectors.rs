//! Test support: runs the oracle batches of `tests/vectors/callables/strings/` (written by `strings.gen.py` there)
//! inside the crate, where the string group's runtimes can be installed directly. `tests/callables_strings.rs` runs
//! the same batches through the public API with the environment's own runtimes.

#[path = "../../../../tests/common/mod.rs"]
mod common;

use std::sync::{Arc, LazyLock};

use common::{Json, base64_decode};
use twig_value::object::{
    ArrayIterator, ArrayObject, CallArgs, ClassBuilder, ClassRef, Object, ObjectRef, PropertyInfo,
    PropertyStore, Visibility, builtin, undefined_method,
};
use twig_value::{
    Array, ArrayKey, Diagnostics, Level, PhpError, PhpResult, PhpString, ThrowableClass, Value,
};

use crate::escaper::DefaultStrategy;
use crate::extension::{EscaperExtension, Extension};
use crate::loader::ArrayLoader;
use crate::runtime::{DiagnosticsPolicy, Environment, EnvironmentOptions, Markup, Notice};
use crate::visitor::EscaperNodeVisitor;

// ---- the PHP classes of the batches (PHP_CLASSES in strings.gen.py) ----------------------------------------------

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

static SAFE_IFACE: LazyLock<ClassRef> =
    LazyLock::new(|| ClassBuilder::interface("TwigRsSafeIface").build());
static STR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsStr")
        .implements(&SAFE_IFACE)
        .property_info(PropertyInfo {
            type_name: Some("string".into()),
            ..property("s", Visibility::Private, None, "TwigRsStr")
        })
        .method("__construct")
        .method("__toString")
        .build()
});
static CHILD: LazyLock<ClassRef> =
    LazyLock::new(|| ClassBuilder::class("TwigRsChild").extends(&STR).build());
static COUNTER: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsCounter")
        .property_info(property(
            "n",
            Visibility::Private,
            Some(Value::Int(0)),
            "TwigRsCounter",
        ))
        .method("__toString")
        .build()
});
static PLAIN: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsPlain")
        .property_info(property(
            "p",
            Visibility::Public,
            Some(Value::from("v")),
            "TwigRsPlain",
        ))
        .build()
});
static THROWER: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsThrowStr")
        .method("__toString")
        .build()
});
static JSON: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsJson")
        .implements(&builtin::json_serializable())
        .method("jsonSerialize")
        .build()
});

/// An object of one of the classes above.
struct TestObject {
    class: ClassRef,
    properties: PropertyStore,
}

impl TestObject {
    fn value(class: &ClassRef, s: Option<&str>) -> Value {
        let object = TestObject {
            class: class.clone(),
            properties: PropertyStore::new(class),
        };
        if let Some(s) = s {
            object.properties.set(b"s", Value::from(s));
        }
        Value::Object(ObjectRef::new(object))
    }
}

impl Object for TestObject {
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
        match (self.class.name(), method) {
            ("TwigRsStr" | "TwigRsChild", "__tostring") => {
                Ok(self.properties.get(b"s").unwrap_or_default())
            }
            ("TwigRsCounter", "__tostring") => {
                let n = self
                    .properties
                    .get(b"n")
                    .and_then(|n| n.as_int())
                    .unwrap_or(0)
                    + 1;
                self.properties.set(b"n", Value::Int(n));
                Ok(Value::from(format!("x{n}Y")))
            }
            ("TwigRsThrowStr", "__tostring") => {
                Err(PhpError::new(ThrowableClass::RuntimeException, "no string"))
            }
            ("TwigRsJson", "jsonserialize") => {
                let mut array = Array::new();
                array.insert("j", Value::from("<x>"));
                Ok(Value::Array(array))
            }
            _ => Err(undefined_method(&self.class, method)),
        }
    }
}

/// A `Twig\Markup` with the UTF-8 charset.
pub(super) fn markup(content: &str, strategies: Option<&[&str]>) -> Value {
    Value::Object(Markup::create(
        PhpString::from(content),
        Some(PhpString::from("UTF-8")),
        strategies.map(|s| s.iter().map(|s| PhpString::from(*s)).collect()),
    ))
}

/// The object of `PHP_OBJECTS[name]` in strings.gen.py.
fn build_object(name: &str) -> Value {
    match name {
        "str" => TestObject::value(&STR, Some("<b>Str</b> é")),
        "child" => TestObject::value(&CHILD, Some("<i>child</i>")),
        "counter" => TestObject::value(&COUNTER, None),
        "plain" => TestObject::value(&PLAIN, None),
        "thrower" => TestObject::value(&THROWER, None),
        "json" => TestObject::value(&JSON, None),
        "iter" => {
            let mut array = Array::new();
            array.insert("a", Value::from("<x>"));
            array.insert("b", Value::from("y"));
            array.insert(ArrayKey::Int(0), Value::from("z"));
            Value::Object(ArrayIterator::new(array).into_ref())
        }
        "arrobj" => {
            let mut array = Array::new();
            array.insert("k", Value::from("v"));
            array.insert(ArrayKey::Int(1), Value::from("w"));
            Value::Object(ArrayObject::new(array).into_ref())
        }
        "markup" => markup("<b>m</b>", None),
        "markup_html" => markup("<b>h</b>", Some(&["html"])),
        "markup_js" => markup("<b>j</b>", Some(&["js"])),
        "markup_attr" => markup("<b a=\"1\">a</b>", Some(&["html_attr"])),
        "markup_none" => markup("<b>n</b>", Some(&[])),
        "markup_empty" => markup("", None),
        "markup_ws" => markup("  <b>w</b>\n ", None),
        other => panic!("unknown object {other}"),
    }
}

// ---- running the cases ---------------------------------------------------------------------------------------------

/// A JSON value as `json_decode($json, true)` decodes it.
fn json_to_value(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(text) => match text.parse::<i64>() {
            Ok(i) if !text.contains(['.', 'e', 'E']) => Value::Int(i),
            _ => Value::Float(text.parse().expect("a JSON number")),
        },
        Json::String(s) => Value::from(s.as_str()),
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

/// The environment of a case: Twig's defaults, the case's config, the escaper visitor, then `customize`.
fn environment(case: &Json, customize: &dyn Fn(&Environment, &Json)) -> Environment {
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
    customize(&env, case);
    let registered = env.with_extensions(|set| {
        set.extension::<EscaperExtension>()
            .is_some_and(|escaper| !escaper.node_visitors().is_empty())
    });
    if !registered {
        env.add_node_visitor(Arc::new(EscaperNodeVisitor))
            .expect("a fresh environment accepts node visitors");
    }
    env
}

/// The oracle's message without the PHP source locations twig-rs cannot reproduce (`, called in <file> on line
/// <n>`, ` in <file> on line <n>` after `N passed`).
fn without_php_locations(message: &str) -> String {
    let mut out = message.to_owned();
    for (marker, replacement) in [(", called in /", ""), (" passed in /", " passed")] {
        while let Some(start) = out.find(marker) {
            let Some(line) = out[start..].find(" on line ") else {
                break;
            };
            let mut end = start + line + " on line ".len();
            while out.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            out.replace_range(start..end, replacement);
        }
    }
    out
}

/// A byte string field of an oracle result (`<field>_b64` holds the bytes when they are not UTF-8).
fn bytes_field(result: &Json, field: &str) -> Vec<u8> {
    match result.get(&format!("{field}_b64")) {
        Some(b64) => base64_decode(b64.as_str().expect("base64")),
        None => result.field(field).as_bytes(),
    }
}

/// Runs one case; returns a description of the mismatch, if any.
fn run_case(
    case: &Json,
    expected: &Json,
    customize: &dyn Fn(&Environment, &Json),
) -> Option<String> {
    let env = environment(case, customize);
    let main = case
        .get("main")
        .and_then(Json::as_str)
        .unwrap_or("index.twig");
    let mut context = match case.get("data_json").or_else(|| case.get("data_json_rust")) {
        Some(data) => match json_to_value(data) {
            Value::Array(array) => array,
            _ => Array::new(),
        },
        None => Array::new(),
    };
    if let Some(objects) = case.get("objects") {
        for name in objects.as_array() {
            let name = name.as_str().expect("object name");
            context.insert(name, build_object(name));
        }
    }
    let result = env
        .load(main)
        .and_then(|template| template.render_bytes(context));
    let notices: Vec<(String, String)> = env
        .take_notices()
        .into_iter()
        .map(|notice| match notice {
            Notice::Php(diagnostic) => (
                match diagnostic.level {
                    Level::Warning => "E_WARNING",
                    Level::Notice => "E_NOTICE",
                    Level::Deprecated => "E_DEPRECATED",
                    Level::UserDeprecated => "E_USER_DEPRECATED",
                }
                .to_owned(),
                diagnostic.message.to_string_lossy().into_owned(),
            ),
            Notice::Deprecation(deprecation) => {
                ("E_USER_DEPRECATED".to_owned(), deprecation.to_string())
            }
        })
        .collect();
    let mut problems = Vec::new();
    if let Some(deviation) = case.get("twig_rs") {
        let want = deviation.field("error").as_lossy_string();
        match &result {
            Err(error) => {
                let got = format!("{}: {}", error.kind().class_name(), error);
                if got != want {
                    problems.push(format!("error {got:?}, expected (deviation) {want:?}"));
                }
            }
            Ok(output) => problems.push(format!(
                "rendered {:?}, expected (deviation) {want:?}",
                String::from_utf8_lossy(output)
            )),
        }
        return (!problems.is_empty()).then(|| problems.join("; "));
    }
    match (&result, expected.field("ok")) {
        (Ok(output), Json::Bool(true)) => {
            let want = bytes_field(expected, "output");
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
            let wanted = format!(
                "{}: {}",
                want.field("class").as_str().unwrap_or_default(),
                without_php_locations(&String::from_utf8_lossy(&bytes_field(want, "message")))
            );
            let got = format!("{}: {}", error.kind().class_name(), error);
            if got != wanted {
                problems.push(format!("error {got:?}, expected {wanted:?}"));
            }
        }
        (Ok(output), _) => problems.push(format!(
            "rendered {:?}, expected an error",
            String::from_utf8_lossy(output)
        )),
        (Err(error), _) => problems.push(format!("error {error}, expected an output")),
    }
    let want_notices: Vec<(String, String)> = expected
        .field("diagnostics")
        .as_array()
        .iter()
        .map(|n| {
            (
                n.field("type").as_str().unwrap_or_default().to_owned(),
                String::from_utf8_lossy(&bytes_field(n, "message")).into_owned(),
            )
        })
        .collect();
    if notices != want_notices {
        problems.push(format!(
            "diagnostics {notices:?}, expected {want_notices:?}"
        ));
    }
    (!problems.is_empty()).then(|| problems.join("; "))
}

/// Runs every case of a topic in environments prepared by `customize` (called with the environment and the case)
/// and fails with the list of mismatches.
pub(super) fn run_topic(topic: &str, customize: &dyn Fn(&Environment, &Json)) {
    let cases = Json::load_vectors(&format!("callables/strings/{topic}.gen.json"));
    let results = Json::load_vectors(&format!("callables/strings/{topic}.json"));
    let cases = cases.field("cases").as_array();
    let results = results.as_array();
    assert_eq!(cases.len(), results.len(), "{topic}: cases and results");
    let mut failures = Vec::new();
    for (case, expected) in cases.iter().zip(results) {
        let id = case.field("id").as_str().unwrap_or_default();
        assert_eq!(expected.field("id").as_str(), Some(id), "result order");
        if let Some(problem) = run_case(case, expected, customize) {
            let templates: Vec<String> = case
                .field("templates")
                .as_object()
                .iter()
                .map(|(name, code)| format!("{name}: {:?}", code.as_lossy_string()))
                .collect();
            failures.push(format!("{id} [{}]\n    {problem}", templates.join(", ")));
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
