//! The oracle's result schema for "twig" cases, read from oracle output and written for twig-rs.
//!
//! ```text
//! {"id", "kind": "twig", "ok": true,  "output" (+ "output_b64"), "deprecations": [...], "diagnostics": [...]}
//! {"id", "kind": "twig", "ok": false, "phase": "setup"|"load"|"data"|"render",
//!  "error": {"class", "message", "formatted", "rawMessage", "templateName", "templateLine",
//!            "templateColumn", "code", "previous"}, "deprecations": [...], "diagnostics": [...]}
//! ```
//!
//! Strings that are not valid UTF-8 carry a `<field>_b64` copy (lib.php's `put_string()`); the
//! model keeps the exact bytes. With `typed_data` the oracle adds `data` (the typed tree of the
//! evaluated `data_php`), `state` (PHP and `CoreExtension` state after the data phase) and
//! `data_diagnostics` / `data_deprecations` (what the data code raised); with `clock` it adds `clock`
//! (`[sec, usec]` of `gettimeofday()` right before rendering). The model keeps them as JSON. A dead oracle process reports `phase` `fatal` (the case that died) or `lost` (cases that
//! never ran).

use super::json::{Json, get_bytes, put_bytes};

/// A recorded diagnostic (`{"type", "message", "silenced"?}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    /// `E_WARNING`, `E_NOTICE`, `E_DEPRECATED`, `E_USER_DEPRECATED`, ...
    pub kind: String,
    /// The message.
    pub message: String,
    /// Raised under `@` (only Twig's filesystem cache does that).
    pub silenced: bool,
}

impl Diag {
    fn from_json(json: &Json) -> Diag {
        Diag {
            kind: json
                .get("type")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_owned(),
            message: json
                .get("message")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_owned(),
            silenced: json.get("silenced").is_some_and(Json::truthy),
        }
    }

    fn to_json(&self) -> Json {
        let mut fields = vec![
            ("type".to_owned(), Json::str(&self.kind)),
            ("message".to_owned(), Json::str(&self.message)),
        ];
        if self.silenced {
            fields.push(("silenced".to_owned(), Json::Bool(true)));
        }
        Json::Object(fields)
    }
}

/// A PHP throwable as `describe_throwable()` records it.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorDesc {
    /// The class (`Twig\Error\SyntaxError`, `ErrorException`, ...).
    pub class: String,
    /// `getMessage()`, exact bytes.
    pub message: Vec<u8>,
    /// Twig errors: what `getRawMessage()` and friends return.
    pub twig: Option<TwigErrorInfo>,
    /// Other throwables: `getCode()` (not compared: twig-rs does not model codes).
    pub code: Option<Json>,
    /// `getPrevious()`.
    pub previous: Option<Box<ErrorDesc>>,
}

/// The template information of a Twig error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TwigErrorInfo {
    /// `getRawMessage()`, exact bytes.
    pub raw_message: Vec<u8>,
    /// `getSourceContext()?->getName()`.
    pub template_name: Option<String>,
    /// `getTemplateLine()`.
    pub template_line: i64,
    /// `getTemplateColumn()`.
    pub template_column: Option<i64>,
}

impl ErrorDesc {
    /// Reads an `error` object.
    pub fn from_json(json: &Json) -> ErrorDesc {
        let twig = json.get("rawMessage").map(|_| TwigErrorInfo {
            raw_message: get_bytes(json, "rawMessage").unwrap_or_default(),
            template_name: json.get("templateName").and_then(|n| match n {
                Json::String(s) => Some(s.clone()),
                Json::Number(n) => Some(n.clone()),
                _ => None,
            }),
            template_line: json
                .get("templateLine")
                .and_then(Json::as_i64)
                .unwrap_or(-1),
            template_column: json.get("templateColumn").and_then(Json::as_i64),
        });
        ErrorDesc {
            class: json
                .get("class")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_owned(),
            message: get_bytes(json, "message").unwrap_or_default(),
            twig,
            code: json.get("code").cloned(),
            previous: json
                .get_non_null("previous")
                .map(|p| Box::new(ErrorDesc::from_json(p))),
        }
    }

