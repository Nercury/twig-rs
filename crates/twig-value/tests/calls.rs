//! Calls checked against PHP 8.5.13 (`tests/vectors/calls.json`): the binding of arguments to
//! parameters ([`CallArgs`]: named arguments, `...` unpacking, skipped parameters, argument counts)
//! and the declared return types of userland functions ([`param::verify_user_return`]).
//!
//! Regenerate the vectors:
//!
//! ```text
//! cd crates/twig-value/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/calls.gen.php > calls.json
//! ```

mod support;

use std::sync::{LazyLock, Mutex};

use serde_json::Value as Json;
use twig_value::object::{
    self, ArrayObject, CallArgs, CalleeParams, ClassBuilder, ClassRef, FunctionKind, Object,
    ObjectRef, Param, StdClass, builtin,
};
use twig_value::param::{self, DeclaredType, TypeMask as T};
use twig_value::{
    Array, ArrayKey, Diagnostic, Diagnostics, PhpError, PhpResult, PhpString, Throw, Value,
};

use support::{Mismatches, bits, diag_list, hex, json_diags, load, s, to_bits, unhex};

// ---------------------------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------------------------

/// The functions of `calls.gen.php`: parameters, kind, and whether unknown named arguments that a
/// variadic internal function collects make `zend_parse_parameters()` fail.
fn signature(function: &str) -> (Vec<Param>, FunctionKind, bool) {
    use FunctionKind::{Internal, User};
    let p = Param::required;
    match function {
        "u_h" => (vec![p("a"), p("b")], User, false),
        "u_g" => (vec![p("a"), Param::optional("b", "B")], User, false),
        "u_t" => (vec![p("a"), p("b"), p("c")], User, false),
        "u_o" => (
            vec![Param::optional("a", "A"), Param::optional("b", "B")],
            User,
            false,
        ),
        "u_v" => (vec![p("a"), Param::variadic("r")], User, false),
        "u_w" => (vec![Param::variadic("r")], User, false),
        "u_n" => (vec![], User, false),
        "u_ov" => (
            vec![Param::optional("a", "A"), Param::variadic("r")],
            User,
            false,
        ),
        "abs" => (vec![p("num")], Internal, false),
        "str_pad" => (
            vec![
                p("string"),
                p("length"),
                Param::optional("pad_string", " "),
                Param::optional("pad_type", 1),
            ],
            Internal,
            false,
        ),
        "sprintf" => (vec![p("format"), Param::variadic("values")], Internal, true),
        "max" => (vec![p("value"), Param::variadic("values")], Internal, true),
        "array_merge" => (vec![Param::variadic("arrays")], Internal, true),
        other => panic!("unknown function {other}"),
    }
}

/// An int, a string or a list of ints.
fn arg_value(j: &Json) -> Value {
    match j {
        Json::Number(n) => Value::Int(n.as_i64().expect("int")),
        Json::String(s) => Value::from(s.as_str()),
        Json::Array(items) => Value::Array(items.iter().map(arg_value).collect::<Array>()),
        Json::Null => Value::Null,
        other => panic!("unexpected value {other}"),
    }
}

/// A key of `pairs()`: an int, a string, a float or null.
fn key_value(j: &Json) -> Value {
    match j {
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().expect("float key")),
        },
        _ => arg_value(j),
    }
}

/// The array literal `[k => v, ...]` (keys normalised, later keys overwrite).
fn literal(pairs: &Json) -> Array {
    let mut array = Array::new();
    for pair in pairs.as_array().expect("pairs") {
        let key = match &pair[0] {
            Json::Number(n) => ArrayKey::Int(n.as_i64().expect("int key")),
            Json::String(s) => ArrayKey::from_string(PhpString::from(s.as_str())),
            other => panic!("unexpected literal key {other}"),
        };
        array.insert(key, arg_value(&pair[1]));
    }
    array
}

