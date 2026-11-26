//! Oracle "twig" cases (the `(a)` kind at the top of research/php-oracle/runner.php).
//!
//! A [`Case`] keeps the JSON object it was read from ([`Case::json`]): that object goes to the
//! oracle unchanged (plus `typed_data` where the driver needs it), and the twig-rs runner reads
//! the same fields the PHP runner reads.

use super::json::{Json, base64_decode};

/// Which environment the oracle builds (`"env"`; the oracle's default is `integration`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvKind {
    /// `new Environment(new ArrayLoader($templates), array_merge(['cache' => false], $config))`
    /// plus the requested extensions, globals and sandbox.
    Plain,
    /// The `IntegrationTest` harness environment (not supported by the twig-rs runner).
    Integration,
}

/// What PHP warnings and notices do (`"errors"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorsMode {
    /// Recorded and swallowed (the oracle's default).
    Collect,
    /// Converted to `ErrorException` (deprecations are still only recorded).
    Throw,
}

/// The context of a case.
#[derive(Clone, Debug, PartialEq)]
pub enum CaseData {
    /// No data: an empty context.
    None,
    /// `data_json`: decoded with `json_decode($json, true)`.
    Json(Json),
    /// `data_php`: PHP code; twig-rs uses the oracle's typed tree of its result (`typed_data`).
    Php(String),
}

/// A parsed oracle "twig" case.
#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    /// The original JSON object.
    pub json: Json,
    /// `"id"` as a display string (empty when missing).
    pub id: String,
    /// The templates in order: `"templates"`, then `"templates_b64"` (base64 sources, replacing a
    /// template of the same name in place).
    pub templates: Vec<(String, Vec<u8>)>,
    /// The template to render: `"main"`, else `index.twig` when present, else the first template.
    pub main: String,
    /// The data.
    pub data: CaseData,
    /// The environment.
    pub env: EnvKind,
    /// `"errors"`.
    pub errors: ErrorsMode,
    /// `"load_all"` (default `true`).
    pub load_all: bool,
}

impl Case {
    /// Parses a case object.
    ///
    /// # Errors
    ///
    /// Not a twig case (`php`/`fixture` kinds), no templates, or malformed fields.
    pub fn from_json(json: &Json) -> Result<Case, String> {
        if !matches!(json, Json::Object(_)) {
            return Err("a case must be a JSON object".to_owned());
        }
        let kind = json.get("kind").and_then(Json::as_str);
        let inferred = if json.get("fixture").is_some() {
            "fixture"
        } else if json.get("php").is_some() || json.get("php_code").is_some() {
            "php"
        } else {
            "twig"
        };
        let kind = kind.unwrap_or(inferred);
        if kind != "twig" {
            return Err(format!("not a twig case (kind \"{kind}\")"));
        }
        let id = match json.get("id") {
            None | Some(Json::Null) => String::new(),
            Some(Json::String(s)) => s.clone(),
            Some(other) => other.to_compact(),
        };
        let mut templates: Vec<(String, Vec<u8>)> = Vec::new();
        for (name, source) in json.get("templates").map(Json::entries).unwrap_or_default() {
            let source = match source {
                Json::String(s) => s.as_bytes().to_vec(),
                Json::Number(n) => n.as_bytes().to_vec(),
                other => return Err(format!("template {name:?} is not a string: {other:?}")),
            };
            set_template(&mut templates, name, source);
        }
        for (name, source) in json
            .get("templates_b64")
            .map(Json::entries)
            .unwrap_or_default()
        {
            let source = source
                .as_str()
                .ok_or_else(|| format!("templates_b64 {name:?} is not a string"))
                .and_then(base64_decode)?;
            set_template(&mut templates, name, source);
        }
        if templates.is_empty() {
            return Err("a twig case needs a non-empty \"templates\" object".to_owned());
        }
        let main = match json.get_non_null("main") {
            Some(Json::String(s)) => s.clone(),
            Some(other) => other.to_compact(),
            None if templates.iter().any(|(n, _)| n == "index.twig") => "index.twig".to_owned(),
            None => templates[0].0.clone(),
        };
        let data = match (json.get_non_null("data_php"), json.get("data_json")) {
            (Some(Json::String(code)), _) => CaseData::Php(code.clone()),
            (Some(other), _) => return Err(format!("data_php is not a string: {other:?}")),
            (None, Some(Json::Null) | None) => CaseData::None,
            (None, Some(data)) => CaseData::Json(data.clone()),
        };
        let env = match json.get("env").and_then(Json::as_str) {
            Some("plain") => EnvKind::Plain,
            _ => EnvKind::Integration,
        };
        let errors = match json.get("errors").and_then(Json::as_str) {
            Some("throw") => ErrorsMode::Throw,
            _ => ErrorsMode::Collect,
        };
        let load_all = json.get_non_null("load_all").is_none_or(Json::truthy);
        Ok(Case {
            json: json.clone(),
            id,
            templates,
            main,
            data,
            env,
            errors,
            load_all,
        })
    }

