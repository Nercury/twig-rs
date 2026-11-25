//! `var_dump()`, `var_export()` and `print_r()` with PHP's exact output formats.
//!
//! reference/php/engine-semantics.md §10. Ports of `php_var_dump()` and `php_var_export_ex()`
//! (PHPSRC/ext/standard/var.c) and `zend_print_zval_r_to_buf()` (PHPSRC/Zend/zend.c). Twig's
//! `dump()` function is `var_dump()` captured into a string.
//!
//! * Floats: `var_dump` prints `%.*H` with `serialize_precision` (-1, shortest round-trip:
//!   `float(0.1)`, `float(1.0E+25)`), `var_export` adds `.0` to integral values (`1.0`), `print_r`
//!   uses `precision` (14) like a string conversion.
//! * Objects: properties come from [`ObjectRef::properties_for`] (`PropertyPurpose::Debug` for
//!   `var_dump`/`print_r`, `PropertyPurpose::VarExport` for `var_export`) with mangled names
//!   (`["p":protected]`, `["p":"Class":private]`); uninitialised typed properties print as
//!   `uninitialized(type)` in `var_dump` and are skipped elsewhere; enum cases print as
//!   `enum(Suit::Hearts)` / `\Suit::Hearts` / `Suit Enum:string ( ... )`.
//! * Recursion through objects prints `*RECURSION*` (`var_dump`, `print_r`) or `NULL` with the
//!   warning `var_export does not handle circular references`. Arrays cannot be recursive in this
//!   value model (no PHP references).
//!
//! **Deviations** (not reproducible): the object handle `#N` of `var_dump` is [`ObjectRef::id`], a
//! process-wide counter, not PHP's per-request handle number; Rust-backed closures have no
//! `name`/`file`/`line` debug properties, so they dump as `object(Closure)#N (0) {}`.
//!
//! Diagnostics: `Illegal member variable name` / `Corrupt member variable name` notices for
//! malformed mangled property names (`zend_unmangle_property_name_ex()`), the `var_export`
//! recursion warning, and whatever the objects' property handlers report.

use crate::array::Array;
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::float;
use crate::key::ArrayKey;
use crate::object::{ClassKind, EnumBacking, ObjectRef, PropertyEntry, PropertyPurpose};
use crate::string::PhpString;
use crate::value::Value;

/// `var_dump($value)`: the dump text, ending with a newline.
pub fn var_dump(value: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let mut out = Vec::new();
    var_dump_into(&mut out, value, diag)?;
    Ok(PhpString::from(out))
}

/// `var_dump($value)` appended to `out`. On an exception the output written so far stays in
/// `out` (PHP prints it before the exception propagates).
pub fn var_dump_into(
    out: &mut Vec<u8>,
    value: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    Dumper::new(out, diag).var_dump(value, 1)
}

/// `var_export($value, true)`: PHP code that evaluates to the value.
pub fn var_export(value: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let mut out = Vec::new();
    var_export_into(&mut out, value, diag)?;
    Ok(PhpString::from(out))
}

/// `var_export($value, true)` appended to `out`.
pub fn var_export_into(
    out: &mut Vec<u8>,
    value: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    Dumper::new(out, diag).var_export(value, 1)
}

/// `print_r($value, true)`.
pub fn print_r(value: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let mut out = Vec::new();
    print_r_into(&mut out, value, diag)?;
    Ok(PhpString::from(out))
}

/// `print_r($value, true)` appended to `out`.
pub fn print_r_into(out: &mut Vec<u8>, value: &Value, diag: &mut dyn Diagnostics) -> PhpResult<()> {
    Dumper::new(out, diag).print_r(value, 0)
}

/// A property name split by [`unmangle_property_name_ex`].
struct Unmangled<'a> {
    /// The class part: `*` for protected, the declaring class for private; `None` for a plain name.
    /// This is the C string PHP prints with `%s` (up to the first NUL).
    class: Option<&'a [u8]>,
    /// The property name with its full length (`prop_len`).
    name: &'a [u8],
    /// `SUCCESS` (false for the "Illegal"/"Corrupt member variable name" cases).
    success: bool,
}

