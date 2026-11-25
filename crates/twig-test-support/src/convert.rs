//! Typed trees → `twig_value::Value`.
//!
//! The conversion is exact: ints and floats keep their bits, strings their bytes, arrays their
//! order and key types, and objects their identity (a `ref` node yields the very `ObjectRef` its
//! `object` node produced, so cycles and shared objects survive). Objects are built in two steps,
//! registered first and filled afterwards, so that a property can refer back to its owner.
//!
//! Problems never panic and never stop the walk: every unsupported construct is recorded as an
//! [`Unsupported`] with its path (`$["items"]["foo"]->position`), `null` stands in for the subtree,
//! and [`convert`] returns all of them.

use std::collections::BTreeMap;
use std::fmt;

use twig_value::{Array, ArrayKey, PhpError, PhpString, Value};

use crate::classes;
use crate::factory::{FACTORY_CLASSES, FactoryRequest, ObjectFactory, RequestKind};
use crate::typed::{EnumNode, Item, Key, ObjectNode, TypedNode};

/// A construct the conversion could not reproduce.
#[derive(Clone, Debug)]
pub enum Unsupported {
    /// An object of a class owned by another crate (or a resource): the factory declined it.
    NeedsFactory {
        /// Where.
        path: String,
        /// The class (`Twig\Markup`, ...) or `resource(<type>)`.
        class: String,
    },
    /// The factory failed.
    FactoryFailed {
        /// Where.
        path: String,
        /// The class.
        class: String,
        /// The error.
        error: PhpError,
    },
    /// A class neither this crate nor the factory knows.
    UnknownClass {
        /// Where.
        path: String,
        /// The class.
        class: String,
    },
    /// The tree lacks the private state of an object (corpus trees do not record private
    /// properties; the supplement does).
    MissingHiddenState {
        /// Where.
        path: String,
        /// The class.
        class: String,
    },
    /// An enum case of an unknown enum.
    UnknownEnum {
        /// Where.
        path: String,
        /// The enum class.
        class: String,
        /// The case.
        case: String,
    },
    /// A closure (its PHP code is not in the tree).
    Closure {
        /// Where.
        path: String,
    },
    /// A `ref` to an object that was not encoded before.
    DanglingRef {
        /// Where.
        path: String,
        /// The id.
        id: i64,
    },
    /// A `truncated` node.
    Truncated {
        /// Where.
        path: String,
    },
    /// A node type this crate does not know.
    UnknownNode {
        /// Where.
        path: String,
    },
    /// A node that cannot come from PHP (missing payload, non-normalised key, ...).
    InvalidNode {
        /// Where.
        path: String,
        /// What is wrong.
        reason: String,
    },
    /// The tree disagrees with the Rust port of the class (interfaces, properties).
    ClassMismatch {
        /// Where.
        path: String,
        /// The class.
        class: String,
        /// What disagrees.
        reason: String,
    },
    /// A `SimpleXMLElement` document outside the supported XML subset.
    Xml {
        /// Where.
        path: String,
        /// Why.
        reason: String,
    },
    /// A `CallbackFilterIterator` whose callback is not constant (only constant callbacks can be
    /// recovered from the recorded decisions).
    NonConstantCallback {
        /// Where.
        path: String,
    },
}

impl Unsupported {
    /// The path of the construct in the converted value.
    pub fn path(&self) -> &str {
        match self {
            Unsupported::NeedsFactory { path, .. }
            | Unsupported::FactoryFailed { path, .. }
            | Unsupported::UnknownClass { path, .. }
            | Unsupported::MissingHiddenState { path, .. }
            | Unsupported::UnknownEnum { path, .. }
            | Unsupported::Closure { path }
            | Unsupported::DanglingRef { path, .. }
            | Unsupported::Truncated { path }
            | Unsupported::UnknownNode { path }
            | Unsupported::InvalidNode { path, .. }
            | Unsupported::ClassMismatch { path, .. }
            | Unsupported::Xml { path, .. }
            | Unsupported::NonConstantCallback { path } => path,
        }
    }

