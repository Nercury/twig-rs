//! The declared return types of the ported test classes (`ToStringStub::__toString(): string`,
//! `CountableStub::count(): int`) against PHP 8.5.13 (`tests/vectors/returns.json`): coercions,
//! their diagnostics, and the `TypeError` that chains an exception the coercion raised.
//!
//! Regenerate the vectors with the Docker oracle (`research/php-oracle`, image `twig-oracle:3.31.1`):
//!
//! ```text
//! cd crates/twig-test-support/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/returns.gen.php > returns.json
//! ```

use serde_json::{Value as Json, json};
use twig_test_support::classes::userland::{UserClass, UserObject};
use twig_value::object::{CallArgs, ObjectRef};
use twig_value::{Array, Diagnostic, Diagnostics, PhpError, PhpResult, Throw, Value};

fn load() -> Json {
    let path = format!("{}/tests/vectors/returns.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("cannot parse {path}: {e}"))
}

/// A ported class built by its constructor.
fn construct(class: UserClass, args: Vec<Value>) -> ObjectRef {
    let object = ObjectRef::new(UserObject::new_without_constructor(class, ""));
    object
        .invoke_method("__construct", CallArgs::from(args), &mut Vec::new())
        .unwrap();
    object
}

/// The values of `returns.gen.php` by label.
fn value(label: &str) -> Value {
    match label {
        "null" => Value::Null,
        "true" => Value::Bool(true),
        "int" => Value::Int(7),
        "float" => Value::Float(1.5),
        "nan" => Value::Float(f64::NAN),
        "string" => Value::from("s"),
        "numeric" => Value::from("1.5"),
        "array" => Value::Array(Array::from_values([Value::Int(1)])),
        "countable" => Value::Object(construct(UserClass::CountableStub, vec![Value::Int(1)])),
        "tostring" => Value::Object(construct(
            UserClass::ToStringStub,
            vec![Value::from("inner")],
        )),
        "foo" => Value::Object(ObjectRef::new(UserObject::new_without_constructor(
            UserClass::TwigTestFoo,
            "",
        ))),
        other => panic!("unknown value {other}"),
    }
}

/// `(string) new ToStringStub($v)` or `(new CountableStub($v))->count()`.
fn run(op: &str, v: Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    match op {
        "string" => construct(UserClass::ToStringStub, vec![v])
            .to_php_string(diag)
            .map(Value::String),
        "count" => construct(UserClass::CountableStub, vec![v]).invoke_method(
            "count",
            CallArgs::new(),
            diag,
        ),
        other => panic!("unknown op {other}"),
    }
}

fn chain(e: &PhpError) -> Json {
    let mut chain = Vec::new();
    let mut e = Some(e);
    while let Some(error) = e {
        chain.push(json!([error.class().name(), error.message_lossy()]));
        e = error.previous();
    }
    Json::Array(chain)
}

#[test]
fn declared_return_types() {
    let v = load();
    let mut failures = Vec::new();
    let cases = v["cases"].as_array().expect("cases");
    for case in cases {
        let mode = case["mode"].as_str().expect("mode");
        let op = case["op"].as_str().expect("op");
        let label = case["value"].as_str().expect("value");
        let mut collected: Vec<Diagnostic> = Vec::new();
        let mut throwing = Throw::error_exception();
        let diag: &mut dyn Diagnostics = if mode == "throw" {
            &mut throwing
        } else {
            &mut collected
        };
        let result = match run(op, value(label), diag) {
            Ok(Value::String(s)) => json!({ "ok": { "string": s.to_string_lossy() } }),
            Ok(Value::Int(i)) => json!({ "ok": { "int": i } }),
            Ok(other) => json!({ "ok": { "other": format!("{other:?}") } }),
            Err(e) => json!({ "err": chain(&e) }),
        };
        let diagnostics: Vec<Json> = collected
            .iter()
            .map(|d| Json::String(d.message.to_string_lossy().into_owned()))
            .collect();
        let got = json!({ "result": result, "diagnostics": diagnostics });
        let want = json!({ "result": case["result"], "diagnostics": case["diagnostics"] });
        if got != want {
            failures.push(format!("{mode} {op} {label}: got {got}, want {want}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ from PHP:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
