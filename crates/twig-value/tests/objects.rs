//! Object-protocol scenarios checked against PHP 8.5.13 (`tests/vectors/objects.json`).
//!
//! The PHP test classes of `tests/vectors/objects.gen.php` are re-implemented here as Rust host
//! objects through the [`Object`] trait, the way embedding code will expose its own classes. Every
//! scenario of the vector file is replayed by name and compared (result, exception class and
//! message, diagnostics).
//!
//! Regenerate the vectors:
//!
//! ```text
//! cd crates/twig-value/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/objects.gen.php > objects.json
//! ```

use std::sync::{Arc, LazyLock, Mutex};

use serde_json::Value as Json;
use twig_value::object::{
    self, ArrayIterator, ArrayObject, CallArgs, ClassBuilder, ClassRef, Closure, EnumBacking,
    FetchMode, Object, ObjectRef, PropertyCheck, PropertyInfo, PropertyStore, StdClass, Visibility,
    builtin, enum_from, iterator_count, iterator_to_array,
};
use twig_value::{
    Array, ArrayKey, Diagnostic, Diagnostics, Level, PhpError, PhpResult, PhpString, Value,
    compare, convert, types,
};

// ---------------------------------------------------------------------------------------------
// Host classes mirroring objects.gen.php
// ---------------------------------------------------------------------------------------------

static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn log(entry: &str) {
    LOG.lock().unwrap().push(entry.to_string());
}

fn take_log() -> Value {
    let entries = std::mem::take(&mut *LOG.lock().unwrap());
    Value::Array(entries.into_iter().map(Value::from).collect())
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

static P_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("P")
        .property("x", 1)
        .property_info(prop("t", Visibility::Public, Some("int"), None))
        .property_info(prop(
            "n",
            Visibility::Public,
            Some("?int"),
            Some(Value::Null),
        ))
        .property_info(prop("priv", Visibility::Private, None, Some(Value::Int(2))))
        .property_info(prop(
            "prot",
            Visibility::Protected,
            None,
            Some(Value::Int(3)),
        ))
        .property_info(PropertyInfo {
            is_static: true,
            ..prop("st", Visibility::Public, None, Some(Value::Int(4)))
        })
        .property_info(PropertyInfo {
            is_readonly: true,
            ..prop("ro", Visibility::Public, Some("int"), None)
        })
        .method("__construct")
        .method("getX")
        .method_with("hidden", Visibility::Private, false)
        .build()
});

struct P {
    props: PropertyStore,
}

impl P {
    fn new_ref() -> ObjectRef {
        let props = PropertyStore::new(&P_CLASS);
        props.set(b"ro", Value::Int(5));
        ObjectRef::new(P { props })
    }
}

impl Object for P {
    fn class(&self) -> &ClassRef {
        &P_CLASS
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
            "__construct" => Ok(Value::Null),
            "getx" => Ok(self.props.get(b"x").unwrap_or_default()),
            "hidden" => Ok(Value::from("hidden")),
            _ => Err(object::undefined_method(&P_CLASS, method)),
        }
    }
}

static M_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("M")
        .property("real", "r")
        .property_info(prop(
            "secret",
            Visibility::Private,
            None,
            Some(Value::from("s")),
        ))
        .method("__get")
        .method("__isset")
        .method("__call")
        .build()
});

struct M {
    props: PropertyStore,
}

impl Object for M {
    fn class(&self) -> &ClassRef {
        &M_CLASS
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let name = args
            .get(0)
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or_default();
        match method {
            "__get" => Ok(Value::from(format!("get:{name}"))),
            "__isset" => Ok(Value::Bool(name != "no")),
            "__call" => {
                let a = args
                    .get(1)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let keys: Vec<String> = a.keys().map(|k| k.to_string()).collect();
                Ok(Value::from(format!(
                    "call:{name}:{}:{}",
                    a.len(),
                    keys.join(",")
                )))
            }
            _ => Err(object::undefined_method(&M_CLASS, method)),
        }
    }
}

fn m_ref() -> ObjectRef {
    ObjectRef::new(M {
        props: PropertyStore::new(&M_CLASS),
    })
}

/// `S`, `S5`, `SBad`: `__toString()` returning a string, the int 5 and an array.
struct Str {
    class: ClassRef,
    ret: Value,
}

fn str_ref(name: &str, ret: Value) -> ObjectRef {
    ObjectRef::new(Str {
        class: ClassBuilder::class(name).method("__toString").build(),
        ret,
    })
}

impl Object for Str {
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
        match method {
            "__tostring" => Ok(self.ret.clone()),
            _ => Err(object::undefined_method(&self.class, method)),
        }
    }
}

static IT_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("It")
        .implements(&builtin::iterator())
        .method("rewind")
        .method("valid")
        .method("current")
        .method("key")
        .method("next")
        .build()
});

struct It {
    i: Mutex<i64>,
}

impl Object for It {
    fn class(&self) -> &ClassRef {
        &IT_CLASS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let mut i = self.i.lock().unwrap();
        log(method);
        Ok(match method {
            "rewind" => {
                *i = 0;
                Value::Null
            }
            "valid" => Value::Bool(*i < 3),
            "current" => Value::Int(*i * 10),
            "key" => Value::from(["a", "b", "c"].get(*i as usize).copied().unwrap_or("")),
            "next" => {
                *i += 1;
                Value::Null
            }
            _ => return Err(object::undefined_method(&IT_CLASS, method)),
        })
    }
}

fn it_ref() -> ObjectRef {
    ObjectRef::new(It { i: Mutex::new(0) })
}

static AGG_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Agg")
        .implements(&builtin::iterator_aggregate())
        .method("getIterator")
        .build()
});

struct Agg;

impl Object for Agg {
    fn class(&self) -> &ClassRef {
        &AGG_CLASS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "getiterator" => Ok(Value::Object(
                ArrayIterator::new(assoc(&[("x", Value::Int(1)), ("y", Value::Int(2))])).into_ref(),
            )),
            _ => Err(object::undefined_method(&AGG_CLASS, method)),
        }
    }
}