static PAIRS_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Pairs")
        .implements(&builtin::iterator())
        .method("rewind")
        .method("valid")
        .method("current")
        .method("key")
        .method("next")
        .build()
});

/// `pairs()` of `calls.gen.php`: an `Iterator` yielding the recorded keys and values (keys may
/// repeat), logging its method calls.
struct Pairs {
    pairs: Vec<(Value, Value)>,
    position: Mutex<usize>,
    log: Mutex<Vec<String>>,
}

impl Pairs {
    fn object(pairs: Vec<(Value, Value)>) -> ObjectRef {
        ObjectRef::new(Pairs {
            pairs,
            position: Mutex::new(0),
            log: Mutex::new(Vec::new()),
        })
    }

    fn from_json(pairs: &Json) -> ObjectRef {
        Pairs::object(
            pairs
                .as_array()
                .expect("pairs")
                .iter()
                .map(|pair| (key_value(&pair[0]), arg_value(&pair[1])))
                .collect(),
        )
    }
}

impl Object for Pairs {
    fn class(&self) -> &ClassRef {
        &PAIRS_CLASS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let mut position = self.position.lock().unwrap();
        self.log.lock().unwrap().push(method.to_string());
        let pair = self.pairs.get(*position);
        Ok(match method {
            "rewind" => {
                *position = 0;
                Value::Null
            }
            "valid" => Value::Bool(pair.is_some()),
            "current" => pair.map(|p| p.1.clone()).unwrap_or_default(),
            "key" => pair.map(|p| p.0.clone()).unwrap_or_default(),
            "next" => {
                *position += 1;
                Value::Null
            }
            _ => return Err(object::undefined_method(&PAIRS_CLASS, method)),
        })
    }
}

/// How the test sends the arguments.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// The callee is known while sending: named arguments are checked as PHP sends them.
    CalleeParams,
    /// The callee is not known: named arguments are checked by `bind()`.
    Blind,
}

/// Sends the arguments of a case, as an engine compiling the call would.
fn send(sends: &Json, callee: &CalleeParams<'_>, mode: Mode) -> PhpResult<CallArgs> {
    let mut args = CallArgs::new();
    let mut diag: Vec<Diagnostic> = Vec::new();
    let known = (mode == Mode::CalleeParams).then_some(callee);
    for send in sends.as_array().expect("sends") {
        match s(&send[0]) {
            "pos" => args.positional.push(arg_value(&send[1])),
            "named" => {
                let name = PhpString::from(s(&send[1]));
                let value = arg_value(&send[2]);
                match known {
                    Some(callee) => args.send_named(name, value, callee)?,
                    None => args.named.push((name, value)),
                }
            }
            "spread" => args.unpack(&Value::Array(literal(&send[1])), known, &mut diag)?,
            "iter" => args.unpack(&Value::Object(Pairs::from_json(&send[1])), known, &mut diag)?,
            other => panic!("unknown send {other}"),
        }
    }
    assert!(diag.is_empty(), "unpacking raised {diag:?}");
    Ok(args)
}

/// Whether a positional element follows a named argument (one of the cases only a known callee
/// handles like PHP).
fn positional_after_named(sends: &Json) -> bool {
    let mut named = false;
    for send in sends.as_array().expect("sends") {
        match s(&send[0]) {
            "named" => named = true,
            "spread" | "iter" => {
                for pair in send[1].as_array().expect("pairs") {
                    let positional = match &pair[0] {
                        Json::Number(n) => n.is_i64(),
                        Json::String(s) => {
                            matches!(
                                ArrayKey::from_string(PhpString::from(s.as_str())),
                                ArrayKey::Int(_)
                            )
                        }
                        _ => false,
                    };
                    if positional && named {
                        return true;
                    }
                    named |= !positional;
                }
            }
            _ => {}
        }
    }
    false
}