/// `zend_strnlen()`.
fn strnlen(s: &[u8], max: usize) -> usize {
    let s = &s[..max.min(s.len())];
    memchr::memchr(0, s).unwrap_or(s.len())
}

/// Port of `zend_unmangle_property_name_ex()` (PHPSRC/Zend/zend_compile.c): splits
/// `"\0Class\0name"` / `"\0*\0name"` (anonymous class names contain a NUL of their own) and reports
/// malformed names with an `E_NOTICE`.
fn unmangle_property_name_ex<'a>(
    name: &'a [u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Unmangled<'a>> {
    if name.is_empty() || name[0] != 0 {
        return Ok(Unmangled {
            class: None,
            name,
            success: true,
        });
    }
    if name.len() < 3 || name[1] == 0 {
        diag.notice("Illegal member variable name")?;
        return Ok(Unmangled {
            class: None,
            name,
            success: false,
        });
    }
    let mut class_name_len = strnlen(&name[1..], name.len() - 2);
    if class_name_len >= name.len() - 2 || name[class_name_len + 1] != 0 {
        diag.notice("Corrupt member variable name")?;
        return Ok(Unmangled {
            class: None,
            name,
            success: false,
        });
    }
    let class = &name[1..1 + class_name_len];
    let anonclass_src_len = strnlen(&name[class_name_len + 2..], name.len() - class_name_len - 2);
    if class_name_len + anonclass_src_len + 2 != name.len() {
        class_name_len += anonclass_src_len + 1;
    }
    Ok(Unmangled {
        class: Some(class),
        name: &name[class_name_len + 2..],
        success: true,
    })
}

/// The bytes up to the first NUL (what C `%s` prints).
fn c_str(s: &[u8]) -> &[u8] {
    match memchr::memchr(0, s) {
        Some(pos) => &s[..pos],
        None => s,
    }
}

/// `php_addcslashes(str, "'\\")`: backslash before `'` and `\`.
fn addcslashes_quote(s: &[u8], out: &mut Vec<u8>) {
    for &c in s {
        if c == b'\'' || c == b'\\' {
            out.push(b'\\');
        }
        out.push(c);
    }
}

/// The string literal body of `var_export`: [`addcslashes_quote`], then every NUL byte replaced by
/// `' . "\0" . '` (`php_str_to_str`).
fn export_string_body(s: &[u8], out: &mut Vec<u8>) {
    let mut escaped = Vec::with_capacity(s.len() + 2);
    addcslashes_quote(s, &mut escaped);
    for &c in &escaped {
        if c == 0 {
            out.extend_from_slice(b"' . \"\\0\" . '");
        } else {
            out.push(c);
        }
    }
}

/// What a recursion guard protects.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Guard {
    /// `ZEND_GUARD_OR_GC_PROTECT_RECURSION(guard, DEBUG, zobj)` (var_dump, print_r).
    Debug(u64),
    /// `ZEND_GUARD_OR_GC_PROTECT_RECURSION(guard, EXPORT, zobj)` (var_export).
    Export(u64),
}

struct Dumper<'o, 'd> {
    out: &'o mut Vec<u8>,
    diag: &'d mut dyn Diagnostics,
    guards: Vec<Guard>,
}

impl<'o, 'd> Dumper<'o, 'd> {
    fn new(out: &'o mut Vec<u8>, diag: &'d mut dyn Diagnostics) -> Self {
        Dumper {
            out,
            diag,
            guards: Vec::new(),
        }
    }

    fn put(&mut self, s: &[u8]) {
        self.out.extend_from_slice(s);
    }

    /// `%*c` with `' '`: `n` characters wide, i.e. `n` spaces (at least one).
    fn spaces(&mut self, n: usize) {
        self.out.extend(std::iter::repeat_n(b' ', n.max(1)));
    }

