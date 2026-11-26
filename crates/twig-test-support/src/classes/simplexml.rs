//! `SimpleXMLElement` (PHPSRC/ext/simplexml/simplexml.c) over a small immutable XML tree.
//!
//! An object is a node of a document (an element or an attribute) plus `iter.type` and `iter.name`,
//! which select the nodes it stands for, like `php_sxe_object`: the node itself (`SXE_ITER_NONE`:
//! the document root, an element yielded by an iteration, an attribute), its element children
//! (`SXE_ITER_CHILD`, `children()`), its element children of one name (`SXE_ITER_ELEMENT`,
//! `$sxe->name`) or its attributes (`SXE_ITER_ATTRLIST`, `attributes()`). Ported: property reads
//! (`sxe_prop_dim_read()` of the elements, `$sxe->child`, `$sxe->child[1]`, also on attribute lists,
//! where they read attributes), dimension reads (the same function on the attributes,
//! `$sxe['attr']`, `$sxe->child[0]`), `isset()` / `empty()` / `property_exists()` of both
//! (`sxe_prop_dim_exists()`), `children()` and `attributes()` without a namespace, the iteration
//! (`php_sxe_reset_iterator()`, `php_sxe_iterator_fetch()`, `php_sxe_move_forward_iterator()`, the
//! `RecursiveIterator` methods), `count()` (`php_sxe_count_elements_helper()`), casts
//! (`sxe_object_cast_ex()`, `sxe_prop_is_empty()`), the property table of `var_dump()`, `(array)`
//! and `json_encode()` (`sxe_get_prop_hash()`), comparisons (`sxe_objects_compare()`: equal over
//! the same node), `getName()` and `asXML()` (libxml2's `xmlSaveDoc()` / `xmlNodeDumpOutput()`
//! without formatting).
//!
//! Supported XML: an optional `<?xml version="1.0" ...?>` declaration, one root element, nested
//! elements with plain attributes, text with the predefined entities and character references,
//! ASCII only, no namespaces, comments, CDATA, processing instructions or DTDs. Anything else is
//! rejected by [`parse_document`]. Writes (`$sxe->child = ...`, `unset()`), `xpath()`,
//! `addChild()`, `addAttribute()`, namespace filters and the namespace methods are not modelled
//! (they throw `Error`).

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use twig_value::object::{
    CallArgs, CastTarget, ClassBuilder, ClassRef, FetchMode, FunctionKind, MethodParams, Object,
    ObjectIterator, ObjectRef, Param, PropertyCheck, PropertyEntry, PropertyPurpose, PropertyStore,
    builtin,
};
use twig_value::param::{self, Arg};
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

/// `sxe->iter.type`: which nodes an object stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IterType {
    /// `SXE_ITER_NONE`: the node itself (the document root, an element or attribute the object was
    /// created for).
    None,
    /// `SXE_ITER_CHILD`: the element children of the node (`children()`).
    Child,
    /// `SXE_ITER_ELEMENT`: the element children named `iter.name` (`$sxe->name`).
    Element,
    /// `SXE_ITER_ATTRLIST`: the attributes of the node (`attributes()`).
    AttrList,
}

/// An `xmlNodePtr` an object can hold: an element or an attribute (text nodes are only walked).
#[derive(Clone, Debug, PartialEq, Eq)]
enum NodeId {
    /// An element, by the indexes of `children` that lead to it from the root element (empty: the
    /// root, whose parent is the document node).
    Element(Vec<usize>),
    /// The attribute at an index of an element's `attributes`.
    Attribute(Vec<usize>, usize),
}

/// Where a `node->next` walk starts: a position in the children or in the attributes of an element.
enum Cursor {
    /// Nothing (`NULL`).
    End,
    /// The children of the element at `parent`, from index `from`.
    Children { parent: Vec<usize>, from: usize },
    /// The attributes of the element at `element`, from index `from`.
    Attributes { element: Vec<usize>, from: usize },
}

/// `node->next` of a node: the root element is the only child of the document node.
fn next_sibling(node: &NodeId) -> Cursor {
    match node {
        NodeId::Element(path) => match path.split_last() {
            None => Cursor::End,
            Some((last, parent)) => Cursor::Children {
                parent: parent.to_vec(),
                from: last + 1,
            },
        },
        NodeId::Attribute(element, index) => Cursor::Attributes {
            element: element.clone(),
            from: index + 1,
        },
    }
}

