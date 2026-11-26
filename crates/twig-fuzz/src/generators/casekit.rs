//! Case assembly shared by the `whitespace`, `unicode`, `dynamic` and `structure` families.
//!
//! - [`Builder`] turns templates, data and settings into an oracle "twig" case (`"env": "plain"`)
//!   with a filesystem cache directory of its own ([`super::with_case_cache`]); a template that is
//!   not valid UTF-8 goes to `templates_b64`, with an empty placeholder in `templates` that keeps
//!   the template order (what [`crate::kit::case::Case::with_templates`] writes).
//! - [`php_bytes`] writes a PHP string literal for `data_php`.
//! - [`batch`] wraps cases as a batch whose cases each run in a PHP process of their own: PHP keeps
//!   a compiled template class for the whole process, so a later case with the same template
//!   source and options would reuse it without its compile-time deprecations.

use std::fmt::Write as _;

use crate::kit::json::{Json, base64_encode};

pub use super::composition::Rng;

/// The seed of case `index` of the batch `seed` of a family (`salt` tells the families apart).
pub fn case_seed(seed: u64, index: u64, salt: u64) -> u64 {
    seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ index.wrapping_mul(0xD1B5_4A32_D192_ED03)
        ^ salt.wrapping_mul(0x94D0_49BB_1331_11EB)
}

/// `{"cases": cases, "isolation": "process"}` (see the module documentation).
pub fn batch(cases: Vec<Json>) -> Json {
    Json::Object(vec![
        ("cases".to_owned(), Json::Array(cases)),
        ("isolation".to_owned(), Json::str("process")),
    ])
}

/// A PHP double-quoted string literal whose value is exactly `bytes` (`\xHH` for everything
/// outside printable ASCII, `$` escaped so that nothing is interpolated).
pub fn php_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'$' => out.push_str("\\$"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('"');
    out
}

/// A PHP single-quoted string literal of the UTF-8 text `s`.
pub fn php_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// What a case is made of; [`Builder::build`] writes the case object.
#[derive(Clone, Debug, Default)]
pub struct Builder {
    /// The case id (also names the cache directory).
    pub id: String,
    /// The templates, in loading order.
    pub templates: Vec<(String, Vec<u8>)>,
    /// The template to render (default: `index.twig`).
    pub main: Option<String>,
    /// `data_json` (an object), unless `data_php` is set.
    pub data_json: Vec<(String, Json)>,
    /// `data_php`: PHP code returning the context.
    pub data_php: Option<String>,
    /// `"test_classes": true` (the helper classes of Twig's `tests/IntegrationTest.php`).
    pub test_classes: bool,
    /// Environment options.
    pub config: Vec<(String, Json)>,
    /// Extensions of the plain environment (`debug`, `string_loader`, `sandbox`).
    pub extensions: Vec<&'static str>,
    /// The `sandbox` field (policy and global flag).
    pub sandbox: Option<Json>,
    /// Globals.
    pub globals: Vec<(String, Json)>,
    /// `"errors": "throw"`.
    pub throw: bool,
    /// `"load_all"` when not the default (`true`).
    pub load_all: Option<bool>,
    /// The PHP default timezone of the case.
    pub timezone: Option<String>,
    /// The `lexer` options (`new \Twig\Lexer($twig, $options)`).
    pub lexer: Option<Json>,
}

impl Builder {
    /// A builder for the case `id` with no templates yet.
    pub fn new(id: impl Into<String>) -> Builder {
        Builder {
            id: id.into(),
            ..Builder::default()
        }
    }

    /// Adds (or replaces) a template.
    pub fn template(&mut self, name: &str, source: impl Into<Vec<u8>>) {
        let source = source.into();
        match self.templates.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = source,
            None => self.templates.push((name.to_owned(), source)),
        }
    }

    /// Sets an environment option.
    pub fn option(&mut self, key: &str, value: Json) {
        match self.config.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.config.push((key.to_owned(), value)),
        }
    }

    /// The case object (with a cache directory of its own).
    pub fn build(self) -> Json {
        let mut fields: Vec<(String, Json)> = vec![
            ("id".to_owned(), Json::str(self.id.clone())),
            ("env".to_owned(), Json::str("plain")),
        ];
        let mut text = Vec::new();
        let mut binary = Vec::new();
        for (name, source) in &self.templates {
            match std::str::from_utf8(source) {
                Ok(s) => text.push((name.clone(), Json::str(s))),
                Err(_) => {
                    text.push((name.clone(), Json::str("")));
                    binary.push((name.clone(), Json::str(base64_encode(source))));
                }
            }
        }
        fields.push(("templates".to_owned(), Json::Object(text)));
        if !binary.is_empty() {
            fields.push(("templates_b64".to_owned(), Json::Object(binary)));
        }
        if let Some(main) = &self.main {
            fields.push(("main".to_owned(), Json::str(main.clone())));
        }
        match &self.data_php {
            Some(code) => fields.push(("data_php".to_owned(), Json::str(code.clone()))),
            None if !self.data_json.is_empty() => {
                fields.push(("data_json".to_owned(), Json::Object(self.data_json.clone())));
            }
            None => {}
        }
        if self.test_classes {
            fields.push(("test_classes".to_owned(), Json::Bool(true)));
        }
        fields.push(("config".to_owned(), Json::Object(self.config.clone())));
        if !self.extensions.is_empty() {
            fields.push((
                "extensions".to_owned(),
                Json::Array(self.extensions.iter().map(|e| Json::str(*e)).collect()),
            ));
        }
        if let Some(sandbox) = &self.sandbox {
            fields.push(("sandbox".to_owned(), sandbox.clone()));
        }
        if !self.globals.is_empty() {
            fields.push(("globals".to_owned(), Json::Object(self.globals.clone())));
        }
        if let Some(timezone) = &self.timezone {
            fields.push(("timezone".to_owned(), Json::str(timezone.clone())));
        }
        if let Some(lexer) = &self.lexer {
            fields.push(("lexer".to_owned(), lexer.clone()));
        }
        if let Some(load_all) = self.load_all {
            fields.push(("load_all".to_owned(), Json::Bool(load_all)));
        }
        if self.throw {
            fields.push(("errors".to_owned(), Json::str("throw")));
        }
        super::with_case_cache(Json::Object(fields))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn php_literals() {
        assert_eq!(php_bytes(b"a\"$\\\xff\n"), "\"a\\\"\\$\\\\\\xff\\x0a\"");
        assert_eq!(php_str("it's \\"), "'it\\'s \\\\'");
    }

    #[test]
    fn builds_parseable_cases() {
        let mut b = Builder::new("t-1");
        b.template("index.twig", "{{ 1 }}");
        b.template("bin.twig", b"\xff".to_vec());
        b.data_json.push(("a".to_owned(), Json::int(1)));
        b.option("autoescape", Json::Bool(false));
        let json = b.build();
        let case = Case::from_json(&json).expect("a twig case");
        assert_eq!(case.template("bin.twig"), Some(&b"\xff"[..]));
        assert_eq!(case.main, "index.twig");
        assert!(case.unsupported_reason().is_none());
        assert!(json.get("config").and_then(|c| c.get("cache")).is_some());
    }
}