    fn unprotect(&mut self, g: Guard) {
        if let Some(pos) = self.guards.iter().rposition(|x| *x == g) {
            self.guards.remove(pos);
        }
    }

    // ---------------------------------------------------------------- var_dump

    /// Port of `php_var_dump()`.
    fn var_dump(&mut self, v: &Value, level: usize) -> PhpResult<()> {
        if level > 1 {
            self.spaces(level - 1);
        }
        match v {
            Value::Bool(false) => self.put(b"bool(false)\n"),
            Value::Bool(true) => self.put(b"bool(true)\n"),
            Value::Null => self.put(b"NULL\n"),
            Value::Int(i) => {
                let s = format!("int({i})\n");
                self.put(s.as_bytes());
            }
            Value::Float(d) => {
                let s = format!("float({})\n", float::format_serialize(*d));
                self.put(s.as_bytes());
            }
            Value::String(s) => {
                let head = format!("string({}) \"", s.len());
                self.put(head.as_bytes());
                self.put(s);
                self.put(b"\"\n");
            }
            Value::Array(a) => self.dump_array(a, level)?,
            Value::Object(o) => self.dump_object(o, level)?,
        }
        Ok(())
    }

    fn dump_array(&mut self, a: &Array, level: usize) -> PhpResult<()> {
        let head = format!("array({}) {{\n", a.len());
        self.put(head.as_bytes());
        for (key, val) in a.iter() {
            // php_array_element_dump()
            self.spaces(level + 1);
            match key {
                ArrayKey::Int(i) => {
                    let s = format!("[{i}]=>\n");
                    self.put(s.as_bytes());
                }
                ArrayKey::Str(s) => {
                    self.put(b"[\"");
                    self.put(s);
                    self.put(b"\"]=>\n");
                }
            }
            self.var_dump(val, level + 2)?;
        }
        if level > 1 {
            self.spaces(level - 1);
        }
        self.put(b"}\n");
        Ok(())
    }

    fn dump_object(&mut self, o: &ObjectRef, level: usize) -> PhpResult<()> {
        if o.class().is_enum() {
            let s = format!(
                "enum({}::{})\n",
                o.class().display_name(),
                o.enum_case_name().unwrap_or_default()
            );
            self.put(s.as_bytes());
            return Ok(());
        }
        let guard = Guard::Debug(o.id());
        if self.guards.contains(&guard) {
            self.put(b"*RECURSION*\n");
            return Ok(());
        }
        self.guards.push(guard);
        let result = self.dump_object_body(o, level);
        self.unprotect(guard);
        result
    }

    fn dump_object_body(&mut self, o: &ObjectRef, level: usize) -> PhpResult<()> {
        let props = o.properties_for(PropertyPurpose::Debug, self.diag)?;
        let count = props.iter().filter(|e| e.value.is_some()).count();
        let head = format!(
            "object({})#{} ({}) {{\n",
            o.class().display_name(),
            o.id(),
            count
        );
        self.put(head.as_bytes());
        for e in &props {
            self.property_dump(e, level)?;
        }
        if level > 1 {
            self.spaces(level - 1);
        }
        self.put(b"}\n");
        Ok(())
    }

    /// Port of `php_object_property_dump()`.
    fn property_dump(&mut self, e: &PropertyEntry, level: usize) -> PhpResult<()> {
        self.spaces(level + 1);
        match &e.key {
            ArrayKey::Int(i) => {
                let s = format!("[{i}]=>\n");
                self.put(s.as_bytes());
            }
            ArrayKey::Str(key) => {
                let u = unmangle_property_name_ex(key, self.diag)?;
                self.put(b"[");
                match u.class {
                    Some(class) if u.success => {
                        self.put(b"\"");
                        self.put(c_str(u.name));
                        if class.first() == Some(&b'*') {
                            self.put(b"\":protected");
                        } else {
                            self.put(b"\":\"");
                            self.put(class);
                            self.put(b"\":private");
                        }
                    }
                    _ => {
                        self.put(b"\"");
                        self.put(key);
                        self.put(b"\"");
                    }
                }
                self.put(b"]=>\n");
            }
        }
        match &e.value {
            Some(v) => self.var_dump(v, level + 2),
            None => {
                self.spaces(level + 1);
                let s = format!(
                    "uninitialized({})\n",
                    e.type_name.as_deref().unwrap_or("mixed")
                );
                self.put(s.as_bytes());
                Ok(())
            }
        }
    }

