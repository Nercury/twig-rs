//! Field-by-field comparison of an oracle result (`expected`) with a twig-rs result (`actual`).
//!
//! Compared: `ok`; the output bytes; the failing `phase`; the error's class, message, raw message,
//! template name, line and column and its `previous` chain; the deprecations; the diagnostics
//! (type and message, in order; `silenced` ones are skipped: only Twig's filesystem cache raises
//! them). Not compared: `formatted` (class + message), `code` (twig-rs does not model exception
//! codes, `reference/deviations.md`), the typed-data extras.
//!
//! Messages are compared the way twig-rs produces them: invalid UTF-8 in a PHP message is decoded
//! lossily (`reference/deviations.md`, "Non-UTF-8 bytes in error and deprecation messages").
//!
//! When fields differ, the [deviation rules](DEVIATION_RULES) (known, accepted differences listed
//! in `reference/deviations.md`) are applied to both sides; a difference that disappears is a
//! [`Verdict::Deviation`], not a mismatch. Two more deviations are recognised by twig-rs's own
//! error texts: `memory-limit` (twig-rs's stand-in for PHP's 128M limit, `Allowed memory size of
//! ...`, while the oracle runs with another limit) and `unsupported-by-twig-rs` (`... not supported
//! by twig-rs`: charsets, `TemplateWrapper` methods). An error of a PHP class port of
//! `twig-test-support` (`... is not supported by twig-test-support`) makes the case
//! [not comparable](Verdict::Skip).
//!
//! Every mismatch gets a [signature](Mismatch::signature): the differing fields with their values
//! reduced to a stable form (error classes kept, quoted strings and numbers replaced by `_` and
//! `N`, outputs reduced to whether twig-rs printed more, less or other text plus the shape of a
//! short difference), so that the same bug keeps its signature while its template shrinks, and
//! mismatches of one bug cluster together. [`family`] coarsens signatures further for reports.

use std::fmt::Write as _;

use super::result::{CaseResult, Diag, ErrorDesc};
use super::run::DATA_UNAVAILABLE;

/// One differing field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDiff {
    /// The field (`ok`, `output`, `error.message`, `error.previous.class`, `diagnostics`, ...).
    pub field: String,
    /// The oracle's value.
    pub expected: String,
    /// twig-rs's value.
    pub actual: String,
    /// The stable signature component.
    pub signature: String,
}

/// A mismatch between the oracle and twig-rs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    /// The differing fields.
    pub fields: Vec<FieldDiff>,
}

impl Mismatch {
    /// The stable signature: the signature components of the fields, in field order.
    pub fn signature(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for field in &self.fields {
            if !parts.contains(&field.signature.as_str()) {
                parts.push(&field.signature);
            }
        }
        parts.join(" | ")
    }

    /// A multi-line description of every field.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        for field in &self.fields {
            let _ = writeln!(
                out,
                "{}:\n    php: {}\n    rs:  {}",
                field.field, field.expected, field.actual
            );
        }
        out
    }
}

/// The verdict on one case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Identical.
    Same,
    /// Identical once the named deviation rules are applied.
    Deviation(Vec<&'static str>),
    /// Different.
    Mismatch(Mismatch),
    /// Not comparable (why): the oracle failed, or twig-rs lacks the data of a `data_php` case.
    Skip(String),
}

/// A known deviation: a text rewrite applied to both sides.
pub struct DeviationRule {
    /// The rule's name.
    pub name: &'static str,
    /// Which fields it applies to: `message` (error messages, deprecations and diagnostics) or
    /// `output`.
    pub applies_to: &'static [&'static str],
    /// The rewrite.
    pub rewrite: fn(&str) -> String,
}

/// The deviation rules (`reference/deviations.md`).
pub const DEVIATION_RULES: &[DeviationRule] = &[
    DeviationRule {
        // "Userland TypeError suffix", "Closure names and source locations in messages"
        name: "called-in-suffix",
        applies_to: &["message"],
        rewrite: strip_called_in,
    },
    DeviationRule {
        name: "passed-in-location",
        applies_to: &["message"],
        rewrite: strip_passed_in,
    },
    DeviationRule {
        name: "closure-name",
        applies_to: &["message", "output"],
        rewrite: plain_closure_names,
    },
    DeviationRule {
        // "Object handles": var_dump() handle numbers
        name: "object-handle",
        applies_to: &["message", "output"],
        rewrite: object_handles,
    },
];