/// PHP's `json_encode()` of the parameters a user function returns.
fn json_encode(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Int(i) => i.to_string(),
        Value::String(s) => format!("\"{}\"", s.to_string_lossy()),
        Value::Array(a) => {
            let list = a
                .keys()
                .enumerate()
                .all(|(i, k)| *k == ArrayKey::Int(i as i64));
            let items: Vec<String> = a
                .iter()
                .map(|(k, v)| {
                    if list {
                        json_encode(v)
                    } else {
                        let key = match k {
                            ArrayKey::Int(i) => i.to_string(),
                            ArrayKey::Str(s) => s.to_string_lossy().into_owned(),
                        };
                        format!("\"{key}\":{}", json_encode(v))
                    }
                })
                .collect();
            if list {
                format!("[{}]", items.join(","))
            } else {
                format!("{{{}}}", items.join(","))
            }
        }
        other => panic!("unexpected bound value {other:?}"),
    }
}

/// The result of a call, as the generator records it.
fn call_result(function: &str, sends: &Json, mode: Mode) -> Json {
    let (params, kind, rejects_named) = signature(function);
    let variadic = params.last().is_some_and(|p| p.variadic);
    let names: Vec<&[u8]> = params
        .iter()
        .filter(|p| !p.variadic)
        .map(|p| p.name.as_bytes())
        .collect();
    let callee = CalleeParams {
        params: &names,
        variadic,
        leading: 0,
        kind,
    };
    let bound = send(sends, &callee, mode).and_then(|args| args.bind(function, &params, kind));
    let bound = bound.and_then(|values| {
        // Z_PARAM_VARIADIC of the internal function (zend_unexpected_extra_named_error()).
        if rejects_named
            && let Some(Value::Array(rest)) = values.last()
            && rest.keys().any(|k| matches!(k, ArrayKey::Str(_)))
        {
            return Err(PhpError::argument_count_error(format!(
                "{function}() does not accept unknown named parameters"
            )));
        }
        Ok(values)
    });
    match bound {
        Ok(_) if kind == FunctionKind::Internal => serde_json::json!({ "ok": true }),
        Ok(values) => {
            let list: Array = values.into_iter().collect();
            serde_json::json!({ "ok": json_encode(&Value::Array(list)) })
        }
        Err(e) => serde_json::json!({ "err": [e.class().name(), e.message_lossy()] }),
    }
}

#[test]
fn call_args() {
    let v = load("calls.json");
    let mut m = Mismatches::new("call_args");
    for case in v["args"].as_array().expect("args") {
        let function = s(&case["fn"]);
        let php = s(&case["php"]);
        let want = case["result"].clone();
        assert_eq!(json_diags(&case["diagnostics"]), Vec::new(), "{php}");
        m.check(
            || format!("{php} (callee known)"),
            call_result(function, &case["sends"], Mode::CalleeParams),
            want.clone(),
        );
        // Without the callee, a positional element after named arguments always throws.
        let blind_want = if positional_after_named(&case["sends"]) {
            serde_json::json!({ "err": ["Error",
                "Cannot use positional argument after named argument during unpacking"] })
        } else {
            want
        };
        m.check(
            || format!("{php} (callee unknown)"),
            call_result(function, &case["sends"], Mode::Blind),
            blind_want,
        );
    }
    m.finish();
}

