//! Values with full contents (`tests/vectors/values.inc.php`): the PHP test classes rebuilt with
//! the crate's class model, and the typed encoding in both directions.

use std::sync::{Arc, LazyLock};

use serde_json::{Value as Json, json};
use twig_value::object::{
    CallArgs, ClassBuilder, ClassRef, Closure, EnumBacking, Object, ObjectRef, PropertyInfo,
    PropertyStore, StdClass, Visibility, undefined_method, unmangle_property_name,
};
use twig_value::{Array, ArrayKey, Diagnostics, PhpError, PhpResult, PhpString, Value, convert};

use super::{bits, hex, s, to_bits, unhex};

/// The classes declared by `values.inc.php`.
pub struct Classes {
    /// `class S { private string $s = 'abc'; __toString() }`
    pub s: ClassRef,
    /// `class P { public $a = 1; protected $b = 2; private $c = 3; }`
    pub p: ClassRef,
    /// `class T { public ?int $typed; public $n = null; }`
    pub t: ClassRef,
    /// `enum Suit: string { case Hearts = 'H'; case Spades = 'S'; }`
    pub suit: ClassRef,
    /// `enum Num: int { case One = 1; }`
    pub num: ClassRef,
    /// `enum U { case A; case B; }`
    pub u: ClassRef,
}

fn prop(
    name: &str,
    visibility: Visibility,
    type_name: Option<&str>,
    default: Option<Value>,
) -> PropertyInfo {
    PropertyInfo {
        name: PhpString::from(name),
        visibility,
        is_static: false,
        is_readonly: false,
        type_name: type_name.map(Arc::from),
        default,
        declaring_class: Arc::from(""),
    }
}

/// The shared class entries (class identity matters for object comparison).
pub static CLASSES: LazyLock<Classes> = LazyLock::new(|| Classes {
    s: ClassBuilder::class("S")
        .property_info(prop(
            "s",
            Visibility::Private,
            Some("string"),
            Some(Value::from("abc")),
        ))
        .method("__construct")
        .method("__toString")
        .build(),
    p: ClassBuilder::class("P")
        .property_info(prop("a", Visibility::Public, None, Some(Value::Int(1))))
        .property_info(prop("b", Visibility::Protected, None, Some(Value::Int(2))))
        .property_info(prop("c", Visibility::Private, None, Some(Value::Int(3))))
        .build(),
    t: ClassBuilder::class("T")
        .property_info(prop("typed", Visibility::Public, Some("?int"), None))
        .property_info(prop("n", Visibility::Public, None, Some(Value::Null)))
        .build(),
    suit: ClassBuilder::enumeration("Suit", Some(EnumBacking::String))
        .case("Hearts", Some(Value::from("H")))
        .case("Spades", Some(Value::from("S")))
        .build(),
    num: ClassBuilder::enumeration("Num", Some(EnumBacking::Int))
        .case("One", Some(Value::Int(1)))
        .build(),
    u: ClassBuilder::enumeration("U", None)
        .case("A", None)
        .case("B", None)
        .build(),
});

/// An instance of one of the user classes (`S`, `P`, `T`).
pub struct UserObject {
    class: ClassRef,
    props: PropertyStore,
}

impl UserObject {
    /// A new instance with default property values, as a handle.
    pub fn create(class: &ClassRef) -> ObjectRef {
        ObjectRef::new(UserObject {
            class: class.clone(),
            props: PropertyStore::new(class),
        })
    }
}

impl Object for UserObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__tostring" => Ok(self.props.get(b"s").unwrap_or_default()),
            _ => Err(undefined_method(&self.class, method)),
        }
    }

    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(UserObject {
            class: self.class.clone(),
            props: self.props.clone(),
        }))
    }
}

/// A class of `values.inc.php` by name.
pub fn class_by_name(name: &str) -> &'static ClassRef {
    let c = &*CLASSES;
    match name {
        "S" => &c.s,
        "P" => &c.p,
        "T" => &c.t,
        "Suit" => &c.suit,
        "Num" => &c.num,
        "U" => &c.u,
        other => panic!("unknown class {other}"),
    }
}

/// A PHP value from the typed encoding of `values.inc.php` (`enc()`): scalars as in
/// `common.inc.php`, `{"t":"array","items":[[key, value]...]}`, `{"t":"object","class","props"}`
/// (props as `(array)` cast keys in hex), `{"t":"enum","class","case"}`, `{"t":"closure"}`, and
/// `{"t":"up","n":k}` (the object `k` levels up the current object path, for recursive graphs).
pub fn typed_value(j: &Json) -> Value {
    typed_value_in(j, &mut Vec::new())
}

