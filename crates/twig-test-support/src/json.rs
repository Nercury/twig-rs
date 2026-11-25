//! JSON helpers shared by the corpus and typed-tree models.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use twig_value::PhpString;

/// Decodes standard base64 (RFC 4648 alphabet, `=` padding), as PHP's `base64_encode()` writes it.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    fn sextet(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(format!("invalid base64 length {}", bytes.len()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (n, chunk) in bytes.chunks(4).enumerate() {
        let last = (n + 1) * 4 == bytes.len();
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return Err("invalid base64 padding".to_string());
        }
        let mut acc = 0u32;
        for &c in &chunk[..4 - pad] {
            acc = (acc << 6) | sextet(c).ok_or_else(|| format!("invalid base64 byte {c:#x}"))?;
        }
        acc <<= 6 * pad as u32;
        let decoded = [(acc >> 16) as u8, (acc >> 8) as u8, acc as u8];
        out.extend_from_slice(&decoded[..3 - pad]);
    }
    Ok(out)
}

/// Bytes of a string field written with lib.php's `put_string()`: the `<field>_b64` copy when
/// present, otherwise the text.
pub fn field_bytes(text: &str, b64: Option<&str>) -> PhpString {
    match b64.map(decode_base64) {
        Some(Ok(bytes)) => PhpString::from(bytes),
        _ => PhpString::from(text),
    }
}

/// A PHP `Throwable` as lib.php's `describe_throwable()` records it.
#[derive(Clone, Debug, Deserialize)]
pub struct ErrorInfo {
    /// The exception class (`Twig\Error\RuntimeError`, `TypeError`, ...).
    pub class: String,
    /// `getMessage()` (lossy when not valid UTF-8, see [`ErrorInfo::message_bytes`]).
    pub message: String,
    /// Base64 bytes of the message when it is not valid UTF-8.
    #[serde(default)]
    pub message_b64: Option<String>,
    /// `"<class>: <message>"`: what `IntegrationTestCase` compares with `--EXCEPTION--`.
    pub formatted: String,
    /// Base64 bytes of `formatted` when it is not valid UTF-8.
    #[serde(default)]
    pub formatted_b64: Option<String>,
    /// Twig errors: `getRawMessage()`.
    #[serde(default, rename = "rawMessage")]
    pub raw_message: Option<String>,
    /// Base64 bytes of the raw message when it is not valid UTF-8.
    #[serde(default, rename = "rawMessage_b64")]
    pub raw_message_b64: Option<String>,
    /// Twig errors: the template name of the source context (`null` when unknown).
    #[serde(default, rename = "templateName")]
    pub template_name: Option<String>,
    /// Twig errors: `getTemplateLine()`.
    #[serde(default, rename = "templateLine")]
    pub template_line: Option<i64>,
    /// Twig errors: `getTemplateColumn()`.
    #[serde(default, rename = "templateColumn")]
    pub template_column: Option<i64>,
    /// Other throwables: `getCode()`.
    #[serde(default)]
    pub code: Option<i64>,
    /// `getPrevious()` (up to 5 levels).
    #[serde(default)]
    pub previous: Option<Box<ErrorInfo>>,
}

impl ErrorInfo {
    /// The exact message bytes.
    pub fn message_bytes(&self) -> PhpString {
        field_bytes(&self.message, self.message_b64.as_deref())
    }

    /// The exact `"<class>: <message>"` bytes.
    pub fn formatted_bytes(&self) -> PhpString {
        field_bytes(&self.formatted, self.formatted_b64.as_deref())
    }

    /// The exact raw message bytes (Twig errors).
    pub fn raw_message_bytes(&self) -> Option<PhpString> {
        self.raw_message
            .as_deref()
            .map(|m| field_bytes(m, self.raw_message_b64.as_deref()))
    }
}

/// Deserializes a JSON object into its entries in document order.
pub(crate) fn ordered_map<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<(String, String)>, D::Error> {
    struct OrderedVisitor;

    impl<'de> Visitor<'de> for OrderedVisitor {
        type Value = Vec<(String, String)>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an object of strings")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some((k, v)) = map.next_entry::<String, String>()? {
                out.push((k, v));
            }
            Ok(out)
        }

        // PHP's json_encode() writes an empty array as [] even where a map is meant.
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            if seq.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom("expected an object"));
            }
            Ok(Vec::new())
        }
    }

    d.deserialize_any(OrderedVisitor)
}

/// `false` or a string (`exception_expected`): `None` for `false`.
pub(crate) fn false_or_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FalseOrString {
        Text(String),
        Flag(bool),
    }
    match FalseOrString::deserialize(d)? {
        FalseOrString::Text(s) => Ok(Some(s)),
        FalseOrString::Flag(false) => Ok(None),
        FalseOrString::Flag(true) => Err(de::Error::custom("expected false or a string")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64() {
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("wyhhYmM=").unwrap(), b"\xC3\x28abc");
        assert_eq!(decode_base64("/w==").unwrap(), b"\xff");
        assert_eq!(decode_base64("YWJj").unwrap(), b"abc");
        assert!(decode_base64("YWJ").is_err());
        assert!(decode_base64("Y=Jj").is_err());
        assert!(decode_base64("YW!j").is_err());
    }
}
