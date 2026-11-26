//! Seeded case generators, one module per construct family (`twig-fuzz generate <family>`).
//!
//! Every generated case has a filesystem cache directory of its own (`config.cache`): with
//! `cache => false` PHP `eval()`s every compiled template from the same line of `Environment.php`,
//! so `Error::guess()` mixes up the frames of different templates (an error in a macro imported
//! from another template gets the line of the handling template's marker), while twig-rs keeps the
//! templates apart as a filesystem cache does (`reference/deviations.md`, "Line guessing with
//! `cache => false`"). The composition and errors families name their own directories; the others
//! go through [`with_case_cache`]. The oracle's runner empties a case's directory before the case
//! runs, so a re-run compiles the templates again.

pub mod callables;
pub mod casekit;
pub mod composition;
pub mod control;
pub mod dynamic;
pub mod errors;
pub mod escaping;
pub mod expressions;
pub mod formats;
pub mod sandbox;
pub mod structure;
pub mod unicode;
pub mod whitespace;

use crate::kit::json::Json;

/// What a generator may produce besides what is expected to match (`twig-fuzz generate --d2`).
///
/// The generators avoid the inputs of accepted deviations by default, so that a round spends its
/// cases on what can still be a bug; the regression vectors of a deviation pin it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Inputs of the accepted deviation D2 (`reference/deviations.md`, Engine: "Lone CR bytes in
    /// the generated PHP"): string escapes that put a CR byte into a compiled string literal
    /// (`\r`, `\x0d`: the escaping, callables and formats families) and lone CR line endings in
    /// templates compiled with `debug` (the errors family). Without it the families write CR
    /// bytes through the data instead, or drop them from literals, and give `debug` templates CRLF
    /// line endings.
    pub d2: bool,
}

/// The parent of the cache directories [`with_case_cache`] gives (in the oracle's container).
pub const CACHE_DIR: &str = "/tmp/twig-fuzz-cache";

/// `case` with the filesystem cache directory `<CACHE_DIR>/<id>` (`config.cache`) unless it has one
/// (see the module documentation). twig-rs ignores the option.
pub fn with_case_cache(mut case: Json) -> Json {
    let id = match case.get("id") {
        Some(Json::String(id)) => id.clone(),
        Some(other) => other.to_compact(),
        None => return case,
    };
    let mut config = match case.get("config") {
        Some(config @ Json::Object(_)) => config.clone(),
        Some(_) => return case,
        None => Json::Object(Vec::new()),
    };
    if config.get("cache").is_some() {
        return case;
    }
    // a path that stays one directory whatever the id holds
    let name: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    config.set("cache", Json::str(format!("{CACHE_DIR}/{name}")));
    case.set("config", config);
    case
}

/// Whether `text` holds a Twig escape of a CR byte (`\r`, `\x0d`, `\xd`): the D2 inputs the
/// families avoid unless [`Options::d2`].
#[cfg(test)]
pub(crate) fn has_cr_escape(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            let rest = &bytes[i + 1..];
            let is_d = |b: Option<&u8>| b.is_some_and(|b| (b | 0x20) == b'd');
            let is_hex = |b: Option<&u8>| b.is_some_and(u8::is_ascii_hexdigit);
            let x = rest[0] == b'x';
            if rest[0] == b'r'
                || (x && rest.get(1) == Some(&b'0') && is_d(rest.get(2)))
                || (x && is_d(rest.get(1)) && !is_hex(rest.get(2)))
            {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// The templates of a case that hold [`has_cr_escape`].
#[cfg(test)]
pub(crate) fn templates_with_cr_escapes(case: &Json) -> usize {
    case.get("templates")
        .map(Json::entries)
        .unwrap_or_default()
        .iter()
        .filter(|(_, source)| has_cr_escape(source.as_str().unwrap_or_default()))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cr_escapes() {
        assert!(has_cr_escape("'a\\x0Db'") && has_cr_escape("\"\\r\"") && has_cr_escape("'\\xd'"));
        assert!(!has_cr_escape("'\\\\r' '\\x0a' '\\xdf' 'r'"));
    }

    #[test]
    fn every_family_gives_each_case_a_cache_directory() {
        let cases = [
            callables::generate_case(3, 1),
            control::case(3, 1),
            escaping::case(3, 1),
            expressions::generate_case(3, 1),
            formats::case(3, 1, &[]),
            sandbox::case(3, 1),
            composition::case(composition::case_seed(3, 1), "comp-3-1", true),
            errors::case(3, 1),
        ];
        let mut dirs = Vec::new();
        for case in &cases {
            let dir = case
                .get("config")
                .and_then(|c| c.get("cache"))
                .and_then(Json::as_str)
                .unwrap_or_else(|| panic!("no cache directory: {}", case.to_compact()));
            assert!(dir.starts_with("/tmp/"), "{dir}");
            dirs.push(dir.to_owned());
        }
        dirs.sort();
        dirs.dedup();
        assert_eq!(dirs.len(), cases.len());
        let odd = with_case_cache(Json::Object(vec![
            ("id".to_owned(), Json::str("a/../b c")),
            ("config".to_owned(), Json::Object(Vec::new())),
        ]));
        assert_eq!(
            odd.get("config").and_then(|c| c.get("cache")),
            Some(&Json::str("/tmp/twig-fuzz-cache/a_.._b_c"))
        );
    }
}
