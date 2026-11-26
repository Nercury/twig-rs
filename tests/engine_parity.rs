//! The engine against Twig 3.31.1 on PHP 8.5.13 where the outcome depends on what PHP's compiler does with the code
//! Twig generates and on the state Twig keeps on node objects while compiling (research/fuzzing.md, P1 to P16, P31
//! and P35 to P37): constant folding and the lines of folded constants inside array literals (chosen branches and
//! values of the parser keep their own line, the literal's line comes after the dynamic key of its first element),
//! the operand order of commutative opcodes, casts of constant arrays in `~` (also of operands that the folding of an
//! array literal makes constant), prefix operators before `**`, null-safe chains, macro defaults evaluated at call
//! time, the lines of a few runtime errors (the statements written after an expression, TrueTest's bool conversion),
//! the `loop` bookkeeping writes, named arguments while unpacking, callable values and spreads in special node
//! classes. The compile-time errors of the generated PHP are the `compile_errors` topic of `tests/engine.rs`.
//!
//! The cases are the oracle-recorded vectors of `tests/vectors/engine/parity/<topic>.json` (the format of
//! `tests/vectors/fuzz/README.md`), replayed with twig-fuzz's runner and diff (compiled into this test with `#[path]`,
//! as `tests/fuzz_regressions.rs` does), allowing only the deviations of `reference/deviations.md` that twig-fuzz
//! knows. A vector marked `known_mismatch` must still differ (a ratchet).
//!
//! Regenerate the batches (`<topic>.gen.json`) and record their results from the repository root (Git Bash on
//! Windows needs `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! python -I tests/vectors/engine/parity/parity.gen.py --record [topic ...]
//! ```

use std::path::Path;
use std::time::Duration;

#[allow(dead_code)]
#[path = "../crates/twig-fuzz/src/kit/mod.rs"]
mod kit;

use kit::vectors::{ReplayStatus, replay_files, vector_files};

/// Replays the vector files of the directory; returns the failures.
fn replay(dir: &Path) -> (usize, Vec<String>) {
    let files: Vec<_> = vector_files(dir)
        .into_iter()
        .filter(|file| {
            !file
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".gen.json"))
        })
        .collect();
    let replayed = replay_files(&files, Duration::from_secs(60));
    let mut failures = Vec::new();
    for r in &replayed {
        let file = r
            .file
            .strip_prefix(dir)
            .unwrap_or(&r.file)
            .display()
            .to_string();
        match &r.status {
            ReplayStatus::Pass | ReplayStatus::PassWithDeviation(_) => {}
            ReplayStatus::PassSignatureChanged { recorded, now } => eprintln!(
                "{file} {}: known mismatch now has another signature\n    was: {recorded}\n    now: {now}",
                r.id
            ),
            ReplayStatus::Fail(reason) => failures.push(format!("{file} {}: {reason}", r.id)),
        }
    }
    (replayed.len(), failures)
}

#[test]
fn parity_vectors() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/engine/parity");
    let (count, failures) = replay(&dir);
    assert!(count > 0, "no vectors under {}", dir.display());
    assert!(
        failures.is_empty(),
        "{} of {count} parity vectors failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
