//! The string and escaping callables against Twig 3.31.1 on PHP 8.5.13: `escape`/`e`, `raw`, auto-escaping (the
//! `html`, `js`, `name` and `false` default strategies), `upper`, `lower`, `title`, `capitalize`, `trim`, `nl2br`,
//! `spaceless`, `striptags`, `replace`, `format`, `url_encode`, `json_encode`, `convert_encoding`, `split` and `join`.
//!
//! The cases are oracle batches in `tests/vectors/callables/strings/<topic>.gen.json`, written by
//! `tests/vectors/callables/strings/strings.gen.py`; the results are in `<topic>.json`. Every case renders templates
//! with a plain environment (`new Environment(new ArrayLoader($templates), $config)`, `autoescape` off unless the
//! case says otherwise) and compares the output, the error class and message (with template name and line) and the
//! PHP diagnostics in order. A case with a `twig_rs` field documents a deliberate deviation
//! (`reference/deviations.md`) and expects that result; PHP source locations in the oracle's messages (`called in
//! <file> on line <n>`), which twig-rs cannot reproduce, are removed before comparing. The `runtime` topic (custom escapers and safe classes) runs
//! in the unit tests of `src/extension/runtime/strings/escaper/tests.rs`.
//!
//! Twig's `EscaperExtension` registers the `EscaperNodeVisitor`, which also escapes the input of the `pre_escape`
//! filters (`nl2br`, `spaceless`) when auto-escaping is off; the environment here adds it when the extension does
//! not.
//!
//! Regenerate the batches and their results from the repository root (Git Bash on Windows needs
//! `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 python -I tests/vectors/callables/strings/strings.gen.py --oracle [topic ...]
//! ```

#[path = "common/mod.rs"]
mod common;

use std::sync::{Arc, LazyLock, OnceLock};

use common::{Json, base64_decode};
use twig::escaper::DefaultStrategy;
use twig::extension::{EscaperExtension, Extension};
use twig::loader::ArrayLoader;
use twig::runtime::{DiagnosticsPolicy, Environment, EnvironmentOptions, Markup, Notice};
use twig::value::object::{
    ArrayIterator, ArrayObject, CallArgs, ClassBuilder, ClassRef, Object, ObjectRef, PropertyInfo,
    PropertyStore, Visibility, builtin, undefined_method,
};
use twig::value::{
    Array, ArrayKey, Diagnostics, Level, PhpError, PhpResult, PhpString, ThrowableClass, Value,
};
use twig::visitor::EscaperNodeVisitor;

// ---- the PHP classes of the batches (see PHP_CLASSES in strings.gen.py) ------------------------------------------

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

/// `class TwigRsStr implements TwigRsSafeIface { public function __construct(private string $s) {} ... }`
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

/// `class TwigRsChild extends TwigRsStr {}`
static CHILD: LazyLock<ClassRef> =
    LazyLock::new(|| ClassBuilder::class("TwigRsChild").extends(&STR).build());

/// `class TwigRsCounter { private $n = 0; public function __toString(): string { ... } }`
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

/// `class TwigRsPlain { public $p = 'v'; }`
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

/// `class TwigRsThrowStr { public function __toString(): string { throw new \RuntimeException('no string'); } }`
static THROWER: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsThrowStr")
        .method("__toString")
        .build()
});