    // ---------------------------------------------------------------- var_export

    /// Port of `php_var_export_ex()`.
    fn var_export(&mut self, v: &Value, level: usize) -> PhpResult<()> {
        match v {
            Value::Bool(false) => self.put(b"false"),
            Value::Bool(true) => self.put(b"true"),
            Value::Null => self.put(b"NULL"),
            Value::Int(i) => {
                // INT_MIN as a literal will be parsed as a float. Emit something like
                // -9223372036854775807-1 to avoid this.
                if *i == i64::MIN {
                    self.put(b"-9223372036854775807-1");
                } else {
                    let s = i.to_string();
                    self.put(s.as_bytes());
                }
            }
            Value::Float(d) => {
                let s = float::format_var_export(*d);
                self.put(s.as_bytes());
            }
            Value::String(s) => {
                self.put(b"'");
                export_string_body(s, self.out);
                self.put(b"'");
            }
            Value::Array(a) => {
                if level > 1 {
                    self.put(b"\n");
                    self.spaces(level - 1);
                }
                self.put(b"array (\n");
                for (key, val) in a.iter() {
                    // php_array_element_export()
                    self.spaces(level + 1);
                    match key {
                        ArrayKey::Int(i) => {
                            let s = format!("{i} => ");
                            self.put(s.as_bytes());
                        }
                        ArrayKey::Str(k) => {
                            self.put(b"'");
                            export_string_body(k, self.out);
                            self.put(b"' => ");
                        }
                    }
                    self.var_export(val, level + 2)?;
                    self.put(b",\n");
                }
                if level > 1 {
                    self.spaces(level - 1);
                }
                self.put(b")");
            }
            Value::Object(o) => self.export_object(o, level)?,
        }
        Ok(())
    }

    fn export_object(&mut self, o: &ObjectRef, level: usize) -> PhpResult<()> {
        let guard = Guard::Export(o.id());
        if self.guards.contains(&guard) {
            self.put(b"NULL");
            self.diag
                .warn("var_export does not handle circular references")?;
            return Ok(());
        }
        self.guards.push(guard);
        let result = self.export_object_body(o, level);
        self.unprotect(guard);
        result
    }

    fn export_object_body(&mut self, o: &ObjectRef, level: usize) -> PhpResult<()> {
        let props = o.properties_for(PropertyPurpose::VarExport, self.diag)?;
        if level > 1 {
            self.put(b"\n");
            self.spaces(level - 1);
        }
        let class = o.class();
        let is_enum = class.is_enum();
        // stdClass has no __set_state method, but can be casted to
        let is_std = class.name().eq_ignore_ascii_case("stdClass");
        if is_std {
            self.put(b"(object) array(\n");
        } else {
            self.put(b"\\");
            self.put(class.name().as_bytes());
            if is_enum {
                self.put(b"::");
                self.put(o.enum_case_name().unwrap_or_default().as_bytes());
            } else {
                self.put(b"::__set_state(array(\n");
            }
        }
        if !is_enum {
            for e in &props {
                let Some(val) = &e.value else { continue };
                // php_object_element_export()
                self.spaces(level + 2);
                match &e.key {
                    ArrayKey::Str(key) => {
                        let u = unmangle_property_name_ex(key, self.diag)?;
                        self.put(b"'");
                        addcslashes_quote(u.name, self.out);
                        self.put(b"'");
                    }
                    ArrayKey::Int(i) => {
                        let s = i.to_string();
                        self.put(s.as_bytes());
                    }
                }
                self.put(b" => ");
                self.var_export(val, level + 2)?;
                self.put(b",\n");
            }
        }
        if level > 1 && !is_enum {
            self.spaces(level - 1);
        }
        if is_std {
            self.put(b")");
        } else if !is_enum {
            self.put(b"))");
        }
        Ok(())
    }

