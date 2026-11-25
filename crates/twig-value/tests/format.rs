//! The formatting builtins (`twig_value::builtins::format`) against PHP 8.5.13: `sprintf()` /
//! `vsprintf()`, `number_format()`, `round()` / `floor()` / `ceil()` / `abs()` and Twig's `round`
//! filter, `json_encode()`, `hash('xxh128')`.
//!
//! Regenerate the vector files under `tests/vectors/format/` with the Docker oracle
//! (`research/php-oracle`), one generator per file:
//!
//! ```text
//! cd crates/twig-value/tests/vectors
//! for g in sprintf number_format round json xxh128; do
//!   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!       php /t/format/$g.gen.php > format/$g.json
//! done
//! ```

mod support;

use std::sync::{Arc, LazyLock};

use serde_json::Value as Json;
use support::values::{UserObject, class_by_name};
use support::{Mismatches, bits, diag_list, hex, load, s, unhex};
use twig_value::builtins::format;
use twig_value::object::{
    ArrayObject, CallArgs, ClassBuilder, ClassRef, Closure, Object, ObjectRef, PropertyInfo,
    PropertyStore, StdClass, Visibility, builtin, undefined_method, unmangle_property_name,
};
use twig_value::{Array, ArrayKey, Diagnostic, Diagnostics, PhpError, PhpResult, PhpString, Value};

// ------------------------------------------------------------------------------------------------
// Values: ../values.inc.php enc() plus the classes of format/classes.inc.php.

/// The classes of `format/classes.inc.php`.
struct Extra {
    thrower: ClassRef,
    j: ClassRef,
    jself: ClassRef,
    jthrow: ClassRef,
}

fn prop(name: &str, visibility: Visibility, default: Value) -> PropertyInfo {
    PropertyInfo {
        name: PhpString::from(name),
        visibility,
        is_static: false,
        is_readonly: false,
        type_name: None,
        default: Some(default),
        declaring_class: Arc::from(""),
    }
}

static EXTRA: LazyLock<Extra> = LazyLock::new(|| Extra {
    thrower: ClassBuilder::class("Thrower")
        .implements(&builtin::stringable())
        .method("__toString")
        .build(),
    j: ClassBuilder::class("J")
        .implements(&builtin::json_serializable())
        .property_info(prop("v", Visibility::Public, Value::Null))
        .method("__construct")
        .method("jsonSerialize")
        .build(),
    jself: ClassBuilder::class("JSelf")
        .implements(&builtin::json_serializable())
        .property_info(prop("a", Visibility::Public, Value::Int(1)))
        .property_info(prop("b", Visibility::Protected, Value::Int(2)))
        .property_info(prop("c", Visibility::Private, Value::Int(3)))
        .method("jsonSerialize")
        .build(),
    jthrow: ClassBuilder::class("JThrow")
        .implements(&builtin::json_serializable())
        .method("jsonSerialize")
        .build(),
});

/// An instance of one of the [`Extra`] classes; behaviour by class name.
struct TestObject {
    class: ClassRef,
    props: PropertyStore,
}

impl TestObject {
    fn create(class: &ClassRef) -> ObjectRef {
        ObjectRef::new(TestObject {
            class: class.clone(),
            props: PropertyStore::new(class),
        })
    }
}

impl Object for TestObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match (self.class.name(), method) {
            ("Thrower", "__tostring") => Err(PhpError::exception("boom")),
            ("J", "jsonserialize") => Ok(self.props.get(b"v").unwrap_or_default()),
            ("JSelf", "jsonserialize") => Ok(Value::Object(this.clone())),
            ("JThrow", "jsonserialize") => Err(PhpError::exception("json boom")),
            _ => Err(undefined_method(&self.class, method)),
        }
    }
}

fn extra_class(name: &str) -> Option<&'static ClassRef> {
    let e = &*EXTRA;
    Some(match name {
        "Thrower" => &e.thrower,
        "J" => &e.j,
        "JSelf" => &e.jself,
        "JThrow" => &e.jthrow,
        _ => return None,
    })
}

