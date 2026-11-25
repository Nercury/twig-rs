//! Rust implementations of the PHP classes the fixtures' DATA uses, and their construction from
//! typed trees.
//!
//! | class | implementation |
//! |---|---|
//! | `stdClass`, `ArrayObject`, `ArrayIterator` | `twig-value` |
//! | `Twig\Tests\TwigTestFoo`, `MagicCallStub`, `ToStringStub`, `CountableStub`, `IteratorAggregateStub`, `SimpleIteratorForTesting` | [`userland`] (TWIG/tests/IntegrationTest.php) |
//! | `Twig\Tests\DummyBackedEnum`, `DummyUnitEnum` | enums in [`fixture_class_table`] (TWIG/tests/Dummy*Enum.php) |
//! | inline DATA classes (`TwigTestStringy`, `TwigTestHolder`, `TestClassForMagicCallAttributes`, `MyObj`, `UserForAutoEscapeTest`, `ItemsIterator`, `ItemsIteratorCountable`, two anonymous classes) | [`userland`] |
//! | `Generator` (returned by a ported generator method) | [`generator`] |
//! | `SplObjectStorage`, `SplFileInfo`, `CallbackFilterIterator` | [`spl`] |
//! | `SimpleXMLElement` | [`simplexml`] |
//! | `Twig\Markup`, `Twig\TemplateWrapper`, `DateTime*`, `DateTimeZone`, `DateInterval` | the caller's [`ObjectFactory`](crate::ObjectFactory) |

pub mod generator;
pub mod simplexml;
pub mod spl;
pub mod userland;

use std::sync::{Arc, LazyLock};

use twig_value::object::builtin::ClassTable;
use twig_value::object::{
    ArrayIterator, ArrayObject, CallArgs, ClassBuilder, ClassRef, EnumBacking, ObjectRef, StdClass,
    Visibility, unmangle_property_name,
};
use twig_value::{Diagnostic, PhpString, Value};

use crate::convert::{Converter, Unsupported};
use crate::typed::{Key, ObjectNode, TypedNode};
use userland::{HiddenState, UserClass, UserObject};

static CLASS_TABLE: LazyLock<ClassTable> = LazyLock::new(|| {
    let mut t = ClassTable::with_builtins();
    let classes = [
        UserClass::TwigTestFoo,
        UserClass::MagicCallStub,
        UserClass::ToStringStub,
        UserClass::CountableStub,
        UserClass::IteratorAggregateStub,
        UserClass::SimpleIteratorForTesting,
    ]
    .into_iter()
    .map(|c| c.class_entry(""))
    .chain([
        ClassBuilder::enumeration("Twig\\Tests\\DummyBackedEnum", Some(EnumBacking::String))
            .case("FOO", Some(Value::from("foo")))
            .case("BAR", Some(Value::from("bar")))
            .build(),
        ClassBuilder::enumeration("Twig\\Tests\\DummyUnitEnum", None)
            .case("BAR", None)
            .case("BAZ", None)
            .build(),
        spl::spl_object_storage_class(),
        spl::spl_file_info_class(),
        spl::outer_iterator_interface(),
        spl::iterator_iterator_class(),
        spl::filter_iterator_class(),
        spl::callback_filter_iterator_class(),
        simplexml::recursive_iterator_interface(),
        simplexml::simple_xml_element_class(),
        generator::generator_class(),
    ]);
    for class in classes {
        // The names are distinct; a failure would be a bug in this list.
        let _ = t.register(class);
    }
    t
});

/// The classes a template can name in the fixture harness (`constant('Twig\\Tests\\TwigTestFoo::BAR_NAME')`,
/// `enum('Twig\\Tests\\DummyBackedEnum')`): `twig-value`'s built-ins, the helper classes of
/// `tests/IntegrationTest.php` (loaded by the harness), the `Twig\Tests` dummy enums (autoloaded on
/// demand) and the internal classes this crate implements. Classes declared inline by one fixture's
/// DATA are not listed: in PHP they only exist after that DATA ran.
pub fn fixture_class_table() -> &'static ClassTable {
    &CLASS_TABLE
}

/// The name of a property entry (`get_object_vars()` turns numeric names into int keys).
fn property_name(key: &Key) -> PhpString {
    match key {
        Key::Int(i) => PhpString::from(i.to_string()),
        Key::Str(s) => s.clone(),
    }
}

fn sorted_interfaces(class: &ClassRef) -> Vec<String> {
    let mut v: Vec<String> = class
        .interfaces()
        .iter()
        .map(|i| i.name().to_string())
        .collect();
    v.sort();
    v
}