    // ---------------------------------------------------------------- print_r

    /// Port of `zend_print_zval_r_to_buf()`.
    fn print_r(&mut self, v: &Value, indent: usize) -> PhpResult<()> {
        match v {
            Value::Array(a) => {
                self.put(b"Array\n");
                let entries: Vec<(ArrayKey, Value)> =
                    a.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                self.print_hash(&entries, indent, false)?;
            }
            Value::Object(o) => {
                let class = o.class();
                self.put(class.display_name().as_bytes());
                match class.kind() {
                    ClassKind::Enum(backing) => {
                        self.put(b" Enum");
                        if let Some(b) = backing {
                            self.put(b":");
                            self.put(match b {
                                EnumBacking::Int => b"int".as_slice(),
                                EnumBacking::String => b"string".as_slice(),
                            });
                        }
                        self.put(b"\n");
                    }
                    _ => self.put(b" Object\n"),
                }
                let guard = Guard::Debug(o.id());
                if self.guards.contains(&guard) {
                    self.put(b" *RECURSION*");
                    return Ok(());
                }
                let props = o.properties_for(PropertyPurpose::Debug, self.diag)?;
                let entries: Vec<(ArrayKey, Value)> = props
                    .into_iter()
                    .filter_map(|e| e.value.map(|v| (e.key, v)))
                    .collect();
                self.guards.push(guard);
                let result = self.print_hash(&entries, indent, true);
                self.unprotect(guard);
                result?;
            }
            Value::Int(i) => {
                let s = i.to_string();
                self.put(s.as_bytes());
            }
            // smart_str_append_double(buf, d, EG(precision), false)
            Value::Float(d) => {
                let s = float::double_to_string(*d);
                self.put(s.as_bytes());
            }
            Value::String(s) => self.put(s),
            Value::Bool(true) => self.put(b"1"),
            Value::Bool(false) | Value::Null => {}
        }
        Ok(())
    }

