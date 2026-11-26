//! `SplObjectStorage::seek()` of the SPL port against PHP 8.5.13 (`tests/vectors/spl_seek.json`):
//! results, exceptions, diagnostics and the iterator state after every step.
//!
//! Regenerate the vectors with the Docker oracle (`research/php-oracle`, image `twig-oracle:3.31.1`):
//!
//! ```text
//! cd crates/twig-test-support/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/spl_seek.gen.php > spl_seek.json
//! ```

use serde_json::{Value as Json, json};
use twig_test_support::classes::spl::SplObjectStorage;
use twig_value::object::{CallArgs, ObjectRef, StdClass};
use twig_value::{Diagnostic, FetchMode, PhpResult, Value};

fn load() -> Json {
    let path = format!("{}/tests/vectors/spl_seek.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("cannot parse {path}: {e}"))
}

fn arg(j: &Json) -> Value {
    match j {
        Json::Null => Value::Null,
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().expect("number")),
        },
        Json::String(s) => Value::from(s.as_str()),
        other => panic!("unexpected argument {other}"),
    }
}

fn to_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => json!(b),
        Value::Int(i) => json!(i),
        Value::String(s) => json!(s.to_string_lossy()),
        other => panic!("unexpected value {other:?}"),
    }
}

fn outcome(r: PhpResult<Value>) -> Json {
    match r {
        Ok(v) => json!({ "ok": to_json(&v) }),
        Err(e) => json!({ "err": [e.class().name(), e.message_lossy()] }),
    }
}

/// `valid()`, `key()`, `current()->n` (or the exception) and `getInfo()`.
fn state(storage: &ObjectRef) -> Json {
    let mut d: Vec<Diagnostic> = Vec::new();
    let call = |m: &str, d: &mut Vec<Diagnostic>| storage.invoke_method(m, CallArgs::new(), d);
    let valid = call("valid", &mut d).unwrap();
    let key = call("key", &mut d).unwrap();
    let current = match call("current", &mut d) {
        Ok(Value::Object(o)) => to_json(&o.read_property(b"n", FetchMode::Read, &mut d).unwrap()),
        Ok(other) => panic!("current() returned {other:?}"),
        Err(e) => json!(format!("{}: {}", e.class().name(), e.message_lossy())),
    };
    let info = call("getinfo", &mut d).unwrap();
    assert!(d.is_empty(), "state reads raised {d:?}");
    json!({ "valid": to_json(&valid), "key": to_json(&key), "current": current, "info": to_json(&info) })
}

#[test]
fn seek() {
    let v = load();
    let mut failures = Vec::new();
    let mut checks = 0;
    for scenario in v.as_array().expect("scenarios") {
        let name = scenario["name"].as_str().expect("name");
        let n = scenario["n"].as_i64().expect("n");
        let storage = SplObjectStorage::new().into_ref();
        let mut objects = Vec::new();
        for i in 0..n {
            let o = ObjectRef::new(StdClass::new());
            o.write_property(b"n", Value::Int(i), &mut Vec::new())
                .unwrap();
            let args = CallArgs::from(vec![Value::Object(o.clone()), Value::from(format!("i{i}"))]);
            storage
                .invoke_method("offsetset", args, &mut Vec::new())
                .unwrap();
            objects.push(o);
        }
        storage
            .invoke_method("rewind", CallArgs::new(), &mut Vec::new())
            .unwrap();
        for step in scenario["steps"].as_array().expect("steps") {
            let op = step["op"].as_array().expect("op");
            let mut diag: Vec<Diagnostic> = Vec::new();
            let args: Vec<Value> = op[1..].iter().map(arg).collect();
            let result = match op[0].as_str().expect("op name") {
                "seek" => storage.invoke_method("seek", CallArgs::from(args), &mut diag),
                "seek_named" => storage.invoke_method(
                    "seek",
                    CallArgs::new().named_arg("offset", args[0].clone()),
                    &mut diag,
                ),
                "rewind" => storage.invoke_method("rewind", CallArgs::new(), &mut diag),
                "next" => storage.invoke_method("next", CallArgs::new(), &mut diag),
                "detach" => {
                    let index = op[1].as_u64().expect("index") as usize;
                    let args = CallArgs::from(vec![Value::Object(objects[index].clone())]);
                    storage.invoke_method("offsetunset", args, &mut diag)
                }
                other => panic!("unknown op {other}"),
            };
            let diagnostics: Vec<Json> = diag
                .iter()
                .map(|d| json!(d.message.to_string_lossy()))
                .collect();
            let got = json!({
                "result": outcome(result),
                "diagnostics": diagnostics,
                "state": state(&storage),
            });
            let want = json!({
                "result": step["result"],
                "diagnostics": step["diagnostics"],
                "state": step["state"],
            });
            checks += 1;
            if got != want {
                failures.push(format!("{name} {op:?}: got {got}, want {want}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {checks} steps differ from PHP:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(checks > 0);
}