/// Compares the oracle's result with twig-rs's.
pub fn compare(expected: &CaseResult, actual: &CaseResult) -> Verdict {
    if expected.is_oracle_failure() {
        return Verdict::Skip(format!(
            "the oracle failed: {}",
            expected
                .error
                .as_ref()
                .map(ErrorDesc::formatted)
                .unwrap_or_default()
        ));
    }
    if let Some(error) = &actual.error
        && error.class == DATA_UNAVAILABLE
        && !(expected.phase.as_deref() == Some("load")
            || expected.phase.as_deref() == Some("setup"))
    {
        return Verdict::Skip(String::from_utf8_lossy(&error.message).into_owned());
    }
    let raw = fields(expected, actual, &[]);
    if raw.is_empty() {
        return Verdict::Same;
    }
    // twig-rs stands in for PHP's default 128M memory limit where PHP would allocate a huge result
    // (reference/deviations.md, "Memory exhaustion"); the oracle runs with another limit
    let memory = |r: &CaseResult| {
        let mut error = r.error.as_ref();
        while let Some(e) = error {
            if lossy(&e.message).contains("Allowed memory size of ") {
                return true;
            }
            error = e.previous.as_deref();
        }
        false
    };
    if memory(actual) && !memory(expected) {
        return Verdict::Deviation(vec!["memory-limit"]);
    }
    // features twig-rs or the Rust ports of the PHP test classes say they do not support
    // (reference/deviations.md: iconv/mbstring charsets, TemplateWrapper methods, rare SPL methods)
    let says = |r: &CaseResult, what: &str| {
        let mut error = r.error.as_ref();
        while let Some(e) = error {
            if lossy(&e.message).contains(what) {
                return true;
            }
            error = e.previous.as_deref();
        }
        false
    };
    if says(actual, "is not supported by twig-test-support") {
        return Verdict::Skip(
            "a PHP class port of twig-test-support does not support the operation".to_owned(),
        );
    }
    if says(actual, "not supported by twig-rs") && !says(expected, "not supported by twig-rs") {
        return Verdict::Deviation(vec!["unsupported-by-twig-rs"]);
    }
    let all: Vec<&DeviationRule> = DEVIATION_RULES.iter().collect();
    let normalised = fields(expected, actual, &all);
    if normalised.is_empty() {
        // name the rules that were needed
        let needed: Vec<&'static str> = DEVIATION_RULES
            .iter()
            .filter(|rule| {
                let without: Vec<&DeviationRule> = all
                    .iter()
                    .copied()
                    .filter(|r| r.name != rule.name)
                    .collect();
                !fields(expected, actual, &without).is_empty()
            })
            .map(|rule| rule.name)
            .collect();
        return Verdict::Deviation(if needed.is_empty() {
            DEVIATION_RULES.iter().map(|r| r.name).collect()
        } else {
            needed
        });
    }
    Verdict::Mismatch(Mismatch { fields: normalised })
}

fn apply(rules: &[&DeviationRule], kind: &str, text: &str) -> String {
    let mut out = text.to_owned();
    for rule in rules {
        if rule.applies_to.contains(&kind) {
            out = (rule.rewrite)(&out);
        }
    }
    out
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A quoted, escaped rendering of bytes: valid UTF-8 as Rust's `{:?}` writes it, invalid bytes as
/// `\xNN`.
pub fn escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for chunk in bytes.utf8_chunks() {
        let escaped = format!("{:?}", chunk.valid());
        out.push_str(&escaped[1..escaped.len() - 1]);
        for b in chunk.invalid() {
            let _ = write!(out, "\\x{b:02x}");
        }
    }
    out.push('"');
    out
}

