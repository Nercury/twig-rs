//! Oracle vectors for the error model: `tests/vectors/errors.json` (message formatting of `Error::updateRepr()`,
//! PHP's `levenshtein()` and `SyntaxError::addSuggestions()` with the registries of a default environment).
//!
//! Regenerate from the repository root with (Git Bash on Windows needs `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/errors.gen.php > tests/vectors/errors.json
//! ```

mod common;

use common::Json;
use twig::error::levenshtein;
use twig::{Error, ErrorKind, Source};

fn kind(class: &str) -> ErrorKind {
    match class {
        "Twig\\Error\\SyntaxError" => ErrorKind::Syntax,
        "Twig\\Error\\LoaderError" => ErrorKind::Loader,
        "Twig\\Error\\RuntimeError" => ErrorKind::Runtime,
        other => panic!("unknown class {other}"),
    }
}

#[test]
fn messages_match_update_repr() {
    let vectors = Json::load_vectors("errors.json");
    let cases = vectors.field("repr").as_array();
    assert!(cases.len() >= 900, "the vector file looks truncated");
    let mut failures = Vec::new();
    for case in cases {
        let fields = case.as_array();
        let class = fields[0].as_str().expect("class");
        let raw = fields[1].as_str().expect("raw message");
        let line = fields[2].as_i64().expect("line");
        let column = fields[3].as_i64().map(|c| c as usize);
        let mut error = Error::new(kind(class), raw)
            .with_line(line)
            .with_column(column);
        if let Some(name) = fields[4].as_str() {
            error = error.with_source(Source::new("", name));
        }
        assert_eq!(error.kind().class_name(), class);
        let expected = fields[5].as_str().expect("message");
        if error.to_string() != expected {
            failures.push(format!("{case:?}: got {:?}", error.to_string()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn levenshtein_matches_php() {
    let vectors = Json::load_vectors("errors.json");
    for case in vectors.field("levenshtein").as_array() {
        let fields = case.as_array();
        let (a, b) = (fields[0].as_bytes(), fields[1].as_bytes());
        let expected = fields[2].as_i64().expect("distance") as usize;
        assert_eq!(levenshtein(&a, &b), expected, "levenshtein({a:?}, {b:?})");
    }
}

#[test]
fn suggestions_match_twig() {
    let vectors = Json::load_vectors("errors.json");
    let registries = vectors.field("registries");
    let mut failures = Vec::new();
    for case in vectors.field("suggestions").as_array() {
        let fields = case.as_array();
        let registry = fields[0].as_str().expect("registry");
        let name = fields[1].as_str().expect("name");
        let items: Vec<Vec<u8>> = match registry {
            "custom" => vec![
                b"for".to_vec(),
                b"fox".to_vec(),
                b"for".to_vec(),
                b"f".to_vec(),
            ],
            _ => registries
                .field(registry)
                .as_array()
                .iter()
                .map(Json::as_bytes)
                .collect(),
        };
        let (mut error, expected_message) = if registry == "custom" {
            (
                Error::syntax("Unknown \"fo\" thing.").with_line(1),
                fields[3].as_str().expect("message"),
            )
        } else {
            (
                Error::syntax(format!("Unknown \"{name}\" {registry}."))
                    .with_line(1)
                    .with_source(Source::new("", "index")),
                fields[3].as_str().expect("message"),
            )
        };
        error.add_suggestions(name.as_bytes(), &items);
        let expected_raw = fields[2].as_str().expect("raw message");
        if error.raw_message() != expected_raw || error.to_string() != expected_message {
            failures.push(format!(
                "{registry} {name:?}: got {:?}, expected {expected_raw:?}",
                error.raw_message()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
