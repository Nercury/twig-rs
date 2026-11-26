//! An exact JSON reader and writer for oracle batches, results and vector files.
//!
//! Numbers keep their source text (so `-0`, `1e400` and integers beyond 64 bits reach
//! [`json_decode()`](decode_php) exactly as PHP's `json_decode()` sees them) and objects keep their
//! key order and duplicate keys. Byte strings that are not valid UTF-8 travel next to a lossy text
//! as `<field>_b64` (lib.php's `put_string()`); [`base64_encode`] / [`base64_decode`] handle them.

use std::fmt::Write as _;

use twig::value::{Array, ArrayKey, PhpString, Value};

/// A parsed JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// A number, as written.
    Number(String),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object: entries in document order (duplicates kept).
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Parses a complete JSON document.
    ///
    /// # Errors
    ///
    /// A syntax error, with its byte offset.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            position: 0,
            depth: 0,
        };
        let value = parser.value()?;
        parser.whitespace();
        if parser.position != parser.bytes.len() {
            return Err(parser.error("trailing characters"));
        }
        Ok(value)
    }

    /// Parses a file.
    ///
    /// # Errors
    ///
    /// The file cannot be read or parsed.
    pub fn load(path: &std::path::Path) -> Result<Json, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Json::parse(text.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))
    }

    /// A string value.
    pub fn str(s: impl Into<String>) -> Json {
        Json::String(s.into())
    }

    /// An integer value.
    pub fn int(i: i64) -> Json {
        Json::Number(i.to_string())
    }

    /// The value of the first entry named `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value of `key`, treating `null` like a missing key (PHP's `??`).
    pub fn get_non_null(&self, key: &str) -> Option<&Json> {
        self.get(key).filter(|v| !v.is_null())
    }

    /// Sets `key` (replacing the first entry of that name, else appending).
    pub fn set(&mut self, key: &str, value: Json) {
        if let Json::Object(fields) = self {
            match fields.iter_mut().find(|(k, _)| k == key) {
                Some(entry) => entry.1 = value,
                None => fields.push((key.to_owned(), value)),
            }
        }
    }

    /// Removes every entry named `key`.
    pub fn remove(&mut self, key: &str) -> Option<Json> {
        if let Json::Object(fields) = self {
            let index = fields.iter().position(|(k, _)| k == key)?;
            let (_, value) = fields.remove(index);
            fields.retain(|(k, _)| k != key);
            return Some(value);
        }
        None
    }

    /// The text of a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    /// The value of a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// A number that is a 64-bit integer.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Number(n) => n.parse().ok(),
            _ => None,
        }
    }

    /// The items of an array (empty for anything else).
    pub fn items(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            _ => &[],
        }
    }

    /// The entries of an object (empty for anything else).
    pub fn entries(&self) -> &[(String, Json)] {
        match self {
            Json::Object(fields) => fields,
            _ => &[],
        }
    }

    /// Whether this is `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// PHP truthiness of the decoded value (`(bool) $case['x']`).
    pub fn truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(b) => *b,
            Json::Number(n) => n.parse::<f64>().is_ok_and(|d| d != 0.0),
            Json::String(s) => !s.is_empty() && s != "0",
            Json::Array(items) => !items.is_empty(),
            Json::Object(fields) => !fields.is_empty(),
        }
    }

    /// Writes compact JSON (one line).
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, None, 0);
        out
    }

    /// Writes indented JSON (4 spaces, like `JSON_PRETTY_PRINT`).
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, Some(4), 0);
        out
    }

    /// Writes JSON where objects and arrays nested deeper than `inline_depth` stay on one line.
    pub fn to_layout(&self, inline_depth: usize) -> String {
        let mut out = String::new();
        self.write_layout(&mut out, 0, inline_depth);
        out
    }

    fn write_layout(&self, out: &mut String, level: usize, inline_depth: usize) {
        if level >= inline_depth {
            self.write(out, None, 0);
            return;
        }
        match self {
            Json::Array(items) if !items.is_empty() => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, Some(2), level + 1);
                    item.write_layout(out, level + 1, inline_depth);
                }
                newline(out, Some(2), level);
                out.push(']');
            }
            Json::Object(fields) if !fields.is_empty() => {
                out.push('{');
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, Some(2), level + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write_layout(out, level + 1, inline_depth);
                }
                newline(out, Some(2), level);
                out.push('}');
            }
            _ => self.write(out, None, 0),
        }
    }

    fn write(&self, out: &mut String, indent: Option<usize>, level: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Number(n) => out.push_str(n),
            Json::String(s) => write_string(out, s),
            Json::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent, level + 1);
                    item.write(out, indent, level + 1);
                }
                newline(out, indent, level);
                out.push(']');
            }
            Json::Object(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent, level + 1);
                    write_string(out, key);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    value.write(out, indent, level + 1);
                }
                newline(out, indent, level);
                out.push('}');
            }
        }
    }

    /// The value as `serde_json` sees it (for the typed-tree models of `twig-test-support`).
    ///
    /// # Errors
    ///
    /// A number `serde_json` cannot represent (infinite).
    pub fn to_serde(&self) -> Result<serde_json::Value, String> {
        Ok(match self {
            Json::Null => serde_json::Value::Null,
            Json::Bool(b) => serde_json::Value::Bool(*b),
            Json::Number(n) => {
                let number = if let Ok(i) = n.parse::<i64>() {
                    serde_json::Number::from(i)
                } else if let Ok(u) = n.parse::<u64>() {
                    serde_json::Number::from(u)
                } else {
                    let d: f64 = n.parse().map_err(|_| format!("invalid number {n}"))?;
                    serde_json::Number::from_f64(d)
                        .ok_or_else(|| format!("number {n} is not finite"))?
                };
                serde_json::Value::Number(number)
            }
            Json::String(s) => serde_json::Value::String(s.clone()),
            Json::Array(items) => serde_json::Value::Array(
                items.iter().map(Json::to_serde).collect::<Result<_, _>>()?,
            ),
            Json::Object(fields) => {
                let mut map = serde_json::Map::new();
                for (key, value) in fields {
                    map.insert(key.clone(), value.to_serde()?);
                }
                serde_json::Value::Object(map)
            }
        })
    }
}

