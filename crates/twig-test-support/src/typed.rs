//! Typed trees: the oracle's lossless JSON view of PHP values.
//!
//! Two producers write the same node format:
//!
//! * `TwigOracle\typed()` (`research/php-oracle/lib.php`), used for the `data` and `config` trees of
//!   `tests/twig/oracle-corpus.json`;
//! * `tests/vectors/fixture_data.gen.php` of this crate, which writes the same fields in the same
//!   order plus the extras documented on [`ObjectNode`] (float bit patterns, hidden properties,
//!   method lists, `SplObjectStorage` contents, ...).
//!
//! | `"t"` | node | payload |
//! |---|---|---|
//! | `null` / `bool` / `int` | [`TypedNode::Null`] / [`Bool`](TypedNode::Bool) / [`Int`](TypedNode::Int) | `v` |
//! | `float` | [`TypedNode::Float`] | `v`: `var_export()` text (shortest round trip) or `INF`/`-INF`/`NAN`; `bits` (extra): IEEE-754 hex |
//! | `string` | [`TypedNode::String`] | `v`, plus `v_b64` (raw bytes) when not valid UTF-8 |
//! | `array` | [`TypedNode::Array`] | `v`: `[[key, node], ...]` in order; int keys as JSON ints; a third element holds the base64 bytes of a string key that is not valid UTF-8 (extra) |
//! | `object` | [`TypedNode::Object`] | see [`ObjectNode`] |
//! | `enum` | [`TypedNode::Enum`] | `class`, `id`, `case`, `value` (backed enums) |
//! | `closure` | [`TypedNode::Closure`] | `class`, `id` |
//! | `resource` | [`TypedNode::Resource`] | `type` (`get_resource_type()`) |
//! | `ref` | [`TypedNode::Ref`] | `class`, `id`: an object already encoded earlier in the same tree (DFS order) |
//! | `truncated` | [`TypedNode::Truncated`] | nesting deeper than 64 levels |
//!
//! Object ids are `spl_object_id()` values of the producing PHP process: they identify objects
//! within one tree (shared objects and cycles appear as `ref` nodes) and mean nothing across trees.
//!
//! Strings are decoded to bytes ([`PhpString`]) while parsing: the `v_b64` copy wins over the lossy
//! `v` text. Floats are parsed exactly: the `bits` pattern when present (it must agree with `v`),
//! otherwise `v`, which must be the `var_export()` text of the parsed value (checked with
//! [`twig_value::float::format_var_export`], so a non-canonical text is rejected rather than
//! silently rounded).

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use twig_value::PhpString;

use crate::json::{ErrorInfo, decode_base64};

/// One node of a typed tree. See the [module documentation](self) for the JSON format.
#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "RawNode")]
pub enum TypedNode {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// An `int`.
    Int(i64),
    /// A `float`, bit-exact.
    Float(f64),
    /// A byte string.
    String(PhpString),
    /// An array: entries in order with their original key types.
    Array(Vec<Item>),
    /// An object encoded in full (the first time the encoder met it).
    Object(Box<ObjectNode>),
    /// An enum case.
    Enum(Box<EnumNode>),
    /// A `Closure` (the encoder records nothing but its identity).
    Closure {
        /// Always `Closure`.
        class: String,
        /// `spl_object_id()`.
        id: i64,
    },
    /// A resource.
    Resource {
        /// `get_resource_type()` (`stream`, ...).
        resource_type: String,
    },
    /// An object encoded earlier in the same tree.
    Ref {
        /// Its class.
        class: String,
        /// Its `spl_object_id()`.
        id: i64,
    },
    /// Nesting deeper than the encoder's limit.
    Truncated,
    /// A `"t"` this model does not know.
    Unknown,
}