static C_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("C")
        .implements(&builtin::countable())
        .implements(&builtin::array_access())
        .method("count")
        .method("offsetExists")
        .method("offsetGet")
        .method("offsetSet")
        .method("offsetUnset")
        .build()
});

struct C {
    data: Mutex<Array>,
}

impl Object for C {
    fn class(&self) -> &ClassRef {
        &C_CLASS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
        match method {
            "count" => Ok(Value::Int(42)),
            "offsetexists" => {
                log("offsetExists");
                let k =
                    ArrayKey::from_value(&arg(0), twig_value::KeyContext::ArrayKeyExists, diag)?;
                Ok(Value::Bool(self.data.lock().unwrap().contains_key(&k)))
            }
            "offsetget" => {
                log("offsetGet");
                let k = ArrayKey::from_value(&arg(0), twig_value::KeyContext::Read, diag)?;
                Ok(self
                    .data
                    .lock()
                    .unwrap()
                    .get(&k)
                    .cloned()
                    .unwrap_or_default())
            }
            "offsetset" => {
                log("offsetSet");
                let mut data = self.data.lock().unwrap();
                if arg(0).is_null() {
                    data.push(arg(1))?;
                } else {
                    let k = ArrayKey::from_value(&arg(0), twig_value::KeyContext::Write, diag)?;
                    data.insert(k, arg(1));
                }
                Ok(Value::Null)
            }
            "offsetunset" => {
                log("offsetUnset");
                let k = ArrayKey::from_value(&arg(0), twig_value::KeyContext::Unset, diag)?;
                self.data.lock().unwrap().remove(&k);
                Ok(Value::Null)
            }
            _ => Err(object::undefined_method(&C_CLASS, method)),
        }
    }
}

fn c_ref() -> ObjectRef {
    ObjectRef::new(C {
        data: Mutex::new(assoc(&[("a", Value::Int(1)), ("z", Value::Null)])),
    })
}

static SUIT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::enumeration("Suit", Some(EnumBacking::String))
        .case("Hearts", Some(Value::from("H")))
        .case("Spades", Some(Value::from("S")))
        .enum_case_constant("Wild", "Spades")
        .build()
});
static NUM: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::enumeration("Num", Some(EnumBacking::Int))
        .case("One", Some(Value::Int(1)))
        .case("Two", Some(Value::Int(2)))
        .build()
});
static U: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::enumeration("U", None)
        .case("A", None)
        .case("B", None)
        .build()
});

fn case(class: &ClassRef, name: &str) -> ObjectRef {
    class.enum_case(name).expect("enum case")
}

// ---------------------------------------------------------------------------------------------
// PHP builtins used by the scenarios
// ---------------------------------------------------------------------------------------------

fn assoc(pairs: &[(&str, Value)]) -> Array {
    pairs.iter().map(|(k, v)| (*k, v.clone())).collect()
}

fn list(values: Vec<Value>) -> Value {
    Value::Array(Array::from_values(values))
}

fn obj(o: ObjectRef) -> Value {
    Value::Object(o)
}

/// `foreach ($o as $k => $v)` with PHP's iterator call order, or over the visible properties.
fn foreach(o: &ObjectRef, d: &mut dyn Diagnostics) -> PhpResult<Vec<(Value, Value)>> {
    let mut out = Vec::new();
    match o.get_iterator(d)? {
        Some(mut it) => {
            it.rewind(d)?;
            if it.valid(d)? {
                loop {
                    let v = it.current(d)?;
                    let k = it.key(d)?;
                    out.push((k, v));
                    it.next(d)?;
                    if !it.valid(d)? {
                        break;
                    }
                }
            }
        }
        None => {
            for (k, v) in o.visible_properties(d)? {
                out.push((ArrayKey::from_string(k).to_value(), v));
            }
        }
    }
    Ok(out)
}

fn pairs_to_assoc(pairs: Vec<(Value, Value)>, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    let mut a = Array::new();
    for (k, v) in pairs {
        a.insert(
            ArrayKey::from_value(&k, twig_value::KeyContext::Write, d)?,
            v,
        );
    }
    Ok(Value::Array(a))
}

fn class_implements(o: &ObjectRef) -> Value {
    Value::Array(
        o.class()
            .interfaces()
            .iter()
            .map(|i| (i.name(), Value::from(i.name())))
            .collect(),
    )
}

fn class_parents(o: &ObjectRef) -> Value {
    Value::Array(
        o.class()
            .parents()
            .iter()
            .map(|c| (c.name(), Value::from(c.name())))
            .collect(),
    )
}

fn get_class_methods(o: &ObjectRef) -> Value {
    list(
        o.class()
            .public_method_names()
            .iter()
            .map(|m| Value::from(&**m))
            .collect(),
    )
}

fn get_object_vars(o: &ObjectRef, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    Ok(Value::Array(
        o.visible_properties(d)?
            .into_iter()
            .map(|(k, v)| (ArrayKey::from_string(k), v))
            .collect(),
    ))
}

/// `count($v)` for objects.
fn count(o: &ObjectRef, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    match o.count(d)? {
        Some(n) => Ok(Value::Int(n)),
        None => Err(PhpError::type_error(format!(
            "count(): Argument #1 ($value) must be of type Countable|array, {} given",
            types::zend_zval_value_name(&Value::Object(o.clone()))
        ))),
    }
}

fn isset_prop(o: &ObjectRef, name: &str, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    o.has_property(name.as_bytes(), PropertyCheck::Isset, d)
        .map(Value::Bool)
}

fn quiet_prop(o: &ObjectRef, name: &str, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    let v = o.read_property(name.as_bytes(), FetchMode::Isset, d)?;
    Ok(if v.is_null() { Value::from("dflt") } else { v })
}

fn read(o: &ObjectRef, name: &str, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    o.read_property(name.as_bytes(), FetchMode::Read, d)
}