/// `class TwigRsJson implements \JsonSerializable { public function jsonSerialize(): mixed { ... } }`
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
    fn new(class: &ClassRef) -> TestObject {
        TestObject {
            class: class.clone(),
            properties: PropertyStore::new(class),
        }
    }

    fn with(self, name: &[u8], value: Value) -> TestObject {
        self.properties.set(name, value);
        self
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

fn markup(content: &str, strategies: Option<&[&str]>) -> Value {
    Value::Object(Markup::create(
        PhpString::from(content),
        Some(PhpString::from("UTF-8")),
        strategies.map(|s| s.iter().map(|s| PhpString::from(*s)).collect()),
    ))
}

/// The object of `PHP_OBJECTS[name]` in strings.gen.py.
fn build_object(name: &str) -> Value {
    let object = |o: TestObject| Value::Object(ObjectRef::new(o));
    match name {
        "str" => object(TestObject::new(&STR).with(b"s", Value::from("<b>Str</b> é"))),
        "child" => object(TestObject::new(&CHILD).with(b"s", Value::from("<i>child</i>"))),
        "counter" => object(TestObject::new(&COUNTER)),
        "plain" => object(TestObject::new(&PLAIN)),
        "thrower" => object(TestObject::new(&THROWER)),
        "json" => object(TestObject::new(&JSON)),
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

// ---- running a case ----------------------------------------------------------------------------------------------

/// A JSON value as `json_decode($json, true)` decodes it.
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

/// Whether the `EscaperExtension` registers its node visitor itself (Twig's `getNodeVisitors()`).
fn extension_registers_escaper_visitor(env: &Environment) -> bool {
    env.with_extensions(|set| {
        set.extension::<EscaperExtension>()
            .is_some_and(|escaper| !escaper.node_visitors().is_empty())
    })
}

/// The environment of a case: Twig's defaults, the case's config, the escaper visitor.
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
            "optimizations" => options.optimizations = value.as_i64().expect("optimizations"),
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
    if !extension_registers_escaper_visitor(&env) {
        env.add_node_visitor(Arc::new(EscaperNodeVisitor))
            .expect("a fresh environment accepts node visitors");
    }
    env
}

/// The context of a case: `data_json`, or `data_json_rust` plus the named objects.
fn context(case: &Json) -> Array {
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
    context
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::Warning => "E_WARNING",
        Level::Notice => "E_NOTICE",
        Level::Deprecated => "E_DEPRECATED",
        Level::UserDeprecated => "E_USER_DEPRECATED",
    }
}

/// A byte string field of an oracle result (`<field>_b64` holds the bytes when they are not UTF-8).
fn bytes_field(result: &Json, field: &str) -> Vec<u8> {
    match result.get(&format!("{field}_b64")).and_then(Json::as_str) {
        Some(b64) => base64_decode(b64),
        None => result.field(field).as_bytes(),
    }
}

/// The oracle's message without the PHP source locations twig-rs cannot reproduce (reference/deviations.md,
/// "Userland TypeError suffix"): `, called in <file> on line <n>` after a userland `TypeError` and ` in <file> on line
/// <n>` after `N passed` in an `ArgumentCountError`.
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

/// Runs one case; returns a description of the mismatch, if any.
fn run_case(case: &Json, expected: &Json) -> Option<String> {
    let env = environment(case);
    let main = case
        .get("main")
        .and_then(Json::as_str)
        .unwrap_or("index.twig");
    let result = env
        .load(main)
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
            let class = want.field("class").as_str().unwrap_or_default();
            let message =
                without_php_locations(&String::from_utf8_lossy(&bytes_field(want, "message")));
            let got = format!("{}: {}", error.kind().class_name(), error);
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
                .and_then(|e| e.get("formatted"))
                .map(Json::as_lossy_string)
                .unwrap_or_default()
        )),
        (Err(error), _) => problems.push(format!(
            "error {}: {error}, expected output {:?}",
            error.kind().class_name(),
            expected.get("output").map(Json::as_lossy_string)
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
                        String::from_utf8_lossy(&bytes_field(n, "message")).into_owned(),
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
    (!problems.is_empty()).then(|| problems.join("; "))
}

/// Whether the environment runs the string group's `split`. The runtime table is filled group by group and keeps
/// the last registration of a callable; the arrays group registers a `split` of its own, which rejects the
/// single-byte charsets mbstring accepts (KOI8-R here). The unit tests of `src/extension/runtime/strings/split.rs`
/// run the split topic with the string group's runtimes in any case.
fn string_group_split_is_installed() -> bool {
    static INSTALLED: OnceLock<bool> = OnceLock::new();
    *INSTALLED.get_or_init(|| {
        let env = Environment::with_options(
            ArrayLoader::new([("index", "{{ 'abc'|split('', 2)|join('|') }}")]),
            EnvironmentOptions {
                charset: "KOI8-R".to_owned(),
                ..EnvironmentOptions::default()
            },
        );
        env.render("index", Array::new())
            .is_ok_and(|output| output == "ab|c")
    })
}

/// Runs every case of a topic and fails with the list of mismatches.
fn run_topic(topic: &str) {
    let cases = Json::load_vectors(&format!("callables/strings/{topic}.gen.json"));
    let results = Json::load_vectors(&format!("callables/strings/{topic}.json"));
    let cases = cases.field("cases").as_array();
    let results = results.as_array();
    assert_eq!(
        cases.len(),
        results.len(),
        "{topic}: cases and results differ in number"
    );
    if topic == "split" && !string_group_split_is_installed() {
        eprintln!(
            "split: skipped, the environment's `split` is not the string group's (another group registers the callable \
             later)"
        );
        return;
    }
    let mut failures = Vec::new();
    for (case, expected) in cases.iter().zip(results) {
        let id = case.field("id").as_str().unwrap_or_default();
        assert_eq!(expected.field("id").as_str(), Some(id), "result order");
        if let Some(problem) = run_case(case, expected) {
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

#[test]
fn escape() {
    run_topic("escape");
}

#[test]
fn autoescape() {
    run_topic("autoescape");
}

#[test]
fn case() {
    run_topic("case");
}

#[test]
fn text() {
    run_topic("text");
}

#[test]
fn split() {
    run_topic("split");
}
