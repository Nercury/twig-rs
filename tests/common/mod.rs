//! Helpers shared by the integration tests: a minimal JSON reader for the oracle vector files (the root crate has
//! no JSON dev-dependency) and decoders for the vector encodings.

#![allow(dead_code)]

use std::fmt::Write as _;

/// A parsed JSON value. Numbers keep their source text; objects keep their key order.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Parses a complete JSON document.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            position: 0,
        };
        let value = parser.value()?;
        parser.whitespace();
        if parser.position != parser.bytes.len() {
            return Err(parser.error("trailing characters"));
        }
        Ok(value)
    }

    /// Reads and parses a vector file under `tests/vectors/`.
    pub fn load_vectors(file: &str) -> Json {
        Json::load_path(&format!(
            "{}/tests/vectors/{file}",
            env!("CARGO_MANIFEST_DIR")
        ))
    }

    /// Reads and parses a JSON file.
    pub fn load_path(path: &str) -> Json {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
        Json::parse(&text).unwrap_or_else(|error| panic!("cannot parse {path}: {error}"))
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The value of a field that must exist.
    pub fn field(&self, key: &str) -> &Json {
        self.get(key)
            .unwrap_or_else(|| panic!("missing field {key:?} in {self:?}"))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Number(n) => n.parse().ok(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            _ => panic!("expected an array, got {self:?}"),
        }
    }

    pub fn as_object(&self) -> &[(String, Json)] {
        match self {
            Json::Object(fields) => fields,
            _ => panic!("expected an object, got {self:?}"),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// A byte string encoded as a JSON string or as `{"b64": "..."}`.
    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            Json::String(s) => s.as_bytes().to_vec(),
            Json::Object(_) => base64_decode(self.field("b64").as_str().expect("b64 string")),
            _ => panic!("expected a byte string, got {self:?}"),
        }
    }

    /// A message that may be encoded as `{"b64": "..."}`, converted the way twig-rs converts non-UTF-8 message
    /// bytes (lossy).
    pub fn as_lossy_string(&self) -> String {
        String::from_utf8_lossy(&self.as_bytes()).into_owned()
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> String {
        format!("{message} at byte {}", self.position)
    }

    fn whitespace(&mut self) {
        while self.position < self.bytes.len()
            && matches!(self.bytes[self.position], b' ' | b'\t' | b'\n' | b'\r')
        {
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
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') if self.eat("true") => Ok(Json::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Json::Bool(false)),
            Some(b'n') if self.eat("null") => Ok(Json::Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.position;
                while self.position < self.bytes.len()
                    && matches!(
                        self.bytes[self.position],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.position += 1;
                }
                Ok(Json::Number(
                    String::from_utf8_lossy(&self.bytes[start..self.position]).into_owned(),
                ))
            }
            _ => Err(self.error("unexpected character")),
        }
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
            while self.position < self.bytes.len()
                && !matches!(self.bytes[self.position], b'"' | b'\\')
            {
                self.position += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.position])
                    .map_err(|_| self.error("bad UTF-8"))?,
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
                                    return Err(self.error("lone surrogate"));
                                }
                                let low = self.hex4()?;
                                code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                            }
                            out.push(
                                char::from_u32(code).ok_or_else(|| self.error("bad code point"))?,
                            );
                        }
                        _ => return Err(self.error("bad escape")),
                    }
                }
                _ => return Err(self.error("unterminated string")),
            }
        }
    }
}

/// Standard base64 (with padding) decoding.
pub fn base64_decode(text: &str) -> Vec<u8> {
    let value = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 character {c:?}"),
        }
    };
    let digits: Vec<u8> = text.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let mut buffer = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            buffer |= value(c) << (18 - 6 * i);
        }
        let bytes = buffer.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    out
}

/// CRC-32 (IEEE 802.3, reflected), what PHP's `hash('crc32b', $s)` computes.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Escapes bytes for failure messages.
pub fn show(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        if b.is_ascii_graphic() || b == b' ' {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "\\x{b:02x}");
        }
    }
    out
}

#[test]
fn json_reader_round_trips() {
    let value = Json::parse(r#"{"a": [1, -2.5e3, true, false, null], "c": {}}"#).unwrap();
    assert_eq!(value.field("a").as_array().len(), 5);
    assert_eq!(
        value.field("a").as_array()[1],
        Json::Number("-2.5e3".into())
    );
    let escaped = concat!(
        r#"["x\ny\""#,
        r"\",
        "u00e9",
        r"\",
        "ud83d",
        r"\",
        r#"ude00"]"#
    );
    assert_eq!(
        Json::parse(escaped).unwrap().as_array()[0].as_str(),
        Some("x\ny\"\u{e9}\u{1f600}")
    );
    assert_eq!(base64_decode("Y2Fm6Q=="), b"caf\xe9");
    assert_eq!(
        crc32(b"The quick brown fox jumps over the lazy dog"),
        0x414F_A339
    );
}
