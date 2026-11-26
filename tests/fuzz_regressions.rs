//! Regression vectors found by differential fuzzing (`crates/twig-fuzz`): every `*.json` file under
//! `tests/vectors/fuzz/` (recursively) holds oracle cases with the results Twig 3.31.1 on PHP 8.5.13
//! produced (format: `tests/vectors/fuzz/README.md`). Each case runs through the twig-fuzz runner and
//! is compared with the recorded result field by field (output bytes, error class, message, template
//! name, line and column, previous errors, deprecations, diagnostics), allowing only the deviations
//! of `reference/deviations.md` that twig-fuzz knows.
//!
//! The diagnosed mismatches of twig-fuzz's own round 0 (`crates/twig-fuzz/vectors/*.json`) are
//! replayed too, until they move under `tests/vectors/fuzz/`.
//!
//! A vector marked `"known_mismatch": {"reason": ...}` must still differ: once a fix makes it match,
//! this test fails until the marker is removed (a ratchet, like `tests/twig/known-failures.txt`).
//! `twig-fuzz replay --update <files or dirs>` removes the markers of the vectors that match now and
//! nothing else.
//!
//! The runner, the diff and the vector format are twig-fuzz's `kit` modules, compiled into this test
//! with `#[path]` so that the `twig` package needs no dependency on `twig-fuzz` (which depends on
//! `twig`). Vectors are written by `twig-fuzz drive` (`<out>/vectors.json`); replay them on their own
//! with `twig-fuzz replay tests/vectors/fuzz crates/twig-fuzz/vectors`.

use std::path::Path;
use std::time::Duration;

#[allow(dead_code)]
#[path = "../crates/twig-fuzz/src/kit/mod.rs"]
mod kit;

use kit::vectors::{ReplayStatus, replay_files, vector_files};

#[test]
fn fuzz_regression_vectors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["tests/vectors/fuzz", "crates/twig-fuzz/vectors"] {
        files.extend(vector_files(&root.join(dir)));
    }
    let replayed = replay_files(&files, Duration::from_secs(60));
    let mut failures = Vec::new();
    for r in &replayed {
        let file = r
            .file
            .strip_prefix(root)
            .unwrap_or(&r.file)
            .display()
            .to_string();
        match &r.status {
            ReplayStatus::Pass | ReplayStatus::PassWithDeviation(_) => {}
            ReplayStatus::PassSignatureChanged { recorded, now } => {
                // still a mismatch, so the marker stays valid; worth a look when it changes
                eprintln!(
                    "{file} {}: known mismatch now has another signature\n    was: {recorded}\n    now: {now}",
                    r.id
                );
            }
            ReplayStatus::Fail(reason) => failures.push(format!("{file} {}: {reason}", r.id)),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} fuzz regression vectors failed (`twig-fuzz replay --update` removes the markers of \
         fixed vectors):\n{}",
        failures.len(),
        replayed.len(),
        failures.join("\n")
    );
}