fn dim(o: &ObjectRef, k: impl Into<Value>, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    o.read_dimension(&k.into(), FetchMode::Read, d)
}

fn new_ao(pairs: &[(&str, Value)]) -> ObjectRef {
    ArrayObject::new(assoc(pairs)).into_ref()
}

fn copy(o: &ObjectRef, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    o.call_method("getArrayCopy", CallArgs::new(), d)
}

/// `$o->method()`.
fn call0(o: &ObjectRef, method: &str, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    o.call_method(method, CallArgs::new(), d)
}

/// The object a scenario step returned.
fn object_of(v: Value) -> ObjectRef {
    match v {
        Value::Object(o) => o,
        other => panic!("expected an object, got {other:?}"),
    }
}

/// `final class TwigValueAoSub extends ArrayObject {}` (and the two iterator classes below).
static AO_SUB: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigValueAoSub")
        .final_class()
        .extends(&builtin::array_object())
        .build()
});

static AI_SUB: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigValueAiSub")
        .final_class()
        .extends(&builtin::array_iterator())
        .build()
});

static RAI_SUB: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigValueRaiSub")
        .final_class()
        .extends(&builtin::recursive_array_iterator())
        .build()
});

// ---------------------------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------------------------

fn run(name: &str, d: &mut dyn Diagnostics) -> PhpResult<Value> {
    let p = P::new_ref;
    match name {
        "P read x" => read(&p(), "x", d),
        "P read t" => read(&p(), "t", d),
        "P isset t" => isset_prop(&p(), "t", d),
        "P read n" => read(&p(), "n", d),
        "P isset n" => isset_prop(&p(), "n", d),
        "P isset x" => isset_prop(&p(), "x", d),
        "P read priv" => read(&p(), "priv", d),
        "P isset priv" => isset_prop(&p(), "priv", d),
        "P read prot" => read(&p(), "prot", d),
        "P read st" => read(&p(), "st", d),
        "P read nope" => read(&p(), "nope", d),
        "P read nope quiet" => quiet_prop(&p(), "nope", d),
        "P read t quiet" => quiet_prop(&p(), "t", d),
        "P exists priv" => p().property_exists(b"priv", d).map(Value::Bool),
        "P exists nope" => p().property_exists(b"nope", d).map(Value::Bool),
        "P exists t" => p().property_exists(b"t", d).map(Value::Bool),
        "P write dyn" => {
            let o = p();
            o.write_property(b"dyn", Value::Int(7), d)?;
            read(&o, "dyn", d)
        }
        "P write ro" => p()
            .write_property(b"ro", Value::Int(6), d)
            .map(|_| Value::Null),
        "P write priv" => p()
            .write_property(b"priv", Value::Int(6), d)
            .map(|_| Value::Null),
        "P write t" => {
            let o = p();
            o.write_property(b"t", Value::Int(9), d)?;
            read(&o, "t", d)
        }
        "P unset x read" => {
            let o = p();
            o.unset_property(b"x", d)?;
            read(&o, "x", d)
        }
        "P unset t read" => {
            let o = p();
            o.unset_property(b"t", d)?;
            read(&o, "t", d)
        }
        "P unset ro" => p().unset_property(b"ro", d).map(|_| Value::Null),
        "P array cast" => convert::to_array(&obj(p()), d).map(Value::Array),
        "P object vars" => get_object_vars(&p(), d),
        "P call getX" => p().call_method("GETX", CallArgs::new(), d),
        "P call nope" => p().call_method("nope", CallArgs::new(), d),
        "P call hidden" => p().call_method("hidden", CallArgs::new(), d),
        "P methods" => Ok(get_class_methods(&p())),
        "M read real" => read(&m_ref(), "real", d),
        "M read secret" => read(&m_ref(), "secret", d),
        "M read nope" => read(&m_ref(), "nope", d),
        "M isset real" => isset_prop(&m_ref(), "real", d),
        "M isset secret" => isset_prop(&m_ref(), "secret", d),
        "M isset nope" => isset_prop(&m_ref(), "nope", d),
        "M isset no" => isset_prop(&m_ref(), "no", d),
        "M empty nope" => m_ref()
            .has_property(b"nope", PropertyCheck::NotEmpty, d)
            .map(|b| Value::Bool(!b)),
        "M quiet no" => quiet_prop(&m_ref(), "no", d),
        "M quiet nope" => quiet_prop(&m_ref(), "nope", d),
        "M call foo" => m_ref().call_method("foo", CallArgs::new().arg(1).arg(2), d),
        "M call named" => m_ref().call_method("foo", CallArgs::new().arg(1).named_arg("b", 2), d),
        "M write secret" => m_ref()
            .write_property(b"secret", Value::Int(1), d)
            .map(|_| Value::Null),
        "M exists secret" => m_ref().property_exists(b"secret", d).map(Value::Bool),
        "S string" => {
            convert::to_string(&obj(str_ref("S", Value::from("str"))), d).map(Value::String)
        }
        "S5 string" => convert::to_string(&obj(str_ref("S5", Value::Int(5))), d).map(Value::String),
        "SBad string" => convert::to_string(&obj(str_ref("SBad", Value::Array(Array::new()))), d)
            .map(Value::String),
        "S int" => convert::to_long(&obj(str_ref("S", Value::from("str"))), d).map(Value::Int),
        "S bool" => convert::to_bool(&obj(str_ref("S", Value::from("str"))), d).map(Value::Bool),
        "P string" => convert::to_string(&obj(p()), d).map(Value::String),
        "S implements" => Ok(class_implements(&str_ref("S", Value::from("str")))),
        "It foreach" => {
            take_log();
            let pairs = foreach(&it_ref(), d)?;
            let out = pairs_to_assoc(pairs, d)?;
            Ok(list(vec![out, take_log()]))
        }
        "It to_array" => {
            take_log();
            let a = iterator_to_array(&it_ref(), true, d)?;
            Ok(list(vec![Value::Array(a), take_log()]))
        }
        "It to_array values" => {
            take_log();
            let a = iterator_to_array(&it_ref(), false, d)?;
            Ok(list(vec![Value::Array(a), take_log()]))
        }
        "It count" => {
            take_log();
            let n = iterator_count(&it_ref(), d)?;
            Ok(list(vec![Value::Int(n), take_log()]))
        }
        "Agg to_array" => iterator_to_array(&ObjectRef::new(Agg), true, d).map(Value::Array),
        "Agg count" => iterator_count(&ObjectRef::new(Agg), d).map(Value::Int),
        "P foreach" => {
            let pairs = foreach(&p(), d)?;
            pairs_to_assoc(pairs, d)
        }
        "C count" => count(&c_ref(), d),
        "C read a" => {
            take_log();
            let v = dim(&c_ref(), "a", d)?;
            Ok(list(vec![v, take_log()]))
        }
        "C read quiet zz" => {
            take_log();
            let v = c_ref().read_dimension(&Value::from("zz"), FetchMode::Isset, d)?;
            let v = if v.is_null() { Value::from("dflt") } else { v };
            Ok(list(vec![v, take_log()]))
        }
        "C isset a" => {
            take_log();
            let v = c_ref().has_dimension(&Value::from("a"), false, d)?;
            Ok(list(vec![Value::Bool(v), take_log()]))
        }
        "C isset z" => {
            take_log();
            let v = c_ref().has_dimension(&Value::from("z"), false, d)?;
            Ok(list(vec![Value::Bool(v), take_log()]))
        }
        "C empty a" => {
            take_log();
            let v = !c_ref().has_dimension(&Value::from("a"), true, d)?;
            Ok(list(vec![Value::Bool(v), take_log()]))
        }
        "C write" => {
            take_log();
            let c = c_ref();
            c.write_dimension(None, Value::Int(5), d)?;
            c.write_dimension(Some(&Value::from("k")), Value::Int(6), d)?;
            c.unset_dimension(&Value::from("a"), d)?;
            let v = dim(&c, "k", d)?;
            Ok(list(vec![v, take_log()]))
        }
        "P dim read" => dim(&p(), "x", d),
        "P count" => count(&p(), d),
        "std dyn" => {
            let o = StdClass::new().into_ref();
            o.write_property(b"a", Value::Int(1), d)?;
            o.write_property(b"0", Value::from("zero"), d)?;
            Ok(list(vec![
                Value::Array(convert::to_array(&obj(o.clone()), d)?),
                isset_prop(&o, "a", d)?,
                isset_prop(&o, "b", d)?,
                Value::Bool(o.property_exists(b"a", d)?),
            ]))
        }
        "std read nope" => read(&StdClass::new().into_ref(), "nope", d),
        "std clone" => {
            let o = StdClass::new().into_ref();
            o.write_property(b"a", Value::Int(1), d)?;
            let c = o.clone_object(d)?;
            c.write_property(b"a", Value::Int(2), d)?;
            Ok(list(vec![read(&o, "a", d)?, read(&c, "a", d)?]))
        }
        "object cast array" => {
            let mut a = Array::new();
            a.insert("a", 1);
            a.insert(5, "five");
            let o = convert::to_object(&Value::Array(a), d)?;
            convert::to_array(&obj(o), d).map(Value::Array)
        }
        "object cast scalar" => {
            let o = convert::to_object(&Value::from("x"), d)?;
            convert::to_array(&obj(o), d).map(Value::Array)
        }
        "array cast closure" => {
            let f = Closure::new(|_, _| Ok(Value::Int(1))).into_ref();
            convert::to_array(&obj(f), d).map(|a| Value::Int(a.len() as i64))
        }
        "enum from" => enum_from(&SUIT, &Value::from("H"), false, d),
        "enum from bad" => enum_from(&SUIT, &Value::from("X"), false, d),
        "enum from int" => enum_from(&SUIT, &Value::Int(5), false, d),
        "enum tryFrom bad" => enum_from(&SUIT, &Value::from("X"), true, d),
        "enum int from" => enum_from(&NUM, &Value::from("2"), false, d),
        "enum int from bad" => enum_from(&NUM, &Value::Int(7), false, d),
        "enum int from str" => enum_from(&NUM, &Value::from("abc"), false, d),
        "enum int from float" => enum_from(&NUM, &Value::Float(1.5), false, d),
        "enum cases" => Ok(list(
            SUIT.enum_cases()
                .iter()
                .cloned()
                .map(Value::Object)
                .collect(),
        )),
        "enum cases via case" => case(&SUIT, "Hearts").call_method("cases", CallArgs::new(), d),
        "enum name value" => Ok(list(vec![
            read(&case(&SUIT, "Hearts"), "name", d)?,
            read(&case(&SUIT, "Hearts"), "value", d)?,
            read(&case(&U, "A"), "name", d)?,
        ])),
        "enum unit value" => read(&case(&U, "A"), "value", d),
        "enum write name" => case(&SUIT, "Hearts")
            .write_property(b"name", Value::from("x"), d)
            .map(|_| Value::Null),
        "enum write dyn" => case(&SUIT, "Hearts")
            .write_property(b"foo", Value::from("x"), d)
            .map(|_| Value::Null),
        "enum read nope" => read(&case(&SUIT, "Hearts"), "nope", d),
        "enum clone" => case(&SUIT, "Hearts").clone_object(d).map(Value::Object),
        "enum string" => convert::to_string(&obj(case(&SUIT, "Hearts")), d).map(Value::String),
        "enum int" => convert::to_long(&obj(case(&U, "A")), d).map(Value::Int),
        "enum bool" => convert::to_bool(&obj(case(&U, "A")), d).map(Value::Bool),
        "enum array cast" => convert::to_array(&obj(case(&SUIT, "Hearts")), d).map(Value::Array),
        "enum constant" => Ok(list(vec![
            SUIT.constant("Wild").unwrap_or_default(),
            SUIT.constant("Hearts").unwrap_or_default(),
        ])),
        "enum implements" => Ok(list(vec![
            class_implements(&case(&SUIT, "Hearts")),
            class_implements(&case(&U, "A")),
        ])),
        "enum methods" => Ok(list(vec![
            get_class_methods(&case(&SUIT, "Hearts")),
            get_class_methods(&case(&U, "A")),
        ])),
        "enum identical" => {
            let a = enum_from(&SUIT, &Value::from("H"), false, d)?;
            Ok(Value::Bool(a == obj(case(&SUIT, "Hearts"))))
        }
        "closure invoke" => {
            let f = Closure::new(|args, _| {
                let a = args.get(0).and_then(Value::as_int).unwrap_or(0);
                let b = args.get(1).and_then(Value::as_int).unwrap_or(0);
                Ok(Value::Int(a + b))
            });
            f.into_ref().invoke(CallArgs::new().arg(2).arg(3), d)
        }
        "closure __invoke" => {
            let f = Closure::new(|args, _| {
                Ok(Value::Int(
                    args.get(0).and_then(Value::as_int).unwrap_or(0) * 2,
                ))
            });
            f.into_ref()
                .call_method("__invoke", CallArgs::new().arg(4), d)
        }
        "closure read" => read(&Closure::new(|_, _| Ok(Value::Int(1))).into_ref(), "foo", d),
        "closure write" => Closure::new(|_, _| Ok(Value::Int(1)))
            .into_ref()
            .write_property(b"foo", Value::Int(1), d)
            .map(|_| Value::Null),
        "closure isset" => isset_prop(&Closure::new(|_, _| Ok(Value::Int(1))).into_ref(), "foo", d),
        "closure clone" => {
            let f = Closure::new(|_, _| Ok(Value::Int(7))).into_ref();
            let g = f.clone_object(d)?;
            Ok(list(vec![
                g.invoke(CallArgs::new(), d)?,
                Value::Bool(ObjectRef::ptr_eq(&f, &g)),
            ]))
        }
        "closure string" => {
            convert::to_string(&obj(Closure::new(|_, _| Ok(Value::Int(1))).into_ref()), d)
                .map(Value::String)
        }
        "closure methods" => Ok(get_class_methods(
            &Closure::new(|_, _| Ok(Value::Null)).into_ref(),
        )),
        "not callable" => p().invoke(CallArgs::new(), d),
        "ao read missing" => dim(&new_ao(&[("a", Value::Int(1))]), "zz", d),
        "ao read missing int" => dim(
            &ArrayObject::new(Array::from_values([Value::Int(1)])).into_ref(),
            5,
            d,
        ),
        "ao read null" => dim(&new_ao(&[("", Value::from("e"))]), Value::Null, d),
        "ao read array key" => dim(&new_ao(&[]), Value::Array(Array::new()), d),
        "ao read float" => {
            let mut a = Array::new();
            a.insert(1, "one");
            dim(&ArrayObject::new(a).into_ref(), 1.5, d)
        }
        "ao read numeric string" => {
            let mut a = Array::new();
            a.insert(10, "ten");
            dim(&ArrayObject::new(a).into_ref(), "10", d)
        }
        "ao quiet missing" => {
            let v = new_ao(&[]).read_dimension(&Value::from("zz"), FetchMode::Isset, d)?;
            Ok(if v.is_null() { Value::from("dflt") } else { v })
        }
        "ao offsetExists null" => new_ao(&[("", Value::Null)]).call_method(
            "offsetExists",
            CallArgs::new().arg(Value::Null),
            d,
        ),
        "ao offsetExists null value" => {
            new_ao(&[("a", Value::Null)]).call_method("offsetExists", CallArgs::new().arg("a"), d)
        }
        "ao isset null value" => new_ao(&[("a", Value::Null)])
            .has_dimension(&Value::from("a"), false, d)
            .map(Value::Bool),
        "ao isset array key" => new_ao(&[])
            .has_dimension(&Value::Array(Array::new()), false, d)
            .map(Value::Bool),
        "ao empty" => Ok(list(vec![
            Value::Bool(!new_ao(&[("a", Value::Int(0))]).has_dimension(
                &Value::from("a"),
                true,
                d,
            )?),
            Value::Bool(!new_ao(&[("a", Value::Int(1))]).has_dimension(
                &Value::from("a"),
                true,
                d,
            )?),
        ])),
        "ao write" => {
            let mut a = Array::new();
            a.insert(5, "a");
            let ao = ArrayObject::new(a).into_ref();
            ao.write_dimension(None, Value::from("b"), d)?;
            ao.write_dimension(Some(&Value::Null), Value::from("c"), d)?;
            ao.write_dimension(Some(&Value::from("k")), Value::from("d"), d)?;
            ao.call_method("append", CallArgs::new().arg("e"), d)?;
            ao.call_method("offsetSet", CallArgs::new().arg(Value::Null).arg("f"), d)?;
            ao.unset_dimension(&Value::Int(5), d)?;
            ao.call_method("offsetUnset", CallArgs::new().arg("k"), d)?;
            copy(&ao, d)
        }
        "ao unset null" => {
            let ao = new_ao(&[("", Value::Int(1)), ("a", Value::Int(2))]);
            ao.unset_dimension(&Value::Null, d)?;
            copy(&ao, d)
        }
        "ao write array key" => new_ao(&[])
            .write_dimension(Some(&Value::Array(Array::new())), Value::Int(1), d)
            .map(|_| Value::Null),
        "ao unset array key" => new_ao(&[])
            .unset_dimension(&Value::Array(Array::new()), d)
            .map(|_| Value::Null),
        "ao count" => Ok(list(vec![
            count(
                &ArrayObject::new((1..=3).map(Value::Int).collect()).into_ref(),
                d,
            )?,
            ArrayObject::new((1..=2).map(Value::Int).collect())
                .into_ref()
                .call_method("count", CallArgs::new(), d)?,
        ])),
        "ao iterate" => {
            let mut a = Array::new();
            a.insert("a", 1);
            a.insert(3, "x");
            iterator_to_array(&ArrayObject::new(a).into_ref(), true, d).map(Value::Array)
        }
        "ao foreach" => {
            let mut a = Array::new();
            a.insert("a", 1);
            a.insert(3, "x");
            let pairs = foreach(&ArrayObject::new(a).into_ref(), d)?;
            Ok(list(
                pairs.into_iter().map(|(k, v)| list(vec![k, v])).collect(),
            ))
        }
        "ao array cast" => {
            let mut a = Array::new();
            a.insert("a", 1);
            a.insert(2, "b");
            convert::to_array(&obj(ArrayObject::new(a).into_ref()), d).map(Value::Array)
        }
        "ao object vars" => get_object_vars(&new_ao(&[("a", Value::Int(1))]), d),
        "ao exchange" => {
            let ao = ArrayObject::new(Array::from_values([Value::Int(1)])).into_ref();
            let old = ao.call_method(
                "exchangeArray",
                CallArgs::new().arg(Value::Array(assoc(&[("n", Value::Int(2))]))),
                d,
            )?;
            Ok(list(vec![old, copy(&ao, d)?]))
        }
        "ao flags" => {
            let ao = ArrayObject::with_flags(Array::new(), 2).into_ref();
            let f1 = ao.call_method("getFlags", CallArgs::new(), d)?;
            ao.call_method("setFlags", CallArgs::new().arg(1), d)?;
            Ok(list(vec![
                f1,
                ao.call_method("getFlags", CallArgs::new(), d)?,
            ]))
        }
        "ao as props" => {
            let ao = ArrayObject::with_flags(assoc(&[("foo", Value::from("bar"))]), 2).into_ref();
            let mut r = vec![
                read(&ao, "foo", d)?,
                isset_prop(&ao, "foo", d)?,
                isset_prop(&ao, "nope", d)?,
            ];
            ao.write_property(b"baz", Value::Int(1), d)?;
            r.push(copy(&ao, d)?);
            Ok(list(r))
        }
        "ao no props" => read(&new_ao(&[("foo", Value::from("bar"))]), "foo", d),
        "ao getIterator" => {
            let ao = new_ao(&[("a", Value::Int(1))]);
            let it = ao.call_method("getIterator", CallArgs::new(), d)?;
            let it = it.as_object().cloned().expect("iterator object");
            it.write_dimension(Some(&Value::from("b")), Value::Int(2), d)?;
            Ok(list(vec![
                Value::from(it.class_name()),
                copy(&ao, d)?,
                ao.call_method("getIteratorClass", CallArgs::new(), d)?,
            ]))
        }
        "ao clone" => {
            let ao = new_ao(&[("a", Value::Int(1))]);
            let c = ao.clone_object(d)?;
            c.write_dimension(Some(&Value::from("b")), Value::Int(2), d)?;
            Ok(list(vec![copy(&ao, d)?, copy(&c, d)?]))
        }
        "ao implements" => {
            let ao = new_ao(&[]);
            Ok(list(vec![class_implements(&ao), class_parents(&ao)]))
        }
        "ao methods" => Ok(get_class_methods(&new_ao(&[]))),
        "ao constants" => {
            let c = builtin::array_object();
            Ok(list(vec![
                c.constant("STD_PROP_LIST").unwrap_or_default(),
                c.constant("ARRAY_AS_PROPS").unwrap_or_default(),
            ]))
        }
        "ao construct bad" => {
            let ao = new_ao(&[]);
            ao.call_method(
                "__construct",
                CallArgs::new().arg(Value::Array(Array::new())).arg("x"),
                d,
            )
        }
        "ao setFlags bad" => new_ao(&[]).call_method("setFlags", CallArgs::new().arg("x"), d),
        "ao offsetGet arity" => new_ao(&[]).call_method("offsetGet", CallArgs::new(), d),
        "ao getArrayCopy arity" => {
            new_ao(&[]).call_method("getArrayCopy", CallArgs::new().arg(1), d)
        }
        "ai iterate" => {
            let it =
                ArrayIterator::new(assoc(&[("a", Value::Int(1)), ("b", Value::Int(2))])).into_ref();
            iterator_to_array(&it, true, d).map(Value::Array)
        }
        "ai manual" => {
            let it =
                ArrayIterator::new(assoc(&[("a", Value::Int(1)), ("b", Value::Int(2))])).into_ref();
            let c = |m: &str, d: &mut dyn Diagnostics| it.call_method(m, CallArgs::new(), d);
            let mut r = vec![c("valid", d)?, c("key", d)?, c("current", d)?];
            c("next", d)?;
            r.push(c("key", d)?);
            c("next", d)?;
            r.push(c("valid", d)?);
            r.push(c("key", d)?);
            r.push(c("current", d)?);
            c("rewind", d)?;
            r.push(c("current", d)?);
            Ok(list(r))
        }
        "ai seek" => {
            let it = ArrayIterator::new((1..=3).map(|i| Value::Int(i * 10)).collect()).into_ref();
            it.call_method("seek", CallArgs::new().arg(2), d)?;
            it.call_method("current", CallArgs::new(), d)
        }
        "ai seek bad" => {
            let it = ArrayIterator::new((1..=3).map(|i| Value::Int(i * 10)).collect()).into_ref();
            it.call_method("seek", CallArgs::new().arg(3), d)
        }
        "ai seek negative" => {
            let it = ArrayIterator::new(Array::from_values([Value::Int(10)])).into_ref();
            it.call_method("seek", CallArgs::new().arg(-1), d)
        }
        "ai count" => count(
            &ArrayIterator::new((1..=2).map(Value::Int).collect()).into_ref(),
            d,
        ),
        "ai implements" => Ok(class_implements(
            &ArrayIterator::new(Array::new()).into_ref(),
        )),
        "ai methods" => Ok(get_class_methods(
            &ArrayIterator::new(Array::new()).into_ref(),
        )),
        "ai foreach" => {
            let it =
                ArrayIterator::new(assoc(&[("x", Value::Int(1)), ("y", Value::Int(2))])).into_ref();
            let pairs = foreach(&it, d)?;
            pairs_to_assoc(pairs, d)
        }
        "ai clone shares storage" => {
            let it = ArrayIterator::new(assoc(&[("a", Value::Int(1))])).into_ref();
            let c = it.clone_object(d)?;
            c.write_dimension(Some(&Value::from("b")), Value::Int(2), d)?;
            copy(&it, d)
        }
        "rai children" => {
            let it = ArrayIterator::recursive(assoc(&[
                ("a", list(vec![Value::Int(1), Value::Int(2)])),
                ("b", Value::Int(3)),
                (
                    "c",
                    obj(ArrayObject::new(Array::from_values([Value::Int(5)])).into_ref()),
                ),
            ]))
            .into_ref();
            // foreach ($it as $k => $v) { $r[] = [$k, $it->hasChildren()]; }
            let mut r = Vec::new();
            call0(&it, "rewind", d)?;
            while convert::to_bool(&call0(&it, "valid", d)?, d)? {
                r.push(list(vec![
                    call0(&it, "key", d)?,
                    call0(&it, "hasChildren", d)?,
                ]));
                call0(&it, "next", d)?;
            }
            call0(&it, "rewind", d)?;
            let child = object_of(call0(&it, "getChildren", d)?);
            r.push(list(vec![
                Value::from(child.class_name()),
                copy(&child, d)?,
                call0(&child, "getFlags", d)?,
            ]));
            for _ in 0..3 {
                call0(&it, "next", d)?;
            }
            r.push(list(vec![
                call0(&it, "valid", d)?,
                call0(&it, "hasChildren", d)?,
                call0(&it, "getChildren", d)?,
            ]));
            Ok(list(r))
        }
        "rai empty" => Ok(list(vec![
            call0(
                &ArrayIterator::recursive(Array::new()).into_ref(),
                "hasChildren",
                d,
            )?,
            call0(
                &ArrayIterator::recursive(Array::new()).into_ref(),
                "getChildren",
                d,
            )?,
        ])),
        "rai child flags" => {
            let it = ArrayIterator::with_class(
                builtin::recursive_array_iterator(),
                assoc(&[("a", list(vec![Value::Int(1)]))]),
                3,
            )
            .into_ref();
            let child = object_of(call0(&it, "getChildren", d)?);
            Ok(list(vec![
                call0(&child, "getFlags", d)?,
                call0(&it, "getFlags", d)?,
            ]))
        }
        "rai child arrays only" => {
            let it = ArrayIterator::with_class(
                builtin::recursive_array_iterator(),
                assoc(&[
                    ("a", list(vec![Value::Int(1), Value::Int(2)])),
                    (
                        "o",
                        obj(ArrayObject::new(Array::from_values([Value::Int(5)])).into_ref()),
                    ),
                ]),
                4,
            )
            .into_ref();
            let mut r = vec![call0(&it, "hasChildren", d)?];
            let child = object_of(call0(&it, "getChildren", d)?);
            r.push(copy(&child, d)?);
            r.push(call0(&child, "getFlags", d)?);
            call0(&it, "next", d)?;
            r.push(call0(&it, "hasChildren", d)?);
            r.push(call0(&it, "getChildren", d)?);
            Ok(list(r))
        }
        "rai object child of its class" => {
            let inner = ArrayIterator::recursive(Array::from_values([Value::Int(1)])).into_ref();
            let it = ArrayIterator::recursive(assoc(&[("i", obj(inner.clone()))])).into_ref();
            let child = object_of(call0(&it, "getChildren", d)?);
            Ok(list(vec![
                call0(&it, "hasChildren", d)?,
                Value::Bool(ObjectRef::ptr_eq(&child, &inner)),
            ]))
        }
        "rai scalar child" => call0(
            &ArrayIterator::recursive(Array::from_values([Value::Int(5)])).into_ref(),
            "getChildren",
            d,
        ),
        "rai implements sorted" => {
            let mut names: Vec<String> = ArrayIterator::recursive(Array::new())
                .into_ref()
                .class()
                .interfaces()
                .iter()
                .map(|i| i.name().to_owned())
                .collect();
            names.sort();
            Ok(list(names.into_iter().map(Value::from).collect()))
        }
        "rai parents" => Ok(class_parents(
            &ArrayIterator::recursive(Array::new()).into_ref(),
        )),
        "rai methods" => Ok(get_class_methods(
            &ArrayIterator::recursive(Array::new()).into_ref(),
        )),
        "rai constant" => Ok(builtin::recursive_array_iterator()
            .constant("CHILD_ARRAYS_ONLY")
            .unwrap_or_default()),
        "rai iterate" => {
            let it = ArrayIterator::recursive(assoc(&[
                ("a", list(vec![Value::Int(1)])),
                ("b", Value::Int(2)),
            ]))
            .into_ref();
            iterator_to_array(&it, true, d).map(Value::Array)
        }
        "rai debug" => get_object_vars(
            &ArrayIterator::recursive(assoc(&[("x", Value::Int(1))])).into_ref(),
            d,
        ),
        "rai clone class" => {
            let it = ArrayIterator::recursive(Array::from_values([Value::Int(1)])).into_ref();
            Ok(Value::from(it.clone_object(d)?.class_name()))
        }
        "rai haschildren arity" => ArrayIterator::recursive(Array::new())
            .into_ref()
            .call_method("hasChildren", CallArgs::new().arg(1), d),
        "ai haschildren" => call0(
            &ArrayIterator::new(Array::from_values([list(vec![Value::Int(1)])])).into_ref(),
            "hasChildren",
            d,
        ),
        "ao rai iterator class" => {
            let ao = ArrayObject::new(assoc(&[("a", list(vec![Value::Int(1)]))])).into_ref();
            ao.call_method(
                "__construct",
                CallArgs::new()
                    .arg(Value::Array(assoc(&[("a", list(vec![Value::Int(1)]))])))
                    .arg(0)
                    .arg("RecursiveArrayIterator"),
                d,
            )?;
            let it = object_of(call0(&ao, "getIterator", d)?);
            let child = object_of(call0(&it, "getChildren", d)?);
            Ok(list(vec![
                Value::from(it.class_name()),
                call0(&it, "hasChildren", d)?,
                Value::from(child.class_name()),
            ]))
        }
        "ao subclass" => {
            let data = assoc(&[("k", Value::from("v")), ("n", list(vec![Value::Int(1)]))]);
            let o = ArrayObject::with_class(AO_SUB.clone(), data.clone(), 0).into_ref();
            let c = o.clone_object(d)?;
            let iterator = object_of(call0(&o, "getIterator", d)?);
            Ok(list(vec![
                Value::from(o.class_name()),
                Value::Bool(o.instance_of("ArrayObject")),
                dim(&o, "k", d)?,
                count(&o, d)?,
                Value::from(iterator.class_name()),
                iterator_to_array(&o, true, d).map(Value::Array)?,
                Value::from(c.class_name()),
                Value::Bool(compare::equals(&obj(c.clone()), &obj(o.clone()), d)?),
                Value::Bool(compare::equals(
                    &obj(o.clone()),
                    &obj(ArrayObject::new(data).into_ref()),
                    d,
                )?),
                get_object_vars(&o, d)?,
                class_parents(&o),
            ]))
        }
        "ai subclass" => {
            let data = assoc(&[("a", Value::Int(1)), ("b", Value::Int(2))]);
            let it = ArrayIterator::with_class(AI_SUB.clone(), data.clone(), 0).into_ref();
            call0(&it, "next", d)?;
            let c = it.clone_object(d)?;
            Ok(list(vec![
                Value::from(it.class_name()),
                call0(&it, "key", d)?,
                iterator_to_array(&it, true, d).map(Value::Array)?,
                Value::from(c.class_name()),
                Value::Bool(compare::equals(
                    &obj(it.clone()),
                    &obj(ArrayIterator::new(data).into_ref()),
                    d,
                )?),
                get_object_vars(&it, d)?,
            ]))
        }
        "rai subclass children" => {
            let it = ArrayIterator::with_class(
                RAI_SUB.clone(),
                assoc(&[("a", list(vec![Value::Int(1), list(vec![Value::Int(2)])]))]),
                0,
            )
            .into_ref();
            let child = object_of(call0(&it, "getChildren", d)?);
            Ok(list(vec![
                Value::from(child.class_name()),
                call0(&child, "hasChildren", d)?,
                copy(&child, d)?,
            ]))
        }
        other => panic!("scenario {other:?} is not implemented in tests/objects.rs"),
    }
}

