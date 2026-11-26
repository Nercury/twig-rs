//! Field-by-field comparison of an oracle result (`expected`) with a twig-rs result (`actual`).
//!
//! Compared: `ok`; the output bytes; the failing `phase`; the error's class, message, raw message,
//! template name, line and column and its `previous` chain; the deprecations; the diagnostics
//! (type and message, in order; the oracle's `silenced` ones are skipped: PHP raised them under
//! `@`, which only Twig's filesystem cache uses). Not compared: `formatted` (class + message),
//! `code` (twig-rs does not model exception codes, `reference/deviations.md`), the typed-data
//! extras.
//!
//! Messages are compared the way twig-rs produces them: invalid UTF-8 in a PHP message is decoded
//! lossily (`reference/deviations.md`, "Non-UTF-8 bytes in error and deprecation messages").
//!
//! When fields differ, the [deviation rules](DEVIATION_RULES) (known, accepted differences listed
//! in `reference/deviations.md`) are applied to both sides; a difference that disappears is a
//! [`Verdict::Deviation`], not a mismatch. Outputs go through the rules byte for byte (each byte
//! read as one character), so outputs that are not valid UTF-8 get them too. The rule
//! [`SILENCED_COMPILE_WARNING`] works on the diagnostics lists instead of texts. Two more
//! deviations are recognised by twig-rs's own error texts: `memory-limit` (twig-rs's stand-in for
//! PHP's 128M limit, `Allowed memory size of ...`, while the oracle runs with another limit) and
//! `unsupported-by-twig-rs` (`... not supported by twig-rs`: charsets, `TemplateWrapper` methods).
//! An error of a PHP class port of `twig-test-support` (`... is not supported by
//! twig-test-support`) makes the case [not comparable](Verdict::Skip).
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
        // "Object handles": the length of a dumped string that holds a dumped handle (before
        // `object-handle` rewrites the handle digits it checks the length against)
        name: "dump-string-length",
        applies_to: &["output"],
        rewrite: dump_string_lengths,
    },
    DeviationRule {
        // "Object handles": var_dump() handle numbers
        name: "object-handle",
        applies_to: &["message", "output"],
        rewrite: object_handles,
    },
];

/// The rule for "Warnings of compiled PHP code" (`reference/deviations.md`, Engine) under a
/// filesystem cache. PHP raises a warning while it compiles the generated class, where it folds
/// constants (`unexpected NAN value was coerced to string` of a folded concatenation, an undefined
/// key of a constant array literal), and `FilesystemCache::load()` includes the class with
/// `@include_once`, so the oracle records the warning as `silenced` and nothing raises it again;
/// twig-rs raises it whenever the expression runs (zero, one or several times, during the render).
///
/// The rule drops the twig-rs diagnostics whose type and message equal a silenced diagnostic of the
/// oracle, wherever they come, except those that pair up, in order, with the oracle's own
/// diagnostics (a warning PHP raised at runtime with the same text still has to match one of
/// twig-rs's). With `"errors": "throw"` twig-rs stops at such a warning (an `ErrorException`, as a
/// render error or as its previous one) where PHP went on: when PHP rendered to the end with a
/// silenced diagnostic of that message, and what twig-rs raised before it stopped is what PHP
/// raised up to there, the case is this deviation too.
pub const SILENCED_COMPILE_WARNING: &str = "silenced-compile-warning";

/// The deviation rules one comparison applies.
#[derive(Clone, Copy)]
struct Rules<'a> {
    /// Text rewrites ([`DEVIATION_RULES`]).
    text: &'a [&'a DeviationRule],
    /// Whether [`SILENCED_COMPILE_WARNING`] applies.
    silenced: bool,
}