/// With the callee known, unpacking stops at the element PHP rejects: `next()` is not called
/// after the second `a`, and the elements after it are never read.
#[test]
fn unpack_stops_at_the_rejected_element() {
    let names: Vec<&[u8]> = vec![b"a", b"b", b"c"];
    let callee = CalleeParams {
        params: &names,
        variadic: false,
        leading: 0,
        kind: FunctionKind::User,
    };
    let pairs = Pairs::object(vec![
        (Value::from("a"), Value::Int(1)),
        (Value::from("a"), Value::Int(2)),
        (Value::from("c"), Value::Int(3)),
    ]);
    let mut args = CallArgs::new();
    let mut diag: Vec<Diagnostic> = Vec::new();
    let e = args
        .unpack(&Value::Object(pairs.clone()), Some(&callee), &mut diag)
        .unwrap_err();
    assert_eq!(
        e.message(),
        "Named parameter $a overwrites previous argument"
    );
    let log = pairs
        .downcast_ref::<Pairs>()
        .unwrap()
        .log
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        log,
        [
            "rewind", "valid", "current", "key", "next", "valid", "current", "key"
        ]
    );

    // The leading arguments of an engine (Twig's `$charset` of `first()`) fill the first slots.
    let names: Vec<&[u8]> = vec![b"charset", b"item"];
    let callee = CalleeParams {
        params: &names,
        variadic: false,
        leading: 1,
        kind: FunctionKind::User,
    };
    let mut args = CallArgs::from(vec![Value::from("x")]);
    let mut spread = Array::new();
    spread.insert("charset", "UTF-8");
    let e = args
        .unpack(&Value::Array(spread), Some(&callee), &mut diag)
        .unwrap_err();
    assert_eq!(
        e.message(),
        "Named parameter $charset overwrites previous argument"
    );
    let mut spread = Array::new();
    spread.insert("k", 1);
    let _ = spread.push(2);
    let e = CallArgs::from(vec![Value::from("x")])
        .unpack(&Value::Array(spread), Some(&callee), &mut diag)
        .unwrap_err();
    assert_eq!(e.message(), "Unknown named parameter $k");
}

/// A positional element after named arguments that skipped a parameter, past the declared
/// parameters of a variadic or internal callee (PHP keeps the skipped slot until the call fills or
/// rejects it, and the element after it), is not supported.
#[test]
fn positional_after_a_skipped_parameter() {
    let names: Vec<&[u8]> = vec![b"a", b"b"];
    let callee = CalleeParams {
        params: &names,
        variadic: true,
        leading: 0,
        kind: FunctionKind::User,
    };
    let mut args = CallArgs::new();
    let mut diag: Vec<Diagnostic> = Vec::new();
    let mut named = Array::new();
    named.insert("b", 1);
    args.unpack(&Value::Array(named), Some(&callee), &mut diag)
        .unwrap();
    let e = args
        .unpack(
            &Value::Array(Array::from_values([Value::Int(2)])),
            Some(&callee),
            &mut diag,
        )
        .unwrap_err();
    assert_eq!(
        e.message(),
        "A positional argument after named arguments that skip a parameter is not supported by twig-rs"
    );
}

// ---------------------------------------------------------------------------------------------
// Return types
// ---------------------------------------------------------------------------------------------

/// `class S { function __toString(): string { return '12'; } }` and
/// `class T { function __toString(): string { throw new \Exception('boom'); } }`.
struct Stringable {
    class: ClassRef,
    throws: bool,
}

impl Object for Stringable {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match (method, self.throws) {
            ("__tostring", false) => Ok(Value::from("12")),
            ("__tostring", true) => Err(PhpError::exception("boom")),
            _ => Err(object::undefined_method(&self.class, method)),
        }
    }
}

fn return_value(j: &Json) -> Value {
    match s(&j["t"]) {
        "null" => Value::Null,
        "bool" => Value::Bool(j["v"].as_bool().expect("bool")),
        "int" => Value::Int(j["v"].as_i64().expect("int")),
        "float" => Value::Float(bits(s(&j["bits"]))),
        "string" => Value::from(unhex(s(&j["hex"]))),
        "array" => {
            let n = j["n"].as_i64().expect("array size");
            Value::Array((0..n).map(Value::Int).collect())
        }
        "object" => {
            let class = s(&j["class"]);
            Value::Object(match class {
                "stdClass" => ObjectRef::new(StdClass::new()),
                "ArrayObject" => ArrayObject::new(Array::from_values([Value::Int(1)])).into_ref(),
                "S" | "T" => ObjectRef::new(Stringable {
                    class: ClassBuilder::class(class).method("__toString").build(),
                    throws: class == "T",
                }),
                other => panic!("unknown class {other}"),
            })
        }
        other => panic!("unknown typed value {other}"),
    }
}

