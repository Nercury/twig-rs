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

#[cfg(test)]
mod tests {
    use super::*;

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