impl Rules<'_> {
    /// No rule at all.
    const NONE: Rules<'static> = Rules {
        text: &[],
        silenced: false,
    };
}

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
    let raw = fields(expected, actual, Rules::NONE);
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
    if threw_silenced_warning(expected, actual) {
        return Verdict::Deviation(vec![SILENCED_COMPILE_WARNING]);
    }
    let text: Vec<&DeviationRule> = DEVIATION_RULES.iter().collect();
    let all = Rules {
        text: &text,
        silenced: true,
    };
    let normalised = fields(expected, actual, all);
    if normalised.is_empty() {
        // name the rules that were needed
        let mut needed: Vec<&'static str> = DEVIATION_RULES
            .iter()
            .filter(|rule| {
                let without: Vec<&DeviationRule> = text
                    .iter()
                    .copied()
                    .filter(|r| r.name != rule.name)
                    .collect();
                !fields(
                    expected,
                    actual,
                    Rules {
                        text: &without,
                        silenced: true,
                    },
                )
                .is_empty()
            })
            .map(|rule| rule.name)
            .collect();
        let without_silenced = Rules {
            text: &text,
            silenced: false,
        };
        if !fields(expected, actual, without_silenced).is_empty() {
            needed.push(SILENCED_COMPILE_WARNING);
        }
        return Verdict::Deviation(if needed.is_empty() {
            DEVIATION_RULES
                .iter()
                .map(|r| r.name)
                .chain([SILENCED_COMPILE_WARNING])
                .collect()
        } else {
            needed
        });
    }
    Verdict::Mismatch(Mismatch { fields: normalised })
}

/// [`SILENCED_COMPILE_WARNING`] with `"errors": "throw"`: PHP rendered to the end, and twig-rs
/// stopped at an `ErrorException` (the render error or one of its previous errors) whose message is
/// that of a silenced diagnostic of the oracle. What twig-rs raised before it stopped must be what
/// PHP raised up to there: its deprecations a prefix of the oracle's, its diagnostics a prefix of
/// the oracle's unsilenced ones followed by the warning itself (recorded, then thrown).
fn threw_silenced_warning(expected: &CaseResult, actual: &CaseResult) -> bool {
    if !expected.ok || actual.ok {
        return false;
    }
    let mut error = actual.error.as_ref();
    let message = loop {
        match error {
            Some(e) if e.class == "ErrorException" => break lossy(&e.message),
            Some(e) => error = e.previous.as_deref(),
            None => return false,
        }
    };
    let is_silenced = |d: &&Diag| d.silenced && d.message == message;
    let Some(silenced) = expected.diagnostics.iter().find(is_silenced) else {
        return false;
    };
    let want: Vec<&Diag> = expected
        .diagnostics
        .iter()
        .filter(|d| !d.silenced)
        .collect();
    let Some((last, before)) = actual.diagnostics.split_last() else {
        return false;
    };
    let same = |a: &Diag, b: &Diag| a.kind == b.kind && a.message == b.message;
    same(last, silenced)
        && before.len() <= want.len()
        && before.iter().zip(&want).all(|(a, b)| same(a, b))
        && expected.deprecations.starts_with(&actual.deprecations)
}

fn apply(rules: Rules<'_>, kind: &str, text: &str) -> String {
    let mut out = text.to_owned();
    for rule in rules.text {
        if rule.applies_to.contains(&kind) {
            out = (rule.rewrite)(&out);
        }
    }
    out
}