    /// Whether a factory could resolve this ([`Unsupported::NeedsFactory`]).
    pub fn needs_factory(&self) -> bool {
        matches!(self, Unsupported::NeedsFactory { .. })
    }
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unsupported::NeedsFactory { path, class } => {
                write!(f, "{path}: {class} needs an object factory")
            }
            Unsupported::FactoryFailed { path, class, error } => {
                write!(f, "{path}: the factory failed for {class}: {error}")
            }
            Unsupported::UnknownClass { path, class } => {
                write!(f, "{path}: unknown class {class:?}")
            }
            Unsupported::MissingHiddenState { path, class } => write!(
                f,
                "{path}: the private state of {class} is not in the tree (load the supplement)"
            ),
            Unsupported::UnknownEnum { path, class, case } => {
                write!(f, "{path}: unknown enum case {class}::{case}")
            }
            Unsupported::Closure { path } => write!(f, "{path}: closures cannot be reconstructed"),
            Unsupported::DanglingRef { path, id } => {
                write!(f, "{path}: ref to unknown object #{id}")
            }
            Unsupported::Truncated { path } => write!(f, "{path}: truncated tree"),
            Unsupported::UnknownNode { path } => write!(f, "{path}: unknown node type"),
            Unsupported::InvalidNode { path, reason } => {
                write!(f, "{path}: invalid node: {reason}")
            }
            Unsupported::ClassMismatch {
                path,
                class,
                reason,
            } => write!(f, "{path}: {class} does not match its port: {reason}"),
            Unsupported::Xml { path, reason } => write!(f, "{path}: unsupported XML: {reason}"),
            Unsupported::NonConstantCallback { path } => {
                write!(f, "{path}: the filter callback is not constant")
            }
        }
    }
}

/// A successful conversion.
#[derive(Clone, Debug)]
pub struct Conversion {
    /// The value.
    pub value: Value,
    /// Every object of the tree by its id in the producing PHP process (factory results
    /// included), to relate the tree's identities to the converted value.
    pub objects: BTreeMap<i64, Value>,
    /// The classes (or `resource(<type>)`) created through the factory, in tree order.
    pub factory_requests: Vec<String>,
}

/// A failed conversion.
#[derive(Clone, Debug)]
pub struct ConversionError {
    /// Everything that could not be converted.
    pub problems: Vec<Unsupported>,
    /// The factory requests made (satisfied or not).
    pub factory_requests: Vec<String>,
}

impl ConversionError {
    /// Whether every problem is a declined factory request.
    pub fn only_needs_factory(&self) -> bool {
        self.problems.iter().all(Unsupported::needs_factory)
    }
}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.problems.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConversionError {}

/// Converts a typed tree; objects of classes this crate does not own come from `factory`.
pub fn convert(
    node: &TypedNode,
    factory: &dyn ObjectFactory,
) -> Result<Conversion, ConversionError> {
    let mut cx = Converter::new(factory);
    let value = cx.node(node);
    cx.finish(value)
}

/// A conversion session: object identities persist across trees, so a later tree's `ref` nodes
/// resolve to objects of earlier trees (the probes of `fixture_data.json` pass context objects as
/// method arguments this way).
pub struct Session<'f> {
    cx: Converter<'f>,
}

impl<'f> Session<'f> {
    /// A new session.
    pub fn new(factory: &'f dyn ObjectFactory) -> Session<'f> {
        Session {
            cx: Converter::new(factory),
        }
    }

    /// Converts one more tree; `Err` lists the problems of this tree only.
    pub fn convert(&mut self, node: &TypedNode) -> Result<Value, Vec<Unsupported>> {
        let value = self.cx.node(node);
        if self.cx.problems.is_empty() {
            Ok(value)
        } else {
            Err(std::mem::take(&mut self.cx.problems))
        }
    }

    /// Every object converted so far, by tree id.
    pub fn objects(&self) -> &BTreeMap<i64, Value> {
        &self.cx.objects
    }

    /// The factory requests made so far.
    pub fn factory_requests(&self) -> &[String] {
        &self.cx.factory_requests
    }
}

/// The conversion state; the class ports in [`crate::classes`] build objects through it.
pub(crate) struct Converter<'f> {
    factory: &'f dyn ObjectFactory,
    objects: BTreeMap<i64, Value>,
    problems: Vec<Unsupported>,
    factory_requests: Vec<String>,
    path: String,
}