fn newline(out: &mut String, indent: Option<usize>, level: usize) {
    if let Some(width) = indent {
        out.push('\n');
        for _ in 0..width * level {
            out.push(' ');
        }
    }
}

/// Writes a JSON string literal (unescaped slashes and Unicode, like the oracle's flags).
pub fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
    depth: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> String {
        format!("{message} at byte {}", self.position)
    }

    fn whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.bytes.get(self.position) {
            self.position += 1;
        }
    }

    fn eat(&mut self, literal: &str) -> bool {
        if self.bytes[self.position..].starts_with(literal.as_bytes()) {
            self.position += literal.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.whitespace();
        match self.bytes.get(self.position) {
            Some(b'{') => self.nested(Parser::object),
            Some(b'[') => self.nested(Parser::array),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') if self.eat("true") => Ok(Json::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Json::Bool(false)),
            Some(b'n') if self.eat("null") => Ok(Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.error("unexpected character")),
        }
    }

    fn nested(&mut self, f: fn(&mut Self) -> Result<Json, String>) -> Result<Json, String> {
        self.depth += 1;
        if self.depth > 10_000 {
            return Err(self.error("nesting too deep"));
        }
        let value = f(self);
        self.depth -= 1;
        value
    }

    fn digits(&mut self) -> usize {
        let start = self.position;
        while let Some(b'0'..=b'9') = self.bytes.get(self.position) {
            self.position += 1;
        }
        self.position - start
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.position;
        self.eat("-");
        match self.bytes.get(self.position) {
            Some(b'0') => self.position += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.error("invalid number")),
        }
        if self.eat(".") && self.digits() == 0 {
            return Err(self.error("invalid number"));
        }
        if let Some(b'e' | b'E') = self.bytes.get(self.position) {
            self.position += 1;
            if let Some(b'+' | b'-') = self.bytes.get(self.position) {
                self.position += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("invalid number"));
            }
        }
        Ok(Json::Number(
            String::from_utf8_lossy(&self.bytes[start..self.position]).into_owned(),
        ))
    }

    fn object(&mut self) -> Result<Json, String> {
        self.position += 1;
        let mut fields = Vec::new();
        self.whitespace();
        if self.eat("}") {
            return Ok(Json::Object(fields));
        }
        loop {
            self.whitespace();
            let key = self.string()?;
            self.whitespace();
            if !self.eat(":") {
                return Err(self.error("expected ':'"));
            }
            fields.push((key, self.value()?));
            self.whitespace();
            if self.eat(",") {
                continue;
            }
            if self.eat("}") {
                return Ok(Json::Object(fields));
            }
            return Err(self.error("expected ',' or '}'"));
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.position += 1;
        let mut items = Vec::new();
        self.whitespace();
        if self.eat("]") {
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.whitespace();
            if self.eat(",") {
                continue;
            }
            if self.eat("]") {
                return Ok(Json::Array(items));
            }
            return Err(self.error("expected ',' or ']'"));
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.position..self.position + 4)
            .ok_or_else(|| self.error("short escape"))?;
        let text = std::str::from_utf8(digits).map_err(|_| self.error("bad escape"))?;
        let value = u32::from_str_radix(text, 16).map_err(|_| self.error("bad escape"))?;
        self.position += 4;
        Ok(value)
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat("\"") {
            return Err(self.error("expected a string"));
        }
        let mut out = String::new();
        loop {
            let start = self.position;
            while let Some(&b) = self.bytes.get(self.position) {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.position += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.position])
                    .map_err(|_| self.error("invalid UTF-8"))?,
            );
            match self.bytes.get(self.position) {
                Some(b'"') => {
                    self.position += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.position += 1;
                    let escape = *self
                        .bytes
                        .get(self.position)
                        .ok_or_else(|| self.error("short escape"))?;
                    self.position += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code) {
                                if !self.eat("\\u") {
                                    return Err(self.error("single unpaired UTF-16 surrogate"));
                                }
                                let low = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return Err(self.error("single unpaired UTF-16 surrogate"));
                                }
                                code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                            } else if (0xDC00..0xE000).contains(&code) {
                                return Err(self.error("single unpaired UTF-16 surrogate"));
                            }
                            out.push(
                                char::from_u32(code).ok_or_else(|| self.error("bad code point"))?,
                            );
                        }
                        _ => return Err(self.error("bad escape")),
                    }
                }
                Some(_) => return Err(self.error("control character in string")),
                None => return Err(self.error("unterminated string")),
            }
        }
    }
}