    /// Port of `print_hash()`.
    fn print_hash(
        &mut self,
        entries: &[(ArrayKey, Value)],
        indent: usize,
        is_object: bool,
    ) -> PhpResult<()> {
        self.out.extend(std::iter::repeat_n(b' ', indent));
        self.put(b"(\n");
        let indent = indent + 4;
        for (key, val) in entries {
            self.out.extend(std::iter::repeat_n(b' ', indent));
            self.put(b"[");
            match key {
                ArrayKey::Str(k) => {
                    if is_object {
                        let u = unmangle_property_name_ex(k, self.diag)?;
                        self.put(u.name);
                        if let (Some(class), true) = (u.class, u.success) {
                            if class.first() == Some(&b'*') {
                                self.put(b":protected");
                            } else {
                                self.put(b":");
                                self.put(class);
                                self.put(b":private");
                            }
                        }
                    } else {
                        self.put(k);
                    }
                }
                ArrayKey::Int(i) => {
                    let s = i.to_string();
                    self.put(s.as_bytes());
                }
            }
            self.put(b"] => ");
            self.print_r(val, indent + 4)?;
            self.put(b"\n");
        }
        let indent = indent - 4;
        self.out.extend(std::iter::repeat_n(b' ', indent));
        self.put(b")\n");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::StdClass;

    fn dump(v: &Value) -> String {
        var_dump(v, &mut Vec::new()).unwrap().to_string()
    }

    fn export(v: &Value) -> String {
        var_export(v, &mut Vec::new()).unwrap().to_string()
    }

    fn pr(v: &Value) -> String {
        print_r(v, &mut Vec::new()).unwrap().to_string()
    }

    #[test]
    fn scalars() {
        assert_eq!(dump(&Value::Float(1.0)), "float(1)\n");
        assert_eq!(dump(&Value::Float(-0.0)), "float(-0)\n");
        assert_eq!(dump(&Value::from("two")), "string(3) \"two\"\n");
        assert_eq!(export(&Value::Float(1.0)), "1.0");
        assert_eq!(export(&Value::Int(i64::MIN)), "-9223372036854775807-1");
        assert_eq!(export(&Value::from(&b"a\0b"[..])), "'a' . \"\\0\" . 'b'");
        assert_eq!(export(&Value::from("k'e\\y")), "'k\\'e\\\\y'");
        assert_eq!(pr(&Value::Float(0.1 + 0.2)), "0.3");
        assert_eq!(pr(&Value::Bool(true)), "1");
        assert_eq!(pr(&Value::Null), "");
    }

    #[test]
    fn nested() {
        let inner = Array::from_values([Value::Int(2)]);
        let a: Array = [
            ("a", Value::Int(1)),
            ("e", Value::Array(Array::new())),
            ("n", Value::Array(inner)),
        ]
        .into_iter()
        .collect();
        let v = Value::Array(a);
        assert_eq!(
            dump(&v),
            "array(3) {\n  [\"a\"]=>\n  int(1)\n  [\"e\"]=>\n  array(0) {\n  }\n  [\"n\"]=>\n  array(1) {\n    [0]=>\n    int(2)\n  }\n}\n"
        );
        assert_eq!(
            export(&v),
            "array (\n  'a' => 1,\n  'e' => \n  array (\n  ),\n  'n' => \n  array (\n    0 => 2,\n  ),\n)"
        );
        assert_eq!(
            pr(&v),
            "Array\n(\n    [a] => 1\n    [e] => Array\n        (\n        )\n\n    [n] => Array\n        (\n            [0] => 2\n        )\n\n)\n"
        );
    }

    #[test]
    fn objects() {
        let o = StdClass::with_properties([("a", Value::Int(1))]).into_ref();
        let v = Value::Object(o.clone());
        assert_eq!(
            dump(&v),
            format!(
                "object(stdClass)#{} (1) {{\n  [\"a\"]=>\n  int(1)\n}}\n",
                o.id()
            )
        );
        assert_eq!(export(&v), "(object) array(\n   'a' => 1,\n)");
        assert_eq!(pr(&v), "stdClass Object\n(\n    [a] => 1\n)\n");
        o.write_property(b"self", v.clone(), &mut Vec::new())
            .unwrap();
        assert_eq!(
            dump(&v),
            format!(
                "object(stdClass)#{0} (2) {{\n  [\"a\"]=>\n  int(1)\n  [\"self\"]=>\n  *RECURSION*\n}}\n",
                o.id()
            )
        );
        assert_eq!(
            pr(&v),
            "stdClass Object\n(\n    [a] => 1\n    [self] => stdClass Object\n *RECURSION*\n)\n"
        );
        let mut diag = Vec::new();
        assert_eq!(
            var_export(&v, &mut diag).unwrap(),
            "(object) array(\n   'a' => 1,\n   'self' => NULL,\n)"
        );
        assert_eq!(diag.len(), 1);
    }

    #[test]
    fn unmangling() {
        let mut d = Vec::new();
        let u = unmangle_property_name_ex(b"\0A\0p", &mut d).unwrap();
        assert_eq!(
            (u.class, u.name, u.success),
            (Some(&b"A"[..]), &b"p"[..], true)
        );
        let u = unmangle_property_name_ex(b"\0class@anonymous\0/x.php:3$0\0p", &mut d).unwrap();
        assert_eq!(u.class, Some(&b"class@anonymous"[..]));
        assert_eq!(u.name, b"p");
        let u = unmangle_property_name_ex(b"\0\0x", &mut d).unwrap();
        assert!(!u.success);
        let u = unmangle_property_name_ex(b"\0abc", &mut d).unwrap();
        assert!(!u.success);
        assert_eq!(d.len(), 2);
    }
}