impl<'f> Converter<'f> {
    pub(crate) fn new(factory: &'f dyn ObjectFactory) -> Converter<'f> {
        Converter {
            factory,
            objects: BTreeMap::new(),
            problems: Vec::new(),
            factory_requests: Vec::new(),
            path: String::from("$"),
        }
    }

    pub(crate) fn finish(self, value: Value) -> Result<Conversion, ConversionError> {
        if self.problems.is_empty() {
            Ok(Conversion {
                value,
                objects: self.objects,
                factory_requests: self.factory_requests,
            })
        } else {
            Err(ConversionError {
                problems: self.problems,
                factory_requests: self.factory_requests,
            })
        }
    }

    pub(crate) fn path(&self) -> String {
        self.path.clone()
    }

    pub(crate) fn problem(&mut self, p: Unsupported) {
        self.problems.push(p);
    }

    pub(crate) fn invalid(&mut self, reason: impl Into<String>) {
        let path = self.path();
        self.problem(Unsupported::InvalidNode {
            path,
            reason: reason.into(),
        });
    }

    pub(crate) fn mismatch(&mut self, class: &str, reason: impl Into<String>) {
        let path = self.path();
        self.problem(Unsupported::ClassMismatch {
            path,
            class: class.to_string(),
            reason: reason.into(),
        });
    }

    /// Registers an object under its tree id (before its children are converted).
    pub(crate) fn register(&mut self, id: i64, value: Value) {
        if self.objects.insert(id, value).is_some() {
            self.invalid(format!("object #{id} is encoded twice"));
        }
    }

    /// Converts `node` with `segment` appended to the path.
    pub(crate) fn child(&mut self, segment: &str, node: &TypedNode) -> Value {
        let len = self.path.len();
        self.path.push_str(segment);
        let v = self.node(node);
        self.path.truncate(len);
        v
    }

    /// Converts one node.
    pub(crate) fn node(&mut self, node: &TypedNode) -> Value {
        match node {
            TypedNode::Null => Value::Null,
            TypedNode::Bool(b) => Value::Bool(*b),
            TypedNode::Int(i) => Value::Int(*i),
            TypedNode::Float(d) => Value::Float(*d),
            TypedNode::String(s) => Value::String(s.clone()),
            TypedNode::Array(items) => Value::Array(self.array(items)),
            TypedNode::Object(o) => self.object(node, o),
            TypedNode::Enum(e) => self.enum_case(e),
            TypedNode::Closure { id, .. } => {
                let path = self.path();
                self.problem(Unsupported::Closure { path });
                // Later refs to the closure resolve to null without further problems.
                self.objects.insert(*id, Value::Null);
                Value::Null
            }
            TypedNode::Resource { resource_type } => {
                let class = format!("resource({resource_type})");
                self.request(
                    &class,
                    node,
                    RequestKind::Resource {
                        resource_type: resource_type.as_str(),
                    },
                    None,
                )
            }
            TypedNode::Ref { id, .. } => match self.objects.get(id) {
                Some(v) => v.clone(),
                None => {
                    let path = self.path();
                    self.problem(Unsupported::DanglingRef { path, id: *id });
                    Value::Null
                }
            },
            TypedNode::Truncated => {
                let path = self.path();
                self.problem(Unsupported::Truncated { path });
                Value::Null
            }
            TypedNode::Unknown => {
                let path = self.path();
                self.problem(Unsupported::UnknownNode { path });
                Value::Null
            }
        }
    }

    /// An array: keys exactly as encoded (a numeric-string key cannot come from a PHP array).
    pub(crate) fn array(&mut self, items: &[Item]) -> Array {
        let mut out = Array::new();
        for item in items {
            let key = match &item.key {
                Key::Int(i) => ArrayKey::Int(*i),
                Key::Str(s) => {
                    if twig_value::key::handle_numeric_str(s.as_bytes()).is_some() {
                        self.invalid(format!(
                            "array key {:?} would be an int key in PHP",
                            s.to_string_lossy()
                        ));
                    }
                    ArrayKey::Str(s.clone())
                }
            };
            let value = self.child(&format!("[{}]", item.key), &item.value);
            if out.insert(key, value).is_some() {
                self.invalid(format!("duplicate array key {}", item.key));
            }
        }
        out
    }

    fn enum_case(&mut self, e: &EnumNode) -> Value {
        let case = classes::fixture_class_table()
            .get(&e.class)
            .filter(|c| c.is_enum())
            .and_then(|c| c.enum_case(&e.case));
        match case {
            Some(case) => {
                let v = Value::Object(case);
                self.register(e.id, v.clone());
                v
            }
            None => {
                let path = self.path();
                self.problem(Unsupported::UnknownEnum {
                    path,
                    class: e.class.clone(),
                    case: e.case.clone(),
                });
                self.objects.insert(e.id, Value::Null);
                Value::Null
            }
        }
    }

    fn object(&mut self, node: &TypedNode, o: &ObjectNode) -> Value {
        if FACTORY_CLASSES.contains(&o.class.as_str()) {
            return self.factory_object(node, o);
        }
        if let Some(v) = classes::build(o, self) {
            return v;
        }
        self.request_as(&o.class, node, RequestKind::Other(o), Some(o.id), true)
    }

    fn factory_object(&mut self, node: &TypedNode, o: &ObjectNode) -> Value {
        let missing = |cx: &mut Self, field: &str| {
            cx.invalid(format!("{} node without {field}", o.class));
            cx.objects.insert(o.id, Value::Null);
            Value::Null
        };
        let kind = match o.class.as_str() {
            "Twig\\Markup" => {
                let Some(content) = o.content.as_ref().and_then(TypedNode::as_bytes) else {
                    return missing(self, "content");
                };
                let Some(charset) = o.charset.as_deref() else {
                    return missing(self, "charset");
                };
                let safe_strategies = match self.markup_strategies(o) {
                    Ok(s) => s,
                    Err(reason) => {
                        self.invalid(reason);
                        None
                    }
                };
                RequestKind::Markup {
                    content: content.clone(),
                    charset,
                    safe_strategies,
                }
            }
            "Twig\\TemplateWrapper" => match o.template.as_deref() {
                Some(template) => RequestKind::TemplateWrapper { template },
                None => return missing(self, "template"),
            },
            "DateTime" | "DateTimeImmutable" => {
                match (o.datetime.as_deref(), o.timezone.as_deref(), o.timestamp) {
                    (Some(datetime), Some(timezone), Some(timestamp)) => RequestKind::DateTime {
                        immutable: o.class == "DateTimeImmutable",
                        datetime,
                        timezone,
                        timestamp,
                    },
                    _ => return missing(self, "datetime/timezone/timestamp"),
                }
            }
            "DateTimeZone" => match o.name.as_deref() {
                Some(name) => RequestKind::DateTimeZone { name },
                None => return missing(self, "name"),
            },
            "DateInterval" => match &o.spec {
                Some(spec) => RequestKind::DateInterval { spec },
                None => return missing(self, "spec"),
            },
            _ => RequestKind::Other(o),
        };
        self.request(&o.class, node, kind, Some(o.id))
    }

    /// The `$safeStrategies` of a Markup node, from the hidden properties of a rich tree.
    fn markup_strategies(&self, o: &ObjectNode) -> Result<Option<Vec<PhpString>>, String> {
        let Some(hidden) = &o.props_hidden else {
            return Ok(None);
        };
        let key = Key::Str(PhpString::from(&b"\0Twig\\Markup\0safeStrategies"[..]));
        let Some(item) = hidden.iter().find(|i| i.key == key) else {
            return Err("Twig\\Markup node without safeStrategies".to_string());
        };
        let items = item
            .value
            .as_array()
            .ok_or("Markup::$safeStrategies is not an array")?;
        items
            .iter()
            .map(|i| {
                i.value
                    .as_bytes()
                    .cloned()
                    .ok_or_else(|| "Markup::$safeStrategies holds a non-string".to_string())
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }

    fn request(
        &mut self,
        class: &str,
        node: &TypedNode,
        kind: RequestKind<'_>,
        id: Option<i64>,
    ) -> Value {
        self.request_as(class, node, kind, id, false)
    }

    /// Asks the factory; a declined request is [`Unsupported::UnknownClass`] for classes this
    /// crate does not know at all, [`Unsupported::NeedsFactory`] otherwise.
    fn request_as(
        &mut self,
        class: &str,
        node: &TypedNode,
        kind: RequestKind<'_>,
        id: Option<i64>,
        unknown: bool,
    ) -> Value {
        let path = self.path();
        let request = FactoryRequest {
            class,
            node,
            kind,
            path: &path,
        };
        let created = self.factory.create(&request);
        if !unknown || created.is_some() {
            self.factory_requests.push(class.to_string());
        }
        let value = match created {
            None if unknown => {
                self.problem(Unsupported::UnknownClass {
                    path,
                    class: class.to_string(),
                });
                Value::Null
            }
            None => {
                self.problem(Unsupported::NeedsFactory {
                    path,
                    class: class.to_string(),
                });
                Value::Null
            }
            Some(Err(error)) => {
                self.problem(Unsupported::FactoryFailed {
                    path,
                    class: class.to_string(),
                    error,
                });
                Value::Null
            }
            Some(Ok(v)) => v,
        };
        if let Some(id) = id {
            // Declined objects map to null so that refs to them do not add problems.
            self.objects.insert(id, value.clone());
        }
        value
    }
}