/// Standard base64 with padding (PHP's `base64_encode()`).
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes standard base64 (padding optional).
///
/// # Errors
///
/// A character outside the alphabet.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let value = |c: u8| -> Result<u32, String> {
        Ok(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(format!("invalid base64 character {:?}", char::from(c))),
        })
    };
    let digits: Vec<u8> = text
        .bytes()
        .filter(|&c| c != b'=' && !c.is_ascii_whitespace())
        .collect();
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        if chunk.len() == 1 {
            return Err("truncated base64".to_owned());
        }
        let mut buffer = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            buffer |= value(c)? << (18 - 6 * i);
        }
        let bytes = buffer.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Ok(out)
}

/// What lib.php's `put_string()` writes for a lossy text: `mb_convert_encoding($s, 'UTF-8',
/// 'UTF-8')`, which replaces every invalid byte sequence with `?`.
pub fn lossy_question_marks(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                out.push_str(valid);
                break;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                out.push('?');
                let skip = error.error_len().unwrap_or(after.len()).max(1);
                rest = &after[skip..];
            }
        }
    }
    out
}

/// Writes `bytes` into `fields` as lib.php's `put_string()` does: the text, plus `<key>_b64` when
/// the bytes are not valid UTF-8.
pub fn put_bytes(fields: &mut Vec<(String, Json)>, key: &str, bytes: &[u8]) {
    match std::str::from_utf8(bytes) {
        Ok(text) => fields.push((key.to_owned(), Json::str(text))),
        Err(_) => {
            fields.push((key.to_owned(), Json::str(lossy_question_marks(bytes))));
            fields.push((format!("{key}_b64"), Json::str(base64_encode(bytes))));
        }
    }
}

