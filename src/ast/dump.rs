//! A textual dump of a node tree, in the format of the oracle generator `tests/vectors/parser/expressions.gen.php`.
//!
//! One node per line: `<class> @<line>[ #<tag>][ doc=<string>]` (the PHP class without the `Twig\Node\` prefix),
//! then the attributes as `.<name> = <value>` in Twig's attribute order, pseudo attributes for state that is not an
//! attribute in Twig (`.(defined_test) = true`, `.(call_parentheses) = <bool>`), then the children as
//! `[<key>] <child>` indented by two spaces; node-valued attributes (`embedded_templates`) come last as
//! `[.<name>] <node>`. Values: `NULL`, `true`, `false`, ints, `float(<var_export>)`, `"<string>"` with `\\`, `\"`
//! and `\xHH` escapes for bytes below 0x20 and 0x7F, `[k => v, ...]` arrays and the string form of objects.

use std::fmt::Write as _;

use super::expr::ExprKind;
use super::node::{Node, NodeKind};
use super::value::{AttrValue, php_var_export_float};
use super::visit::NodeRef;
use super::{Arguments, Expr};

/// Dumps a node tree (bytes: template strings may not be valid UTF-8).
pub fn dump_node_bytes(node: &Node) -> Vec<u8> {
    let mut out = Vec::new();
    write_ref(&mut out, NodeRef::Node(node), "", 0);
    out
}

/// Dumps an expression tree (bytes).
pub fn dump_expr_bytes(expr: &Expr) -> Vec<u8> {
    let mut out = Vec::new();
    write_ref(&mut out, NodeRef::Expr(expr), "", 0);
    out
}

/// Dumps a node tree (invalid UTF-8 replaced by U+FFFD).
pub fn dump_node(node: &Node) -> String {
    String::from_utf8_lossy(&dump_node_bytes(node)).into_owned()
}

/// Dumps an expression tree (invalid UTF-8 replaced by U+FFFD).
pub fn dump_expr(expr: &Expr) -> String {
    String::from_utf8_lossy(&dump_expr_bytes(expr)).into_owned()
}

fn short_class(class: &str) -> &str {
    class.strip_prefix("Twig\\Node\\").unwrap_or(class)
}

fn write_ref(out: &mut Vec<u8>, node: NodeRef<'_>, prefix: &str, depth: usize) {
    if !out.is_empty() {
        out.push(b'\n');
    }
    let pad = "  ".repeat(depth);
    out.extend_from_slice(pad.as_bytes());
    out.extend_from_slice(prefix.as_bytes());
    match node {
        NodeRef::Node(node) => write_node(out, node, &pad, depth),
        NodeRef::Expr(expr) => write_expr(out, expr, &pad, depth, None, None),
        NodeRef::Arguments(arguments) => write_arguments(out, arguments, depth),
    }
}

fn write_header(out: &mut Vec<u8>, class: &str, line: i64, tag: Option<&str>, doc: Option<&[u8]>) {
    out.extend_from_slice(short_class(class).as_bytes());
    let _ = write!(Sink(out), " @{line}");
    if let Some(tag) = tag {
        out.extend_from_slice(b" #");
        out.extend_from_slice(tag.as_bytes());
    }
    if let Some(doc) = doc {
        out.extend_from_slice(b" doc=");
        quote(out, doc);
    }
}

fn write_attributes<'a>(
    out: &mut Vec<u8>,
    pad: &str,
    attributes: impl IntoIterator<Item = (std::borrow::Cow<'a, str>, AttrValue)>,
) {
    for (name, value) in attributes {
        out.push(b'\n');
        out.extend_from_slice(pad.as_bytes());
        out.extend_from_slice(b"  .");
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(b" = ");
        write_value(out, &value);
    }
}

fn write_node(out: &mut Vec<u8>, node: &Node, pad: &str, depth: usize) {
    if let NodeKind::Expression(expr) = &node.kind {
        write_expr(
            out,
            expr,
            pad,
            depth,
            node.tag.as_deref(),
            node.documentation.as_deref(),
        );
        return;
    }
    write_header(
        out,
        &node.class_name(),
        node.line,
        node.tag.as_deref(),
        node.documentation.as_deref(),
    );
    write_attributes(out, pad, node.attributes());
    for (key, child) in node.children() {
        write_ref(out, child, &format!("[{key}] "), depth + 1);
    }
    if let NodeKind::Module(module) = &node.kind {
        write_ref(
            out,
            NodeRef::Node(&module.embedded_templates),
            "[.embedded_templates] ",
            depth + 1,
        );
    }
}

fn write_expr(
    out: &mut Vec<u8>,
    expr: &Expr,
    pad: &str,
    depth: usize,
    tag: Option<&str>,
    doc: Option<&[u8]>,
) {
    write_header(
        out,
        &expr.class_name(),
        expr.line,
        tag,
        doc.or(expr.documentation.as_deref()),
    );
    write_attributes(out, pad, expr.attributes());
    if expr.is_defined_test_enabled() {
        out.push(b'\n');
        out.extend_from_slice(pad.as_bytes());
        out.extend_from_slice(b"  .(defined_test) = true");
    }
    if let ExprKind::MacroReference(m) = &expr.kind {
        out.push(b'\n');
        out.extend_from_slice(pad.as_bytes());
        out.extend_from_slice(b"  .(call_parentheses) = ");
        out.extend_from_slice(if m.has_call_parentheses {
            b"true"
        } else {
            b"false"
        });
    }
    for (key, child) in expr.children() {
        write_ref(out, child, &format!("[{key}] "), depth + 1);
    }
}

fn write_arguments(out: &mut Vec<u8>, arguments: &Arguments, depth: usize) {
    write_header(out, arguments.class_name(), 0, None, None);
    for (key, child) in arguments.children() {
        write_ref(out, child, &format!("[{key}] "), depth + 1);
    }
}

fn write_value(out: &mut Vec<u8>, value: &AttrValue) {
    match value {
        AttrValue::Null => out.extend_from_slice(b"NULL"),
        AttrValue::Bool(true) => out.extend_from_slice(b"true"),
        AttrValue::Bool(false) => out.extend_from_slice(b"false"),
        AttrValue::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
        AttrValue::Float(f) => {
            out.extend_from_slice(b"float(");
            out.extend_from_slice(php_var_export_float(*f).as_bytes());
            out.push(b')');
        }
        AttrValue::String(s) => quote(out, s),
        AttrValue::Array(items) => {
            out.push(b'[');
            for (i, (k, v)) in items.iter().enumerate() {
                if i > 0 {
                    out.extend_from_slice(b", ");
                }
                write_value(out, k);
                out.extend_from_slice(b" => ");
                write_value(out, v);
            }
            out.push(b']');
        }
        AttrValue::Object(repr) => out.extend_from_slice(repr.as_bytes()),
    }
}

fn quote(out: &mut Vec<u8>, s: &[u8]) {
    out.push(b'"');
    for &b in s {
        match b {
            b'"' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            0..0x20 | 0x7F => {
                let _ = write!(Sink(out), "\\x{b:02x}");
            }
            _ => out.push(b),
        }
    }
    out.push(b'"');
}

/// `fmt::Write` into a byte buffer.
struct Sink<'a>(&'a mut Vec<u8>);

impl std::fmt::Write for Sink<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}
