//! The `SimpleXMLElement` port against PHP 8.5.13 (`tests/vectors/simplexml.json`): objects reached from a document
//! root by property reads, dimension reads, `children()` and `attributes()`, and their casts, counts, names, XML,
//! property tables, JSON, iterations, `isset()` / `empty()` / `property_exists()` answers and comparisons.
//!
//! Regenerate the vectors with the Docker oracle (`research/php-oracle`, image `twig-oracle:3.31.1`):
//!
//! ```text
//! cd crates/twig-test-support/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/simplexml.gen.php > simplexml.json
//! ```
//!
//! The steps and finals of a case are explained at the top of `simplexml.gen.php`; this test performs each one with
//! the object handlers PHP's engine would use.

use std::sync::Arc;

use serde_json::{Value as Json, json};
use twig_test_support::classes::simplexml::{SimpleXmlElement, parse_document};
use twig_value::builtins::format::json_encode;
use twig_value::object::{CallArgs, ObjectRef, PropertyCheck, PropertyPurpose};
use twig_value::{Diagnostic, FetchMode, PhpResult, Value, compare, convert};

fn load() -> Json {
    let path = format!(
        "{}/tests/vectors/simplexml.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("cannot parse {path}: {e}"))
}

/// A key of a step or a final (`$o[key]`).
fn key(j: &Json) -> Value {
    match j {
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().expect("number")),
        },
        Json::String(s) => Value::from(s.as_str()),
        other => panic!("unexpected key {other}"),
    }
}

fn object(v: &Value) -> PhpResult<&ObjectRef> {
    match v {
        Value::Object(o) => Ok(o),
        other => panic!("not an object: {other:?}"),
    }
}

fn call(o: &ObjectRef, method: &str, d: &mut Vec<Diagnostic>) -> PhpResult<Value> {
    o.call_method(method, CallArgs::new(), d)
}

/// `describe()` of simplexml.gen.php.
fn describe(v: &Value, d: &mut Vec<Diagnostic>) -> PhpResult<Json> {
    Ok(match v {
        Value::Null => Json::Null,
        Value::Bool(b) => json!(b),
        Value::Int(i) => json!(i),
        Value::Float(f) => json!(f),
        Value::String(s) => json!(s.to_string_lossy()),
        Value::Array(a) => {
            let mut items = Vec::new();
            for (k, x) in a.iter() {
                let k = match k {
                    twig_value::ArrayKey::Int(i) => json!(i),
                    twig_value::ArrayKey::Str(s) => json!(s.to_string_lossy()),
                };
                items.push(json!([k, describe(x, d)?]));
            }
            json!(["array", items])
        }
        Value::Object(o) => {
            let xml = call(o, "asXML", d)?;
            json!([
                "sxe",
                describe(&call(o, "getName", d)?, d)?,
                convert::to_string(v, d)?.to_string_lossy(),
                twig_value::dim::count(v, 0, d)?,
                describe(&xml, d)?
            ])
        }
    })
}

fn step(o: &Value, s: &Json, d: &mut Vec<Diagnostic>) -> PhpResult<Value> {
    let op = s[0].as_str().expect("step op");
    let o = object(o)?;
    match op {
        "p" => o.read_property(s[1].as_str().unwrap().as_bytes(), FetchMode::Read, d),
        "pi" => o.read_property(s[1].as_str().unwrap().as_bytes(), FetchMode::Isset, d),
        "d" => o.read_dimension(&key(&s[1]), FetchMode::Read, d),
        "m" => call(o, s[1].as_str().unwrap(), d),
        other => panic!("unknown step {other}"),
    }
}

fn walk(root: &Value, steps: &Json, d: &mut Vec<Diagnostic>) -> PhpResult<Value> {
    let mut o = root.clone();
    for s in steps.as_array().expect("steps") {
        o = step(&o, s, d)?;
    }
    Ok(o)
}

/// `foreach ($o as $k => $x)`.
fn iterate(o: &ObjectRef, d: &mut Vec<Diagnostic>) -> PhpResult<Json> {
    let mut out = Vec::new();
    let mut it = o.get_iterator(d)?.expect("an iterator");
    it.rewind(d)?;
    while it.valid(d)? {
        let k = it.key(d)?;
        let x = it.current(d)?;
        out.push(json!([describe(&k, d)?, describe(&x, d)?]));
        it.next(d)?;
    }
    Ok(Json::Array(out))
}

/// The `walk` final: the `RecursiveIterator` methods.
fn walk_methods(o: &ObjectRef, d: &mut Vec<Diagnostic>) -> PhpResult<Json> {
    let mut out = Vec::new();
    call(o, "rewind", d)?;
    while convert::to_bool(&call(o, "valid", d)?, d)? {
        out.push(json!([
            describe(&call(o, "key", d)?, d)?,
            describe(&call(o, "current", d)?, d)?,
            describe(&call(o, "hasChildren", d)?, d)?,
            describe(&call(o, "getChildren", d)?, d)?
        ]));
        call(o, "next", d)?;
    }
    out.push(json!([
        describe(&call(o, "valid", d)?, d)?,
        describe(&call(o, "getChildren", d)?, d)?
    ]));
    Ok(Json::Array(out))
}