    /// Writes the `error` object in `describe_throwable()`'s field order.
    pub fn to_json(&self) -> Json {
        let mut fields = vec![("class".to_owned(), Json::str(&self.class))];
        put_bytes(&mut fields, "message", &self.message);
        let mut formatted = format!("{}: ", self.class).into_bytes();
        formatted.extend_from_slice(&self.message);
        put_bytes(&mut fields, "formatted", &formatted);
        match &self.twig {
            Some(twig) => {
                put_bytes(&mut fields, "rawMessage", &twig.raw_message);
                fields.push((
                    "templateName".to_owned(),
                    twig.template_name.clone().map_or(Json::Null, Json::String),
                ));
                fields.push(("templateLine".to_owned(), Json::int(twig.template_line)));
                fields.push((
                    "templateColumn".to_owned(),
                    twig.template_column.map_or(Json::Null, Json::int),
                ));
            }
            None => {
                if let Some(code) = &self.code {
                    fields.push(("code".to_owned(), code.clone()));
                }
            }
        }
        if let Some(previous) = &self.previous {
            fields.push(("previous".to_owned(), previous.to_json()));
        }
        Json::Object(fields)
    }

    /// `"<class>: <message>"` (lossy), for reports.
    pub fn formatted(&self) -> String {
        format!("{}: {}", self.class, String::from_utf8_lossy(&self.message))
    }
}

/// The result of one "twig" case.
#[derive(Clone, Debug, PartialEq)]
pub struct CaseResult {
    /// `"id"` as given by the case.
    pub id: Json,
    /// Whether the render succeeded.
    pub ok: bool,
    /// Where it failed: `setup`, `load`, `data`, `render` (oracle failures: `fatal`, `lost`,
    /// `oracle`; twig-rs runner failures: `timeout`, `panic`).
    pub phase: Option<String>,
    /// The output, exact bytes.
    pub output: Option<Vec<u8>>,
    /// The error.
    pub error: Option<ErrorDesc>,
    /// The `E_USER_DEPRECATED` messages, in order.
    pub deprecations: Vec<String>,
    /// Every diagnostic, in order.
    pub diagnostics: Vec<Diag>,
    /// Further fields (`data`, `state`, `data_diagnostics`, ...), in order.
    pub extra: Vec<(String, Json)>,
}

impl CaseResult {
    /// A successful result.
    pub fn success(id: Json, output: Vec<u8>) -> CaseResult {
        CaseResult {
            id,
            ok: true,
            phase: None,
            output: Some(output),
            error: None,
            deprecations: Vec::new(),
            diagnostics: Vec::new(),
            extra: Vec::new(),
        }
    }

    /// A failed result.
    pub fn failure(id: Json, phase: &str, error: ErrorDesc) -> CaseResult {
        CaseResult {
            id,
            ok: false,
            phase: Some(phase.to_owned()),
            output: None,
            error: Some(error),
            deprecations: Vec::new(),
            diagnostics: Vec::new(),
            extra: Vec::new(),
        }
    }

    /// Reads a result object.
    pub fn from_json(json: &Json) -> CaseResult {
        const KNOWN: &[&str] = &[
            "id",
            "kind",
            "ok",
            "phase",
            "output",
            "output_b64",
            "error",
            "deprecations",
            "diagnostics",
        ];
        CaseResult {
            id: json.get("id").cloned().unwrap_or(Json::Null),
            ok: json.get("ok").is_some_and(Json::truthy),
            phase: json.get("phase").and_then(Json::as_str).map(str::to_owned),
            output: get_bytes(json, "output"),
            error: json.get_non_null("error").map(ErrorDesc::from_json),
            deprecations: json
                .get("deprecations")
                .map(Json::items)
                .unwrap_or_default()
                .iter()
                .map(|d| d.as_str().unwrap_or_default().to_owned())
                .collect(),
            diagnostics: json
                .get("diagnostics")
                .map(Json::items)
                .unwrap_or_default()
                .iter()
                .map(Diag::from_json)
                .collect(),
            extra: json
                .entries()
                .iter()
                .filter(|(k, _)| !KNOWN.contains(&k.as_str()))
                .cloned()
                .collect(),
        }
    }