impl TypedNode {
    /// The `"t"` tag of the node.
    pub fn tag(&self) -> &'static str {
        match self {
            TypedNode::Null => "null",
            TypedNode::Bool(_) => "bool",
            TypedNode::Int(_) => "int",
            TypedNode::Float(_) => "float",
            TypedNode::String(_) => "string",
            TypedNode::Array(_) => "array",
            TypedNode::Object(_) => "object",
            TypedNode::Enum(_) => "enum",
            TypedNode::Closure { .. } => "closure",
            TypedNode::Resource { .. } => "resource",
            TypedNode::Ref { .. } => "ref",
            TypedNode::Truncated => "truncated",
            TypedNode::Unknown => "unknown",
        }
    }

    /// The entries when this is an array node.
    pub fn as_array(&self) -> Option<&[Item]> {
        match self {
            TypedNode::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The bytes when this is a string node.
    pub fn as_bytes(&self) -> Option<&PhpString> {
        match self {
            TypedNode::String(s) => Some(s),
            _ => None,
        }
    }

    /// The object payload when this is an object node.
    pub fn as_object(&self) -> Option<&ObjectNode> {
        match self {
            TypedNode::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The value of the array entry with string key `key` (top-level lookup in a context tree).
    pub fn get(&self, key: &str) -> Option<&TypedNode> {
        self.as_array()?
            .iter()
            .find(|i| matches!(&i.key, Key::Str(s) if s.as_bytes() == key.as_bytes()))
            .map(|i| &i.value)
    }

    /// Visits this node and every node below it (object payloads included) in DFS order.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a TypedNode)) {
        f(self);
        match self {
            TypedNode::Array(items) => {
                for i in items {
                    i.value.walk(f);
                }
            }
            TypedNode::Object(o) => o.walk_children(f),
            TypedNode::Enum(e) => {
                if let Some(v) = &e.value {
                    v.walk(f);
                }
            }
            _ => {}
        }
    }
}

/// An array entry (or a property entry of an object node).
#[derive(Clone, Debug)]
pub struct Item {
    /// The key.
    pub key: Key,
    /// The value.
    pub value: TypedNode,
}

/// An array key as the encoder wrote it: JSON ints are int keys, JSON strings string keys.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// An int key.
    Int(i64),
    /// A string key (bytes; decoded from the base64 copy when the key is not valid UTF-8).
    Str(PhpString),
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Int(i) => write!(f, "{i}"),
            Key::Str(s) => write!(f, "{:?}", s.to_string_lossy()),
        }
    }
}

