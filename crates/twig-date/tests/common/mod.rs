//! Shared helpers of the twig-date oracle tests.

#![allow(dead_code)]

use std::sync::Arc;

use serde_json::Value as Json;
use twig_date::{DateContext, FixedClock};
use twig_value::object::{CallArgs, ObjectRef};
use twig_value::{Diagnostic, Level, PhpError, PhpString, Value};

/// Loads `tests/vectors/<topic>.json`.
pub fn load(topic: &str) -> Json {
    let path = format!("{}/tests/vectors/{topic}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let v: Json = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(v["php"].as_str(), Some("8.5.13"), "{path}: PHP version");
    assert_eq!(
        v["timezone_version"].as_str(),
        Some("2026.6"),
        "{path}: tzdb version"
    );
    v
}

pub fn s(v: &Json) -> &str {
    v.as_str().unwrap_or_else(|| panic!("string expected: {v}"))
}

pub fn i(v: &Json) -> i64 {
    v.as_i64()
        .unwrap_or_else(|| panic!("integer expected: {v}"))
}

/// Decodes a generator string (`hex:` prefix for non-UTF-8 bytes).
pub fn bytes(v: &Json) -> Vec<u8> {
    let t = s(v);
    match t.strip_prefix("hex:") {
        Some(h) => (0..h.len())
            .step_by(2)
            .map(|k| u8::from_str_radix(&h[k..k + 2], 16).unwrap())
            .collect(),
        None => t.as_bytes().to_vec(),
    }
}

/// Encodes bytes like the generators (`hex:` for non-UTF-8).
pub fn enc(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_owned(),
        Err(_) => format!(
            "hex:{}",
            b.iter().map(|c| format!("{c:02x}")).collect::<String>()
        ),
    }
}

/// `Class: message` of an exception, like the generators' `exc()`.
pub fn exc(e: &PhpError) -> String {
    format!("{}: {}", e.class().name(), enc(e.message().as_bytes()))
}

/// Diagnostics like the generators' `take_diags()`.
pub fn diag_strings(d: &[Diagnostic]) -> Vec<String> {
    d.iter()
        .map(|d| {
            let level = match d.level {
                Level::Warning => "W",
                Level::Notice => "N",
                Level::Deprecated | Level::UserDeprecated => "D",
            };
            format!("{level}: {}", enc(d.message.as_bytes()))
        })
        .collect()
}

/// A context whose clock the test controls.
pub struct Ctx {
    pub ctx: DateContext,
    pub clock: Arc<FixedClock>,
}

impl Ctx {
    pub fn new() -> Ctx {
        let clock = Arc::new(FixedClock::new(0, 0));
        Ctx {
            ctx: DateContext::with_clock(Arc::clone(&clock)),
            clock,
        }
    }

    pub fn now(&self, sec: i64, usec: i64) {
        self.clock.set(sec, usec);
    }
}

/// Calls a method by name with positional arguments.
pub fn call(
    obj: &ObjectRef,
    method: &str,
    args: Vec<Value>,
    diags: &mut Vec<Diagnostic>,
) -> Result<Value, PhpError> {
    obj.call_method(method, CallArgs::from(args), diags)
}

pub fn str_val(v: &Value) -> String {
    match v {
        Value::String(s) => enc(s.as_bytes()),
        other => panic!("string expected, got {other:?}"),
    }
}

pub fn obj(v: Value) -> ObjectRef {
    match v {
        Value::Object(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

pub fn php_str(t: &str) -> Value {
    Value::String(PhpString::from(t))
}

/// `json_encode()` of a value (lists as JSON arrays, other arrays as objects; non-UTF-8 strings as `hex:`).
pub fn to_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(n) => Json::from(*n),
        Value::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
        Value::String(s) => Json::String(enc(s.as_bytes())),
        Value::Array(a) => {
            if a.is_list() {
                Json::Array(a.values().map(to_json).collect())
            } else {
                let mut m = serde_json::Map::new();
                for (k, v) in a.iter() {
                    m.insert(enc(k.to_php_string().as_bytes()), to_json(v));
                }
                Json::Object(m)
            }
        }
        Value::Object(o) => Json::String(format!("object({})", o.class_name())),
    }
}

/// The generators' `show()`: `format('Y-m-d H:i:s.u T P U e')`.
pub fn show(d: &ObjectRef) -> String {
    let v = call(
        d,
        "format",
        vec![php_str("Y-m-d H:i:s.u T P U e")],
        &mut Vec::new(),
    )
    .unwrap();
    str_val(&v)
}

/// The generators' `result()`: `show($d) . ' ' . ((array) $d)['timezone_type']`.
pub fn result(d: &ObjectRef) -> String {
    let a = twig_value::convert::to_array(&Value::Object(d.clone()), &mut Vec::new()).unwrap();
    let tt = match a.get_str(b"timezone_type") {
        Some(Value::Int(n)) => n.to_string(),
        _ => "none".to_owned(),
    };
    format!("{} {tt}", show(d))
}

/// Collects mismatches and fails with the first few once the whole vector set has been checked.
pub struct Report {
    name: &'static str,
    total: usize,
    failures: Vec<String>,
}

impl Report {
    pub fn new(name: &'static str) -> Self {
        Report {
            name,
            total: 0,
            failures: Vec::new(),
        }
    }

    pub fn check(&mut self, ok: bool, describe: impl FnOnce() -> String) {
        self.total += 1;
        if !ok {
            self.failures.push(describe());
        }
    }

    pub fn eq<T: PartialEq + std::fmt::Debug>(
        &mut self,
        got: T,
        want: T,
        what: impl FnOnce() -> String,
    ) {
        let ok = got == want;
        self.check(ok, || {
            format!("  {}\n    php  {want:?}\n    rust {got:?}", what())
        });
    }

    pub fn finish(self) {
        assert!(self.total > 0, "{}: no vectors", self.name);
        if !self.failures.is_empty() {
            let shown: Vec<_> = self.failures.iter().take(12).cloned().collect();
            panic!(
                "{}: {} of {} cases differ:\n{}",
                self.name,
                self.failures.len(),
                self.total,
                shown.join("\n")
            );
        }
    }
}