    /// Writes the result in the oracle's field order.
    pub fn to_json(&self) -> Json {
        let mut fields = vec![
            ("id".to_owned(), self.id.clone()),
            ("kind".to_owned(), Json::str("twig")),
            ("ok".to_owned(), Json::Bool(self.ok)),
        ];
        if let Some(phase) = &self.phase {
            fields.push(("phase".to_owned(), Json::str(phase)));
        }
        if let Some(output) = &self.output {
            put_bytes(&mut fields, "output", output);
        }
        if let Some(error) = &self.error {
            fields.push(("error".to_owned(), error.to_json()));
        }
        fields.push((
            "deprecations".to_owned(),
            Json::Array(self.deprecations.iter().map(Json::str).collect()),
        ));
        fields.push((
            "diagnostics".to_owned(),
            Json::Array(self.diagnostics.iter().map(Diag::to_json).collect()),
        ));
        fields.extend(self.extra.iter().cloned());
        Json::Object(fields)
    }

    /// An extra field (`data`, `state`, ...).
    pub fn extra(&self, key: &str) -> Option<&Json> {
        self.extra
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .filter(|v| !v.is_null())
    }

    /// Whether the oracle itself failed for this case (`fatal`, `lost`, `oracle` phases): such
    /// results say nothing about Twig.
    pub fn is_oracle_failure(&self) -> bool {
        matches!(self.phase.as_deref(), Some("fatal" | "lost" | "oracle"))
    }

    /// A one-line summary for reports.
    pub fn summary(&self) -> String {
        match (&self.output, &self.error) {
            (Some(output), _) if self.ok => {
                format!("output {:?}", String::from_utf8_lossy(output))
            }
            (_, Some(error)) => format!(
                "{} error {}",
                self.phase.as_deref().unwrap_or("?"),
                error.formatted()
            ),
            _ => format!("ok={} phase={:?}", self.ok, self.phase),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_the_oracle_schema() {
        let text = r#"{"id":"x","kind":"twig","ok":false,"phase":"render","error":{"class":"Twig\\Error\\RuntimeError","message":"An exception has been thrown during the rendering of a template (\"?\") in \"index.twig\" at line 1.","message_b64":"QW4gZXhjZXB0aW9uIGhhcyBiZWVuIHRocm93biBkdXJpbmcgdGhlIHJlbmRlcmluZyBvZiBhIHRlbXBsYXRlICgi/yIpIGluICJpbmRleC50d2lnIiBhdCBsaW5lIDEu","formatted":"x","rawMessage":"r","templateName":"index.twig","templateLine":1,"templateColumn":null,"previous":{"class":"ErrorException","message":"m","formatted":"ErrorException: m","code":0}},"deprecations":[],"diagnostics":[{"type":"E_WARNING","message":"m"}],"data":{"t":"null"}}"#;
        let json = Json::parse(text).unwrap();
        let result = CaseResult::from_json(&json);
        assert!(!result.ok);
        let error = result.error.as_ref().unwrap();
        assert!(error.message.contains(&0xff));
        assert_eq!(error.twig.as_ref().unwrap().template_line, 1);
        assert_eq!(error.previous.as_ref().unwrap().class, "ErrorException");
        assert!(result.extra("data").is_some());
        let again = CaseResult::from_json(&result.to_json());
        assert_eq!(again, result);
    }
}