/// The payload of an `object` node.
///
/// Which fields are present depends on the class, exactly as `TwigOracle\typed()` writes them:
///
/// * `Twig\Markup`: [`content`](Self::content), [`charset`](Self::charset);
/// * `DateTimeInterface`: [`datetime`](Self::datetime) (`Y-m-d\TH:i:s.uP`),
///   [`timezone`](Self::timezone), [`timestamp`](Self::timestamp);
/// * `DateTimeZone`: [`name`](Self::name); `DateInterval`: [`spec`](Self::spec);
/// * `Twig\TemplateWrapper`: [`template`](Self::template) (the template name);
/// * `ArrayObject` / `ArrayIterator`: [`entries`](Self::entries) (`getArrayCopy()`);
/// * `SimpleXMLElement`: [`xml`](Self::xml) (`asXML()`);
/// * any other object: [`interfaces`](Self::interfaces) (sorted `class_implements()`),
///   [`to_string`](Self::to_string) or [`to_string_error`](Self::to_string_error) when the class
///   has `__toString()`, and [`props`](Self::props) (`get_object_vars()` from the global scope:
///   public initialised properties, numeric names as int keys).
///
/// Extras written only by this crate's `fixture_data.gen.php` (all `None` in corpus trees):
/// [`methods`](Self::methods), [`parents`](Self::parents),
/// [`interfaces_raw`](Self::interfaces_raw), [`props_hidden`](Self::props_hidden),
/// [`flags`](Self::flags), [`iterator_class`](Self::iterator_class), [`storage`](Self::storage),
/// [`inner`](Self::inner), [`accepts`](Self::accepts).
#[derive(Clone, Debug, Deserialize)]
pub struct ObjectNode {
    /// The class name (`$obj::class`; anonymous classes include PHP's NUL-separated suffix).
    pub class: String,
    /// `spl_object_id()` in the producing process.
    pub id: i64,
    /// `Twig\Markup`: `(string) $markup` (a string node).
    #[serde(default)]
    pub content: Option<TypedNode>,
    /// `Twig\Markup`: `getCharset()`.
    #[serde(default)]
    pub charset: Option<String>,
    /// `DateTimeInterface`: `format('Y-m-d\TH:i:s.uP')`.
    #[serde(default)]
    pub datetime: Option<String>,
    /// `DateTimeInterface`: `getTimezone()->getName()`.
    #[serde(default)]
    pub timezone: Option<String>,
    /// `DateTimeInterface`: `getTimestamp()`.
    #[serde(default)]
    pub timestamp: Option<i64>,
    /// `DateTimeZone`: `getName()`.
    #[serde(default)]
    pub name: Option<String>,
    /// `DateInterval`: the public fields.
    #[serde(default)]
    pub spec: Option<DateIntervalSpec>,
    /// `Twig\TemplateWrapper`: `getTemplateName()`.
    #[serde(default)]
    pub template: Option<String>,
    /// `ArrayObject` / `ArrayIterator`: `getArrayCopy()` (an array node).
    #[serde(default)]
    pub entries: Option<TypedNode>,
    /// `SimpleXMLElement`: `asXML()` (`None` when PHP returned `false`).
    #[serde(default, deserialize_with = "xml_text")]
    pub xml: Option<Option<String>>,
    /// Other objects: `class_implements()`, sorted.
    #[serde(default)]
    pub interfaces: Option<Vec<String>>,
    /// Other objects with `__toString()`: the string conversion.
    #[serde(default, rename = "toString")]
    pub to_string: Option<TypedNode>,
    /// Other objects with `__toString()` that threw.
    #[serde(default, rename = "toString_error")]
    pub to_string_error: Option<ErrorInfo>,
    /// Other objects: `get_object_vars()` from the global scope.
    #[serde(default)]
    pub props: Option<Vec<Item>>,
    /// Extra: `get_class_methods()` from the global scope, in PHP's order.
    #[serde(default)]
    pub methods: Option<Vec<String>>,
    /// Extra: `class_parents()`, nearest first.
    #[serde(default)]
    pub parents: Option<Vec<String>>,
    /// Extra: `class_implements()` in PHP's interface-table order.
    #[serde(default)]
    pub interfaces_raw: Option<Vec<String>>,
    /// Extra (userland classes): the private and protected properties, keyed by mangled name
    /// (`"\0Class\0name"`, `"\0*\0name"`), from `get_mangled_object_vars()`.
    #[serde(default)]
    pub props_hidden: Option<Vec<Item>>,
    /// Extra (`ArrayObject` / `ArrayIterator`): `getFlags()`.
    #[serde(default)]
    pub flags: Option<i64>,
    /// Extra (`ArrayObject`): `getIteratorClass()`.
    #[serde(default)]
    pub iterator_class: Option<String>,
    /// Extra (`SplObjectStorage`): `[object, info]` pairs in storage order.
    #[serde(default)]
    pub storage: Option<Vec<(TypedNode, TypedNode)>>,
    /// Extra (`CallbackFilterIterator`): `getInnerIterator()`.
    #[serde(default)]
    pub inner: Option<TypedNode>,
    /// Extra (`CallbackFilterIterator`): per element of the inner iterator, whether the filter
    /// yields it.
    #[serde(default)]
    pub accepts: Option<Vec<bool>>,
}

impl ObjectNode {
    /// The class name up to the first NUL byte (what PHP prints for anonymous classes).
    pub fn display_class(&self) -> &str {
        self.class.split('\0').next().unwrap_or_default()
    }

    /// Visits the nodes of the payload in the encoder's order.
    pub fn walk_children<'a>(&'a self, f: &mut dyn FnMut(&'a TypedNode)) {
        if let Some(n) = &self.content {
            n.walk(f);
        }
        if let Some(n) = &self.entries {
            n.walk(f);
        }
        if let Some(n) = &self.to_string {
            n.walk(f);
        }
        for i in self.props.iter().flatten() {
            i.value.walk(f);
        }
        for i in self.props_hidden.iter().flatten() {
            i.value.walk(f);
        }
        for (o, inf) in self.storage.iter().flatten() {
            o.walk(f);
            inf.walk(f);
        }
        if let Some(n) = &self.inner {
            n.walk(f);
        }
    }
}