fn typed_json(v: &Value) -> Json {
    match v {
        Value::Null => serde_json::json!({"t": "null"}),
        Value::Bool(b) => serde_json::json!({"t": "bool", "v": b}),
        Value::Int(i) => serde_json::json!({"t": "int", "v": i}),
        Value::Float(f) => serde_json::json!({"t": "float", "bits": to_bits(*f)}),
        Value::String(s) => serde_json::json!({"t": "string", "hex": hex(s.as_bytes())}),
        Value::Array(a) => serde_json::json!({"t": "array", "n": a.len()}),
        Value::Object(o) => serde_json::json!({"t": "object", "class": o.class_name()}),
    }
}

fn declared_type(ty: &str) -> DeclaredType<'static> {
    match ty {
        "int" => DeclaredType::builtin(T::INT),
        "float" => DeclaredType::builtin(T::FLOAT),
        "string" => DeclaredType::builtin(T::STRING),
        "bool" => DeclaredType::builtin(T::BOOL),
        "?int" => DeclaredType::builtin(T::INT | T::NULL),
        "?string" => DeclaredType::builtin(T::STRING | T::NULL),
        "int|float" => DeclaredType::builtin(T::INT | T::FLOAT),
        "string|int" => DeclaredType::builtin(T::STRING | T::INT),
        "int|bool" => DeclaredType::builtin(T::INT | T::BOOL),
        "float|string" => DeclaredType::builtin(T::FLOAT | T::STRING),
        "bool|string|null" => DeclaredType::builtin(T::BOOL | T::STRING | T::NULL),
        "false|int" => DeclaredType::builtin(T::FALSE | T::INT),
        "array" => DeclaredType::builtin(T::ARRAY),
        "iterable" => DeclaredType::iterable(),
        "Countable|array" => DeclaredType::new(&["Countable"], T::ARRAY),
        "object" => DeclaredType::builtin(T::OBJECT),
        "?Stringable" => DeclaredType::new(&["Stringable"], T::NULL),
        "mixed" => DeclaredType::builtin(T::MIXED),
        other => panic!("unknown type {other}"),
    }
}

fn chain(e: &PhpError) -> Json {
    let mut chain = Vec::new();
    let mut e = Some(e);
    while let Some(error) = e {
        chain.push(serde_json::json!([
            error.class().name(),
            hex(error.message().as_bytes())
        ]));
        e = error.previous();
    }
    Json::Array(chain)
}

#[test]
fn user_returns() {
    let v = load("calls.json");
    let mut m = Mismatches::new("user_returns");
    for case in v["returns"].as_array().expect("returns") {
        let mode = s(&case["mode"]);
        let ty = declared_type(s(&case["type"]));
        let function = s(&case["fn"]);
        let value = return_value(&case["typed"]);
        let label = format!("{mode} {} {}", s(&case["type"]), case["typed"]);
        let mut collected: Vec<Diagnostic> = Vec::new();
        let mut throwing = Throw::error_exception();
        let diag: &mut dyn Diagnostics = if mode == "throw" {
            &mut throwing
        } else {
            &mut collected
        };
        let result = match param::verify_user_return(value, &ty, function, diag) {
            Ok(v) => serde_json::json!({ "ok": typed_json(&v) }),
            Err(e) => serde_json::json!({ "err": chain(&e) }),
        };
        m.check(|| label.clone(), result, case["result"].clone());
        m.check(
            || format!("{label} diagnostics"),
            diag_list(&collected),
            json_diags(&case["diagnostics"]),
        );
    }
    m.finish();
}