impl XmlDocument {
    /// The element at `path` (indexes of `children` from the root element).
    fn element_at(&self, path: &[usize]) -> &Arc<XmlElement> {
        let mut element = &self.root;
        for &i in path {
            match &element.children[i] {
                XmlNode::Element(child) => element = child,
                XmlNode::Text(_) => unreachable!("an element path leads through elements"),
            }
        }
        element
    }

    /// `node->name`.
    fn node_name(&self, node: &NodeId) -> &str {
        match node {
            NodeId::Element(path) => &self.element_at(path).name,
            NodeId::Attribute(element, index) => &self.element_at(element).attributes[*index].0,
        }
    }

    /// `xmlNodeListGetString(doc, node->children, 1)`: the text of an element's children, the value
    /// of an attribute.
    fn node_text(&self, node: &NodeId) -> String {
        match node {
            NodeId::Element(path) => list_text(&self.element_at(path).children),
            NodeId::Attribute(element, index) => {
                self.element_at(element).attributes[*index].1.clone()
            }
        }
    }

    /// `node->parent->children == node->parent->last`: the parent has a single child (the document
    /// node holds nothing but the root element).
    fn parent_has_one_child(&self, path: &[usize]) -> bool {
        match path.split_last() {
            None => true,
            Some((_, parent)) => self.element_at(parent).children.len() == 1,
        }
    }

    /// `node->next != NULL` for an element.
    fn has_next_sibling(&self, path: &[usize]) -> bool {
        match path.split_last() {
            None => false,
            Some((last, parent)) => last + 1 < self.element_at(parent).children.len(),
        }
    }
}

/// `node->children->children`: the first child of an element is an element with children.
fn first_child_has_children(element: &XmlElement) -> bool {
    matches!(element.children.first(), Some(XmlNode::Element(e)) if !e.children.is_empty())
}

/// `ZEND_UNCOMPARABLE`.
const UNCOMPARABLE: i32 = 1;

/// A `SimpleXMLElement` object (`php_sxe_object`): a node of a document and `iter.type` /
/// `iter.name`, which select the nodes it stands for, plus the iteration state `iter.data`.
pub struct SimpleXmlElement {
    class: ClassRef,
    props: PropertyStore,
    doc: Arc<XmlDocument>,
    /// `sxe->node->node`.
    node: NodeId,
    iter_type: IterType,
    /// `sxe->iter.name`.
    iter_name: Option<String>,
    /// `sxe->iter.data`: the current node and the object standing for it.
    iter: Mutex<Option<(NodeId, ObjectRef)>>,
}

impl SimpleXmlElement {
    /// The object for the document's root element (`new SimpleXMLElement($xml)`).
    pub fn root(doc: Arc<XmlDocument>) -> SimpleXmlElement {
        SimpleXmlElement::over(doc, NodeId::Element(Vec::new()), IterType::None, None)
    }

    fn over(
        doc: Arc<XmlDocument>,
        node: NodeId,
        iter_type: IterType,
        iter_name: Option<String>,
    ) -> SimpleXmlElement {
        let class = simple_xml_element_class();
        let props = PropertyStore::new(&class);
        SimpleXmlElement {
            class,
            props,
            doc,
            node,
            iter_type,
            iter_name,
            iter: Mutex::new(None),
        }
    }

    /// Wraps into a handle.
    pub fn into_ref(self) -> ObjectRef {
        ObjectRef::new(self)
    }

    fn iter(&self) -> MutexGuard<'_, Option<(NodeId, ObjectRef)>> {
        self.iter.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `node_as_zval()`: a new object of the same document.
    fn node_as_zval(&self, node: NodeId, iter_type: IterType, name: Option<&str>) -> Value {
        Value::Object(
            SimpleXmlElement::over(
                Arc::clone(&self.doc),
                node,
                iter_type,
                name.map(str::to_owned),
            )
            .into_ref(),
        )
    }

    /// The start of `php_sxe_reset_iterator_no_clear_iter_data()`: `node->children`, or
    /// `node->properties` for an attribute list. An attribute has no element children, and its
    /// `properties` (the `psvi` of an `xmlAttr`) is `NULL`.
    fn first_cursor(&self) -> Cursor {
        match (&self.node, self.iter_type) {
            (NodeId::Element(path), IterType::AttrList) => Cursor::Attributes {
                element: path.clone(),
                from: 0,
            },
            (NodeId::Element(path), _) => Cursor::Children {
                parent: path.clone(),
                from: 0,
            },
            (NodeId::Attribute(..), _) => Cursor::End,
        }
    }