/// Checks the class-describing fields of a node against a port's class entry.
fn check_class(o: &ObjectNode, class: &ClassRef, cx: &mut Converter<'_>) -> bool {
    if let Some(expected) = &o.interfaces
        && *expected != sorted_interfaces(class)
    {
        cx.mismatch(&o.class, format!("interfaces {expected:?}"));
        return false;
    }
    if let Some(methods) = &o.methods {
        let ours: Vec<String> = class
            .public_method_names()
            .iter()
            .map(|m| m.to_string())
            .collect();
        if *methods != ours {
            cx.mismatch(&o.class, format!("methods {methods:?}, port has {ours:?}"));
            return false;
        }
    }
    true
}

/// Builds the object for `o` when this crate implements its class; `None` otherwise.
pub(crate) fn build(o: &ObjectNode, cx: &mut Converter<'_>) -> Option<Value> {
    Some(match o.class.as_str() {
        "stdClass" => std_class(o, cx),
        "ArrayObject" | "ArrayIterator" => array_object(o, cx),
        "SplObjectStorage" => storage(o, cx),
        "SplFileInfo" => file_info(o, cx),
        "CallbackFilterIterator" => callback_filter(o, cx),
        "SimpleXMLElement" => simple_xml(o, cx),
        _ => userland_object(UserClass::from_name(&o.class)?, o, cx),
    })
}

fn std_class(o: &ObjectNode, cx: &mut Converter<'_>) -> Value {
    let obj = StdClass::new().into_ref();
    let v = Value::Object(obj.clone());
    cx.register(o.id, v.clone());
    if o.interfaces.as_ref().is_some_and(|i| !i.is_empty()) {
        cx.mismatch("stdClass", "interfaces");
    }
    if o.props.is_none() {
        cx.invalid("stdClass node without props");
    }
    let Some(store) = obj.downcast_ref::<StdClass>().map(StdClass::properties) else {
        return v;
    };
    for item in o.props.iter().flatten() {
        let name = property_name(&item.key);
        let value = cx.child(&format!("->{}", name.to_string_lossy()), &item.value);
        store.set(name.as_bytes(), value);
    }
    v
}

fn array_object(o: &ObjectNode, cx: &mut Converter<'_>) -> Value {
    let is_iterator = o.class == "ArrayIterator";
    let obj = if is_iterator {
        ArrayIterator::new(twig_value::Array::new()).into_ref()
    } else {
        ArrayObject::new(twig_value::Array::new()).into_ref()
    };
    let v = Value::Object(obj.clone());
    cx.register(o.id, v.clone());
    let Some(entries @ TypedNode::Array(_)) = &o.entries else {
        cx.invalid(format!("{} node without entries", o.class));
        return v;
    };
    let array = cx.child(".entries", entries);
    // flags / iteratorClass: only rich trees record them (corpus trees: the defaults).
    let mut args = vec![array, Value::Int(o.flags.unwrap_or(0))];
    if !is_iterator && let Some(class) = &o.iterator_class {
        args.push(Value::from(class.as_str()));
    }
    let mut diag: Vec<Diagnostic> = Vec::new();
    if let Err(e) = obj.invoke_method("__construct", CallArgs::from(args), &mut diag) {
        cx.invalid(format!("{}::__construct() failed: {e}", o.class));
    }
    v
}

fn storage(o: &ObjectNode, cx: &mut Converter<'_>) -> Value {
    let ok = check_class(o, &spl::spl_object_storage_class(), cx);
    let obj = spl::SplObjectStorage::new().into_ref();
    let v = Value::Object(obj.clone());
    cx.register(o.id, v.clone());
    if !ok {
        return v;
    }
    let Some(entries) = &o.storage else {
        let path = cx.path();
        cx.problem(Unsupported::MissingHiddenState {
            path,
            class: o.class.clone(),
        });
        return v;
    };
    let Some(s) = obj.downcast_ref::<spl::SplObjectStorage>() else {
        return v;
    };
    for (i, (key, info)) in entries.iter().enumerate() {
        let key = cx.child(&format!(".storage[{i}].obj"), key);
        let info = cx.child(&format!(".storage[{i}].inf"), info);
        match key {
            Value::Object(k) => s.attach(k, info),
            _ => cx.invalid("SplObjectStorage key is not an object"),
        }
    }
    v
}

fn file_info(o: &ObjectNode, cx: &mut Converter<'_>) -> Value {
    let path = o.to_string.as_ref().and_then(TypedNode::as_bytes).cloned();
    let info = spl::SplFileInfo::new(path.as_deref().unwrap_or_default());
    let ok = check_class(o, &spl::spl_file_info_class(), cx);
    let v = Value::Object(info.into_ref());
    cx.register(o.id, v.clone());
    if ok && path.is_none() {
        cx.invalid("SplFileInfo node without toString");
    }
    v
}