fn fields(expected: &CaseResult, actual: &CaseResult, rules: &[&DeviationRule]) -> Vec<FieldDiff> {
    let mut out = Vec::new();
    if expected.ok != actual.ok {
        let side = |r: &CaseResult| -> (String, String) {
            if r.ok {
                ("ok".to_owned(), r.summary())
            } else {
                let error = r.error.as_ref();
                (
                    error.map(|e| e.class.clone()).unwrap_or_default(),
                    format!(
                        "{}: {}",
                        r.phase.as_deref().unwrap_or("?"),
                        error.map(ErrorDesc::formatted).unwrap_or_default()
                    ),
                )
            }
        };
        let (expected_class, expected_text) = side(expected);
        let (actual_class, actual_text) = side(actual);
        let failing = if expected.ok { actual } else { expected };
        let message = failing
            .error
            .as_ref()
            .map(|e| shape(&apply(rules, "message", &lossy(&e.message))))
            .unwrap_or_default();
        out.push(FieldDiff {
            field: "ok".to_owned(),
            expected: expected_text,
            actual: actual_text,
            signature: format!(
                "ok:{expected_class}>{actual_class} [{}{}]",
                failing.phase.as_deref().unwrap_or("?"),
                if message.is_empty() {
                    String::new()
                } else {
                    format!(": {message}")
                }
            ),
        });
    } else if expected.ok {
        let want = expected.output.clone().unwrap_or_default();
        let got = actual.output.clone().unwrap_or_default();
        if want != got {
            // the rules rewrite text; outputs that are not valid UTF-8 are compared as bytes
            let equal_after_rules = !rules.is_empty()
                && match (std::str::from_utf8(&want), std::str::from_utf8(&got)) {
                    (Ok(w), Ok(g)) => apply(rules, "output", w) == apply(rules, "output", g),
                    _ => false,
                };
            if !equal_after_rules {
                out.push(FieldDiff {
                    field: "output".to_owned(),
                    expected: escape_bytes(&want),
                    actual: escape_bytes(&got),
                    signature: output_signature(&lossy(&want), &lossy(&got)),
                });
            }
        }
    } else {
        if expected.phase != actual.phase {
            out.push(FieldDiff {
                field: "phase".to_owned(),
                expected: format!("{:?}", expected.phase),
                actual: format!("{:?}", actual.phase),
                signature: format!(
                    "phase:{}>{}",
                    expected.phase.as_deref().unwrap_or("?"),
                    actual.phase.as_deref().unwrap_or("?")
                ),
            });
        }
        if let (Some(want), Some(got)) = (&expected.error, &actual.error) {
            error_fields("error", want, got, rules, &mut out);
        }
    }
    let want: Vec<String> = expected
        .deprecations
        .iter()
        .map(|d| apply(rules, "message", d))
        .collect();
    let got: Vec<String> = actual
        .deprecations
        .iter()
        .map(|d| apply(rules, "message", d))
        .collect();
    if want != got {
        out.push(FieldDiff {
            field: "deprecations".to_owned(),
            expected: format!("{want:?}"),
            actual: format!("{got:?}"),
            signature: list_signature("deprecations", &want, &got),
        });
    }
    let diag_text = |d: &Diag| format!("{}: {}", d.kind, apply(rules, "message", &d.message));
    let want: Vec<String> = expected
        .diagnostics
        .iter()
        .filter(|d| !d.silenced)
        .map(diag_text)
        .collect();
    let got: Vec<String> = actual
        .diagnostics
        .iter()
        .filter(|d| !d.silenced)
        .map(diag_text)
        .collect();
    if want != got {
        out.push(FieldDiff {
            field: "diagnostics".to_owned(),
            expected: format!("{want:?}"),
            actual: format!("{got:?}"),
            signature: list_signature("diagnostics", &want, &got),
        });
    }
    out
}