    /// `php_sxe_iterator_fetch()` without `use_data`: the first node from `cursor` on that the
    /// object stands for (no namespaces: `match_ns()` always holds).
    fn fetch(&self, cursor: Cursor) -> Option<NodeId> {
        match cursor {
            Cursor::End => None,
            // only an attribute list looks for attribute nodes
            Cursor::Attributes { element, from } if self.iter_type == IterType::AttrList => self
                .doc
                .element_at(&element)
                .attributes
                .iter()
                .enumerate()
                .skip(from)
                .find(|(_, (name, _))| self.iter_name.as_ref().is_none_or(|n| n == name))
                .map(|(i, _)| NodeId::Attribute(element.clone(), i)),
            Cursor::Attributes { .. } => None,
            Cursor::Children { .. } if self.iter_type == IterType::AttrList => None,
            Cursor::Children { parent, from } => self
                .doc
                .element_at(&parent)
                .children
                .iter()
                .enumerate()
                .skip(from)
                .find(|(_, node)| match node {
                    XmlNode::Element(e) => match (self.iter_type, &self.iter_name) {
                        (IterType::Element, Some(name)) => e.name == *name,
                        _ => true,
                    },
                    XmlNode::Text(_) => false,
                })
                .map(|(i, _)| {
                    let mut path = parent.clone();
                    path.push(i);
                    NodeId::Element(path)
                }),
        }
    }

    /// `php_sxe_reset_iterator_no_clear_iter_data(sxe, 0)`.
    fn reset_no_clear(&self) -> Option<NodeId> {
        self.fetch(self.first_cursor())
    }

    /// `php_sxe_get_first_node_non_destructive()`: the first node of the iteration, the node itself
    /// for `SXE_ITER_NONE`.
    fn first_node(&self) -> Option<NodeId> {
        if self.iter_type == IterType::None {
            Some(self.node.clone())
        } else {
            self.reset_no_clear()
        }
    }

    /// `php_sxe_reset_iterator()`.
    fn reset_iterator(&self) {
        let data = self.reset_no_clear().map(|node| self.iter_data(node));
        *self.iter() = data;
    }

    /// `php_sxe_iterator_fetch(sxe, node, 1)`'s `iter.data` for `node`.
    fn iter_data(&self, node: NodeId) -> (NodeId, ObjectRef) {
        let Value::Object(object) = self.node_as_zval(node.clone(), IterType::None, None) else {
            unreachable!("node_as_zval() makes an object");
        };
        (node, object)
    }

    /// `php_sxe_move_forward_iterator()`.
    fn move_forward(&self) {
        let current = self.iter().take().map(|(node, _)| node);
        let data = current
            .and_then(|node| self.fetch(next_sibling(&node)))
            .map(|node| self.iter_data(node));
        *self.iter() = data;
    }

    /// `php_sxe_count_elements_helper()`.
    fn count_children(&self) -> i64 {
        let mut count = 0;
        let mut node = self.reset_no_clear();
        while let Some(current) = node {
            count += 1;
            node = self.fetch(next_sibling(&current));
        }
        count
    }

    /// The contents `sxe_object_cast_ex()` converts: the text of the first node (`NULL`, converted
    /// like an empty string, when there is none).
    fn contents(&self) -> String {
        self.first_node()
            .map(|node| self.doc.node_text(&node))
            .unwrap_or_default()
    }

    /// The attributes `sxe_get_prop_hash()` and `sxe_prop_is_empty()` look at: those of the first
    /// node for `SXE_ITER_ELEMENT`, of the node otherwise, only the named one of a named attribute
    /// list.
    fn listed_attributes(&self) -> Vec<(String, String)> {
        let node = if self.iter_type == IterType::Element {
            self.first_node()
        } else {
            Some(self.node.clone())
        };
        let Some(NodeId::Element(path)) = node else {
            return Vec::new();
        };
        let test = self.iter_type == IterType::AttrList && self.iter_name.is_some();
        self.doc
            .element_at(&path)
            .attributes
            .iter()
            .filter(|(name, _)| !test || self.iter_name.as_ref() == Some(name))
            .cloned()
            .collect()
    }

