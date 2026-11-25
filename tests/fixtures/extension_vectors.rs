//! The harness environment against the oracle: `tests/fixtures/vectors/test_extension.gen.json` renders templates in
//! the oracle's `integration` environment (exactly `IntegrationTestCase`'s setup, warnings and notices thrown); the
//! results are `test_extension.json`. Each case runs here through [`environment::build`] and must give the same
//! output or exception (class, message), the same deprecations and the same PHP diagnostics. The cases exercise
//! every callable of the ported test extensions (`tests/fixtures/extension.rs`) with edge-case inputs, the
//! undefined-callable callbacks and the rest of the harness environment (globals, `dump`, the sandbox policy,
//! `template_from_string`, auto-escaping, classes). Regenerate with `test_extension.gen.py` (its header says how).
//!
//! PHP's messages are compared without the locations twig-rs cannot reproduce (`reference/deviations.md`): the
//! `, called in <file> on line <n>` of userland type errors and the ` in <file> on line <n>` of `Too few arguments`.

use std::fmt;

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};
use twig::runtime::{Markup, Notice};
use twig::value::object::StdClass;
use twig::value::{Array, ArrayKey, Diagnostic, Level, PhpString, Value};
use twig_test_support::json::decode_base64;
use twig_test_support::{EnvironmentOptions, ErrorInfo};

use super::Suite;
use super::environment;
use super::parse::TemplateSection;
use super::runner::error_class;

/// The batch.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    cases: Vec<BatchCase>,
}

/// One case of the batch.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchCase {
    id: String,
    env: String,
    errors: String,
    #[serde(deserialize_with = "ordered_templates")]
    templates: Vec<(String, String)>,
    #[serde(default)]
    config: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    data_php: Option<String>,
    #[serde(default)]
    data_rust: Vec<(String, serde_json::Value)>,
}

/// One oracle result.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleResult {
    id: String,
    kind: String,
    ok: bool,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    output_b64: Option<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    error: Option<ErrorInfo>,
    deprecations: Vec<String>,
    diagnostics: Vec<OracleDiagnostic>,
}

/// A PHP diagnostic as the oracle records it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleDiagnostic {
    #[serde(rename = "type")]
    kind: String,
    message: String,
    #[serde(default)]
    silenced: bool,
}

/// A JSON object of strings as its entries in document order (the templates are loaded in order).
fn ordered_templates<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, String)>, D::Error> {
    struct Entries;

    impl<'de> Visitor<'de> for Entries {
        type Value = Vec<(String, String)>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an object of strings")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries = Vec::new();
            while let Some(entry) = map.next_entry::<String, String>()? {
                entries.push(entry);
            }
            Ok(entries)
        }
    }

    d.deserialize_map(Entries)
}

/// Replays every case.
pub fn check(suite: &Suite) -> Result<(), String> {
    let read = |name: &str| {
        let path = suite.root.join("tests/fixtures/vectors").join(name);
        std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
    };
    let batch: Batch = serde_json::from_str(&read("test_extension.gen.json")?)
        .map_err(|e| format!("invalid test_extension.gen.json: {e}"))?;
    let results: Vec<OracleResult> = serde_json::from_str(&read("test_extension.json")?)
        .map_err(|e| format!("invalid test_extension.json: {e}"))?;
    if batch.cases.len() != results.len() || batch.cases.len() < 80 {
        return Err(format!(
            "{} cases and {} results",
            batch.cases.len(),
            results.len()
        ));
    }
    let mut failures = Vec::new();
    for (case, result) in batch.cases.iter().zip(&results) {
        if let Err(problem) = run_case(case, result) {
            failures.push(format!("{}: {problem}", case.id));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} of {} cases differ from the oracle:\n{}",
            failures.len(),
            batch.cases.len(),
            failures.join("\n")
        ))
    }
}

/// A JSON value of `data_rust` or `config` as a PHP value.
fn json_value(json: &serde_json::Value) -> Result<Value, String> {
    use serde_json::Value as J;
    Ok(match json {
        J::Null => Value::Null,
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().ok_or_else(|| format!("number {n}"))?),
        },
        J::String(s) => Value::from(s.as_str()),
        J::Array(items) => Value::Array(Array::from_values(
            items
                .iter()
                .map(json_value)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        J::Object(map) => match map.iter().next() {
            Some((marker, J::String(content))) if map.len() == 1 && marker == "$markup" => {
                Markup::new_value(PhpString::from(content.as_str()), "UTF-8")
            }
            Some((marker, _)) if map.len() == 1 && marker == "$stdClass" => {
                Value::Object(StdClass::new().into_ref())
            }
            _ => return Err(format!("unsupported JSON object {json}")),
        },
    })
}

/// The name of a diagnostic level as the oracle reports it.
fn level_name(level: Level) -> &'static str {
    match level {
        Level::Warning => "E_WARNING",
        Level::Notice => "E_NOTICE",
        Level::Deprecated => "E_DEPRECATED",
        Level::UserDeprecated => "E_USER_DEPRECATED",
    }
}