// ---------------------------------------------------------------------------------------------
// Comparison with the vectors
// ---------------------------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn deep(v: &Value) -> Json {
    match v {
        Value::Null => serde_json::json!({"t": "null"}),
        Value::Bool(b) => serde_json::json!({"t": "bool", "v": b}),
        Value::Int(i) => serde_json::json!({"t": "int", "v": i}),
        Value::Float(f) => {
            serde_json::json!({"t": "float", "bits": format!("{:016x}", f.to_bits())})
        }
        Value::String(s) => serde_json::json!({"t": "string", "hex": hex(s.as_bytes())}),
        Value::Array(a) => {
            let e: Vec<Json> = a
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        ArrayKey::Int(i) => serde_json::json!({"i": i}),
                        ArrayKey::Str(s) => serde_json::json!({"s": hex(s.as_bytes())}),
                    };
                    serde_json::json!([key, deep(v)])
                })
                .collect();
            serde_json::json!({"t": "array", "e": e})
        }
        Value::Object(o) => match o.enum_case_name() {
            Some(case) => serde_json::json!({"t": "object", "class": o.class_name(), "case": case}),
            None => serde_json::json!({"t": "object", "class": o.class_name()}),
        },
    }
}

fn diags(d: &[Diagnostic]) -> Json {
    Json::Array(
        d.iter()
            .map(|d| {
                let level = match d.level {
                    Level::Warning => "W",
                    Level::Notice => "N",
                    Level::Deprecated => "D",
                    Level::UserDeprecated => "UD",
                };
                serde_json::json!([level, hex(d.message.as_bytes())])
            })
            .collect(),
    )
}

