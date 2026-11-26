//! `SimpleXMLElement` (PHPSRC/ext/simplexml/simplexml.c), as far as the fixtures use it.
//!
//! The fixtures create element objects with `new \SimpleXMLElement($xml)` and iterate them
//! (`filter`, `find`, `slice`, `length`); every object involved is an element node with
//! `iter.type == SXE_ITER_NONE` (the document root and the children iteration yields). This module
//! models exactly those: a small immutable XML tree and element objects over it, with PHP's
//! iteration (`php_sxe_reset_iterator()` / `php_sxe_move_forward_iterator()`), `count()`
//! (`php_sxe_count_elements_helper()`), casts (`sxe_object_cast_ex()`, `sxe_prop_is_empty()`),
//! the property table of `var_dump()`/`var_export()`/`(array)` (`sxe_get_prop_hash()`),
//! `getName()` and `asXML()` (libxml2's `xmlSaveDoc()` / `xmlNodeDumpOutput()` without
//! formatting).
//!
//! Supported XML: an optional `<?xml version="1.0" ...?>` declaration, one root element, nested
//! elements with plain attributes, text with the predefined entities and character references,
//! ASCII only, no namespaces, comments, CDATA, processing instructions or DTDs. Anything else is
//! rejected by [`parse_document`]. Property access (`$sxe->child`), `children()`, `attributes()`,
//! `xpath()` and the other list-object features are not modelled (they throw `Error`).

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use twig_value::object::{
    CallArgs, CastTarget, ClassBuilder, ClassRef, FetchMode, FunctionKind, MethodParams, Object,
    ObjectIterator, ObjectRef, Param, PropertyCheck, PropertyEntry, PropertyPurpose, PropertyStore,
    builtin,
};
use twig_value::{Array, ArrayKey, Diagnostics, PhpError, PhpResult, PhpString, Value, convert};

/// An XML element.
#[derive(Debug)]
pub struct XmlElement {
    /// The element name.
    pub name: String,
    /// Attributes in document order.
    pub attributes: Vec<(String, String)>,
    /// Child nodes in document order.
    pub children: Vec<XmlNode>,
}

/// A child node.
#[derive(Clone, Debug)]
pub enum XmlNode {
    /// An element.
    Element(Arc<XmlElement>),
    /// A text node (entity references decoded).
    Text(String),
}

/// A parsed document.
#[derive(Debug)]
pub struct XmlDocument {
    /// `version` of the XML declaration (`1.0` without one, like `xmlNewDoc()`).
    pub version: String,
    /// `encoding` of the XML declaration.
    pub encoding: Option<String>,
    /// `standalone` of the XML declaration.
    pub standalone: Option<bool>,
    /// The root element.
    pub root: Arc<XmlElement>,
}