/// The fields of a `DateInterval` as `TwigOracle\typed()` records them.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DateIntervalSpec {
    /// Years.
    pub y: i64,
    /// Months.
    pub m: i64,
    /// Days.
    pub d: i64,
    /// Hours.
    pub h: i64,
    /// Minutes.
    pub i: i64,
    /// Seconds.
    pub s: i64,
    /// Microseconds as a fraction of a second.
    pub f: f64,
    /// 1 when the interval is negative.
    pub invert: i64,
    /// Total days (`false` unless the interval came from `DateTime::diff()`).
    #[serde(deserialize_with = "false_or_int")]
    pub days: Option<i64>,
}

/// The payload of an `enum` node.
#[derive(Clone, Debug, Deserialize)]
pub struct EnumNode {
    /// The enum class.
    pub class: String,
    /// `spl_object_id()`.
    pub id: i64,
    /// The case name.
    pub case: String,
    /// The backing value (backed enums only).
    #[serde(default)]
    pub value: Option<TypedNode>,
}

// ---------------------------------------------------------------------------------------------
// Deserialization
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum RawNode {
    Null,
    Bool {
        v: bool,
    },
    Int {
        v: i64,
    },
    Float {
        v: String,
        #[serde(default)]
        bits: Option<String>,
    },
    String {
        v: String,
        #[serde(default)]
        v_b64: Option<String>,
    },
    Array {
        v: Vec<Item>,
    },
    Object(Box<ObjectNode>),
    Enum(Box<EnumNode>),
    Closure {
        class: String,
        id: i64,
    },
    Resource {
        #[serde(rename = "type")]
        resource_type: String,
    },
    Ref {
        class: String,
        id: i64,
    },
    Truncated,
    #[serde(other)]
    Unknown,
}

impl TryFrom<RawNode> for TypedNode {
    type Error = String;

    fn try_from(raw: RawNode) -> Result<TypedNode, String> {
        Ok(match raw {
            RawNode::Null => TypedNode::Null,
            RawNode::Bool { v } => TypedNode::Bool(v),
            RawNode::Int { v } => TypedNode::Int(v),
            RawNode::Float { v, bits } => TypedNode::Float(parse_float(&v, bits.as_deref())?),
            RawNode::String { v, v_b64 } => TypedNode::String(match v_b64 {
                Some(b64) => PhpString::from(decode_base64(&b64)?),
                None => PhpString::from(v),
            }),
            RawNode::Array { v } => TypedNode::Array(v),
            RawNode::Object(o) => TypedNode::Object(o),
            RawNode::Enum(e) => TypedNode::Enum(e),
            RawNode::Closure { class, id } => TypedNode::Closure { class, id },
            RawNode::Resource { resource_type } => TypedNode::Resource { resource_type },
            RawNode::Ref { class, id } => TypedNode::Ref { class, id },
            RawNode::Truncated => TypedNode::Truncated,
            RawNode::Unknown => TypedNode::Unknown,
        })
    }
}

/// Parses a float node: `INF`/`-INF`/`NAN` or the `var_export()` text, cross-checked with the bit
/// pattern when the encoder wrote one.
pub(crate) fn parse_float(text: &str, bits: Option<&str>) -> Result<f64, String> {
    let from_text = match text {
        "INF" => f64::INFINITY,
        "-INF" => f64::NEG_INFINITY,
        // PHP's NAN constant (ZEND_NAN): the positive quiet NaN.
        "NAN" => f64::NAN,
        _ => {
            let d: f64 = text
                .parse()
                .map_err(|_| format!("invalid float text {text:?}"))?;
            if !d.is_finite() || twig_value::float::format_var_export(d) != text {
                return Err(format!(
                    "float text {text:?} is not the var_export() form of a double"
                ));
            }
            d
        }
    };
    match bits {
        None => Ok(from_text),
        Some(hex) => {
            let b = u64::from_str_radix(hex, 16)
                .ok()
                .filter(|_| hex.len() == 16)
                .ok_or_else(|| format!("invalid float bits {hex:?}"))?;
            let d = f64::from_bits(b);
            let agrees = if d.is_nan() {
                text == "NAN"
            } else {
                d.to_bits() == from_text.to_bits()
            };
            if !agrees {
                return Err(format!("float bits {hex} disagree with text {text:?}"));
            }
            Ok(d)
        }
    }
}