/// The twig-rs diagnostics `got` without those [`SILENCED_COMPILE_WARNING`] accepts: an entry
/// equal to one of `silenced` is dropped unless it is the next one of the oracle's own diagnostics
/// (`want`, in order). Pairing an entry with `want` as early as possible is never worse than
/// dropping it: a later equal entry can be dropped instead.
fn without_silenced(want: &[String], silenced: &[String], got: Vec<String>) -> Vec<String> {
    let mut next = 0;
    let mut out = Vec::with_capacity(got.len());
    for entry in got {
        if want.get(next) == Some(&entry) {
            next += 1;
            out.push(entry);
        } else if !silenced.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The bytes as a string of one character per byte (U+0000 to U+00FF): a bijection, so texts are
/// equal exactly when the bytes are, and the rules' ASCII patterns match as they would on bytes.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
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

fn fields(expected: &CaseResult, actual: &CaseResult, rules: Rules<'_>) -> Vec<FieldDiff> {
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
            // the rules rewrite ASCII text: each byte is one character, so outputs that are not
            // valid UTF-8 get them too and the comparison stays exact
            let equal_after_rules = !rules.text.is_empty()
                && apply(rules, "output", &latin1(&want)) == apply(rules, "output", &latin1(&got));
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
    let mut got: Vec<String> = actual
        .diagnostics
        .iter()
        .filter(|d| !d.silenced)
        .map(diag_text)
        .collect();
    if rules.silenced && want != got {
        let silenced: Vec<String> = expected
            .diagnostics
            .iter()
            .filter(|d| d.silenced)
            .map(diag_text)
            .collect();
        got = without_silenced(&want, &silenced, got);
    }
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
    rules: Rules<'_>,
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

/// "Closure names and source locations in messages": PHP 8.4+ names a closure after where it is
/// declared (`zend_begin_func_decl()`, PHPSRC/Zend/zend_compile.c): `{closure:<file>:<line>}`
/// outside functions, `{closure:<Class>::<method>():<line>}` in a method and
/// `{closure:<name of the enclosing closure>:<line>}` in another closure, so the arrow functions of
/// a compiled template are `{closure:__TwigTemplate_<hash>::doDisplay():44}`, nested ones
/// `{closure:{closure:...}:47}`, and macro bodies sit in the closures of `loadDeclaredMacros()`.
/// Messages that name a function also give its scope (`zend_missing_arg_error()`,
/// PHPSRC/Zend/zend_execute.c: `Too few arguments to function %s%s%s()`), the template class for
/// these closures. twig-rs writes `{closure}` without a scope: every closure name becomes
/// `{closure}` (braces matched, so nested names go whole), and the scope of a closure declared in a
/// compiled template class (`__TwigTemplate_<hash>::`, `___<n>` for an embedded template) is
/// dropped.
fn plain_closure_names(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{closure:") {
        let Some(len) = braced_len(&rest[start..]) else {
            break;
        };
        out.push_str(without_template_scope(&rest[..start]));
        out.push_str("{closure}");
        rest = &rest[start + len..];
    }
    out.push_str(rest);
    out
}

/// The length of the brace group `text` starts with (nested groups included).
fn braced_len(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in text.bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// `text` without a trailing `__TwigTemplate_<hex>::` / `__TwigTemplate_<hex>___<n>::`.
fn without_template_scope(text: &str) -> &str {
    let Some(before) = text.strip_suffix("::") else {
        return text;
    };
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
        .last()
        .map_or(before.len(), |(i, _)| i);
    if is_template_class(&before[start..]) {
        &text[..start]
    } else {
        text
    }
}

/// `Environment::getTemplateClass()` names: `__TwigTemplate_` and a hash, plus `___<index>` for an
/// embedded template (`Environment::loadTemplate()`).
fn is_template_class(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("__TwigTemplate_") else {
        return false;
    };
    let (hash, index) = match rest.split_once("___") {
        Some((hash, index)) => (hash, Some(index)),
        None => (rest, None),
    };
    !hash.is_empty()
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && index.is_none_or(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit()))
}

/// How `(`, `)` and `#` look in a `var_dump()` printed through an escaping strategy: as they are
/// (no escaping, `html`), `html_attr` / `html_attr_relaxed` (`&#x23;`), `js` (`#`), `css`
/// (`\23 `) and `url` (`%23`): `EscaperRuntime::escape()` (TWIG/src/Runtime/EscaperRuntime.php).
const DUMP_PUNCTUATION: &[(&str, &str, &str)] = &[
    ("(", ")", "#"),
    ("&#x28;", "&#x29;", "&#x23;"),
    ("\\u0028", "\\u0029", "\\u0023"),
    ("\\28 ", "\\29 ", "\\23 "),
    ("%28", "%29", "%23"),
];

/// "Object handles" inside a derived value: `var_dump()` prints a string as
/// `string(<byte length>) "<bytes>"` (`php_var_dump()`, PHPSRC/ext/standard/var.c), so the dump of
/// a string that holds the dump of an object (the `Markup` of a `{% set %}` capture of `dump()`)
/// has a length that depends on the number of digits of the inner handles. A length that equals
/// the length of the quoted bytes after it, when those bytes hold an object handle (text that
/// [`object_handles`] changes), becomes `N`, and the strings nested in them get the same treatment.
/// A length that does not fit its content stays, so a wrong length still shows. For outputs, whose
/// characters are bytes (not for messages, whose characters are UTF-8).
fn dump_string_lengths(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    rewrite_dump_strings(&chars, &mut out);
    out
}

fn rewrite_dump_strings(chars: &[char], out: &mut String) {
    const OPEN: &[char] = &['s', 't', 'r', 'i', 'n', 'g', '('];
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(OPEN) {
            let digits_start = i + OPEN.len();
            let digits = chars[digits_start..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
            let after = digits_start + digits;
            let length: Option<usize> = chars[digits_start..after]
                .iter()
                .collect::<String>()
                .parse()
                .ok();
            if let Some(length) = length
                && chars[after..].starts_with(&[')', ' ', '"'])
                && let Some(end) = (after + 3).checked_add(length)
                && chars.get(end) == Some(&'"')
            {
                let start = after + 3;
                let content: String = chars[start..end].iter().collect();
                if object_handles(&content) != content {
                    out.push_str("string(N) \"");
                    rewrite_dump_strings(&chars[start..end], out);
                    out.push('"');
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
}

/// "Object handles": the `#<handle>` after `object(<class>)` in `var_dump()` output
/// (`php_var_dump()`, PHPSRC/ext/standard/var.c) becomes `#N`, also when an escaping strategy
/// rewrote the punctuation ([`DUMP_PUNCTUATION`]) and after case filters (`|upper`, `|title`:
/// `OBJECT(`, `Object(`); `enum(Foo::Bar)` has no handle.
fn object_handles(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    // where `object(` first appears, per spelling: a handle needs one before it
    let first_object: Vec<Option<usize>> = DUMP_PUNCTUATION
        .iter()
        .map(|(open, _, _)| lower.find(&format!("object{open}")))
        .collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut last = 0;
    while i < bytes.len() {
        let found =
            DUMP_PUNCTUATION
                .iter()
                .zip(&first_object)
                .find_map(|((_, close, hash), object)| {
                    let marker = close.len() + hash.len();
                    let rest = &bytes[i..];
                    if !(rest.starts_with(close.as_bytes())
                        && rest[close.len()..].starts_with(hash.as_bytes())
                        && object.is_some_and(|o| o < i))
                    {
                        return None;
                    }
                    let digits = rest[marker..]
                        .iter()
                        .take_while(|b| b.is_ascii_digit())
                        .count();
                    (digits > 0).then_some((marker, digits))
                });
        match found {
            Some((marker, digits)) => {
                out.push_str(&text[last..i + marker]);
                out.push('N');
                i += marker + digits;
                last = i;
            }
            None => i += 1,
        }
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

    #[test]
    fn closure_names_of_compiled_templates() {
        // the forms the oracle prints (tests/vectors/deviations.json)
        let class = "__TwigTemplate_10e02d70964b1815a5011b3d0ffa02aa";
        for name in [
            format!("{class}::{{closure:{class}::doDisplay():44}}"),
            format!("{class}::{{closure:{{closure:{class}::doDisplay():47}}:47}}"),
            format!(
                "{class}::{{closure:{{closure:{{closure:{class}::loadDeclaredMacros():54}}:62}}:63}}"
            ),
            format!("{class}___1::{{closure:{class}___1::block_b():120}}"),
            "{closure:/x/y.php:3}".to_owned(),
        ] {
            assert_eq!(
                plain_closure_names(&format!(
                    "Too few arguments to function {name}(), 2 passed and exactly 3 expected"
                )),
                "Too few arguments to function {closure}(), 2 passed and exactly 3 expected",
                "{name}"
            );
        }
        // other scopes stay; an unbalanced name is left alone
        assert_eq!(
            plain_closure_names("Foo::{closure:Foo::bar():3}() and {closure:x"),
            "Foo::{closure}() and {closure:x"
        );
        assert!(!is_template_class("__TwigTemplate_"));
        assert!(!is_template_class("__TwigTemplate_abc___"));
        assert!(is_template_class("__TwigTemplate_abc___12"));
    }

    #[test]
    fn object_handles_through_escaping_and_case_filters() {
        for (php, rs) in [
            ("object(stdClass)#217 (0) {", "object(stdClass)#7 (0) {"),
            (
                "object&#x28;stdClass&#x29;&#x23;217&#x20;&#x28;0&#x29;",
                "object&#x28;stdClass&#x29;&#x23;7&#x20;&#x28;0&#x29;",
            ),
            (
                "object\\u0028stdClass\\u0029\\u0023217\\u0020",
                "object\\u0028stdClass\\u0029\\u00239\\u0020",
            ),
            (
                "object\\28 stdClass\\29 \\23 217\\20 \\28 0\\29 ",
                "object\\28 stdClass\\29 \\23 11\\20 \\28 0\\29 ",
            ),
            (
                "object%28stdClass%29%23217%20%280%29",
                "object%28stdClass%29%2313%20%280%29",
            ),
            ("Object(Stdclass)#1123 (0) {", "Object(Stdclass)#15 (0) {"),
            ("OBJECT(STDCLASS)#1302 (0) {", "OBJECT(STDCLASS)#17 (0) {"),
        ] {
            assert_eq!(object_handles(php), object_handles(rs), "{php}");
            assert_ne!(php, rs);
        }
        // no object before it, or no digits: not a handle
        assert_eq!(object_handles("f(x)#12"), "f(x)#12");
        assert_eq!(object_handles("object(a) (b)#x"), "object(a) (b)#x");
        assert_eq!(object_handles("enum(Suit::Hearts)"), "enum(Suit::Hearts)");
    }

    #[test]
    fn rules_apply_to_outputs_that_are_not_utf8() {
        let php = CaseResult::success(Json::Null, b"object(stdClass)#1654 (0) {\n}\n\xff".to_vec());
        let rs = CaseResult::success(Json::Null, b"object(stdClass)#21 (0) {\n}\n\xff".to_vec());
        assert_eq!(
            compare(&php, &rs),
            Verdict::Deviation(vec!["object-handle"])
        );
        // the bytes themselves still count
        let other = CaseResult::success(Json::Null, b"object(stdClass)#21 (0) {\n}\n\xfe".to_vec());
        assert!(matches!(compare(&php, &other), Verdict::Mismatch(_)));
    }

    #[test]
    fn lengths_of_dumped_strings_that_hold_handles() {
        let markup = |handle: &str| {
            let inner = format!("object(stdClass){handle} (0) {{\n}}\n");
            format!(
                "object(Twig\\Markup)#3 (2) {{\n  [\"content\":protected]=>\n  string({}) \"{inner}\"\n  [\"charset\":protected]=>\n  string(5) \"UTF-8\"\n}}\n",
                inner.len()
            )
        };
        let php = CaseResult::success(Json::Null, markup("#1210").into_bytes());
        let rs = CaseResult::success(Json::Null, markup("#7").into_bytes());
        assert_eq!(
            compare(&php, &rs),
            Verdict::Deviation(vec!["dump-string-length", "object-handle"])
        );
        // nested: the dump of a string holding the dump of a Markup holding a dump
        let nested = |handle: &str| {
            let inner = markup(handle);
            format!("string({}) \"{inner}\"\n", inner.len())
        };
        let rewritten = dump_string_lengths(&nested("#1210"));
        assert_eq!(rewritten.matches("string(N) \"").count(), 2, "{rewritten}");
        assert!(rewritten.contains("string(5) \"UTF-8\""), "{rewritten}");
        assert_eq!(
            object_handles(&rewritten),
            object_handles(&dump_string_lengths(&nested("#7")))
        );
        // a length that does not fit its bytes, or bytes without a handle, stay as they are
        let wrong = "string(99) \"object(stdClass)#1 (0) {}\"";
        assert_eq!(dump_string_lengths(wrong), wrong);
        let plain = "string(3) \"abc\" string(1) \"#\"";
        assert_eq!(dump_string_lengths(plain), plain);
        let fitting = markup("#7");
        let length = "object(stdClass)#7 (0) {\n}\n".len();
        let bad = CaseResult::success(
            Json::Null,
            fitting
                .replace(
                    &format!("string({length})"),
                    &format!("string({})", length - 1),
                )
                .into_bytes(),
        );
        assert!(matches!(compare(&php, &bad), Verdict::Mismatch(_)));
    }

    #[test]
    fn warnings_a_filesystem_cache_silences() {
        // tests/vectors/fuzz/expressions/round3-compile-warning.json
        let php = result(
            r#"{"ok":true,"output":"NAN]","deprecations":[],"diagnostics":[{"type":"E_WARNING","message":"unexpected NAN value was coerced to string","silenced":true}]}"#,
        );
        let once = result(
            r#"{"ok":true,"output":"NAN]","deprecations":[],"diagnostics":[{"type":"E_WARNING","message":"unexpected NAN value was coerced to string"}]}"#,
        );
        assert_eq!(
            compare(&php, &once),
            Verdict::Deviation(vec![SILENCED_COMPILE_WARNING])
        );
        // raised in a loop, or never (a branch that does not run)
        let mut twice = once.clone();
        twice.diagnostics.push(twice.diagnostics[0].clone());
        assert_eq!(
            compare(&php, &twice),
            Verdict::Deviation(vec![SILENCED_COMPILE_WARNING])
        );
        let mut never = once.clone();
        never.diagnostics.clear();
        assert_eq!(compare(&php, &never), Verdict::Same);
        // another message, or another type, is no deviation
        let mut other = once.clone();
        other.diagnostics[0].message = "Undefined array key 1".to_owned();
        assert!(matches!(compare(&php, &other), Verdict::Mismatch(_)));
        let mut notice = once.clone();
        notice.diagnostics[0].kind = "E_NOTICE".to_owned();
        assert!(matches!(compare(&php, &notice), Verdict::Mismatch(_)));
        // a warning PHP raised at runtime with the same text still has to be there
        let mut both = php.clone();
        both.diagnostics.push(Diag {
            kind: "E_WARNING".to_owned(),
            message: "unexpected NAN value was coerced to string".to_owned(),
            silenced: false,
        });
        assert_eq!(
            compare(&both, &twice),
            Verdict::Deviation(vec![SILENCED_COMPILE_WARNING])
        );
        assert_eq!(compare(&both, &once), Verdict::Same);
        assert!(matches!(compare(&both, &never), Verdict::Mismatch(_)));
        assert_eq!(
            without_silenced(
                &["b".into(), "a".into()],
                &["a".into()],
                vec!["a".into(), "b".into(), "a".into()]
            ),
            ["b", "a"]
        );
        // "errors": "throw": twig-rs stops at the warning where PHP went on
        let thrown = result(
            r#"{"ok":false,"phase":"render","error":{"class":"Twig\\Error\\RuntimeError","message":"An exception has been thrown during the rendering of a template (\"unexpected NAN value was coerced to string\") in \"index.twig\" at line 1.","rawMessage":"x","templateName":"index.twig","templateLine":1,"previous":{"class":"ErrorException","message":"unexpected NAN value was coerced to string"}},"deprecations":[],"diagnostics":[{"type":"E_WARNING","message":"unexpected NAN value was coerced to string"}]}"#,
        );
        assert_eq!(
            compare(&php, &thrown),
            Verdict::Deviation(vec![SILENCED_COMPILE_WARNING])
        );
        assert!(matches!(compare(&once, &thrown), Verdict::Mismatch(_)));
        assert_eq!(compare(&thrown, &thrown), Verdict::Same);
        // ... but only when PHP rendered to the end and twig-rs raised nothing else before
        let mut php_failed = php.clone();
        php_failed.ok = false;
        php_failed.phase = Some("render".to_owned());
        php_failed.error = thrown
            .error
            .as_ref()
            .and_then(|e| e.previous.as_deref())
            .cloned();
        php_failed.output = None;
        assert!(matches!(
            compare(&php_failed, &thrown),
            Verdict::Mismatch(_)
        ));
        let mut more = thrown.clone();
        more.diagnostics.insert(
            0,
            Diag {
                kind: "E_DEPRECATED".to_owned(),
                message: "x".to_owned(),
                silenced: false,
            },
        );
        assert!(matches!(compare(&php, &more), Verdict::Mismatch(_)));
    }
}