/// Parses the supported XML subset (see the [module documentation](self)).
pub fn parse_document(xml: &str) -> Result<XmlDocument, String> {
    if !xml.is_ascii() {
        return Err("non-ASCII XML is not supported".to_string());
    }
    // XML end-of-line handling (libxml2 does it while reading the input).
    let text = xml.replace("\r\n", "\n").replace('\r', "\n");
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    let (version, encoding, standalone) = p.declaration()?;
    p.skip_ws();
    let root = p.element()?;
    p.skip_ws();
    if p.i != p.s.len() {
        return Err(format!(
            "unsupported content after the root element at byte {}",
            p.i
        ));
    }
    Ok(XmlDocument {
        version,
        encoding,
        standalone,
        root,
    })
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn starts_with(&self, lit: &str) -> bool {
        self.s[self.i..].starts_with(lit.as_bytes())
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n')) {
            self.i += 1;
        }
    }

    fn expect(&mut self, lit: &str) -> Result<(), String> {
        if self.starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(format!("expected {lit:?} at byte {}", self.i))
        }
    }

    fn name(&mut self) -> Result<String, String> {
        let start = self.i;
        while let Some(c) = self.peek() {
            let ok = c.is_ascii_alphabetic()
                || c == b'_'
                || (self.i > start && (c.is_ascii_digit() || c == b'-' || c == b'.'));
            if !ok {
                break;
            }
            self.i += 1;
        }
        if self.i == start {
            return Err(format!("expected a name at byte {start}"));
        }
        if self.peek() == Some(b':') {
            return Err("namespaces are not supported".to_string());
        }
        Ok(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }

    /// `name="value"` with a plain value.
    fn attribute(&mut self) -> Result<(String, String), String> {
        let name = self.name()?;
        self.skip_ws();
        self.expect("=")?;
        self.skip_ws();
        let quote = self.peek().filter(|q| *q == b'"' || *q == b'\'');
        let Some(quote) = quote else {
            return Err(format!("expected a quoted value at byte {}", self.i));
        };
        self.i += 1;
        let start = self.i;
        while let Some(c) = self.peek() {
            if c == quote {
                break;
            }
            if matches!(c, b'<' | b'&' | b'"' | b'\'' | b'\t' | b'\n' | b'>') {
                return Err("attribute values needing escapes are not supported".to_string());
            }
            self.i += 1;
        }
        let value = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
        self.expect(if quote == b'"' { "\"" } else { "'" })?;
        Ok((name, value))
    }

    fn declaration(&mut self) -> Result<(String, Option<String>, Option<bool>), String> {
        if !self.starts_with("<?xml") {
            return Ok(("1.0".to_string(), None, None));
        }
        self.i += 5;
        let mut version = None;
        let mut encoding = None;
        let mut standalone = None;
        loop {
            self.skip_ws();
            if self.starts_with("?>") {
                self.i += 2;
                break;
            }
            let (k, v) = self.attribute()?;
            match k.as_str() {
                "version" => version = Some(v),
                "encoding" => encoding = Some(v),
                "standalone" => {
                    standalone = Some(match v.as_str() {
                        "yes" => true,
                        "no" => false,
                        _ => return Err("invalid standalone value".to_string()),
                    })
                }
                _ => return Err(format!("unsupported declaration attribute {k}")),
            }
        }
        let version = version.ok_or("the XML declaration lacks a version")?;
        if version != "1.0" {
            return Err(format!("unsupported XML version {version}"));
        }
        Ok((version, encoding, standalone))
    }

    fn element(&mut self) -> Result<Arc<XmlElement>, String> {
        if self.starts_with("<!") || self.starts_with("<?") {
            return Err(
                "comments, CDATA, DTDs and processing instructions are not supported".into(),
            );
        }
        self.expect("<")?;
        let name = self.name()?;
        let mut attributes = Vec::new();
        loop {
            self.skip_ws();
            if self.starts_with("/>") {
                self.i += 2;
                return Ok(Arc::new(XmlElement {
                    name,
                    attributes,
                    children: Vec::new(),
                }));
            }
            if self.starts_with(">") {
                self.i += 1;
                break;
            }
            let attr = self.attribute()?;
            if attr.0 == "xmlns" {
                return Err("namespaces are not supported".to_string());
            }
            if attributes.iter().any(|(n, _)| *n == attr.0) {
                return Err(format!("duplicate attribute {}", attr.0));
            }
            attributes.push(attr);
        }
        let mut children = Vec::new();
        let mut text = String::new();
        loop {
            match self.peek() {
                None => return Err(format!("unterminated element {name}")),
                Some(b'<') if self.starts_with("</") => {
                    if !text.is_empty() {
                        children.push(XmlNode::Text(std::mem::take(&mut text)));
                    }
                    self.i += 2;
                    let end = self.name()?;
                    if end != name {
                        return Err(format!("mismatched end tag {end} for {name}"));
                    }
                    self.skip_ws();
                    self.expect(">")?;
                    return Ok(Arc::new(XmlElement {
                        name,
                        attributes,
                        children,
                    }));
                }
                Some(b'<') => {
                    if !text.is_empty() {
                        children.push(XmlNode::Text(std::mem::take(&mut text)));
                    }
                    children.push(XmlNode::Element(self.element()?));
                }
                Some(b'&') => text.push_str(&self.reference()?),
                Some(c) => {
                    text.push(char::from(c));
                    self.i += 1;
                }
            }
        }
    }

    /// The predefined entities and character references.
    fn reference(&mut self) -> Result<String, String> {
        let end = self.s[self.i..]
            .iter()
            .position(|&c| c == b';')
            .ok_or("unterminated reference")?;
        let body = String::from_utf8_lossy(&self.s[self.i + 1..self.i + end]).into_owned();
        self.i += end + 1;
        let code = match body.as_str() {
            "lt" => '<' as u32,
            "gt" => '>' as u32,
            "amp" => '&' as u32,
            "apos" => '\'' as u32,
            "quot" => '"' as u32,
            _ => {
                let digits = body
                    .strip_prefix('#')
                    .ok_or("only predefined entities are supported")?;
                let n = match digits.strip_prefix('x') {
                    Some(hex) => u32::from_str_radix(hex, 16),
                    None => digits.parse(),
                }
                .map_err(|_| format!("invalid character reference &{body};"))?;
                if n == 0 || n > 0x7f {
                    return Err("only ASCII character references are supported".to_string());
                }
                n
            }
        };
        Ok(char::from_u32(code).map(String::from).unwrap_or_default())
    }
}