/// A value from the typed encoding (`enc()`).
fn decode(j: &Json) -> Value {
    match s(&j["t"]) {
        "null" => Value::Null,
        "bool" => Value::Bool(j["v"].as_bool().expect("bool")),
        "int" => Value::Int(j["v"].as_i64().expect("int")),
        "float" => Value::Float(bits(s(&j["bits"]))),
        "string" => Value::from(unhex(s(&j["hex"]))),
        "array" => {
            let mut a = Array::new();
            for item in j["items"].as_array().expect("items") {
                let key = match decode(&item[0]) {
                    Value::Int(i) => ArrayKey::Int(i),
                    Value::String(s) => ArrayKey::Str(s),
                    other => panic!("bad key {other:?}"),
                };
                a.insert(key, decode(&item[1]));
            }
            Value::Array(a)
        }
        "enum" => {
            let class = match s(&j["class"]) {
                "RoundingMode" => format::rounding_mode_class(),
                name => class_by_name(name).clone(),
            };
            Value::Object(class.enum_case(s(&j["case"])).expect("enum case"))
        }
        "closure" => Value::Object(Closure::new(|_, _| Ok(Value::Null)).into_ref()),
        "object" if s(&j["class"]) == "ArrayObject" => {
            // `(array)` of an ArrayObject is its storage.
            let mut storage = Array::new();
            for p in j["props"].as_array().expect("props") {
                let key = ArrayKey::from_string(PhpString::from(unhex(s(&p[0]))));
                storage.insert(key, decode(&p[1]));
            }
            Value::Object(ArrayObject::new(storage).into_ref())
        }
        "object" => {
            let name = s(&j["class"]);
            let obj = match name {
                "stdClass" => StdClass::new().into_ref(),
                _ => match extra_class(name) {
                    Some(class) => TestObject::create(class),
                    None => UserObject::create(class_by_name(name)),
                },
            };
            let is_std = name == "stdClass";
            let store = obj.object().property_store().expect("store");
            for p in j["props"].as_array().expect("props") {
                let key = unhex(s(&p[0]));
                let value = decode(&p[1]);
                if is_std {
                    store.set(&key, value);
                } else {
                    let (_, name) = unmangle_property_name(&key);
                    store.set(name, value);
                }
            }
            Value::Object(obj)
        }
        other => panic!("unknown typed value {other}"),
    }
}

// ------------------------------------------------------------------------------------------------
// Results and diagnostics in the generators' format (format/classes.inc.php).

/// Bytes as text when valid UTF-8, else `hex:...` (the generators' `out_enc()` choice).
fn show(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => format!("hex:{}", hex(b)),
    }
}

/// The bytes of an `out_enc()` node.
fn text(j: &Json) -> Vec<u8> {
    match j {
        Json::String(s) => s.as_bytes().to_vec(),
        Json::Object(o) => unhex(s(&o["hex"])),
        other => panic!("expected out_enc() text, got {other}"),
    }
}

fn show_error(e: &PhpError) -> String {
    let mut out = format!("err {}: {}", e.class().name(), show(e.message()));
    if let Some(p) = e.previous() {
        out.push_str(&format!(" <- {}: {}", p.class().name(), show(p.message())));
    }
    out
}

/// An expected `out_res()` node as a comparable string; `ok` renders successful results.
fn expected(j: &Json, ok: impl Fn(&Json) -> String) -> String {
    match j.get("err") {
        Some(e) => {
            let mut out = format!("err {}: {}", s(&e[0]), show(&text(&e[1])));
            if let Some(p) = j.get("prev") {
                out.push_str(&format!(" <- {}: {}", s(&p[0]), show(&text(&p[1]))));
            }
            out
        }
        None => ok(j),
    }
}

/// A Rust result as a comparable string.
fn actual<T>(r: &PhpResult<T>, ok: impl Fn(&T) -> String) -> String {
    match r {
        Ok(v) => ok(v),
        Err(e) => show_error(e),
    }
}

/// String results: `ok <text>`.
fn ok_text(j: &Json) -> String {
    format!("ok {}", show(&text(j)))
}

fn ok_value_text(v: &Value) -> String {
    match v {
        Value::String(s) => format!("ok {}", show(s)),
        other => format!("ok {other:?}"),
    }
}

fn ok_string_text(s: &PhpString) -> String {
    format!("ok {}", show(s))
}

/// Expected diagnostics (optional trailing row element) as `(level, hex)` pairs.
fn expected_diags(j: Option<&Json>) -> Vec<(String, String)> {
    match j {
        None => Vec::new(),
        Some(list) => list
            .as_array()
            .expect("diags")
            .iter()
            .map(|p| (s(&p[0]).to_string(), hex(&text(&p[1]))))
            .collect(),
    }
}

fn diags(d: &[Diagnostic]) -> Vec<(String, String)> {
    diag_list(d)
}