fn run_final(v: &Value, root: &Value, f: &Json, d: &mut Vec<Diagnostic>) -> PhpResult<Json> {
    let op = f.as_str().or_else(|| f[0].as_str()).expect("final op");
    let name = || f[1].as_str().expect("a name").as_bytes();
    Ok(match op {
        "describe" => describe(v, d)?,
        "string" => json!(convert::to_string(v, d)?.to_string_lossy()),
        "int" => json!(convert::to_long(v, d)?),
        "float" => json!(convert::to_double(v, d)?),
        "bool" => json!(convert::to_bool(v, d)?),
        "count" => json!(twig_value::dim::count(v, 0, d)?),
        "name" => describe(&call(object(v)?, "getName", d)?, d)?,
        "xml" => describe(&call(object(v)?, "asXML", d)?, d)?,
        "array" => {
            let entries = object(v)?.properties_for(PropertyPurpose::ArrayCast, d)?;
            let mut array = twig_value::Array::new();
            for entry in entries {
                if let Some(value) = entry.value {
                    array.insert(entry.key, value);
                }
            }
            describe(&Value::Array(array), d)?
        }
        "debug" => describe(&call(object(v)?, "__debugInfo", d)?, d)?,
        "json" => describe(&json_encode(v, None, None, d)?, d)?,
        "iter" => iterate(object(v)?, d)?,
        "walk" => walk_methods(object(v)?, d)?,
        "isset_p" => json!(object(v)?.has_property(name(), PropertyCheck::Isset, d)?),
        "empty_p" => json!(!object(v)?.has_property(name(), PropertyCheck::NotEmpty, d)?),
        "exists_p" => json!(object(v)?.property_exists(name(), d)?),
        "isset_d" => json!(object(v)?.has_dimension(&key(&f[1]), false, d)?),
        "empty_d" => json!(!object(v)?.has_dimension(&key(&f[1]), true, d)?),
        "eq" => {
            let other = walk(root, &f[1], d)?;
            json!(compare::equals(v, &other, d)?)
        }
        other => panic!("unknown final {other}"),
    })
}

fn messages(d: &[Diagnostic]) -> Json {
    Json::Array(
        d.iter()
            .map(|x| json!(x.message.to_string_lossy()))
            .collect(),
    )
}

/// The same JSON numbers for PHP's floats and twig-rs's (`0.0` and `0`).
fn normalize(j: &Json) -> Json {
    match j {
        Json::Number(n) if n.is_f64() => json!(n.as_f64()),
        Json::Array(items) => Json::Array(items.iter().map(normalize).collect()),
        other => other.clone(),
    }
}

#[test]
fn simplexml_vectors() {
    let cases = load();
    let mut failures = Vec::new();
    let mut checked = 0;
    for (n, case) in cases.as_array().expect("cases").iter().enumerate() {
        let doc = parse_document(case["doc"].as_str().unwrap()).expect("a supported document");
        let root = Value::Object(SimpleXmlElement::root(Arc::new(doc)).into_ref());
        let label = format!("case {n} {} {}", case["doc"], case["steps"]);
        let mut d: Vec<Diagnostic> = Vec::new();
        let start = walk(&root, &case["steps"], &mut d);
        let v = match (start, case.get("error")) {
            (Ok(v), None) => v,
            (Err(e), Some(want)) => {
                let got = json!([e.class().name(), e.message_lossy()]);
                if &got != want {
                    failures.push(format!("{label}: steps threw {got}, PHP {want}"));
                }
                continue;
            }
            (got, want) => {
                failures.push(format!("{label}: steps gave {got:?}, PHP {want:?}"));
                continue;
            }
        };
        if messages(&d) != case["step_diags"] {
            failures.push(format!(
                "{label}: step diagnostics {} != {}",
                messages(&d),
                case["step_diags"]
            ));
        }
        for result in case["results"].as_array().expect("results") {
            let mut d: Vec<Diagnostic> = Vec::new();
            let got = match run_final(&v, &root, &result["final"], &mut d) {
                Ok(value) => json!({ "ok": value }),
                Err(e) => json!({ "error": [e.class().name(), e.message_lossy()] }),
            };
            let want = match result.get("error") {
                Some(error) => json!({ "error": error }),
                None => json!({ "ok": result["ok"] }),
            };
            checked += 1;
            if normalize(&got) != normalize(&want) {
                failures.push(format!("{label} {}: {got} != PHP {want}", result["final"]));
            }
            if messages(&d) != result["diags"] {
                failures.push(format!(
                    "{label} {}: diagnostics {} != {}",
                    result["final"],
                    messages(&d),
                    result["diags"]
                ));
            }
        }
    }
    assert!(checked > 500, "{checked} results");
    assert!(
        failures.is_empty(),
        "{} of {checked} results differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