/// `xmlIsBlankNode()`: a text node of blanks only.
fn is_blank_text(t: &str) -> bool {
    t.bytes().all(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
}

/// `xmlNodeListGetString(doc, list, 1)`: the concatenated text nodes of the list.
fn list_text(nodes: &[XmlNode]) -> String {
    let mut out = String::new();
    for n in nodes {
        if let XmlNode::Text(t) = n {
            out.push_str(t);
        }
    }
    out
}

/// libxml2's `xmlEscapeContent()` for text nodes.
fn escape_text(t: &str, out: &mut String) {
    for c in t.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

/// `xmlNodeDumpOutputInternal()` without formatting.
fn dump_element(e: &XmlElement, out: &mut String) {
    out.push('<');
    out.push_str(&e.name);
    for (k, v) in &e.attributes {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(v);
        out.push('"');
    }
    if e.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in &e.children {
        match c {
            XmlNode::Element(child) => dump_element(child, out),
            XmlNode::Text(t) => escape_text(t, out),
        }
    }
    out.push_str("</");
    out.push_str(&e.name);
    out.push('>');
}

impl XmlDocument {
    /// `xmlSaveDoc()` without formatting: the declaration, the root element and a newline.
    pub fn dump(&self) -> String {
        let mut out = String::from("<?xml version=\"");
        out.push_str(&self.version);
        out.push('"');
        if let Some(enc) = &self.encoding {
            out.push_str(" encoding=\"");
            out.push_str(enc);
            out.push('"');
        }
        match self.standalone {
            Some(false) => out.push_str(" standalone=\"no\""),
            Some(true) => out.push_str(" standalone=\"yes\""),
            None => {}
        }
        out.push_str("?>\n");
        dump_element(&self.root, &mut out);
        out.push('\n');
        out
    }
}

static RECURSIVE_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::interface("RecursiveIterator")
        .implements(&builtin::iterator())
        .method("hasChildren")
        .method("getChildren")
        .build()
});

/// `interface RecursiveIterator extends Iterator`.
pub fn recursive_iterator_interface() -> ClassRef {
    RECURSIVE_ITERATOR.clone()
}

static SIMPLE_XML_ELEMENT: LazyLock<ClassRef> = LazyLock::new(|| {
    let mut b = ClassBuilder::class("SimpleXMLElement")
        .implements(&builtin::stringable())
        .implements(&builtin::countable())
        .implements(&RECURSIVE_ITERATOR);
    for m in [
        "xpath",
        "registerXPathNamespace",
        "asXML",
        "saveXML",
        "getNamespaces",
        "getDocNamespaces",
        "children",
        "attributes",
        "__construct",
        "addChild",
        "addAttribute",
        "getName",
        "__toString",
        "__debugInfo",
        "count",
        "rewind",
        "valid",
        "current",
        "key",
        "next",
        "hasChildren",
        "getChildren",
    ] {
        b = b.method(m);
    }
    b.build()
});

/// `class SimpleXMLElement implements Stringable, Countable, RecursiveIterator`.
pub fn simple_xml_element_class() -> ClassRef {
    SIMPLE_XML_ELEMENT.clone()
}

/// The iteration state (`sxe->iter.data`): the current child element object and its position.
type IterData = Option<(usize, ObjectRef)>;