    /// The first child the loops of `sxe_get_prop_hash()` and `sxe_prop_is_empty()` look at, and
    /// whether they walk with the iterator (`use_iter`); `extra_next` adds the `!node->next` test of
    /// `sxe_get_prop_hash()`.
    fn child_walk(&self, path: &[usize], extra_next: bool) -> (Option<(Vec<usize>, usize)>, bool) {
        let element = self.doc.element_at(path);
        if self.iter_type == IterType::Child {
            // the loop starts at the first child element itself
            let (index, parent) = path.split_last().expect("a child element has a parent");
            return (Some((parent.to_vec(), *index)), false);
        }
        let plain = self.iter_type == IterType::None
            || element.children.is_empty()
            || (extra_next && !self.doc.has_next_sibling(path))
            || element.children.len() > 1
            || first_child_has_children(element)
            || self.doc.parent_has_one_child(path);
        if plain {
            let start = (!element.children.is_empty()).then(|| (path.to_vec(), 0));
            (start, false)
        } else {
            let start = self.reset_no_clear().map(|node| match node {
                NodeId::Element(p) => {
                    let (last, parent) = p.split_last().expect("an iterated element has a parent");
                    (parent.to_vec(), *last)
                }
                NodeId::Attribute(..) => unreachable!("only attribute lists iterate attributes"),
            });
            (start, true)
        }
    }

    /// The next position of the loops of `sxe_get_prop_hash()` and `sxe_prop_is_empty()`.
    fn walk_next(
        &self,
        parent: Vec<usize>,
        index: usize,
        use_iter: bool,
    ) -> Option<(Vec<usize>, usize)> {
        if use_iter {
            let mut path = parent.clone();
            path.push(index);
            match self.fetch(next_sibling(&NodeId::Element(path))) {
                Some(NodeId::Element(p)) => {
                    let (last, parent) = p.split_last().expect("an iterated element has a parent");
                    Some((parent.to_vec(), *last))
                }
                _ => None,
            }
        } else if index + 1 < self.doc.element_at(&parent).children.len() {
            Some((parent, index + 1))
        } else {
            None
        }
    }

    /// Port of `sxe_prop_is_empty()`.
    fn prop_is_empty(&self) -> bool {
        if !self.listed_attributes().is_empty() {
            return false;
        }
        let node = self.first_node();
        let Some(node) = node.filter(|_| self.iter_type != IterType::AttrList) else {
            return true;
        };
        let path = match node {
            NodeId::Attribute(..) => return false,
            NodeId::Element(path) => path,
        };
        let (mut at, use_iter) = self.child_walk(&path, false);
        while let Some((parent, index)) = at {
            let siblings = &self.doc.element_at(&parent).children;
            let child = &siblings[index];
            let has_children = matches!(child, XmlNode::Element(e) if !e.children.is_empty());
            let skip_text = has_children || index > 0 || index + 1 < siblings.len();
            match child {
                XmlNode::Text(_) if skip_text => {}
                XmlNode::Text(t) => {
                    if !t.is_empty() {
                        return false;
                    }
                }
                XmlNode::Element(_) => return false,
            }
            at = self.walk_next(parent, index, use_iter);
        }
        true
    }

    /// Port of `sxe_get_prop_hash()` (`is_debug` for `__debugInfo()`).
    fn prop_hash(&self, is_debug: bool) -> Array {
        let mut rv = Array::new();
        if is_debug || self.iter_type != IterType::Child {
            let attributes = self.listed_attributes();
            if !attributes.is_empty() {
                let mut zattr = Array::new();
                for (name, value) in attributes {
                    zattr.insert(name.as_str(), Value::from(value));
                }
                properties_add(&mut rv, "@attributes", Value::Array(zattr));
            }
        }
        let node = self.first_node();
        let Some(node) = node.filter(|_| self.iter_type != IterType::AttrList) else {
            return rv;
        };
        let path = match node {
            NodeId::Attribute(..) => {
                let _ = rv.push(Value::from(self.doc.node_text(&node)));
                return rv;
            }
            NodeId::Element(path) => path,
        };
        let (mut at, use_iter) = self.child_walk(&path, true);
        while let Some((parent, index)) = at {
            let siblings = &self.doc.element_at(&parent).children;
            let child = &siblings[index];
            let has_children = matches!(child, XmlNode::Element(e) if !e.children.is_empty());
            let blank = matches!(child, XmlNode::Text(t) if is_blank_text(t));
            let skip_text = has_children || index > 0 || index + 1 < siblings.len() || blank;
            match child {
                XmlNode::Text(_) if skip_text => {}
                XmlNode::Text(t) => {
                    if !t.is_empty() {
                        let _ = rv.push(Value::from(t.as_str()));
                    }
                }
                XmlNode::Element(e) => {
                    let mut child_path = parent.clone();
                    child_path.push(index);
                    let value = self.base_node_value(e, child_path);
                    if use_iter {
                        let _ = rv.push(value);
                    } else {
                        properties_add(&mut rv, &e.name, value);
                    }
                }
            }
            at = self.walk_next(parent, index, use_iter);
        }
        rv
    }