/// A PHP message without the locations twig-rs cannot reproduce: `, called in <file> on line <n>`
/// (`zend_verify_arg_error()`) and the ` in <file> on line <n>` after `N passed` (`zend_missing_arg_error()`).
fn without_php_locations(message: &str) -> String {
    let mut out = message.to_owned();
    for marker in [", called in ", " passed in "] {
        let keep = if marker == " passed in " {
            " passed".len()
        } else {
            0
        };
        let mut from = 0;
        while let Some(found) = out[from..].find(marker) {
            let start = from + found;
            let Some(on_line) = out[start..].find(" on line ") else {
                break;
            };
            let digits_start = start + on_line + " on line ".len();
            let digits = out[digits_start..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            out.replace_range(start + keep..digits_start + digits, "");
            from = start + keep;
        }
    }
    out
}

/// Runs one case and compares it with the oracle.
fn run_case(case: &BatchCase, result: &OracleResult) -> Result<(), String> {
    if case.id != result.id || result.kind != "twig" {
        return Err(format!("result {} of kind {}", result.id, result.kind));
    }
    if case.env != "integration" || case.errors != "throw" {
        return Err(format!(
            "environment {} with errors {}",
            case.env, case.errors
        ));
    }
    if case.data_php.is_none() != case.data_rust.is_empty() {
        return Err("data_php and data_rust disagree".to_owned());
    }
    // `integration_environment()`: `array_merge(['cache' => false, 'strict_variables' => true], $config)`
    let mut options = Array::new();
    options.insert("cache", false);
    options.insert("strict_variables", true);
    for (name, value) in &case.config {
        options.insert(
            ArrayKey::from_string(PhpString::from(name.as_str())),
            json_value(value)?,
        );
    }
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let resolved =
        EnvironmentOptions::from_options(&options, &mut diagnostics).map_err(|e| e.to_string())?;
    let templates: Vec<TemplateSection> = case
        .templates
        .iter()
        .map(|(name, source)| TemplateSection {
            name: name.as_bytes().to_vec(),
            source: source.as_bytes().to_vec(),
        })
        .collect();
    let env = environment::build(&templates, environment::options(&resolved)?)?;
    let mut context = Array::new();
    for (name, value) in &case.data_rust {
        context.insert(
            ArrayKey::from_string(PhpString::from(name.as_str())),
            json_value(value)?,
        );
    }
    // `$main = 'index.twig'` when it exists, else the first template; every template is loaded first, in order
    let main = case
        .templates
        .iter()
        .find(|(name, _)| name == "index.twig")
        .or_else(|| case.templates.first())
        .map(|(name, _)| name.clone())
        .ok_or("a case without templates")?;
    let loaded = templates
        .iter()
        .try_for_each(|t| env.load(&t.name_lossy()).map(|_| ()))
        .and_then(|()| env.load(&main));
    let (phase, rendered) = match loaded {
        Err(error) => ("load", Err(error)),
        Ok(template) => ("render", template.render_bytes(context)),
    };
    let notices = env.take_notices();

    let mut problems = Vec::new();
    match (&rendered, result.ok) {
        (Ok(output), true) => {
            let expected = match &result.output_b64 {
                Some(b64) => decode_base64(b64)?,
                None => result.output.clone().unwrap_or_default().into_bytes(),
            };
            if *output != expected {
                problems.push(format!(
                    "output {:?}, PHP {:?}",
                    String::from_utf8_lossy(output),
                    String::from_utf8_lossy(&expected)
                ));
            }
        }
        (Err(error), false) => {
            let php = result
                .error
                .as_ref()
                .ok_or("an error result without error")?;
            let expected = without_php_locations(&php.formatted_bytes().to_string_lossy());
            let actual = format!("{}: {error}", error_class(error));
            if actual != expected {
                problems.push(format!("exception {actual:?}, PHP {expected:?}"));
            }
            if result.phase.as_deref() != Some(phase) {
                problems.push(format!("{phase} phase, PHP {:?}", result.phase));
            }
        }
        (Ok(output), false) => problems.push(format!(
            "output {:?}, PHP throws {:?}",
            String::from_utf8_lossy(output),
            result.error.as_ref().map(|e| e.formatted.clone())
        )),
        (Err(error), true) => problems.push(format!(
            "{}: {error}, PHP outputs {:?}",
            error_class(error),
            result.output
        )),
    }
    let deprecations: Vec<String> = notices
        .iter()
        .filter_map(|notice| match notice {
            Notice::Deprecation(deprecation) => Some(deprecation.to_string()),
            Notice::Php(d) if d.level == Level::UserDeprecated => {
                Some(d.message.to_string_lossy().into_owned())
            }
            Notice::Php(_) => None,
        })
        .collect();
    if deprecations != result.deprecations {
        problems.push(format!(
            "deprecations {deprecations:?}, PHP {:?}",
            result.deprecations
        ));
    }
    let diagnostics: Vec<(String, String)> = notices
        .iter()
        .map(|notice| match notice {
            Notice::Php(d) => (
                level_name(d.level).to_owned(),
                d.message.to_string_lossy().into_owned(),
            ),
            Notice::Deprecation(d) => ("E_USER_DEPRECATED".to_owned(), d.to_string()),
        })
        .collect();
    let php_diagnostics: Vec<(String, String)> = result
        .diagnostics
        .iter()
        .filter(|d| !d.silenced)
        .map(|d| (d.kind.clone(), d.message.clone()))
        .collect();
    if diagnostics != php_diagnostics {
        problems.push(format!(
            "diagnostics {diagnostics:?}, PHP {php_diagnostics:?}"
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}