fn error_fields(
    path: &str,
    want: &ErrorDesc,
    got: &ErrorDesc,
    rules: &[&DeviationRule],
    out: &mut Vec<FieldDiff>,
) {
    let want_message = apply(rules, "message", &lossy(&want.message));
    let got_message = apply(rules, "message", &lossy(&got.message));
    if want.class != got.class {
        out.push(FieldDiff {
            field: format!("{path}.class"),
            expected: want.class.clone(),
            actual: got.class.clone(),
            signature: format!(
                "{path}.class:{}>{} [{} > {}]",
                want.class,
                got.class,
                shape(&want_message),
                shape(&got_message)
            ),
        });
    } else if want_message != got_message {
        out.push(FieldDiff {
            field: format!("{path}.message"),
            expected: want_message.clone(),
            actual: got_message.clone(),
            signature: format!(
                "{path}.message:{} [{} > {}]",
                want.class,
                shape(&want_message),
                shape(&got_message)
            ),
        });
    }
    match (&want.twig, &got.twig) {
        (Some(w), Some(g)) => {
            let w_raw = apply(rules, "message", &lossy(&w.raw_message));
            let g_raw = apply(rules, "message", &lossy(&g.raw_message));
            if w_raw != g_raw && want_message == got_message {
                out.push(FieldDiff {
                    field: format!("{path}.rawMessage"),
                    expected: w_raw.clone(),
                    actual: g_raw.clone(),
                    signature: format!("{path}.rawMessage [{} > {}]", shape(&w_raw), shape(&g_raw)),
                });
            }
            if w.template_name != g.template_name {
                out.push(FieldDiff {
                    field: format!("{path}.templateName"),
                    expected: format!("{:?}", w.template_name),
                    actual: format!("{:?}", g.template_name),
                    signature: format!("{path}.templateName"),
                });
            }
            if w.template_line != g.template_line {
                let relation = match (w.template_line, g.template_line) {
                    (w, _) if w <= 0 => format!("php={w}"),
                    (_, g) if g <= 0 => format!("rs={g}"),
                    (w, g) if w < g => "php<rs".to_owned(),
                    _ => "php>rs".to_owned(),
                };
                out.push(FieldDiff {
                    field: format!("{path}.templateLine"),
                    expected: w.template_line.to_string(),
                    actual: g.template_line.to_string(),
                    signature: format!("{path}.templateLine:{relation}"),
                });
            }
            if w.template_column != g.template_column {
                out.push(FieldDiff {
                    field: format!("{path}.templateColumn"),
                    expected: format!("{:?}", w.template_column),
                    actual: format!("{:?}", g.template_column),
                    signature: format!("{path}.templateColumn"),
                });
            }
        }
        (None, None) => {}
        _ => {} // the class differs already
    }
    match (&want.previous, &got.previous) {
        (Some(w), Some(g)) => error_fields(&format!("{path}.previous"), w, g, rules, out),
        (None, None) => {}
        (w, g) => out.push(FieldDiff {
            field: format!("{path}.previous"),
            expected: w.as_ref().map(|e| e.formatted()).unwrap_or_default(),
            actual: g.as_ref().map(|e| e.formatted()).unwrap_or_default(),
            signature: format!(
                "{path}.previous:{}>{}",
                w.as_ref().map_or("none", |e| e.class.as_str()),
                g.as_ref().map_or("none", |e| e.class.as_str())
            ),
        }),
    }
}