fn callback_filter(o: &ObjectNode, cx: &mut Converter<'_>) -> Value {
    let it = spl::CallbackFilterIterator::uninitialised();
    let ok = check_class(o, &spl::callback_filter_iterator_class(), cx);
    let obj = ObjectRef::new(it);
    let v = Value::Object(obj.clone());
    cx.register(o.id, v.clone());
    if !ok {
        return v;
    }
    let (Some(inner), Some(accepts)) = (&o.inner, &o.accepts) else {
        let path = cx.path();
        cx.problem(Unsupported::MissingHiddenState {
            path,
            class: o.class.clone(),
        });
        return v;
    };
    let inner = cx.child(".inner", inner);
    let constant = if accepts.iter().all(|a| *a) {
        // An inner iterator without elements never calls the callback: any constant will do.
        !accepts.is_empty()
    } else if accepts.iter().all(|a| !*a) {
        false
    } else {
        let path = cx.path();
        cx.problem(Unsupported::NonConstantCallback { path });
        return v;
    };
    let Value::Object(inner) = inner else {
        cx.invalid("CallbackFilterIterator inner iterator is not an object");
        return v;
    };
    let mut diag: Vec<Diagnostic> = Vec::new();
    if let Some(cfi) = obj.downcast_ref::<spl::CallbackFilterIterator>()
        && let Err(e) = cfi.init(inner, spl::constant_closure(constant), &mut diag)
    {
        cx.invalid(format!("CallbackFilterIterator: {e}"));
    }
    v
}

fn simple_xml(o: &ObjectNode, cx: &mut Converter<'_>) -> Value {
    let doc = match &o.xml {
        Some(Some(text)) => simplexml::parse_document(text),
        Some(None) => Err("asXML() returned false".to_string()),
        None => {
            cx.invalid("SimpleXMLElement node without xml");
            cx.register(o.id, Value::Null);
            return Value::Null;
        }
    };
    match doc {
        Ok(doc) => {
            let elem = simplexml::SimpleXmlElement::root(Arc::new(doc));
            check_class(o, &simplexml::simple_xml_element_class(), cx);
            let v = Value::Object(elem.into_ref());
            cx.register(o.id, v.clone());
            v
        }
        Err(reason) => {
            let path = cx.path();
            cx.problem(Unsupported::Xml { path, reason });
            cx.register(o.id, Value::Null);
            Value::Null
        }
    }
}

fn userland_object(kind: UserClass, o: &ObjectNode, cx: &mut Converter<'_>) -> Value {
    let obj = UserObject::new_without_constructor(kind, &o.class);
    let class = twig_value::object::Object::class(&obj).clone();
    let ok = check_class(o, &class, cx);
    let handle = ObjectRef::new(obj);
    let v = Value::Object(handle.clone());
    cx.register(o.id, v.clone());
    if !ok {
        return v;
    }
    let Some(obj) = handle.downcast_ref::<UserObject>() else {
        return v;
    };
    let store = obj.properties();
    for item in o.props.iter().flatten() {
        let name = property_name(&item.key);
        if let Some(p) = class.property(name.as_bytes())
            && (p.visibility != Visibility::Public || p.is_static)
        {
            cx.mismatch(
                &o.class,
                format!(
                    "{} is not a public instance property",
                    name.to_string_lossy()
                ),
            );
            continue;
        }
        let value = cx.child(&format!("->{}", name.to_string_lossy()), &item.value);
        store.set(name.as_bytes(), value);
    }
    match &o.props_hidden {
        Some(hidden) => {
            for item in hidden {
                let Key::Str(mangled) = &item.key else {
                    cx.invalid("hidden property with an int key");
                    continue;
                };
                let (scope, name) = unmangle_property_name(mangled.as_bytes());
                let declared = class.property(name).filter(|p| match scope {
                    Some(b"*") => p.visibility == Visibility::Protected,
                    Some(c) => {
                        p.visibility == Visibility::Private && p.declaring_class.as_bytes() == c
                    }
                    None => false,
                });
                if declared.is_none() {
                    cx.mismatch(
                        &o.class,
                        format!(
                            "hidden property {:?} is not declared by the port",
                            mangled.to_string_lossy()
                        ),
                    );
                    continue;
                }
                let name = PhpString::from(name);
                let value = cx.child(&format!("->{}", name.to_string_lossy()), &item.value);
                store.set(name.as_bytes(), value);
            }
        }
        None => match kind.hidden_state() {
            HiddenState::NotNeeded => {}
            HiddenState::FromToString(prop) => {
                match o.to_string.as_ref().and_then(TypedNode::as_bytes) {
                    Some(s) => store.set(prop.as_bytes(), Value::String(s.clone())),
                    None => {
                        let path = cx.path();
                        cx.problem(Unsupported::MissingHiddenState {
                            path,
                            class: o.class.clone(),
                        });
                    }
                }
            }
            HiddenState::Required => {
                let path = cx.path();
                cx.problem(Unsupported::MissingHiddenState {
                    path,
                    class: o.class.clone(),
                });
            }
        },
    }
    v
}