    /// A field of the case object.
    pub fn field(&self, key: &str) -> Option<&Json> {
        self.json.get_non_null(key)
    }

    /// The source of a template.
    pub fn template(&self, name: &str) -> Option<&[u8]> {
        self.templates
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.as_slice())
    }

    /// The case object with `templates` replaced. A source that is not valid UTF-8 goes to
    /// `templates_b64`, with an empty placeholder in `templates` that keeps the template order.
    pub fn with_templates(&self, templates: &[(String, Vec<u8>)]) -> Json {
        let mut json = self.json.clone();
        let mut text = Vec::new();
        let mut binary = Vec::new();
        for (name, source) in templates {
            match std::str::from_utf8(source) {
                Ok(s) => text.push((name.clone(), Json::str(s))),
                Err(_) => {
                    text.push((name.clone(), Json::str("")));
                    binary.push((name.clone(), Json::str(super::json::base64_encode(source))));
                }
            }
        }
        // in place, so that the case keeps its field order
        json.set("templates", Json::Object(text));
        if binary.is_empty() {
            json.remove("templates_b64");
        } else {
            json.set("templates_b64", Json::Object(binary));
        }
        json
    }

    /// Whether the twig-rs runner can run this case at all (see [`super::run`]).
    pub fn unsupported_reason(&self) -> Option<String> {
        if self.env != EnvKind::Plain {
            return Some(
                "the integration environment is not supported (set \"env\": \"plain\")".to_owned(),
            );
        }
        if self.field("config_php").is_some() {
            return Some("config_php is not supported (use \"config\")".to_owned());
        }
        None
    }
}

fn set_template(templates: &mut Vec<(String, Vec<u8>)>, name: &str, source: Vec<u8>) {
    match templates.iter_mut().find(|(n, _)| n == name) {
        Some(entry) => entry.1 = source,
        None => templates.push((name.to_owned(), source)),
    }
}

/// The cases of a batch file: a list of cases, `{"cases": [...], "isolation": ...}`, or one case.
/// Returns the case objects and the batch's isolation mode.
pub fn batch_cases(batch: &Json) -> (Vec<Json>, Option<String>) {
    match batch {
        Json::Array(items) => (items.clone(), None),
        Json::Object(_) if batch.get("cases").is_some() => (
            batch
                .get("cases")
                .map(Json::items)
                .unwrap_or_default()
                .to_vec(),
            batch
                .get("isolation")
                .and_then(Json::as_str)
                .map(str::to_owned),
        ),
        other => (vec![other.clone()], None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_the_runner() {
        let json = Json::parse(
            r#"{"id":7,"env":"plain","templates":{"a.twig":"x","index.twig":"y"},
                "templates_b64":{"a.twig":"/w=="},"data_json":[1],"errors":"throw","load_all":0}"#,
        )
        .unwrap();
        let case = Case::from_json(&json).unwrap();
        assert_eq!(case.id, "7");
        assert_eq!(case.main, "index.twig");
        assert_eq!(case.template("a.twig"), Some(&b"\xff"[..]));
        assert_eq!(case.templates[0].0, "a.twig");
        assert_eq!(case.env, EnvKind::Plain);
        assert_eq!(case.errors, ErrorsMode::Throw);
        assert!(!case.load_all);
        assert!(matches!(case.data, CaseData::Json(_)));
        let rebuilt = Case::from_json(&case.with_templates(&case.templates)).unwrap();
        assert_eq!(rebuilt.templates, case.templates);
        let php = Json::parse(r#"{"php":"1"}"#).unwrap();
        assert!(Case::from_json(&php).is_err());
    }
}