    /// Port of `get_base_node_value()`: the text of an element whose first child is text that is
    /// not blank, an object for the element otherwise.
    fn base_node_value(&self, element: &XmlElement, path: Vec<usize>) -> Value {
        match element.children.first() {
            Some(XmlNode::Text(t)) if !is_blank_text(t) => {
                Value::from(list_text(&element.children))
            }
            _ => self.node_as_zval(NodeId::Element(path), IterType::None, None),
        }
    }

    fn prop_entries(&self, is_debug: bool) -> Vec<PropertyEntry> {
        self.prop_hash(is_debug)
            .into_iter()
            .map(|(key, value)| PropertyEntry {
                key,
                value: Some(value),
                type_name: None,
            })
            .collect()
    }

    /// The member of `sxe_prop_dim_read()` / `sxe_prop_dim_exists()`: an int offset, else a name
    /// (`zval_try_get_string_func()` of anything that is not a string).
    fn member(offset: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Member> {
        Ok(match offset {
            Value::Int(i) => Member::Long(*i),
            Value::String(s) => Member::Name(s.to_string_lossy().into_owned()),
            other => Member::Name(
                convert::to_string(other, diag)?
                    .to_string_lossy()
                    .into_owned(),
            ),
        })
    }

    /// The attributes `sxe_prop_dim_read()` and `sxe_prop_dim_exists()` walk (`attr`, then
    /// `attr->next`): from the first node of an attribute list, the attributes of the first node
    /// otherwise; none for `SXE_ITER_CHILD` (`attr` stays `NULL`). With `test`, only those named
    /// `iter.name` count.
    fn dim_attributes(&self, node: Option<&NodeId>) -> (Vec<(usize, String, String)>, bool) {
        let all = |path: &[usize], from: usize| -> Vec<(usize, String, String)> {
            self.doc.element_at(path).attributes[from..]
                .iter()
                .enumerate()
                .map(|(i, (n, v))| (from + i, n.clone(), v.clone()))
                .collect()
        };
        match (self.iter_type, node) {
            (IterType::AttrList, Some(NodeId::Attribute(element, index))) => {
                (all(element, *index), self.iter_name.is_some())
            }
            (IterType::AttrList | IterType::Child, _) => (Vec::new(), false),
            (_, Some(NodeId::Element(path))) => (all(path, 0), false),
            (_, _) => (Vec::new(), false),
        }
    }

    /// Port of `sxe_prop_dim_read()` (reads only: `BP_VAR_R` and `BP_VAR_IS`; no namespaces).
    fn prop_dim_read(
        &self,
        offset: &Value,
        mut access: Access,
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let member = SimpleXmlElement::member(offset, diag)?;
        if matches!(member, Member::Long(_)) && self.iter_type != IterType::AttrList {
            access = Access::Elements;
        }
        let node = match self.iter_type {
            IterType::AttrList => {
                access = Access::Attributes;
                self.first_node()
            }
            IterType::Child => Some(self.node.clone()),
            _ => self.first_node(),
        };
        let (attributes, test) = self.dim_attributes(node.as_ref());
        let Some(node) = node else {
            return Ok(Value::Null);
        };
        let named = |name: &str| !test || self.iter_name.as_deref() == Some(name);
        match access {
            Access::Attributes => {
                let owner = match &node {
                    NodeId::Attribute(element, _) | NodeId::Element(element) => element.clone(),
                };
                let found = match &member {
                    // a long offset of an attribute list: the n-th listed attribute
                    Member::Long(n) => attributes
                        .iter()
                        .filter(|(_, name, _)| named(name))
                        .nth(usize::try_from(*n).unwrap_or(usize::MAX))
                        .map(|(i, _, _)| *i),
                    Member::Name(wanted) => attributes
                        .iter()
                        .find(|(_, name, _)| named(name) && name == wanted)
                        .map(|(i, _, _)| *i),
                };
                Ok(match found {
                    Some(index) => {
                        self.node_as_zval(NodeId::Attribute(owner, index), IterType::None, None)
                    }
                    None => Value::Null,
                })
            }
            Access::Elements => match member {
                Member::Long(n) => {
                    let start = if self.iter_type == IterType::Child {
                        self.first_node()
                    } else {
                        Some(node)
                    };
                    let found = match self.iter_type {
                        IterType::None => start.filter(|_| n == 0),
                        _ => start.and_then(|start| self.element_by_offset(n, start)),
                    };
                    Ok(match found {
                        Some(node) => self.node_as_zval(node, IterType::None, None),
                        None => Value::Null,
                    })
                }
                Member::Name(name) => {
                    // in BP_VAR_IS mode only return a proper node if it actually exists
                    if mode == FetchMode::Isset && self.find_element_by_name(&node, &name).is_none()
                    {
                        return Ok(Value::Null);
                    }
                    Ok(self.node_as_zval(node, IterType::Element, Some(&name)))
                }
            },
        }
    }

    /// Port of `sxe_get_element_by_offset()` from `node` on (`SXE_ITER_NONE` is handled by the
    /// callers): the element at `offset` among the nodes the object stands for, counting from
    /// `node` and walking its next siblings.
    fn element_by_offset(&self, offset: i64, node: NodeId) -> Option<NodeId> {
        let NodeId::Element(path) = node else {
            return None;
        };
        let (last, parent) = path.split_last()?;
        let mut count = 0;
        for (i, child) in self
            .doc
            .element_at(parent)
            .children
            .iter()
            .enumerate()
            .skip(*last)
        {
            let XmlNode::Element(e) = child else {
                continue;
            };
            let counted = match self.iter_type {
                IterType::Child => true,
                IterType::Element => self.iter_name.as_deref() == Some(e.name.as_str()),
                _ => false,
            };
            if counted {
                if count == offset {
                    let mut found = parent.to_vec();
                    found.push(i);
                    return Some(NodeId::Element(found));
                }
                count += 1;
            }
        }
        None
    }

    /// Port of `sxe_find_element_by_name()` over the children of `node`.
    fn find_element_by_name(&self, node: &NodeId, name: &str) -> Option<NodeId> {
        let NodeId::Element(path) = node else {
            return None;
        };
        self.doc
            .element_at(path)
            .children
            .iter()
            .position(|child| matches!(child, XmlNode::Element(e) if e.name == name))
            .map(|i| {
                let mut found = path.clone();
                found.push(i);
                NodeId::Element(found)
            })
    }

    /// Port of `sxe_prop_dim_exists()` (`check_empty`: 0 for `isset`, 1 for `empty`, 2 for
    /// `property_exists()`).
    fn prop_dim_exists(
        &self,
        offset: &Value,
        check_empty: u8,
        mut access: Access,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        let member = SimpleXmlElement::member(offset, diag)?;
        let mut node = Some(self.node.clone());
        if matches!(member, Member::Long(_)) && self.iter_type != IterType::AttrList {
            access = Access::Elements;
            if self.iter_type == IterType::Child {
                node = self.first_node();
            }
        }
        match self.iter_type {
            IterType::AttrList => {
                access = Access::Attributes;
                node = self.first_node();
            }
            IterType::Child => {}
            _ => node = self.first_node(),
        }
        let (attributes, test) = self.dim_attributes(node.as_ref());
        let Some(node) = node else {
            return Ok(false);
        };
        let named = |name: &str| !test || self.iter_name.as_deref() == Some(name);
        // empty(): an attribute or element without content, or with the content "0"
        let empty_text = |text: &str| text.is_empty() || text == "0";
        match access {
            Access::Attributes => {
                let found = match &member {
                    Member::Long(n) => attributes
                        .iter()
                        .filter(|(_, name, _)| named(name))
                        .nth(usize::try_from(*n).unwrap_or(usize::MAX)),
                    Member::Name(wanted) => attributes
                        .iter()
                        .find(|(_, name, _)| named(name) && name == wanted),
                };
                Ok(match found {
                    None => false,
                    Some((_, _, value)) => !(check_empty == 1 && empty_text(value)),
                })
            }
            Access::Elements => {
                let found = match member {
                    Member::Long(n) => {
                        let start = if self.iter_type == IterType::Child {
                            self.first_node()
                        } else {
                            Some(node)
                        };
                        match self.iter_type {
                            IterType::None => start.filter(|_| n == 0),
                            _ => start.and_then(|start| self.element_by_offset(n, start)),
                        }
                    }
                    Member::Name(name) => self.find_element_by_name(&node, &name),
                };
                let Some(found) = found else {
                    return Ok(false);
                };
                if check_empty == 1 {
                    let empty = match &found {
                        NodeId::Element(path) => {
                            match self.doc.element_at(path).children.as_slice() {
                                [] => true,
                                [XmlNode::Text(t)] => empty_text(t),
                                _ => false,
                            }
                        }
                        // `node->children` of an attribute is its text
                        NodeId::Attribute(..) => empty_text(&self.doc.node_text(&found)),
                    };
                    return Ok(!empty);
                }
                Ok(true)
            }
        }
    }

    /// `asXML()`: the whole document for the root element, the first node alone otherwise (an
    /// attribute as ` name="value"`); `None` (PHP's `false`) without a first node.
    pub fn as_xml(&self) -> Option<String> {
        let node = self.first_node()?;
        Some(match &node {
            NodeId::Element(path) if path.is_empty() => self.doc.dump(),
            NodeId::Element(path) => {
                let mut out = String::new();
                dump_element(self.doc.element_at(path), &mut out);
                out
            }
            NodeId::Attribute(element, index) => {
                let (name, value) = &self.doc.element_at(element).attributes[*index];
                format!(" {name}=\"{value}\"")
            }
        })
    }

    /// `children()` and `attributes()`: `?string $namespaceOrPrefix = null, bool $isPrefix = false`.
    fn namespace_args(
        &self,
        method: &str,
        args: &CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        let function = format!("SimpleXMLElement::{method}");
        let values = args.bind(
            &function,
            &[
                Param::optional("namespaceOrPrefix", Value::Null),
                Param::optional("isPrefix", false),
            ],
            FunctionKind::Internal,
        )?;
        let prefix = param::parse_str_or_null(
            &values[0],
            &Arg::new(&function, 1, "namespaceOrPrefix").declared("?string"),
            diag,
        )?;
        param::parse_bool(&values[1], &Arg::new(&function, 2, "isPrefix"), diag)?;
        if prefix.is_some_and(|p| !p.is_empty()) {
            return Err(self.unsupported("namespace filtering"));
        }
        Ok(())
    }

    fn unsupported(&self, what: &str) -> PhpError {
        PhpError::error(format!(
            "SimpleXMLElement {what} is not supported by twig-test-support"
        ))
    }
}

/// The member of a property or dimension access.
enum Member {
    Long(i64),
    Name(String),
}

/// `sxe_access_mode`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    /// `SXE_ACCESS_ELEMENTS` (properties).
    Elements,
    /// `SXE_ACCESS_ATTRIBS` (dimensions).
    Attributes,
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
        // children(?string $namespaceOrPrefix = null, bool $isPrefix = false), attributes(...)
        "children" | "attributes" => vec![
            Param::optional("namespaceOrPrefix", Value::Null),
            Param::optional("isPrefix", false),
        ],
        "count" | "rewind" | "valid" | "current" | "key" | "next" | "getname" | "__tostring"
        | "__debuginfo" | "haschildren" | "getchildren" => Vec::new(),
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
        diag: &mut dyn Diagnostics,
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
                let (node, _) = iter.as_ref().ok_or_else(not_initialized)?;
                Ok(Value::from(self.doc.node_name(node)))
            }
            "next" => {
                none("next")?;
                self.move_forward();
                Ok(Value::Null)
            }
            // hasChildren(): whether the current element has element children
            "haschildren" => {
                none("hasChildren")?;
                if self.iter_type == IterType::AttrList {
                    return Ok(Value::Bool(false));
                }
                Ok(Value::Bool(match self.iter().as_ref() {
                    Some((NodeId::Element(path), _)) => self
                        .doc
                        .element_at(path)
                        .children
                        .iter()
                        .any(|child| matches!(child, XmlNode::Element(_))),
                    _ => false,
                }))
            }
            "getchildren" => {
                none("getChildren")?;
                if self.iter_type == IterType::AttrList {
                    return Ok(Value::Null);
                }
                Ok(self
                    .iter()
                    .as_ref()
                    .map_or(Value::Null, |(_, o)| Value::Object(o.clone())))
            }
            "getname" => none("getName").map(|_| {
                Value::from(
                    self.first_node()
                        .map(|node| self.doc.node_name(&node).to_owned())
                        .unwrap_or_default(),
                )
            }),
            "__tostring" => none("__toString").map(|_| Value::from(self.contents())),
            "asxml" | "savexml" => {
                if !args.is_empty() {
                    return Err(self.unsupported("asXML() with a file name"));
                }
                Ok(self.as_xml().map_or(Value::Bool(false), Value::from))
            }
            "children" => {
                self.namespace_args("children", &args, diag)?;
                if self.iter_type == IterType::AttrList {
                    // attributes don't have attributes
                    return Ok(Value::Null);
                }
                Ok(match self.first_node() {
                    Some(node) => self.node_as_zval(node, IterType::Child, None),
                    None => Value::Null,
                })
            }
            "attributes" => {
                self.namespace_args("attributes", &args, diag)?;
                let Some(node) = self.first_node() else {
                    return Ok(Value::Null);
                };
                if self.iter_type == IterType::AttrList {
                    return Ok(Value::Null);
                }
                Ok(self.node_as_zval(node, IterType::AttrList, None))
            }
            "__debuginfo" => none("__debugInfo").map(|_| Value::Array(self.prop_hash(true))),
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

    /// `sxe_property_read()`: `sxe_prop_dim_read()` of the elements.
    fn read_property(
        &self,
        _this: &ObjectRef,
        name: &[u8],
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        self.prop_dim_read(
            &Value::String(PhpString::from(name)),
            Access::Elements,
            mode,
            diag,
        )
    }

    /// `sxe_property_exists()`: `sxe_prop_dim_exists()` of the elements.
    fn has_property(
        &self,
        _this: &ObjectRef,
        name: &[u8],
        check: PropertyCheck,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        let check_empty = match check {
            PropertyCheck::Isset => 0,
            PropertyCheck::NotEmpty => 1,
            PropertyCheck::Exists => 2,
        };
        self.prop_dim_exists(
            &Value::String(PhpString::from(name)),
            check_empty,
            Access::Elements,
            diag,
        )
    }

    /// `sxe_dimension_read()`: `sxe_prop_dim_read()` of the attributes.
    fn read_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        self.prop_dim_read(offset, Access::Attributes, mode, diag)
    }

    /// `sxe_dimension_exists()`: `sxe_prop_dim_exists()` of the attributes.
    fn has_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        check_empty: bool,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        self.prop_dim_exists(offset, u8::from(check_empty), Access::Attributes, diag)
    }

    /// `sxe_get_prop_hash()`: `__debugInfo()` for `var_dump()` (`is_debug`), `get_properties`
    /// otherwise.
    fn properties_for(
        &self,
        _this: &ObjectRef,
        purpose: PropertyPurpose,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        Ok(self.prop_entries(purpose == PropertyPurpose::Debug))
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

    /// Port of `sxe_objects_compare()`: two objects over the same node are equal, any other pair
    /// of `SimpleXMLElement`s is uncomparable; other operands compare as objects do
    /// (`ZEND_COMPARE_OBJECTS_FALLBACK`).
    fn compare(
        &self,
        _this: &ObjectRef,
        other: &Value,
        _this_is_left: bool,
        _compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        let Value::Object(other) = other else {
            return None;
        };
        let other = other.downcast_ref::<SimpleXmlElement>()?;
        let same = Arc::ptr_eq(&self.doc, &other.doc) && self.node == other.node;
        Some(Ok(if same { 0 } else { UNCOMPARABLE }))
    }

    /// `sxe_object_cast_ex()` + `cast_object()`: bool is whether there is a first node to iterate
    /// (never for `SXE_ITER_NONE`, which passes no node) or `!sxe_prop_is_empty()`; the string
    /// contents for the other types, `convert_to_long()` / `convert_to_double()` /
    /// `convert_scalar_to_number()` of them for numbers.
    fn cast(
        &self,
        _this: &ObjectRef,
        target: CastTarget,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Value>> {
        if target == CastTarget::Bool {
            let node = if self.iter_type == IterType::None {
                None
            } else {
                self.reset_no_clear()
            };
            return Ok(Some(Value::Bool(node.is_some() || !self.prop_is_empty())));
        }
        let contents = Value::from(self.contents());
        Ok(Some(match target {
            CastTarget::Bool | CastTarget::String => contents,
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

    /// `php_sxe_iterator_current_key()`: the name of the current node.
    fn key(&mut self, _diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        let sxe = self.sxe()?;
        let iter = sxe.iter();
        Ok(match iter.as_ref() {
            Some((node, _)) => Value::from(sxe.doc.node_name(node)),
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
