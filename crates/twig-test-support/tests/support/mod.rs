//! Shared helpers of the integration tests: the test-side model of `tests/vectors/fixture_data.json`
//! (dumps and probes), a stub factory for the classes owned by other crates, and the matching
//! encode hook.
//!
//! Regenerate the vectors with the Docker oracle (`research/php-oracle`, image `twig-oracle:3.31.1`):
//!
//! ```text
//! cd crates/twig-test-support/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/fixture_data.gen.php > fixture_data.json
//! ```

#![allow(dead_code)]

use std::sync::{Arc, LazyLock};

use serde::Deserialize;
use twig_test_support::json::field_bytes;
use twig_test_support::{FactoryRequest, ObjectFactory, ObjectNode, RequestKind, TypedNode};
use twig_value::object::{
    CallArgs, ClassBuilder, ClassRef, Object, ObjectRef, PropertyInfo, PropertyStore, Visibility,
    builtin,
};
use twig_value::{Array, Diagnostic, Diagnostics, Level, PhpError, PhpResult, PhpString, Value};

/// `fixture_data.json`.
#[derive(Deserialize)]
pub struct DataFile {
    pub generator: String,
    pub php: String,
    pub twig: String,
    pub cases: Vec<DataCase>,
}

#[derive(Deserialize)]
pub struct DataCase {
    pub file: String,
    pub index: usize,
    #[serde(default)]
    pub data: Option<TypedNode>,
    #[serde(default)]
    pub data_error: Option<serde_json::Value>,
    #[serde(default)]
    pub dumps: Vec<(DumpKey, Dumps)>,
    #[serde(default)]
    pub probes: Vec<Probe>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DumpKey {
    Int(i64),
    Str(String),
}

#[derive(Deserialize, Default)]
pub struct Dumps {
    pub skipped: Option<String>,
    pub var_dump: Option<String>,
    pub var_dump_b64: Option<String>,
    pub var_dump_diag: Option<Vec<Vec<String>>>,
    pub var_dump_error: Option<Err>,
    pub var_export: Option<String>,
    pub var_export_b64: Option<String>,
    pub var_export_diag: Option<Vec<Vec<String>>>,
    pub var_export_error: Option<Err>,
    pub print_r: Option<String>,
    pub print_r_b64: Option<String>,
    pub print_r_diag: Option<Vec<Vec<String>>>,
    pub print_r_error: Option<Err>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Err {
    pub class: String,
    pub message: String,
    #[serde(default)]
    pub message_b64: Option<String>,
}

impl Err {
    pub fn message_bytes(&self) -> PhpString {
        field_bytes(&self.message, self.message_b64.as_deref())
    }
}

#[derive(Deserialize)]
pub struct Probe {
    pub id: i64,
    pub class: String,
    pub ops: Vec<Op>,
}

#[derive(Deserialize)]
pub struct Op {
    pub op: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub args: Vec<TypedNode>,
    #[serde(default)]
    pub named: Vec<(String, TypedNode)>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub offset: Option<TypedNode>,
    #[serde(default)]
    pub keys: Option<bool>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub ok: Option<TypedNode>,
    #[serde(default)]
    pub err: Option<Err>,
    #[serde(default)]
    pub diag: Vec<Vec<String>>,
}

pub fn data_file() -> &'static DataFile {
    static FILE: LazyLock<DataFile> = LazyLock::new(|| {
        let path = twig_test_support::corpus::default_supplement_path();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("invalid {}: {e}", path.display()))
    });
    &FILE
}

/// A PHP diagnostic list entry `[level, message(, message_b64)]` as `(level, bytes)`.
pub fn php_diag(entry: &[String]) -> (String, PhpString) {
    let level = entry.first().cloned().unwrap_or_default();
    let msg = field_bytes(
        entry.get(1).map(String::as_str).unwrap_or_default(),
        entry.get(2).map(String::as_str),
    );
    (level, msg)
}

pub fn rust_diag(d: &Diagnostic) -> (String, PhpString) {
    let level = match d.level {
        Level::Warning => "W",
        Level::Notice => "N",
        Level::Deprecated => "D",
        Level::UserDeprecated => "UD",
    };
    (level.to_string(), d.message.clone())
}

// ---------------------------------------------------------------------------------------------
// Stub factory
// ---------------------------------------------------------------------------------------------

static MARKUP: LazyLock<ClassRef> = LazyLock::new(|| {
    let private = |name: &str, ty: Option<&str>| PropertyInfo {
        name: PhpString::from(name),
        visibility: Visibility::Private,
        is_static: false,
        is_readonly: false,
        type_name: ty.map(Arc::from),
        default: if ty.is_some() {
            None
        } else {
            Some(Value::Null)
        },
        declaring_class: Arc::from(""),
    };
    let mut b = ClassBuilder::class("Twig\\Markup")
        .implements(&builtin::countable())
        .implements(&builtin::json_serializable())
        .implements(&builtin::stringable())
        .property_info(private("content", None))
        .property_info(private("charset", Some("?string")))
        .property_info(private("safeStrategies", Some("array")));
    for m in [
        "__construct",
        "__toString",
        "getCharset",
        "withContent",
        "isSafeFor",
        "count",
        "jsonSerialize",
    ] {
        b = b.method(m);
    }
    b.build()
});