/// Calls a PHP-level builtin by name.
fn call(name: &str, args: &[Value], d: &mut Vec<Diagnostic>) -> PhpResult<Value> {
    format::function(name).unwrap_or_else(|| panic!("no builtin {name}"))(args, d)
}

// ------------------------------------------------------------------------------------------------
// sprintf / vsprintf

#[test]
fn sprintf_vectors() {
    let v = load("format/sprintf.json");
    let values: Vec<Value> = v["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(decode)
        .collect();
    let mut m = Mismatches::new("sprintf");

    for entry in v["matrix"].as_array().expect("matrix") {
        let fmt = s(&entry[0]);
        for row in entry[1].as_array().expect("rows") {
            let value = &values[row[0].as_u64().expect("index") as usize];
            let mut d = Vec::new();
            let r = format::sprintf(&Value::from(fmt), std::slice::from_ref(value), &mut d);
            m.check(
                || format!("sprintf({fmt:?}, {value:?})"),
                (actual(&r, ok_string_text), diags(&d)),
                (expected(&row[1], ok_text), expected_diags(row.get(2))),
            );
        }
    }

    for row in v["floats"].as_array().expect("floats") {
        let fmt = s(&row[0]);
        let x = bits(s(&row[1]));
        let mut args = Vec::new();
        if fmt.starts_with("%.*") {
            args.push(Value::Int(-1));
        }
        args.push(Value::Float(x));
        let mut d = Vec::new();
        let r = format::sprintf(&Value::from(fmt), &args, &mut d);
        m.check(
            || format!("sprintf({fmt:?}, {x:e}) [{}]", s(&row[1])),
            (actual(&r, ok_string_text), diags(&d)),
            (expected(&row[2], ok_text), expected_diags(row.get(3))),
        );
    }

    for row in v["ints"].as_array().expect("ints") {
        let fmt = s(&row[0]);
        let n = row[1].as_i64().expect("int");
        let mut d = Vec::new();
        let r = format::sprintf(&Value::from(fmt), &[Value::Int(n)], &mut d);
        m.check(
            || format!("sprintf({fmt:?}, {n})"),
            (actual(&r, ok_string_text), diags(&d)),
            (expected(&row[2], ok_text), expected_diags(row.get(3))),
        );
    }

    for row in v["cases"].as_array().expect("cases") {
        let name = s(&row[0]);
        let args: Vec<Value> = row[1]
            .as_array()
            .expect("args")
            .iter()
            .map(|i| values[i.as_u64().expect("index") as usize].clone())
            .collect();
        let mut d = Vec::new();
        let r = call(name, &args, &mut d);
        m.check(
            || format!("{name}({args:?})"),
            (actual(&r, ok_value_text), diags(&d)),
            (expected(&row[2], ok_text), expected_diags(row.get(3))),
        );
    }
    m.finish();
}

// ------------------------------------------------------------------------------------------------
// number_format

#[test]
fn number_format_vectors() {
    let v = load("format/number_format.json");
    let mut m = Mismatches::new("number_format");
    let check = |m: &mut Mismatches, args: Vec<Value>, row: &Json, at: usize| {
        let mut d = Vec::new();
        // Whole argument lists go through the arity check; the others through the typed form.
        let r = if args.is_empty() || args.len() > 4 {
            format::number_format_call(&args, &mut d)
        } else {
            format::number_format(&args[0], args.get(1), args.get(2), args.get(3), &mut d)
                .map(Value::String)
        };
        m.check(
            || format!("number_format({args:?})"),
            (actual(&r, ok_value_text), diags(&d)),
            (expected(&row[at], ok_text), expected_diags(row.get(at + 1))),
        );
    };
    for row in v["floats"].as_array().expect("floats") {
        let x = bits(s(&row[0]));
        let dec = row[1].as_i64().expect("decimals");
        check(&mut m, vec![Value::Float(x), Value::Int(dec)], row, 2);
    }
    for row in v["seps"].as_array().expect("seps") {
        let args = vec![
            Value::Float(bits(s(&row[0]))),
            Value::Int(row[1].as_i64().expect("decimals")),
            Value::from(s(&row[2])),
            Value::from(s(&row[3])),
        ];
        check(&mut m, args, row, 4);
    }
    for row in v["ints"].as_array().expect("ints") {
        let n = row[0].as_i64().expect("int");
        let dec = row[1].as_i64().expect("decimals");
        check(&mut m, vec![Value::Int(n), Value::Int(dec)], row, 2);
    }
    let values: Vec<Value> = v["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(decode)
        .collect();
    for row in v["cases"].as_array().expect("cases") {
        let args: Vec<Value> = row[0]
            .as_array()
            .expect("args")
            .iter()
            .map(|i| values[i.as_u64().expect("index") as usize].clone())
            .collect();
        check(&mut m, args, row, 1);
    }
    m.finish();
}

// ------------------------------------------------------------------------------------------------
// round / floor / ceil / abs / Twig's round filter

/// A typed result (`enc()`) of a scalar as a comparable string (floats by bits, NaN by kind).
fn typed_text(j: &Json) -> String {
    match s(&j["t"]) {
        "float" => float_text(bits(s(&j["bits"]))),
        "int" => format!("int {}", j["v"].as_i64().expect("int")),
        "string" => format!("string {}", show(&unhex(s(&j["hex"])))),
        "bool" => format!("bool {}", j["v"].as_bool().expect("bool")),
        "null" => "null".to_string(),
        other => format!("{other} {j}"),
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Float(f) => float_text(*f),
        Value::Int(i) => format!("int {i}"),
        Value::String(s) => format!("string {}", show(s)),
        Value::Bool(b) => format!("bool {b}"),
        Value::Null => "null".to_string(),
        other => format!("{other:?}"),
    }
}

/// Floats by bit pattern; NaN payloads and signs are not observable in PHP output.
fn float_text(f: f64) -> String {
    if f.is_nan() {
        "float NAN".to_string()
    } else {
        format!("float {:016x}", f.to_bits())
    }
}

#[test]
fn round_vectors() {
    let v = load("format/round.json");
    let mut m = Mismatches::new("round");
    for row in v["floats"].as_array().expect("floats") {
        let x = bits(s(&row[0]));
        let places = row[1].as_i64().expect("places");
        let mode = row[2].as_i64().expect("mode");
        let want = float_text(bits(s(&row[3])));
        let mut d = Vec::new();
        let r = format::round(
            &Value::Float(x),
            Some(&Value::Int(places)),
            Some(&Value::Int(mode)),
            &mut d,
        );
        m.check(
            || format!("round({x:e}, {places}, {mode}) [{}]", s(&row[0])),
            actual(&r, |f| float_text(*f)),
            want.clone(),
        );
        // The typed core agrees with the PHP function.
        let rounding = format::RoundingMode::from_php(mode).expect("mode");
        let p = places.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        m.check(
            || format!("math_round({x:e}, {p}, {mode})"),
            float_text(format::math_round(x, p, rounding)),
            want,
        );
    }
    for row in v["ints"].as_array().expect("ints") {
        let n = row[0].as_i64().expect("int");
        let places = row[1].as_i64().expect("places");
        let mut d = Vec::new();
        let r = format::round(&Value::Int(n), Some(&Value::Int(places)), None, &mut d);
        m.check(
            || format!("round({n}, {places})"),
            actual(&r, |f| float_text(*f)),
            float_text(bits(s(&row[2]))),
        );
    }
    let values: Vec<Value> = v["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(decode)
        .collect();
    let pick = |list: &Json| -> Vec<Value> {
        list.as_array()
            .expect("indices")
            .iter()
            .map(|i| values[i.as_u64().expect("index") as usize].clone())
            .collect()
    };
    for row in v["cases"].as_array().expect("cases") {
        let name = s(&row[0]);
        let args = pick(&row[1]);
        let mut d = Vec::new();
        let r = call(name, &args, &mut d);
        m.check(
            || format!("{name}({args:?})"),
            (actual(&r, value_text), diags(&d)),
            (expected(&row[2], typed_text), expected_diags(row.get(3))),
        );
    }
    for row in v["twig"].as_array().expect("twig") {
        let args = pick(&row[0]);
        let mut d = Vec::new();
        let r = format::twig_round(&args[0], &args[1], &args[2], &mut d);
        m.check(
            || format!("CoreExtension::round({args:?})"),
            (actual(&r, value_text), diags(&d)),
            (expected(&row[1], typed_text), expected_diags(row.get(2))),
        );
    }
    m.finish();
}

// ------------------------------------------------------------------------------------------------
// json_encode

/// JSON results: `ok <text>`, `false`, or the exception.
fn json_expected(j: &Json) -> String {
    match j {
        Json::Bool(false) => "false".to_string(),
        other => expected(other, ok_text),
    }
}

fn json_actual(r: &PhpResult<Value>) -> String {
    match r {
        Ok(Value::Bool(false)) => "false".to_string(),
        other => actual(other, ok_value_text),
    }
}

/// The expected error code: the `JsonException` code, else `json_last_error()`.
fn json_expected_code(result: &Json, last_error: &Json) -> i64 {
    match result.get("code") {
        Some(code) => code.as_i64().expect("code"),
        None => last_error.as_i64().expect("error code"),
    }
}

/// The error code PHP reports for a call: the `JsonException` code when one is thrown, else
/// `json_last_error()` (not updated by `JSON_THROW_ON_ERROR` without partial output). `None` for
/// calls that fail before encoding (argument errors) or throw from user code.
fn json_actual_code(value: &Value, flags: i64, depth: i64, r: &PhpResult<Value>) -> Option<i64> {
    if let Err(e) = r
        && e.class().name() != "JsonException"
    {
        return None;
    }
    let encoded =
        format::json_encode_value(value, flags as i32, depth as i32, &mut Vec::new()).ok()?;
    let throws = flags & format::JSON_THROW_ON_ERROR != 0
        && flags & format::JSON_PARTIAL_OUTPUT_ON_ERROR == 0;
    Some(if throws && r.is_ok() {
        0
    } else {
        encoded.error.code()
    })
}

/// The object graphs of the generator's `$scenario_builders`.
fn json_scenario(name: &str) -> Value {
    let set = |o: &ObjectRef, k: &str, v: Value| {
        o.object()
            .property_store()
            .expect("store")
            .set(k.as_bytes(), v);
    };
    let std_obj = |props: &[(&str, Value)]| {
        let o = StdClass::new().into_ref();
        for (k, v) in props {
            set(&o, k, v.clone());
        }
        o
    };
    match name {
        "std_self" => {
            let o = std_obj(&[("a", Value::Int(1))]);
            set(&o, "self", Value::Object(o.clone()));
            Value::Object(o)
        }
        "std_mutual" => {
            let a = std_obj(&[("x", Value::Int(1))]);
            let b = std_obj(&[("a", Value::Object(a.clone()))]);
            set(&a, "b", Value::Object(b));
            Value::Array(Array::from_values([Value::Object(a), Value::from("tail")]))
        }
        "std_shared" => {
            let o = Value::Object(std_obj(&[("v", Value::from("shared"))]));
            let mut again = Array::new();
            again.insert("again", o.clone());
            Value::Array(Array::from_values([o.clone(), o, Value::Array(again)]))
        }
        "j_self_value" => {
            let j = TestObject::create(&EXTRA.j);
            set(&j, "v", Value::Object(j.clone()));
            Value::Object(j)
        }
        "j_mutual" => {
            let a = TestObject::create(&EXTRA.j);
            let b = TestObject::create(&EXTRA.j);
            set(&b, "v", Value::Object(a.clone()));
            set(&a, "v", Value::Object(b));
            Value::Array(Array::from_values([Value::Int(1), Value::Object(a)]))
        }
        "j_in_own_array" => {
            let j = TestObject::create(&EXTRA.j);
            let mut inner = Array::new();
            inner.insert("me", Value::Object(j.clone()));
            inner.insert("n", Value::Int(1));
            set(&j, "v", Value::Array(inner));
            Value::Object(j)
        }
        "ao_self" => {
            let ao = ArrayObject::new(Array::new()).into_ref();
            let mut d = Vec::new();
            ao.write_dimension(Some(&Value::from("x")), Value::Int(1), &mut d)
                .expect("offsetSet");
            ao.write_dimension(
                Some(&Value::from("self")),
                Value::Object(ao.clone()),
                &mut d,
            )
            .expect("offsetSet");
            Value::Object(ao)
        }
        other => panic!("unknown scenario {other}"),
    }
}

/// One `json_encode()` call checked against a row: result at `row[at]`, error code at
/// `row[at + 1]`, diagnostics at `row[at + 2]`.
fn json_check(m: &mut Mismatches, value: &Value, args: &[Value], row: &Json, at: usize) {
    let flags = args.get(1).and_then(Value::as_int).unwrap_or(0);
    let depth = args.get(2).and_then(Value::as_int).unwrap_or(512);
    let mut d = Vec::new();
    let r = format::json_encode(&args[0], args.get(1), args.get(2), &mut d);
    let code = json_actual_code(value, flags, depth, &r);
    let want_code = code.map(|_| json_expected_code(&row[at], &row[at + 1]));
    m.check(
        || format!("json_encode({args:?})"),
        (json_actual(&r), code, diags(&d)),
        (
            json_expected(&row[at]),
            want_code,
            expected_diags(row.get(at + 2)),
        ),
    );
}

#[test]
fn json_vectors() {
    let v = load("format/json.json");
    let values: Vec<Value> = v["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(decode)
        .collect();
    let flags: Vec<i64> = v["flags"]
        .as_array()
        .expect("flags")
        .iter()
        .map(|f| f.as_i64().expect("flag"))
        .collect();
    let index = |j: &Json| j.as_u64().expect("index") as usize;
    let mut m = Mismatches::new("json_encode");

    for entry in v["matrix"].as_array().expect("matrix") {
        let value = &values[index(&entry[0])];
        for row in entry[1].as_array().expect("rows") {
            let fl = flags[index(&row[0])];
            json_check(&mut m, value, &[value.clone(), Value::Int(fl)], row, 1);
        }
    }
    for entry in v["strings"].as_array().expect("strings") {
        let value = Value::from(text(&entry[0]));
        for row in entry[1].as_array().expect("rows") {
            let fl = flags[index(&row[0])];
            json_check(&mut m, &value, &[value.clone(), Value::Int(fl)], row, 1);
        }
    }
    for row in v["floats"].as_array().expect("floats") {
        let x = Value::Float(bits(s(&row[0])));
        for (fl, at) in [(0, 1), (format::JSON_PRESERVE_ZERO_FRACTION, 2)] {
            let r = format::json_encode(&x, Some(&Value::Int(fl)), None, &mut Vec::new());
            m.check(
                || format!("json_encode({x:?}, {fl})"),
                json_actual(&r),
                json_expected(&row[at]),
            );
        }
    }
    for row in v["depth"].as_array().expect("depth") {
        let value = &values[index(&row[0])];
        let fl = row[1].as_i64().expect("flags");
        let depth = row[2].as_i64().expect("depth");
        let args = [value.clone(), Value::Int(fl), Value::Int(depth)];
        json_check(&mut m, value, &args, row, 3);
    }
    for row in v["deep"].as_array().expect("deep") {
        let levels = row[0].as_u64().expect("levels");
        let depth = row[1].as_i64().expect("depth");
        let mut value = Value::from("x");
        for _ in 0..levels {
            value = Value::Array(Array::from_values([value]));
        }
        let fl = format::JSON_PARTIAL_OUTPUT_ON_ERROR;
        let args = [value.clone(), Value::Int(fl), Value::Int(depth)];
        json_check(&mut m, &value, &args, row, 2);
    }
    for row in v["cases"].as_array().expect("cases") {
        let args: Vec<Value> = row[0]
            .as_array()
            .expect("args")
            .iter()
            .map(|i| values[index(i)].clone())
            .collect();
        let mut d = Vec::new();
        let r = format::json_encode_call(&args, &mut d);
        m.check(
            || format!("json_encode({args:?})"),
            (json_actual(&r), diags(&d)),
            (json_expected(&row[1]), expected_diags(row.get(3))),
        );
    }
    for row in v["scenarios"].as_array().expect("scenarios") {
        let fl = row[1].as_i64().expect("flags");
        let value = json_scenario(s(&row[0]));
        json_check(&mut m, &value, &[value.clone(), Value::Int(fl)], row, 2);
    }
    m.finish();
}

// ------------------------------------------------------------------------------------------------
// hash('xxh128')

#[test]
fn xxh128_vectors() {
    let v = load("format/xxh128.json");
    let mut m = Mismatches::new("xxh128");
    for row in v["lengths"].as_array().expect("lengths") {
        let n = row[0].as_u64().expect("length") as usize;
        let input: Vec<u8> = (0..n).map(|i| ((i * 31 + n * 7) & 0xff) as u8).collect();
        let got = format::hash_xxh128(&input);
        m.check(
            || format!("xxh128 of {n} bytes"),
            show(&got),
            s(&row[1]).to_string(),
        );
        m.check(
            || format!("xxh128 binary of {n} bytes"),
            hex(&format::hash_xxh128_binary(&input)),
            s(&row[1]).to_string(),
        );
    }
    for row in v["strings"].as_array().expect("strings") {
        let input = text(&row[0]);
        m.check(
            || format!("xxh128({})", show(&input)),
            show(&format::hash_xxh128(&input)),
            s(&row[1]).to_string(),
        );
    }
    m.finish();
}
