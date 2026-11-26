//! Random cases of the composition family: template sets that use `extends` (constant, dynamic,
//! conditional, array and wrapper parents, several levels), blocks (nested, defined under tags,
//! `block()` with and without a template, `parent()` chains, `is defined`), `use` (traits, aliases,
//! traits using traits), `embed` (nested, `with`, `only`, `ignore missing`), `include` (the tag, the
//! `include()` and `include_only()` functions, `with`, `only`, `ignore missing`, arrays of names,
//! dynamic names, a template including itself), macros (`import`, `from`, `_self`, defaults,
//! `varargs` and `...rest`, named, misordered, duplicate and spread arguments, recursion, macros
//! calling macros of other templates), and the context and globals they pass around.
//!
//! Around these: `Twig\TemplateWrapper` and `Twig\Markup` objects built by `data_php` (as parents,
//! include and block template names, wrapper method calls), `template_from_string()` (named and
//! unnamed, failing sources), dynamic macro names (`m.(expr)()`) and `?.`, `{% deprecated %}` in
//! composed templates, assignments to special names (`_self`, `_context`, `_charset`), numeric
//! context keys and global names, multi-line tags and argument lists, whitespace control, CR and
//! CRLF line endings, odd template names (with the `name` escaping strategy), run-time errors thrown
//! from deep inside (line guessing across templates), the sandbox (a policy allowing what the
//! templates use, now and then minus one name), and the options `strict_variables`, `autoescape`,
//! `optimizations`, `debug`, `use_yield`, `load_all` and `"errors": "throw"`.
//!
//! [`batch`] writes an oracle batch (one case per seed, `"isolation": "process"`, see below);
//! [`case`] makes one case from its seed, so any case of a batch can be regenerated alone:
//!
//! ```ignore
//! // `gen` is a reserved keyword in Rust 2024: the module is `r#gen`
//! use twig_fuzz::r#gen::composition::{Settings, batch};
//!
//! let settings = Settings { seed: 1, count: 2000, ..Settings::default() };
//! std::fs::write("composition-1.json", batch(&settings).to_layout(1)).unwrap();
//! // twig-fuzz drive composition-1.json --out out-1
//! ```
//!
//! How the generator avoids differences that are not bugs:
//!
//! - **Process isolation.** PHP keeps a compiled template class for the whole process, so a later
//!   case with the same template source and options would reuse it without compiling it again
//!   (without its compile-time deprecations, and with the `autoescape`/`optimizations` of the first
//!   case). Batches therefore run every case in a PHP process of its own.
//! - **A filesystem cache per case** (`"cache": "/tmp/tfc/<id>"`, [`Settings::cache`]). With
//!   `cache => false` PHP `eval()`s every template from the same line of `Environment.php`, so
//!   `Error::guess()` mixes the frames of different templates (an exception in a macro of another
//!   template gets the handling template's line); twig-rs keeps templates apart like a filesystem
//!   cache does (`reference/deviations.md`, "Line guessing with `cache => false`"). The minimiser's
//!   candidates of one case must not share that directory (a compiled file of an earlier candidate
//!   would be loaded instead of compiling the template again, with the options of that candidate):
//!   [`crate::minimize::SameSignature`] gives each candidate a directory of its own, and the oracle's
//!   runner empties a case's directory before the case runs; [`FreshCache`] does the former for any
//!   other predicate.
//! - **No unbounded recursion.** Templates only refer to templates generated before them; blocks only
//!   contain or call blocks of a lower rank (`a` < `b` < ... < `content`), and `block(name, _self)`
//!   keeps the rank limit of the enclosing block inside an embed (`_self` still names the enclosing
//!   template there); blocks named `a` never call macros, the only blocks macros render; a macro only
//!   calls the macros its template defines before it, except `rec(n, sep)`, which calls itself while
//!   `n > 0`; a template includes itself only from the root of its body (where nothing can drop the
//!   `depth` guard from the context).
//!   Twig has no recursion limit and PHP dies with a fatal error, while twig-rs reports its own
//!   limit (`reference/deviations.md`, "Recursion depth").
//! - **No unseeded randomness**, no dates, no `dump()` of objects, no closures in the data.

mod rng;
mod templates;

pub use rng::Rng;

use crate::kit::json::Json;
use crate::minimize::Predicate;

/// What [`batch`] generates.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The seed of the batch: case `i` uses [`case_seed`]`(seed, i)`.
    pub seed: u64,
    /// Number of cases.
    pub count: usize,
    /// Case ids are `<prefix><seed>-<i>`.
    pub prefix: String,
    /// Give every case a filesystem cache directory of its own (see the module documentation).
    pub cache: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            seed: 1,
            count: 1000,
            prefix: "comp-".to_owned(),
            cache: true,
        }
    }
}

/// The seed of case `index` of the batch with seed `seed`.
pub fn case_seed(seed: u64, index: usize) -> u64 {
    seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(17)
        .wrapping_add(index as u64)
}

/// The batch of `settings`: `{"cases": [...], "isolation": "process"}`.
pub fn batch(settings: &Settings) -> Json {
    let cases = (0..settings.count)
        .map(|i| {
            let id = format!("{}{}-{i}", settings.prefix, settings.seed);
            case(case_seed(settings.seed, i), &id, settings.cache)
        })
        .collect();
    Json::Object(vec![
        ("cases".to_owned(), Json::Array(cases)),
        ("isolation".to_owned(), Json::str("process")),
    ])
}

