//! `twig_value::Value` → typed tree: a port of `TwigOracle\typed()` (research/php-oracle/lib.php)
//! plus the extras of `fixture_data.gen.php`, evaluated on the Rust objects.
//!
//! Encoding a converted value and comparing the result with the source tree checks that the Rust
//! objects present the same public face as PHP's (interfaces, `__toString()` results, visible
//! properties, method lists, hidden properties). Like the PHP encoder it calls methods
//! (`__toString()`, iteration of `CallbackFilterIterator`s), so it can change object state the same
//! way PHP's did.

use std::collections::HashMap;

use twig_value::object::{ArrayIterator, ArrayObject, ObjectRef, PropertyPurpose, StdClass};
use twig_value::{Array, ArrayKey, Collect, Diagnostics, Value};

use crate::classes::simplexml::SimpleXmlElement;
use crate::classes::spl::{CallbackFilterIterator, SplObjectStorage};
use crate::classes::userland::UserObject;
use crate::json::ErrorInfo;
use crate::typed::{EnumNode, Item, Key, ObjectNode, TypedNode};

/// Encodes objects the encoder does not know (factory-created ones, stand-ins): returns the node
/// for the object, which gets the given id.
pub type EncodeHook<'h> = &'h dyn Fn(&ObjectRef, i64) -> Option<TypedNode>;

/// An encoding session: object identities persist across [`Encoder::encode`] calls, like the
/// `$seen` map of the PHP encoder.
pub struct Encoder<'h> {
    rich: bool,
    hook: Option<EncodeHook<'h>>,
    /// `ObjectRef::id()` → encoded id, for objects already encoded.
    seen: HashMap<u64, i64>,
    /// Ids to use for known objects (`ObjectRef::id()` → id in the PHP tree).
    known: HashMap<u64, i64>,
    next_id: i64,
    diag: Collect,
}

