//! Parity fixes of the callables group (`research/fuzzing.md`, G3; item history: `git log -- plans/parity-fixes.md`)
//! against Twig 3.31.1 on PHP 8.5.13: oracle cases with the result the oracle recorded, in the vector format of the
//! fuzz regressions (`tests/vectors/fuzz/README.md`), replayed through the twig-fuzz runner and compared field by field
//! (output bytes, error class, message, template name and line, previous errors, deprecations, diagnostics), allowing
//! only the deviations twig-fuzz knows from `reference/deviations.md` (here: static methods PHP would run,
//! `not supported by twig-rs`).
//!
//! | file | covers |
//! |---|---|
//! | `classes.json` | P27: Twig's classes, interfaces, traits and enums in `constant()`, `constant(...) is defined`, the `constant` test (also with an object), `enum()` and `enum_cases()`; every constant of every Twig class; `self`, `parent` and `static` in the scope of `CoreExtension::constant()` and of the template class |
//! | `callables.json` | P15 (the callables group's part): callables checked by `sort` and `filter` (`zend_is_callable_ex()`) and called by `map`, `reduce` and `find` (`ZEND_INIT_DYNAMIC_CALL`), with class lookups through the class table |
//! | `return_types.json` | P28: the `string` return type of `CoreExtension::join()` when the element's `__toString()` throws |
//!
//! A vector marked `known_mismatch` must still differ (a ratchet, like `tests/fuzz_regressions.rs`).
//!
//! The batches are `tests/vectors/callables/parity/<topic>.gen.json`; record them again from the repository root
//! (needs the Docker oracle, `research/php-oracle`; Git Bash on Windows needs `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! cargo run -p twig-fuzz --release -- record tests/vectors/callables/parity/<topic>.gen.json \
//!     --out tests/vectors/callables/parity/<topic>.json \
//!     --description "G3 parity vectors (<topic>), recorded from tests/vectors/callables/parity/<topic>.gen.json"
//! ```
//!
//! then give each `known_mismatch` its diagnosis.

use std::path::{Path, PathBuf};
use std::time::Duration;

#[allow(dead_code)]
#[path = "../crates/twig-fuzz/src/kit/mod.rs"]
mod kit;

use kit::vectors::{ReplayStatus, replay_files};

fn replay(topic: &str) {
    let file: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/callables/parity")
        .join(format!("{topic}.json"));
    let replayed = replay_files(std::slice::from_ref(&file), Duration::from_secs(60));
    assert!(!replayed.is_empty(), "no vectors in {}", file.display());
    let mut failures = Vec::new();
    for r in &replayed {
        match &r.status {
            ReplayStatus::Pass | ReplayStatus::PassWithDeviation(_) => {}
            ReplayStatus::PassSignatureChanged { recorded, now } => {
                eprintln!(
                    "{topic} {}: known mismatch now has another signature\n    was: {recorded}\n    now: {now}",
                    r.id
                );
            }
            ReplayStatus::Fail(reason) => failures.push(format!("{} {}: {reason}", topic, r.id)),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} {topic} vectors failed:\n{}",
        failures.len(),
        replayed.len(),
        failures.join("\n")
    );
}

#[test]
fn twig_classes_and_constants() {
    replay("classes");
}

#[test]
fn callables_checked_and_called_like_php() {
    replay("callables");
}

#[test]
fn string_return_types() {
    replay("return_types");
}