/// The case generated from `seed`, with id `id`; `cache` adds a filesystem cache directory named
/// after the id.
pub fn case(seed: u64, id: &str, cache: bool) -> Json {
    let plan = templates::Gen::new(seed).plan();
    let mut fields: Vec<(String, Json)> = vec![
        ("id".to_owned(), Json::str(id)),
        ("env".to_owned(), Json::str("plain")),
        (
            "templates".to_owned(),
            Json::Object(
                plan.templates
                    .iter()
                    .map(|(name, source)| (name.clone(), Json::str(source)))
                    .collect(),
            ),
        ),
    ];
    if plan.php.is_empty() {
        if !plan.data.is_empty() {
            fields.push(("data_json".to_owned(), Json::Object(plan.data.clone())));
        }
    } else {
        let json = Json::Object(plan.data.clone()).to_compact();
        let extras: Vec<String> = plan
            .php
            .iter()
            .map(|(key, code)| format!("{} => {code}", templates::php_string(key)))
            .collect();
        fields.push((
            "data_php".to_owned(),
            Json::str(format!(
                "return array_merge(json_decode({}, true), [{}]);",
                templates::php_string(&json),
                extras.join(", ")
            )),
        ));
    }
    let mut config: Vec<(String, Json)> =
        vec![("strict_variables".to_owned(), Json::Bool(plan.strict))];
    if let Some(autoescape) = &plan.autoescape {
        config.push(("autoescape".to_owned(), autoescape.clone()));
    }
    if let Some(level) = plan.optimizations {
        config.push(("optimizations".to_owned(), Json::int(level)));
    }
    if plan.debug {
        config.push(("debug".to_owned(), Json::Bool(true)));
    }
    if plan.use_yield {
        config.push(("use_yield".to_owned(), Json::Bool(true)));
    }
    if cache {
        config.push(("cache".to_owned(), Json::str(format!("/tmp/tfc/{id}"))));
    }
    fields.push(("config".to_owned(), Json::Object(config)));
    let mut extensions: Vec<Json> = Vec::new();
    if plan.string_loader {
        extensions.push(Json::str("string_loader"));
    }
    if plan.sandbox.is_some() {
        extensions.push(Json::str("sandbox"));
    }
    if !extensions.is_empty() {
        fields.push(("extensions".to_owned(), Json::Array(extensions)));
    }
    if let Some(sandbox) = plan.sandbox {
        fields.push(("sandbox".to_owned(), sandbox));
    }
    if !plan.globals.is_empty() {
        fields.push(("globals".to_owned(), Json::Object(plan.globals.clone())));
    }
    fields.push(("load_all".to_owned(), Json::Bool(plan.load_all)));
    if plan.throw {
        fields.push(("errors".to_owned(), Json::str("throw")));
    }
    Json::Object(fields)
}

/// `case` with the filesystem cache directory `dir` (`None`: without a cache).
pub fn with_cache_dir(case: &Json, dir: Option<&str>) -> Json {
    let mut case = case.clone();
    let mut config = match case.get("config") {
        Some(config @ Json::Object(_)) => config.clone(),
        _ => Json::Object(Vec::new()),
    };
    match dir {
        Some(dir) => config.set("cache", Json::str(dir)),
        None => {
            config.remove("cache");
        }
    }
    case.set("config", config);
    case
}

/// A minimiser predicate that gives every candidate a filesystem cache directory of its own
/// (`<base>/<n>`), so that no candidate loads a template another candidate compiled.
pub struct FreshCache<'p> {
    inner: &'p mut dyn Predicate,
    base: String,
    counter: usize,
}

impl<'p> FreshCache<'p> {
    /// Wraps `inner`; directories are created under `base` (a path in the oracle's container).
    pub fn new(inner: &'p mut dyn Predicate, base: &str) -> Self {
        FreshCache {
            inner,
            base: base.trim_end_matches('/').to_owned(),
            counter: 0,
        }
    }
}

impl Predicate for FreshCache<'_> {
    fn test(&mut self, candidates: &[Json]) -> Vec<bool> {
        let rewritten: Vec<Json> = candidates
            .iter()
            .map(|candidate| {
                self.counter += 1;
                let dir = format!("{}/{}", self.base, self.counter);
                with_cache_dir(candidate, Some(&dir))
            })
            .collect();
        self.inner.test(&rewritten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn cases_are_reproducible_and_parse() {
        let settings = Settings {
            seed: 7,
            count: 300,
            ..Settings::default()
        };
        let first = batch(&settings);
        assert_eq!(first, batch(&settings));
        let cases = first.get("cases").map(Json::items).unwrap_or_default();
        assert_eq!(cases.len(), 300);
        for json in cases {
            let case = Case::from_json(json).expect("a twig case");
            assert_eq!(case.main, "index.twig");
            assert!(case.unsupported_reason().is_none());
        }
        let one = case(case_seed(7, 5), "comp-7-5", true);
        assert_eq!(&one, &cases[5]);
        assert!(
            with_cache_dir(&one, None)
                .get("config")
                .and_then(|c| c.get("cache"))
                .is_none()
        );
        let moved = with_cache_dir(&one, Some("/tmp/x"));
        assert_eq!(
            moved.get("config").and_then(|c| c.get("cache")),
            Some(&Json::str("/tmp/x"))
        );
    }
}
