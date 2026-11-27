//! An independent oracle cross-check of the clean-room `iconv()` (`BasicIconv`, the default build's backend) for the
//! charsets it gained with the multi-byte work: the 13 multi-byte charsets, ISO-2022-JP, ISO-2022-JP-2, ISO-2022-KR
//! and the 187 single-byte charsets added with them, converted to and from each other and the charsets it had
//! before, each input under the four suffixes (none, //TRANSLIT, //IGNORE, //TRANSLIT//IGNORE).
//!
//! `tests/vectors/string/charset_basic/crosscheck.json` holds the inputs and what PHP 8.5.13's `iconv()` (glibc
//! 2.41, the Docker oracle) gave for them. Its generator, `crosscheck.gen.php` next to it (the format is described
//! at its top), was written apart from the other generators of that directory and from the module's tables: it
//! measures each charset black-box, then draws random inputs biased to the charset's structure, long inputs around
//! the 8160-character rounds and edge cases from the standards. Regenerate, from `crates/twig-value/tests/vectors`:
//!
//! ```text
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/string/charset_basic/crosscheck.gen.php > string/charset_basic/crosscheck.json
//! ```
//!
//! twig-rs may refuse (with its `Error`) only the conversions between EUC-CN and GBK or BIG5, which glibc does with
//! modules of their own (reference/deviations.md); every other result must be PHP's, byte for byte, notices included.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use serde_json::Value as Json;
use twig_value::builtins::string::{BasicIconv, IconvBackend};
use twig_value::{Diagnostic, Level};

const FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/vectors/string/charset_basic/crosscheck.json"
);

/// What one conversion gave, in the terms of the vector file.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    /// The output: hex when at most 12 bytes long, else `length:fnv1a32`.
    Converted(String),
    /// `false` after one diagnostic: PHP's `E_*` value for its level, and its message.
    False(u64, String),
    /// twig-rs' `Error` for a conversion it does not do.
    Refused(String),
    /// Anything else (a broken invariant).
    Other(String),
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Converted(out) => write!(f, "\"{out}\""),
            Outcome::False(level, message) => write!(f, "false after E_{level} {message:?}"),
            Outcome::Refused(message) => write!(f, "Error {message:?}"),
            Outcome::Other(what) => write!(f, "{what}"),
        }
    }
}

/// FNV-1a, 32 bits (`hash('fnv1a32', ...)`).
fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Standard base64 (RFC 4648 §4, padded), as PHP's `base64_encode()` writes it.
fn base64(s: &str) -> Vec<u8> {
    fn value(c: u8) -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("bad base64 {c}"),
        }
    }
    let s = s.as_bytes();
    assert!(s.len().is_multiple_of(4), "base64 length");
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for quad in s.chunks(4) {
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        let mut n = 0u32;
        for &c in &quad[..4 - pad] {
            n = n << 6 | value(c);
        }
        n <<= 6 * pad;
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..4 - pad]);
    }
    out
}

/// An input: base64, or units repeated (`[[base64, count], ...]`).
fn input(j: &Json) -> Vec<u8> {
    match j {
        Json::String(b) => base64(b),
        Json::Array(units) => {
            let mut out = Vec::new();
            for unit in units {
                let bytes = base64(unit[0].as_str().expect("unit"));
                for _ in 0..unit[1].as_u64().expect("count") {
                    out.extend_from_slice(&bytes);
                }
            }
            out
        }
        other => panic!("bad input {other}"),
    }
}

/// A short description of an input for messages (hex).
fn show_input(j: &Json, bytes: &[u8]) -> String {
    match j {
        Json::Array(units) => {
            let parts: Vec<String> = units
                .iter()
                .map(|u| format!("{}x{}", hex(&base64(u[0].as_str().unwrap_or(""))), u[1]))
                .collect();
            format!("[{}] ({} bytes)", parts.join(" "), bytes.len())
        }
        _ if bytes.len() <= 80 => hex(bytes),
        _ => format!("{}... ({} bytes)", hex(&bytes[..80]), bytes.len()),
    }
}

/// PHP's `E_*` value of a diagnostic level.
fn php_level(level: Level) -> u64 {
    match level {
        Level::Warning => 2,
        Level::Notice => 8,
        Level::Deprecated => 8192,
        Level::UserDeprecated => 16384,
    }
}

/// `iconv($from, $to, $input)` through the default backend.
fn convert(from: &str, to: &str, input: &[u8]) -> Outcome {
    let mut diag: Vec<Diagnostic> = Vec::new();
    let r = BasicIconv.iconv_bytes(from.as_bytes(), to.as_bytes(), input, &mut diag);
    match (r, diag.as_slice()) {
        (Ok(Some(out)), []) => {
            let out = out.as_bytes();
            Outcome::Converted(if out.len() <= 12 {
                hex(out)
            } else {
                format!("{}:{:08x}", out.len(), fnv1a32(out))
            })
        }
        (Ok(None), [one]) => Outcome::False(
            php_level(one.level),
            String::from_utf8_lossy(one.message.as_bytes()).into_owned(),
        ),
        (Err(e), [])
            if e.message()
                .as_bytes()
                .ends_with(b"\" is not supported by twig-rs") =>
        {
            Outcome::Refused(e.message_lossy().into_owned())
        }
        (r, d) => Outcome::Other(format!("{r:?} {d:?}")),
    }
}