/// The family of a signature: its components without the bracketed message shapes, and list
/// components reduced to the kinds of their entries. Clusters of one family often share a cause
/// (the same operator, the same error path); reports group them.
pub fn family(signature: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in split_components(signature) {
        let head = component.split(" [").next().unwrap_or_default();
        let part = if let Some(rest) = head.strip_prefix("diagnostics:") {
            let mut kinds: Vec<String> = rest
                .split(' ')
                .filter(|w| w.starts_with(['+', '-']) && w.ends_with(':'))
                .map(|w| w.trim_end_matches(':').to_owned())
                .collect();
            kinds.sort();
            kinds.dedup();
            format!("diagnostics:{}", kinds.join(","))
        } else if let Some(rest) = head.strip_prefix("deprecations:") {
            let mut signs: Vec<&str> = rest
                .split(' ')
                .filter_map(|w| match w.chars().next() {
                    Some('+') => Some("+"),
                    Some('-') => Some("-"),
                    _ => None,
                })
                .collect();
            signs.sort_unstable();
            signs.dedup();
            format!("deprecations:{}", signs.concat())
        } else {
            head.to_owned()
        };
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts.join(" | ")
}

/// The components of a signature (split at ` | ` outside brackets).
fn split_components(signature: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let bytes = signature.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b' ' if depth == 0 && signature[i..].starts_with(" | ") => {
                out.push(&signature[start..i]);
                start = i + 3;
                i += 3;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&signature[start..]);
    out
}

/// `deprecations:+<shape>` for entries only twig-rs has, `-<shape>` for entries only PHP has,
/// `~order` when both have the same entries in another order.
fn list_signature(name: &str, want: &[String], got: &[String]) -> String {
    let mut want_left: Vec<String> = want.iter().map(|s| shape(s)).collect();
    let mut parts: Vec<String> = Vec::new();
    for g in got {
        let g = shape(g);
        match want_left.iter().position(|w| *w == g) {
            Some(i) => {
                want_left.remove(i);
            }
            None => parts.push(format!("+{g}")),
        }
    }
    for w in want_left {
        parts.push(format!("-{w}"));
    }
    parts.sort();
    parts.dedup();
    if parts.is_empty() {
        format!("{name}:~order")
    } else {
        format!("{name}:{}", parts.join(" "))
    }
}

/// `output:<kind>`: what twig-rs printed compared with PHP once the common prefix and suffix are
/// removed: `missing` (twig-rs printed nothing there), `extra` (PHP printed nothing there),
/// `case` (only letter case differs), `whitespace` (only whitespace differs) or `changed`.
fn output_signature(want: &str, got: &str) -> String {
    let (w, g) = diff_core(want, got);
    let kind = if g.is_empty() {
        "missing"
    } else if w.is_empty() {
        "extra"
    } else if w.to_lowercase() == g.to_lowercase() {
        "case"
    } else if w.split_whitespace().eq(g.split_whitespace()) {
        "whitespace"
    } else {
        "changed"
    };
    // short differences keep their shape (a formatting bug stays apart from another one); long
    // ones would change while the minimiser shrinks the template
    let (w, g) = (shape(w), shape(g));
    if w.chars().count() <= 24 && g.chars().count() <= 24 {
        format!("output:{kind} [{w:?} > {g:?}]")
    } else {
        format!("output:{kind}")
    }
}

/// The parts of `a` and `b` left after removing their common prefix and suffix (on characters).
pub fn diff_core<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    let prefix: usize = a
        .chars()
        .zip(b.chars())
        .take_while(|(x, y)| x == y)
        .map(|(x, _)| x.len_utf8())
        .sum();
    let (a_rest, b_rest) = (&a[prefix..], &b[prefix..]);
    let suffix: usize = a_rest
        .chars()
        .rev()
        .zip(b_rest.chars().rev())
        .take_while(|(x, y)| x == y)
        .map(|(x, _)| x.len_utf8())
        .sum();
    (
        &a_rest[..a_rest.len() - suffix],
        &b_rest[..b_rest.len() - suffix],
    )
}

/// A message reduced to its stable shape: quoted strings become `"_"` (`'_'`), numbers `N`,
/// whitespace runs one space, and the text is cut at 160 characters.
pub fn shape(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let chars: Vec<char> = message.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"'
            || (c == '\'' && i > 0 && !chars[i - 1].is_alphanumeric())
            || (c == '\'' && i == 0)
        {
            // a quoted string: up to the matching quote on the same line
            if let Some(end) = chars[i + 1..]
                .iter()
                .position(|&x| x == c || x == '\n')
                .map(|p| p + i + 1)
                && chars[end] == c
            {
                out.push(c);
                out.push('_');
                out.push(c);
                i = end + 1;
                continue;
            }
            out.push(c);
            i += 1;
        } else if c.is_ascii_digit() {
            // \d+(\.\d+)?
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            out.push('N');
        } else if c.is_whitespace() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            out.push(' ');
        } else {
            out.push(c);
            i += 1;
        }
    }
    if out.chars().count() > 160 {
        out = out.chars().take(160).collect::<String>() + "…";
    }
    out
}