impl<'de> Deserialize<'de> for Item {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Item, D::Error> {
        struct ItemVisitor;

        impl<'de> Visitor<'de> for ItemVisitor {
            type Value = Item;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array entry [key, node] or [key, node, key_b64]")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Item, A::Error> {
                let key: RawKey = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let value: TypedNode = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let b64: Option<String> = seq.next_element()?;
                if seq.next_element::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::invalid_length(4, &self));
                }
                let key = match (key, b64) {
                    (RawKey::Int(i), None) => Key::Int(i),
                    (RawKey::Int(_), Some(_)) => {
                        return Err(de::Error::custom("an int key cannot have a base64 copy"));
                    }
                    (RawKey::Str(s), None) => Key::Str(PhpString::from(s)),
                    (RawKey::Str(_), Some(b64)) => Key::Str(PhpString::from(
                        decode_base64(&b64).map_err(de::Error::custom)?,
                    )),
                };
                Ok(Item { key, value })
            }
        }

        d.deserialize_seq(ItemVisitor)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawKey {
    Int(i64),
    Str(String),
}

fn xml_text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Xml {
        Text(String),
        Flag(bool),
    }
    match Xml::deserialize(d)? {
        Xml::Text(s) => Ok(Some(Some(s))),
        Xml::Flag(false) => Ok(Some(None)),
        Xml::Flag(true) => Err(de::Error::custom("asXML() cannot return true")),
    }
}

fn false_or_int<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Days {
        Int(i64),
        Flag(bool),
    }
    match Days::deserialize(d)? {
        Days::Int(i) => Ok(Some(i)),
        Days::Flag(false) => Ok(None),
        Days::Flag(true) => Err(de::Error::custom("DateInterval::$days cannot be true")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(json: &str) -> TypedNode {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn scalars_and_strings() {
        assert!(matches!(node(r#"{"t":"null"}"#), TypedNode::Null));
        assert!(matches!(node(r#"{"t":"int","v":-5}"#), TypedNode::Int(-5)));
        match node(r#"{"t":"string","v":"?(abc","v_b64":"wyhhYmM="}"#) {
            TypedNode::String(s) => assert_eq!(s.as_bytes(), b"\xC3\x28abc"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn floats_are_exact() {
        match node(r#"{"t":"float","v":"0.1"}"#) {
            TypedNode::Float(d) => assert_eq!(d.to_bits(), 0.1f64.to_bits()),
            other => panic!("{other:?}"),
        }
        match node(r#"{"t":"float","v":"-0.0","bits":"8000000000000000"}"#) {
            TypedNode::Float(d) => assert!(d == 0.0 && d.is_sign_negative()),
            other => panic!("{other:?}"),
        }
        match node(r#"{"t":"float","v":"NAN","bits":"fff8000000000000"}"#) {
            TypedNode::Float(d) => assert_eq!(d.to_bits(), 0xfff8_0000_0000_0000),
            other => panic!("{other:?}"),
        }
        assert!(serde_json::from_str::<TypedNode>(r#"{"t":"float","v":"0.10"}"#).is_err());
        assert!(
            serde_json::from_str::<TypedNode>(
                r#"{"t":"float","v":"1.0","bits":"0000000000000000"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn arrays_keep_keys() {
        let n = node(
            r#"{"t":"array","v":[[1,{"t":"int","v":1}],["a",{"t":"null"}],["?",{"t":"null"},"/w=="]]}"#,
        );
        let items = n.as_array().unwrap();
        assert_eq!(items[0].key, Key::Int(1));
        assert_eq!(items[1].key, Key::Str(PhpString::from("a")));
        assert_eq!(items[2].key, Key::Str(PhpString::from(&b"\xff"[..])));
    }
}
