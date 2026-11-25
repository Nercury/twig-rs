//! Helpers shared by the oracle-vector tests (`mod support;` in a test file).
//!
//! Byte strings are stored hex-encoded and doubles as big-endian IEEE bit patterns (hex), the
//! formats of the generators in `tests/vectors/` (see `common.inc.php`).

#![allow(dead_code)]

pub mod values;

use serde_json::Value as Json;
use twig_value::{Diagnostic, Level, PhpError};

/// Parses `tests/vectors/<name>`.
pub fn load(name: &str) -> Json {
    let path = format!("{}/tests/vectors/{}", env!("CARGO_MANIFEST_DIR"), name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("cannot parse {path}: {e}"))
}

/// A double from its big-endian hex bit pattern.
pub fn bits(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).expect("hex bits"))
}

/// The hex bit pattern of a double.
pub fn to_bits(f: f64) -> String {
    format!("{:016x}", f.to_bits())
}

/// Bytes from hex.
pub fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
        .collect()
}

/// Hex of bytes.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The string of a JSON string node.
pub fn s(j: &Json) -> &str {
    j.as_str()
        .unwrap_or_else(|| panic!("expected string, got {j}"))
}

/// Collects mismatches and fails with a readable summary.
pub struct Mismatches {
    name: &'static str,
    total: usize,
    items: Vec<String>,
}

impl Mismatches {
    /// A collector named `name`.
    pub fn new(name: &'static str) -> Self {
        Mismatches {
            name,
            total: 0,
            items: Vec::new(),
        }
    }

    /// Records one check.
    pub fn check<T: PartialEq + std::fmt::Debug>(
        &mut self,
        what: impl FnOnce() -> String,
        got: T,
        want: T,
    ) {
        self.total += 1;
        if got != want {
            self.items
                .push(format!("{}: got {:?}, want {:?}", what(), got, want));
        }
    }

    /// Number of checks so far.
    pub fn total(&self) -> usize {
        self.total
    }

    /// Panics with the first mismatches, if any.
    pub fn finish(self) {
        if !self.items.is_empty() {
            let shown: Vec<&String> = self.items.iter().take(40).collect();
            panic!(
                "{}: {} of {} checks differ from PHP:\n{}",
                self.name,
                self.items.len(),
                self.total,
                shown
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        assert!(self.total > 0, "{}: no checks ran", self.name);
        eprintln!("{}: {} checks match PHP", self.name, self.total);
    }
}

/// Diagnostics as `(level, hex message)` pairs (`W`, `N`, `D`, `UD`), the generators' format.
pub fn diag_list(d: &[Diagnostic]) -> Vec<(String, String)> {
    d.iter()
        .map(|d| {
            let level = match d.level {
                Level::Warning => "W",
                Level::Notice => "N",
                Level::Deprecated => "D",
                Level::UserDeprecated => "UD",
            };
            (level.to_string(), hex(d.message.as_bytes()))
        })
        .collect()
}

/// Diagnostics from their JSON form.
pub fn json_diags(j: &Json) -> Vec<(String, String)> {
    j.as_array()
        .unwrap_or_else(|| panic!("diagnostics must be a list: {j}"))
        .iter()
        .map(|p| (s(&p[0]).to_string(), s(&p[1]).to_string()))
        .collect()
}

/// An exception as `(class, hex message)`.
pub fn error_pair(e: &PhpError) -> (String, String) {
    (e.class().name().to_string(), hex(e.message().as_bytes()))
}