fn typed_value_in(j: &Json, path: &mut Vec<ObjectRef>) -> Value {
    match s(&j["t"]) {
        "null" => Value::Null,
        "bool" => Value::Bool(j["v"].as_bool().expect("bool")),
        "int" => Value::Int(j["v"].as_i64().expect("int")),
        "float" => Value::Float(bits(s(&j["bits"]))),
        "string" => Value::from(unhex(s(&j["hex"]))),
        "array" => {
            let mut a = Array::new();
            for item in j["items"].as_array().expect("items") {
                let key = match typed_value_in(&item[0], path) {
                    Value::Int(i) => ArrayKey::Int(i),
                    Value::String(s) => ArrayKey::Str(s),
                    other => panic!("bad key {other:?}"),
                };
                a.insert(key, typed_value_in(&item[1], path));
            }
            Value::Array(a)
        }
        "enum" => {
            let class = class_by_name(s(&j["class"]));
            Value::Object(class.enum_case(s(&j["case"])).expect("enum case"))
        }
        "closure" => Value::Object(Closure::new(|_, _| Ok(Value::Null)).into_ref()),
        "up" => {
            let n = j["n"].as_u64().expect("n") as usize;
            Value::Object(path[path.len() - 1 - n].clone())
        }
        "object" => {
            let obj = match s(&j["class"]) {
                "stdClass" => StdClass::new().into_ref(),
                name => UserObject::create(class_by_name(name)),
            };
            path.push(obj.clone());
            let is_std = obj.class_name() == "stdClass";
            let store = obj.object().property_store().expect("store");
            for p in j["props"].as_array().expect("props") {
                let key = unhex(s(&p[0]));
                let value = typed_value_in(&p[1], path);
                if is_std {
                    store.set(&key, value);
                } else {
                    let (_, name) = unmangle_property_name(&key);
                    store.set(name, value);
                }
            }
            path.pop();
            Value::Object(obj)
        }
        other => panic!("unknown typed value {other}"),
    }
}

/// The typed encoding of a Rust value (the inverse of [`typed_value`], like PHP's `enc()`).
pub fn enc_value(v: &Value) -> Json {
    match v {
        Value::Null => json!({"t": "null"}),
        Value::Bool(b) => json!({"t": "bool", "v": b}),
        Value::Int(i) => json!({"t": "int", "v": i}),
        Value::Float(f) => json!({"t": "float", "bits": to_bits(*f)}),
        Value::String(s) => json!({"t": "string", "hex": hex(s.as_bytes())}),
        Value::Array(a) => {
            let items: Vec<Json> = a
                .iter()
                .map(|(k, v)| json!([enc_value(&k.to_value()), enc_value(v)]))
                .collect();
            json!({"t": "array", "items": items})
        }
        Value::Object(o) => {
            if let Some(case) = o.enum_case_name() {
                return json!({"t": "enum", "class": o.class_name(), "case": case});
            }
            if o.downcast_ref::<Closure>().is_some() {
                return json!({"t": "closure"});
            }
            let arr = convert::to_array(v, &mut Vec::new()).expect("(array)");
            let props: Vec<Json> = arr
                .iter()
                .map(|(k, v)| json!([hex(&k.to_php_string()), enc_value(v)]))
                .collect();
            let mut out = json!({"t": "object", "class": o.class_name(), "props": props});
            if o.class_name() == "S" {
                let s = o.to_php_string(&mut Vec::new()).expect("__toString");
                out["str"] = json!(hex(&s));
            }
            out
        }
    }
}

/// A result in the `res()` format with the message resolved: `{"ok": typed}` or
/// `{"err": [class, hex message]}`.
pub fn res_json(r: &Result<Value, PhpError>) -> Json {
    match r {
        Ok(v) => json!({ "ok": enc_value(v) }),
        Err(e) => json!({ "err": [e.class().name(), hex(e.message().as_bytes())] }),
    }
}

/// A PHP `res()` entry with its message index resolved against `messages`.
pub fn resolve_res(j: &Json, messages: &[String]) -> Json {
    match j.get("err") {
        Some(e) => json!({ "err": [e[0].clone(), messages[e[1].as_u64().expect("idx") as usize]] }),
        None => j.clone(),
    }
}

/// A PHP `diags()` list with message indices resolved.
pub fn resolve_diags(j: &Json, messages: &[String]) -> Vec<(String, String)> {
    j.as_array()
        .expect("diags")
        .iter()
        .map(|p| {
            (
                s(&p[0]).to_string(),
                messages[p[1].as_u64().expect("idx") as usize].clone(),
            )
        })
        .collect()
}

/// The `messages` table of a vector file.
pub fn messages(v: &Json) -> Vec<String> {
    v["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|m| s(m).to_string())
        .collect()
}