/// A `SimpleXMLElement` over an element node (`SXE_ITER_NONE`).
pub struct SimpleXmlElement {
    class: ClassRef,
    props: PropertyStore,
    doc: Arc<XmlDocument>,
    element: Arc<XmlElement>,
    is_root: bool,
    iter: Mutex<IterData>,
}

impl SimpleXmlElement {
    /// The object for the document's root element (`new SimpleXMLElement($xml)`).
    pub fn root(doc: Arc<XmlDocument>) -> SimpleXmlElement {
        let element = doc.root.clone();
        SimpleXmlElement::over(doc, element, true)
    }

    fn over(doc: Arc<XmlDocument>, element: Arc<XmlElement>, is_root: bool) -> SimpleXmlElement {
        let class = simple_xml_element_class();
        let props = PropertyStore::new(&class);
        SimpleXmlElement {
            class,
            props,
            doc,
            element,
            is_root,
            iter: Mutex::new(None),
        }
    }

    /// Wraps into a handle.
    pub fn into_ref(self) -> ObjectRef {
        ObjectRef::new(self)
    }

    /// The element.
    pub fn element(&self) -> &Arc<XmlElement> {
        &self.element
    }

    fn iter(&self) -> MutexGuard<'_, IterData> {
        self.iter.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn child_object(&self, e: &Arc<XmlElement>) -> ObjectRef {
        SimpleXmlElement::over(self.doc.clone(), e.clone(), false).into_ref()
    }

    /// `php_sxe_iterator_fetch()` from child position `from`: the next element child.
    fn fetch(&self, from: usize) -> IterData {
        self.element.children[from.min(self.element.children.len())..]
            .iter()
            .enumerate()
            .find_map(|(i, n)| match n {
                XmlNode::Element(e) => Some((from + i, self.child_object(e))),
                XmlNode::Text(_) => None,
            })
    }

    /// `php_sxe_reset_iterator()`.
    fn reset_iterator(&self) {
        let data = self.fetch(0);
        *self.iter() = data;
    }

    /// `php_sxe_move_forward_iterator()`.
    fn move_forward(&self) {
        let pos = self.iter().as_ref().map(|(i, _)| *i);
        let data = match pos {
            Some(i) => self.fetch(i + 1),
            None => None,
        };
        *self.iter() = data;
    }

    /// `php_sxe_count_elements_helper()`.
    fn count_children(&self) -> i64 {
        self.element
            .children
            .iter()
            .filter(|n| matches!(n, XmlNode::Element(_)))
            .count() as i64
    }

    /// `sxe_object_cast_ex(IS_STRING)` for an element node.
    fn contents(&self) -> String {
        list_text(&self.element.children)
    }

    /// `sxe_prop_is_empty()` for an element node.
    fn prop_is_empty(&self) -> bool {
        if !self.element.attributes.is_empty() {
            return false;
        }
        let children = &self.element.children;
        for n in children {
            let has_children = matches!(n, XmlNode::Element(e) if !e.children.is_empty());
            if has_children || children.len() > 1 {
                if let XmlNode::Text(_) = n {
                    continue;
                }
            } else if let XmlNode::Text(t) = n {
                if !t.is_empty() {
                    return false;
                }
                continue;
            }
            return false;
        }
        true
    }

    /// `sxe_get_prop_hash()` for an element node.
    fn prop_hash(&self) -> Array {
        let mut rv = Array::new();
        if !self.element.attributes.is_empty() {
            let mut attrs = Array::new();
            for (k, v) in &self.element.attributes {
                attrs.insert(k.as_str(), Value::from(v.as_str()));
            }
            properties_add(&mut rv, "@attributes", Value::Array(attrs));
        }
        let children = &self.element.children;
        for n in children {
            let has_children = matches!(n, XmlNode::Element(e) if !e.children.is_empty());
            let blank = matches!(n, XmlNode::Text(t) if is_blank_text(t));
            if has_children || children.len() > 1 || blank {
                if let XmlNode::Text(_) = n {
                    continue;
                }
            } else if let XmlNode::Text(t) = n {
                if !t.is_empty() {
                    let _ = rv.push(Value::from(t.as_str()));
                }
                continue;
            }
            if let XmlNode::Element(e) = n {
                // get_base_node_value()
                let value = match e.children.first() {
                    Some(XmlNode::Text(t)) if !is_blank_text(t) => {
                        Value::from(list_text(&e.children))
                    }
                    _ => Value::Object(self.child_object(e)),
                };
                properties_add(&mut rv, &e.name, value);
            }
        }
        rv
    }