/// What a stub stands for.
pub enum StubKind {
    /// A `Twig\Markup` with PHP's property layout.
    Markup,
    /// Any other factory class: the node it came from is replayed by the encode hook.
    Opaque(TypedNode),
}

/// A stand-in for a factory-created object.
pub struct Stub {
    class: ClassRef,
    props: PropertyStore,
    pub kind: StubKind,
}

impl Object for Stub {
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
        match (&self.kind, method) {
            (StubKind::Markup, "__tostring") => {
                Ok(self.props.get(b"content").unwrap_or(Value::from("")))
            }
            (StubKind::Markup, "getcharset") => Ok(self.props.get(b"charset").unwrap_or_default()),
            _ => Err(PhpError::error(format!(
                "stub {}::{method}()",
                self.class.name()
            ))),
        }
    }
}

/// Creates stubs for every factory request.
pub struct StubFactory;

impl ObjectFactory for StubFactory {
    fn create(&self, request: &FactoryRequest<'_>) -> Option<PhpResult<Value>> {
        let obj = match &request.kind {
            RequestKind::Markup {
                content,
                charset,
                safe_strategies,
            } => {
                let props = PropertyStore::new(&MARKUP);
                props.set(b"content", Value::String(content.clone()));
                props.set(b"charset", Value::from(*charset));
                let strategies = safe_strategies
                    .clone()
                    .unwrap_or_else(|| vec![PhpString::from("all")]);
                props.set(
                    b"safeStrategies",
                    Value::Array(Array::from_values(
                        strategies.into_iter().map(Value::String),
                    )),
                );
                Stub {
                    class: MARKUP.clone(),
                    props,
                    kind: StubKind::Markup,
                }
            }
            _ => {
                let class = ClassBuilder::class(request.class).build();
                let props = PropertyStore::new(&class);
                Stub {
                    class,
                    props,
                    kind: StubKind::Opaque(request.node.clone()),
                }
            }
        };
        Some(Ok(Value::Object(ObjectRef::new(obj))))
    }
}

/// Whether a value contains an opaque stub (its dumps cannot match PHP's).
pub fn contains_opaque_stub(v: &Value) -> bool {
    fn walk(v: &Value, seen: &mut std::collections::HashSet<u64>) -> bool {
        match v {
            Value::Array(a) => a.values().any(|v| walk(v, seen)),
            Value::Object(o) => {
                if !seen.insert(o.id()) {
                    return false;
                }
                if let Some(s) = o.downcast_ref::<Stub>() {
                    return matches!(s.kind, StubKind::Opaque(_));
                }
                if let Some(ao) = o.downcast_ref::<twig_value::object::ArrayObject>() {
                    return ao.array().values().any(|v| walk(v, seen));
                }
                if let Some(ai) = o.downcast_ref::<twig_value::object::ArrayIterator>() {
                    return ai.array().values().any(|v| walk(v, seen));
                }
                let mut diag: Vec<Diagnostic> = Vec::new();
                o.visible_properties(&mut diag)
                    .unwrap_or_default()
                    .iter()
                    .any(|(_, v)| walk(v, seen))
            }
            _ => false,
        }
    }
    walk(v, &mut std::collections::HashSet::new())
}

/// The encode hook for stubs: Markup with PHP's fields, opaque stubs replay their node.
pub fn stub_hook(o: &ObjectRef, id: i64) -> Option<TypedNode> {
    let stub = o.downcast_ref::<Stub>()?;
    match &stub.kind {
        StubKind::Opaque(node) => Some(match node {
            TypedNode::Object(n) => {
                let mut n: ObjectNode = (**n).clone();
                n.id = id;
                TypedNode::Object(Box::new(n))
            }
            other => other.clone(),
        }),
        StubKind::Markup => {
            let mut n = twig_test_support::encode::empty_node("Twig\\Markup".to_string(), id);
            n.content = Some(TypedNode::String(
                stub.props
                    .get(b"content")
                    .and_then(|v| v.as_str().cloned())
                    .unwrap_or_default(),
            ));
            n.charset = stub
                .props
                .get(b"charset")
                .and_then(|v| v.as_str().map(|s| s.to_string_lossy().into_owned()));
            n.methods = Some(
                MARKUP
                    .public_method_names()
                    .iter()
                    .map(|m| m.to_string())
                    .collect(),
            );
            n.parents = Some(Vec::new());
            n.interfaces_raw = Some(
                MARKUP
                    .interfaces()
                    .iter()
                    .map(|i| i.name().to_string())
                    .collect(),
            );
            let mangled = |name: &str| {
                twig_test_support::Key::Str(twig_value::object::mangle_property_name(
                    Visibility::Private,
                    "Twig\\Markup",
                    name.as_bytes(),
                ))
            };
            let strategies = stub.props.get(b"safeStrategies").unwrap_or_default();
            let mut enc = twig_test_support::encode::Encoder::new(false, None);
            n.props_hidden = Some(vec![
                twig_test_support::Item {
                    key: mangled("content"),
                    value: n.content.clone().unwrap_or(TypedNode::Null),
                },
                twig_test_support::Item {
                    key: mangled("charset"),
                    value: match &n.charset {
                        Some(c) => TypedNode::String(PhpString::from(c.as_str())),
                        None => TypedNode::Null,
                    },
                },
                twig_test_support::Item {
                    key: mangled("safeStrategies"),
                    value: enc.encode(&strategies),
                },
            ]);
            Some(TypedNode::Object(Box::new(n)))
        }
    }
}