fn strip_called_in(text: &str) -> String {
    let mut out = text.to_owned();
    while let Some(start) = out.find(", called in ") {
        let Some(on_line) = out[start..].find(" on line ") else {
            break;
        };
        let digits_start = start + on_line + " on line ".len();
        let digits = out[digits_start..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if digits == 0 {
            break;
        }
        out.replace_range(start..digits_start + digits, "");
    }
    out
}

fn strip_passed_in(text: &str) -> String {
    let mut out = text.to_owned();
    let mut from = 0;
    while let Some(found) = out[from..].find(" passed in ") {
        let start = from + found;
        let Some(on_line) = out[start..].find(" on line ") else {
            break;
        };
        let digits_start = start + on_line + " on line ".len();
        let digits = out[digits_start..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if digits == 0 {
            from = start + 1;
            continue;
        }
        out.replace_range(start + " passed".len()..digits_start + digits, "");
        from = start + 1;
    }
    out
}

fn plain_closure_names(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{closure:") {
        out.push_str(&rest[..start]);
        match rest[start..].find('}') {
            Some(end) => {
                out.push_str("{closure}");
                rest = &rest[start + end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn object_handles(text: &str) -> String {
    // `object(Foo)#12 (0)` / `enum(Foo::Bar)` never carry handles; replace `)#<digits>`
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut last = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b')' && bytes[i + 1] == b'#' {
            let digits = bytes[i + 2..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            if digits > 0 && text[..i].rfind("object(").is_some() {
                out.push_str(&text[last..i + 2]);
                out.push('N');
                i += 2 + digits;
                last = i;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::super::json::Json;
    use super::*;

    fn result(json: &str) -> CaseResult {
        CaseResult::from_json(&Json::parse(json).unwrap())
    }

    #[test]
    fn identical_and_different_outputs() {
        let a = result(r#"{"ok":true,"output":"a1.0b","deprecations":[],"diagnostics":[]}"#);
        let b = result(r#"{"ok":true,"output":"a1b","deprecations":[],"diagnostics":[]}"#);
        assert_eq!(compare(&a, &a), Verdict::Same);
        match compare(&a, &b) {
            Verdict::Mismatch(m) => assert_eq!(m.signature(), r#"output:missing [".N" > ""]"#),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn errors_and_deviations() {
        let php = result(
            r#"{"ok":false,"phase":"render","error":{"class":"TypeError","message":"f(): Argument #1 ($a) must be of type int, string given, called in /tmp/x.php on line 12"},"deprecations":[],"diagnostics":[]}"#,
        );
        let rs = result(
            r#"{"ok":false,"phase":"render","error":{"class":"TypeError","message":"f(): Argument #1 ($a) must be of type int, string given"},"deprecations":[],"diagnostics":[]}"#,
        );
        assert_eq!(
            compare(&php, &rs),
            Verdict::Deviation(vec!["called-in-suffix"])
        );
        let other = result(
            r#"{"ok":false,"phase":"load","error":{"class":"Twig\\Error\\SyntaxError","message":"Unexpected token \"name\" of value \"foo\" in \"index.twig\" at line 3.","rawMessage":"x","templateName":"index.twig","templateLine":3},"deprecations":[],"diagnostics":[]}"#,
        );
        match compare(&php, &other) {
            Verdict::Mismatch(m) => {
                let sig = m.signature();
                assert!(
                    sig.starts_with(
                        "phase:render>load | error.class:TypeError>Twig\\Error\\SyntaxError"
                    ),
                    "{sig}"
                );
                assert!(
                    sig.contains("Unexpected token \"_\" of value \"_\" in \"_\" at line N."),
                    "{sig}"
                );
            }
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn shapes_and_rules() {
        assert_eq!(
            shape("Variable \"x\" does not exist in \"a.twig\" at line 12."),
            "Variable \"_\" does not exist in \"_\" at line N."
        );
        assert_eq!(shape("it's 'a' 1.5"), "it's '_' N");
        assert_eq!(
            object_handles("object(Foo)#12 (0) {}"),
            "object(Foo)#N (0) {}"
        );
        assert_eq!(plain_closure_names("{closure:/a.php:3}()"), "{closure}()");
        assert_eq!(
            strip_passed_in(
                "Too few arguments to function {closure}(), 1 passed in /x.php on line 4 and exactly 2 expected"
            ),
            "Too few arguments to function {closure}(), 1 passed and exactly 2 expected"
        );
        assert_eq!(diff_core("abcXdef", "abcdef"), ("X", ""));
        assert_eq!(
            list_signature("d", &["a 1".into()], &["b".into()]),
            "d:+b -a N"
        );
        assert_eq!(
            family(
                "error.message:TypeError [a | b > c] | diagnostics:-E_DEPRECATED: x y +E_WARNING: z"
            ),
            "error.message:TypeError | diagnostics:+E_WARNING,-E_DEPRECATED"
        );
    }
}