fn unhex_lossy(h: &str) -> String {
    let bytes: Vec<u8> = (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn object_scenarios() {
    let path = format!("{}/tests/vectors/objects.json", env!("CARGO_MANIFEST_DIR"));
    let v: Json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut failures = Vec::new();
    let scenarios = v["scenarios"].as_array().unwrap();
    for sc in scenarios {
        let name = sc["name"].as_str().unwrap();
        let mut d: Vec<Diagnostic> = Vec::new();
        let r = run(name, &mut d);
        let got = match &r {
            Ok(v) => serde_json::json!({"ok": deep(v)}),
            Err(e) => serde_json::json!({"err": [e.class().name(), hex(e.message().as_bytes())]}),
        };
        if got != sc["result"] {
            let readable = |j: &Json| match j.get("err") {
                Some(e) => format!("{}: {}", e[0], unhex_lossy(e[1].as_str().unwrap_or(""))),
                None => j.to_string(),
            };
            failures.push(format!(
                "{name}: got {}, want {}",
                readable(&got),
                readable(&sc["result"])
            ));
        }
        if diags(&d) != sc["diag"] {
            let want: Vec<String> = sc["diag"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}",
                        p[0].as_str().unwrap(),
                        unhex_lossy(p[1].as_str().unwrap())
                    )
                })
                .collect();
            failures.push(format!("{name}: diagnostics {d:?}, want {want:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} object scenarios differ from PHP:\n{}",
        failures.len(),
        failures.join("\n")
    );
    eprintln!("objects: {} scenarios match PHP", scenarios.len());
}