    fn prop_entries(&self) -> Vec<PropertyEntry> {
        self.prop_hash()
            .into_iter()
            .map(|(key, value)| PropertyEntry {
                key,
                value: Some(value),
                type_name: None,
            })
            .collect()
    }

    /// `asXML()`: the whole document for the root element, the element alone otherwise.
    pub fn as_xml(&self) -> String {
        if self.is_root {
            self.doc.dump()
        } else {
            let mut out = String::new();
            dump_element(&self.element, &mut out);
            out
        }
    }

    fn unsupported(&self, what: &str) -> PhpError {
        PhpError::error(format!(
            "SimpleXMLElement {what} is not supported by twig-test-support"
        ))
    }
}

/// `sxe_properties_add()`: a repeated name becomes a list.
fn properties_add(rv: &mut Array, name: &str, value: Value) {
    let key = ArrayKey::Str(PhpString::from(name));
    match rv.get_mut(&key) {
        Some(Value::Array(list)) => {
            let _ = list.push(value);
        }
        Some(existing) => {
            let first = std::mem::take(existing);
            *existing = Value::Array(Array::from_values([first, value]));
        }
        None => {
            rv.insert(key, value);
        }
    }
}

/// The parameters of the implemented methods of `SimpleXMLElement` (by lowercase name), from
/// PHPSRC/ext/simplexml/simplexml.stub.php.
fn simple_xml_element_params(method: &str) -> Option<Vec<Param>> {
    Some(match method {
        // asXML(?string $filename = null), saveXML(?string $filename = null)
        "asxml" | "savexml" => vec![Param::optional("filename", Value::Null)],
        "count" | "rewind" | "valid" | "current" | "key" | "next" | "getname" | "__tostring"
        | "__debuginfo" => Vec::new(),
        _ => return None,
    })
}

impl Object for SimpleXmlElement {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn method_params(&self, method: &str) -> Option<MethodParams> {
        simple_xml_element_params(method).map(MethodParams::internal)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let none = |m: &str| {
            args.bind(
                &format!("SimpleXMLElement::{m}"),
                &[],
                FunctionKind::Internal,
            )
            .map(|_| ())
        };
        let not_initialized = || PhpError::error("Iterator not initialized or already consumed");
        match method {
            "count" => none("count").map(|_| Value::Int(self.count_children())),
            "rewind" => {
                none("rewind")?;
                self.reset_iterator();
                Ok(Value::Null)
            }
            "valid" => none("valid").map(|_| Value::Bool(self.iter().is_some())),
            "current" => {
                none("current")?;
                self.iter()
                    .as_ref()
                    .map(|(_, o)| Value::Object(o.clone()))
                    .ok_or_else(not_initialized)
            }
            "key" => {
                none("key")?;
                let iter = self.iter();
                let (_, o) = iter.as_ref().ok_or_else(not_initialized)?;
                let name = o
                    .downcast_ref::<SimpleXmlElement>()
                    .map(|c| c.element.name.clone())
                    .unwrap_or_default();
                Ok(Value::from(name))
            }
            "next" => {
                none("next")?;
                self.move_forward();
                Ok(Value::Null)
            }
            "getname" => none("getName").map(|_| Value::from(self.element.name.as_str())),
            "__tostring" => none("__toString").map(|_| Value::from(self.contents())),
            "asxml" | "savexml" => {
                if !args.is_empty() {
                    return Err(self.unsupported("asXML() with a file name"));
                }
                Ok(Value::from(self.as_xml()))
            }
            "__debuginfo" => none("__debugInfo").map(|_| Value::Array(self.prop_hash())),
            m if self.class.method(m).is_some() => {
                let name = self
                    .class
                    .method(m)
                    .map(|i| i.name.to_string())
                    .unwrap_or_default();
                Err(self.unsupported(&format!("::{name}()")))
            }
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }

    fn read_property(
        &self,
        _this: &ObjectRef,
        _name: &[u8],
        _mode: FetchMode,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        Err(self.unsupported("property access"))
    }

    fn has_property(
        &self,
        _this: &ObjectRef,
        _name: &[u8],
        _check: PropertyCheck,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        Err(self.unsupported("property access"))
    }

    fn read_dimension(
        &self,
        _this: &ObjectRef,
        _offset: &Value,
        _mode: FetchMode,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        Err(self.unsupported("array access"))
    }

    fn has_dimension(
        &self,
        _this: &ObjectRef,
        _offset: &Value,
        _check_empty: bool,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        Err(self.unsupported("array access"))
    }

    /// `sxe_get_prop_hash()` for every purpose (`get_properties` and `__debugInfo`).
    fn properties_for(
        &self,
        _this: &ObjectRef,
        _purpose: PropertyPurpose,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        Ok(self.prop_entries())
    }

    /// `sxe_count_elements()`.
    fn count_elements(
        &self,
        _this: &ObjectRef,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<i64>> {
        Ok(Some(self.count_children()))
    }

    /// `php_sxe_get_iterator()`: drives the object's own iteration state.
    fn get_iterator(
        &self,
        this: &ObjectRef,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Box<dyn ObjectIterator>>> {
        Ok(Some(Box::new(SxeIterator {
            object: this.clone(),
        })))
    }

    /// `sxe_object_cast_ex()` + `cast_object()`: string contents, `!sxe_prop_is_empty()` for
    /// bool, `convert_to_long()` / `convert_to_double()` / `convert_scalar_to_number()` of the
    /// contents for numbers.
    fn cast(
        &self,
        _this: &ObjectRef,
        target: CastTarget,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Value>> {
        let contents = Value::from(self.contents());
        Ok(Some(match target {
            CastTarget::Bool => Value::Bool(!self.prop_is_empty()),
            CastTarget::String => contents,
            CastTarget::Long => Value::Int(convert::to_long(&contents, diag)?),
            CastTarget::Double => Value::Float(convert::to_double(&contents, diag)?),
            // convert_scalar_to_number(): numeric strings (trailing data allowed), else 0.
            CastTarget::Number => match convert::convert_scalar_to_number(&contents, diag)? {
                Some(n) => Value::from(n),
                None => Value::Int(0),
            },
        }))
    }
}

/// `php_sxe_iterator_funcs`.
struct SxeIterator {
    object: ObjectRef,
}

impl SxeIterator {
    fn sxe(&self) -> PhpResult<&SimpleXmlElement> {
        self.object
            .downcast_ref::<SimpleXmlElement>()
            .ok_or_else(|| PhpError::error("not a SimpleXMLElement"))
    }
}

impl ObjectIterator for SxeIterator {
    fn rewind(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.sxe()?.reset_iterator();
        Ok(())
    }

    fn valid(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        Ok(self.sxe()?.iter().is_some())
    }

    fn current(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        Ok(self
            .sxe()?
            .iter()
            .as_ref()
            .map(|(_, o)| Value::Object(o.clone()))
            .unwrap_or(Value::Null))
    }

    fn key(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        let sxe = self.sxe()?;
        let iter = sxe.iter();
        Ok(match iter.as_ref() {
            Some((_, o)) => o
                .downcast_ref::<SimpleXmlElement>()
                .map(|c| Value::from(c.element.name.as_str()))
                .unwrap_or(Value::Null),
            None => Value::Null,
        })
    }

    fn next(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.sxe()?.move_forward();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let src = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><doc><elem>foo</elem><elem/></doc>";
        let doc = parse_document(src).unwrap();
        assert_eq!(
            doc.dump(),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<doc><elem>foo</elem><elem/></doc>\n"
        );
        let doc = parse_document("<items><item>1 &amp; &lt;2&gt;</item></items>").unwrap();
        assert_eq!(
            doc.dump(),
            "<?xml version=\"1.0\"?>\n<items><item>1 &amp; &lt;2&gt;</item></items>\n"
        );
        assert!(parse_document("<a><!-- c --></a>").is_err());
        assert!(parse_document("<a xmlns=\"x\"/>").is_err());
        assert!(parse_document("<a>\u{e9}</a>").is_err());
    }
}