/// The conversions twig-rs refuses on purpose: glibc does them with modules of their own.
fn may_refuse(from_family: &str, to_family: &str) -> bool {
    matches!(
        (from_family, to_family),
        ("EUC-CN", "GBK" | "BIG5") | ("GBK" | "BIG5", "EUC-CN")
    )
}

/// The vector file.
struct Vectors {
    charsets: Vec<String>,
    families: Vec<String>,
    flags: Vec<String>,
    messages: Vec<(u64, String)>,
    sections: Vec<(usize, String)>,
    cases: Vec<Json>,
    total: u64,
}

impl Vectors {
    fn load() -> Self {
        let text = std::fs::read_to_string(FILE).unwrap_or_else(|e| panic!("{FILE}: {e}"));
        let mut json: Json = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{FILE}: {e}"));
        let strings = |j: &Json| -> Vec<String> {
            j.as_array()
                .expect("list")
                .iter()
                .map(|s| s.as_str().expect("string").to_string())
                .collect()
        };
        Vectors {
            charsets: strings(&json["charsets"]),
            families: strings(&json["families"]),
            flags: strings(&json["flags"]),
            messages: json["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .map(|m| {
                    (
                        m[0].as_u64().expect("level"),
                        m[1].as_str().expect("message").to_string(),
                    )
                })
                .collect(),
            sections: json["sections"]
                .as_array()
                .expect("sections")
                .iter()
                .map(|s| {
                    (
                        usize::try_from(s[0].as_u64().expect("first")).expect("index"),
                        s[1].as_str().expect("name").to_string(),
                    )
                })
                .collect(),
            total: json["cases_total"].as_u64().expect("cases_total"),
            cases: match json["cases"].take() {
                Json::Array(cases) => cases,
                _ => panic!("cases"),
            },
        }
    }

    fn section(&self, case: usize) -> &str {
        let i = self.sections.partition_point(|(first, _)| *first <= case);
        self.sections[i.saturating_sub(1)].1.as_str()
    }

    fn index(j: &Json) -> usize {
        usize::try_from(j.as_u64().expect("index")).expect("index")
    }

    /// The expected outcome `j` (`first` is the outcome without a suffix, which 0 repeats).
    fn expected(&self, j: &Json, first: Option<&Outcome>) -> Outcome {
        match j {
            Json::String(out) => Outcome::Converted(out.clone()),
            Json::Number(n) => match n.as_u64().expect("result") {
                0 => first.expect("0 repeats the first result").clone(),
                n => {
                    let (level, message) = &self.messages[usize::try_from(n - 1).expect("index")];
                    Outcome::False(*level, message.clone())
                }
            },
            other => panic!("bad result {other}"),
        }
    }
}

/// Replays the vector file; panics with a report of the differences.
fn replay() {
    let v = Vectors::load();
    let mut run = 0u64;
    let mut refused = 0u64;
    let mut mismatches: Vec<String> = Vec::new();
    let mut groups: BTreeMap<String, usize> = BTreeMap::new();
    let mut differing: Vec<usize> = Vec::new();
    for (i, case) in v.cases.iter().enumerate() {
        let (from, to) = (Vectors::index(&case[0]), Vectors::index(&case[1]));
        let data = input(&case[2]);
        let first = v.expected(&case[3], None);
        let results = case.as_array().expect("case").len() - 3;
        assert!(
            results == 1 || results == v.flags.len(),
            "case {i}: results"
        );
        for (k, flag) in v.flags.iter().enumerate() {
            let want = if k == 0 || results == 1 {
                first.clone()
            } else {
                v.expected(&case[3 + k], Some(&first))
            };
            let target = format!("{}{flag}", v.charsets[to]);
            let got = convert(&v.charsets[from], &target, &data);
            run += 1;
            if got == want {
                continue;
            }
            if matches!(got, Outcome::Refused(_)) && may_refuse(&v.families[from], &v.families[to])
            {
                refused += 1;
                continue;
            }
            *groups
                .entry(format!(
                    "{}: {} -> {}{flag}",
                    v.section(i),
                    v.families[from],
                    v.families[to]
                ))
                .or_default() += 1;
            if differing.last() != Some(&i) {
                differing.push(i);
            }
            if mismatches.len() < 80 {
                mismatches.push(format!(
                    "case {i} ({}): iconv({:?}, {target:?}, {}): PHP {want}, twig-rs {got}",
                    v.section(i),
                    v.charsets[from],
                    show_input(&case[2], &data)
                ));
            }
        }
    }
    assert_eq!(run, v.total, "every case was run");
    let total: usize = groups.values().sum();
    if total > 0 {
        let mut report = format!(
            "{total} of {run} conversions differ from PHP ({refused} refused as documented), in the cases {differing:?}\n"
        );
        for (group, n) in &groups {
            let _ = writeln!(report, "  {n:6} {group}");
        }
        for m in &mismatches {
            let _ = writeln!(report, "{m}");
        }
        panic!("{report}");
    }
    eprintln!("crosscheck: {run} conversions match PHP ({refused} refused as documented)");
}

#[test]
fn crosscheck() {
    replay();
}
