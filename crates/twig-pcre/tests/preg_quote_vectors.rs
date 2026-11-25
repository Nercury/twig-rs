//! Replays the PHP 8.5.13 oracle vectors for `preg_quote()` in `tests/vectors/preg_quote.json`.
//!
//! Regenerate from the repository root (Git Bash, Docker image `twig-oracle:3.31.1` from
//! `research/php-oracle`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(cygpath -wa crates/twig-pcre/tests/vectors):/v:ro" \
//!     twig-oracle:3.31.1 php /v/preg_quote.gen.php > crates/twig-pcre/tests/vectors/preg_quote.json
//! ```

use serde_json::Value;
use twig_pcre::preg::preg_quote;

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/preg_quote.json");

fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(o) => {
            let h = o["hex"].as_str().expect("hex string");
            (0..h.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex digit"))
                .collect()
        }
        other => panic!("not a byte string: {other}"),
    }
}

#[test]
fn preg_quote_oracle_vectors() {
    let text = std::fs::read_to_string(VECTORS).expect("read preg_quote.json");
    let root: Value = serde_json::from_str(&text).expect("parse preg_quote.json");
    assert_eq!(root["php"], "8.5.13");
    let cases = root["cases"].as_array().expect("cases");
    assert!(cases.len() > 900, "only {} cases", cases.len());
    let mut failures = Vec::new();
    for case in cases {
        let s = bytes(&case["s"]);
        let d = if case["d"].is_null() {
            None
        } else {
            Some(bytes(&case["d"]))
        };
        let expected = bytes(&case["out"]);
        let actual = preg_quote(&s, d.as_deref());
        if actual != expected {
            failures.push(format!(
                "preg_quote({s:?}, {d:?}): expected {expected:?}, got {actual:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