impl<'h> Encoder<'h> {
    /// A session; `rich` adds the extras of `fixture_data.gen.php`.
    pub fn new(rich: bool, hook: Option<EncodeHook<'h>>) -> Encoder<'h> {
        Encoder {
            rich,
            hook,
            seen: HashMap::new(),
            known: HashMap::new(),
            next_id: 1_000_000_000,
            diag: Collect::new(),
        }
    }

    /// Uses `php_id` as the id of `object` when it is encoded.
    pub fn assign_id(&mut self, object: &ObjectRef, php_id: i64) {
        self.known.insert(object.id(), php_id);
    }

    /// Marks `object` as already encoded (later occurrences become `ref` nodes).
    pub fn mark_seen(&mut self, object: &ObjectRef, php_id: i64) {
        self.known.insert(object.id(), php_id);
        self.seen.insert(object.id(), php_id);
    }

    /// The diagnostics raised by the methods the encoder called.
    pub fn diagnostics(&mut self) -> &mut Collect {
        &mut self.diag
    }

    /// Encodes a value.
    pub fn encode(&mut self, v: &Value) -> TypedNode {
        self.value(v, 0)
    }

    fn value(&mut self, v: &Value, depth: usize) -> TypedNode {
        if depth > 64 {
            return TypedNode::Truncated;
        }
        match v {
            Value::Null => TypedNode::Null,
            Value::Bool(b) => TypedNode::Bool(*b),
            Value::Int(i) => TypedNode::Int(*i),
            Value::Float(d) => TypedNode::Float(*d),
            Value::String(s) => TypedNode::String(s.clone()),
            Value::Array(a) => TypedNode::Array(self.items(a, depth)),
            Value::Object(o) => self.object(o, depth),
        }
    }

    fn items(&mut self, a: &Array, depth: usize) -> Vec<Item> {
        a.iter()
            .map(|(k, v)| Item {
                key: match k {
                    ArrayKey::Int(i) => Key::Int(*i),
                    ArrayKey::Str(s) => Key::Str(s.clone()),
                },
                value: self.value(v, depth + 1),
            })
            .collect()
    }

    fn object(&mut self, o: &ObjectRef, depth: usize) -> TypedNode {
        let class_name = o.class_name().to_string();
        if let Some(&id) = self.seen.get(&o.id()) {
            return TypedNode::Ref {
                class: class_name,
                id,
            };
        }
        let id = match self.known.get(&o.id()) {
            Some(&id) => id,
            None => {
                self.next_id += 1;
                self.next_id
            }
        };
        self.seen.insert(o.id(), id);
        if let Some(case) = o.enum_case_name() {
            return TypedNode::Enum(Box::new(EnumNode {
                class: class_name,
                id,
                case: case.to_string(),
                value: o.enum_case_value().map(|v| self.value(v, depth + 1)),
            }));
        }
        if o.downcast_ref::<twig_value::object::Closure>().is_some() {
            return TypedNode::Closure {
                class: class_name,
                id,
            };
        }
        let mut n = empty_node(class_name, id);
        let known_class = o.downcast_ref::<UserObject>().is_some()
            || o.downcast_ref::<StdClass>().is_some()
            || o.downcast_ref::<SplObjectStorage>().is_some()
            || o.downcast_ref::<crate::classes::spl::SplFileInfo>()
                .is_some()
            || o.downcast_ref::<CallbackFilterIterator>().is_some()
            || o.downcast_ref::<crate::classes::generator::Generator>()
                .is_some();
        if let Some(ao) = o.downcast_ref::<ArrayObject>() {
            n.entries = Some(TypedNode::Array(self.items(&ao.array(), depth + 1)));
        } else if let Some(ai) = o.downcast_ref::<ArrayIterator>() {
            n.entries = Some(TypedNode::Array(self.items(&ai.array(), depth + 1)));
        } else if let Some(sxe) = o.downcast_ref::<SimpleXmlElement>() {
            n.xml = Some(Some(sxe.as_xml()));
        } else if known_class {
            let mut ifaces: Vec<String> = o
                .class()
                .interfaces()
                .iter()
                .map(|i| i.name().to_string())
                .collect();
            ifaces.sort();
            n.interfaces = Some(ifaces);
            if o.has_method("__toString") {
                let mut diag = Collect::new();
                match o.to_php_string(&mut diag) {
                    Ok(s) => n.to_string = Some(TypedNode::String(s)),
                    Err(e) => {
                        n.to_string_error = Some(ErrorInfo {
                            class: e.class().name().to_string(),
                            message: e.message_lossy().into_owned(),
                            message_b64: None,
                            formatted: format!("{}: {}", e.class().name(), e.message_lossy()),
                            formatted_b64: None,
                            raw_message: None,
                            raw_message_b64: None,
                            template_name: None,
                            template_line: None,
                            template_column: None,
                            code: Some(0),
                            previous: None,
                        })
                    }
                }
            }
            let props = o.visible_properties(&mut self.diag).unwrap_or_default();
            n.props = Some(
                props
                    .into_iter()
                    .map(|(name, v)| Item {
                        key: match twig_value::key::handle_numeric_str(name.as_bytes()) {
                            Some(i) => Key::Int(i),
                            None => Key::Str(name),
                        },
                        value: self.value(&v, depth + 1),
                    })
                    .collect(),
            );
        } else {
            if let Some(node) = self.hook.and_then(|h| h(o, id)) {
                return node;
            }
            n.interfaces = Some(Vec::new());
            n.props = Some(Vec::new());
            return TypedNode::Object(Box::new(n));
        }
        if self.rich {
            self.extras(o, &mut n, depth);
        }
        TypedNode::Object(Box::new(n))
    }

    fn extras(&mut self, o: &ObjectRef, n: &mut ObjectNode, depth: usize) {
        let class = o.class().clone();
        n.methods = Some(
            class
                .public_method_names()
                .iter()
                .map(|m| m.to_string())
                .collect(),
        );
        n.parents = Some(
            class
                .parents()
                .iter()
                .map(|p| p.name().to_string())
                .collect(),
        );
        n.interfaces_raw = Some(
            class
                .interfaces()
                .iter()
                .map(|i| i.name().to_string())
                .collect(),
        );
        if o.downcast_ref::<UserObject>().is_some() {
            let table = o
                .properties_for(PropertyPurpose::GetObjectVars, &mut self.diag)
                .unwrap_or_default();
            let mut hidden = Vec::new();
            for e in table {
                let (ArrayKey::Str(k), Some(v)) = (&e.key, &e.value) else {
                    continue;
                };
                if k.first() == Some(&0) {
                    hidden.push(Item {
                        key: Key::Str(k.clone()),
                        value: self.value(v, depth + 1),
                    });
                }
            }
            n.props_hidden = Some(hidden);
        }
        if o.downcast_ref::<ArrayObject>().is_some() || o.downcast_ref::<ArrayIterator>().is_some()
        {
            n.flags = o
                .invoke_method("getflags", Default::default(), &mut self.diag)
                .ok()
                .and_then(|v| v.as_int());
            if o.downcast_ref::<ArrayObject>().is_some() {
                n.iterator_class = o
                    .invoke_method("getiteratorclass", Default::default(), &mut self.diag)
                    .ok()
                    .and_then(|v| v.as_str().map(|s| s.to_string_lossy().into_owned()));
            }
        }
        if let Some(s) = o.downcast_ref::<SplObjectStorage>() {
            let entries = s.entries();
            n.storage = Some(
                entries
                    .iter()
                    .map(|(obj, inf)| {
                        (
                            self.value(&Value::Object(obj.clone()), depth + 1),
                            self.value(inf, depth + 1),
                        )
                    })
                    .collect(),
            );
            // The PHP encoder iterates the storage, then rewinds it.
            let _ = o.invoke_method("rewind", Default::default(), &mut self.diag);
        }
        if let Some(cfi) = o.downcast_ref::<CallbackFilterIterator>() {
            let inner = cfi.inner();
            n.inner = Some(match &inner {
                Some(i) => self.value(&Value::Object(i.clone()), depth + 1),
                None => TypedNode::Null,
            });
            n.accepts = Some(accepts(o, inner.as_ref(), &mut self.diag));
        }
    }
}

/// The PHP encoder's `accepts`: the keys the filter yields, then for each element of the inner
/// iterator whether its key is among them; the inner iterator is rewound afterwards.
fn accepts(cfi: &ObjectRef, inner: Option<&ObjectRef>, diag: &mut dyn Diagnostics) -> Vec<bool> {
    let mut accepted = Vec::new();
    let _ = twig_value::object::traverse(cfi, true, diag, |k, _v, _d| {
        accepted.push(k);
        Ok(true)
    });
    let mut out = Vec::new();
    if let Some(inner) = inner {
        let _ = twig_value::object::traverse(inner, true, diag, |k, _v, _d| {
            out.push(accepted.contains(&k));
            Ok(true)
        });
        let _ = inner.invoke_method("rewind", Default::default(), diag);
    }
    out
}

/// An object node with only the class and id set.
pub fn empty_node(class: String, id: i64) -> ObjectNode {
    ObjectNode {
        class,
        id,
        content: None,
        charset: None,
        datetime: None,
        timezone: None,
        timestamp: None,
        name: None,
        spec: None,
        template: None,
        entries: None,
        xml: None,
        interfaces: None,
        to_string: None,
        to_string_error: None,
        props: None,
        methods: None,
        parents: None,
        interfaces_raw: None,
        props_hidden: None,
        flags: None,
        iterator_class: None,
        storage: None,
        inner: None,
        accepts: None,
    }
}