/// Reads a field written by `put_string()`: the `<key>_b64` copy when present, else the text.
pub fn get_bytes(object: &Json, key: &str) -> Option<Vec<u8>> {
    if let Some(b64) = object.get(&format!("{key}_b64")).and_then(Json::as_str)
        && let Ok(bytes) = base64_decode(b64)
    {
        return Some(bytes);
    }
    object
        .get(key)
        .and_then(Json::as_str)
        .map(|s| s.as_bytes().to_vec())
}

/// PHP's `json_decode($json, true)` of an already parsed value: objects become arrays with
/// normalised keys (`"1"` is the int key 1; a repeated key keeps its first position), integers
/// that fit stay ints (`-0` is `0`), longer integers and numbers with a fraction or exponent are
/// floats (`1e400` is `INF`).
pub fn decode_php(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(text) => decode_number(text),
        Json::String(s) => Value::String(PhpString::from(s.as_str())),
        Json::Array(items) => Value::Array(Array::from_values(items.iter().map(decode_php))),
        Json::Object(fields) => {
            let mut array = Array::new();
            for (key, value) in fields {
                array.insert(
                    ArrayKey::from_string(PhpString::from(key.as_str())),
                    decode_php(value),
                );
            }
            Value::Array(array)
        }
    }
}

/// The number scanner of `ext/json` (PHPSRC/ext/json/json_scanner.re): an integer literal that
/// fits a `zend_long` is an int, anything else a double from `zend_strtod()`.
fn decode_number(text: &str) -> Value {
    if !text.contains(['.', 'e', 'E'])
        && let Ok(i) = text.parse::<i64>()
    {
        return Value::Int(i);
    }
    Value::Float(text.parse::<f64>().unwrap_or(f64::NAN))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let text = r#"{"a":[1,-0,1e400,true,null],"b":"x\nyé😀","a":{}}"#;
        let json = Json::parse(text).unwrap();
        assert_eq!(json.entries().len(), 3);
        assert_eq!(json.get("a").unwrap().items()[1], Json::Number("-0".into()));
        assert_eq!(json.get("b").unwrap().as_str(), Some("x\ny\u{e9}\u{1f600}"));
        assert_eq!(Json::parse(&json.to_compact()).unwrap(), json);
        assert_eq!(Json::parse(&json.to_pretty()).unwrap(), json);
        assert_eq!(Json::parse(&json.to_layout(2)).unwrap(), json);
        assert!(Json::parse(r#""\ud800""#).is_err());
        assert!(Json::parse("01").is_err());
        assert!(Json::parse("[1,]").is_err());
    }

    #[test]
    fn decodes_like_php() {
        let json = Json::parse(r#"{"1":-0,"a":9223372036854775808,"b":1e400,"1":2.5}"#).unwrap();
        let Value::Array(array) = decode_php(&json) else {
            panic!("an array")
        };
        assert_eq!(array.len(), 3);
        assert!(matches!(array.get_int(1), Some(Value::Float(d)) if *d == 2.5));
        assert!(
            matches!(array.get_str(b"a"), Some(Value::Float(d)) if *d == 9_223_372_036_854_775_808_f64)
        );
        assert!(matches!(array.get_str(b"b"), Some(Value::Float(d)) if d.is_infinite()));
        assert!(matches!(
            decode_php(&Json::Number("-0".into())),
            Value::Int(0)
        ));
    }

    #[test]
    fn base64_and_lossy_text() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"\xff\x00\xfe"] {
            assert_eq!(base64_decode(&base64_encode(bytes)).unwrap(), bytes);
        }
        assert_eq!(base64_encode(b"caf\xe9"), "Y2Fm6Q==");
        assert_eq!(lossy_question_marks(b"a\xffb\xc3"), "a?b?");
        let mut fields = Vec::new();
        put_bytes(&mut fields, "output", b"x\xff");
        let object = Json::Object(fields);
        assert_eq!(get_bytes(&object, "output").unwrap(), b"x\xff");
    }
}
