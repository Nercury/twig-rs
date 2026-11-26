# Fuzz regression vectors

Mismatches between twig-rs and Twig 3.31.1 on PHP 8.5.13 found by differential fuzzing (`crates/twig-fuzz`),
minimised, with the result the PHP oracle recorded. `tests/fuzz_regressions.rs` replays every `*.json` file in this
directory and its subdirectories without Docker (and twig-fuzz's own round-0 vectors,
`crates/twig-fuzz/vectors/*.json`); `twig-fuzz replay tests/vectors/fuzz crates/twig-fuzz/vectors` does the same from
the command line.

## Format

```json
{
  "description": "what the file covers",
  "vectors": [
    {"id": "round-precision-1",
     "note": "optional free text",
     "case": {"id": "round-precision-1", "env": "plain", "templates": {"index.twig": "{{ 2.5|round }}"}},
     "oracle": {"id": "round-precision-1", "kind": "twig", "ok": true, "output": "3",
                "deprecations": [], "diagnostics": []},
     "known_mismatch": {"reason": "round() of halves (src/extension/runtime/misc/...)",
                        "signature": "output:changed"}}
  ]
}
```

- `case` is an oracle "twig" case (protocol at the top of `research/php-oracle/runner.php`) and must use
  `"env": "plain"`. Data is `data_json`, or `data_php` together with the typed tree the oracle returned for it. It may
  set Lexer options (`"lexer"`).
- `oracle` is the oracle's result for that case, unchanged. For `data_php` cases it must contain the fields
  `"typed_data": true` makes the oracle add (`data`, `state`, `data_diagnostics`, `data_deprecations`): twig-rs
  rebuilds the data from them. A `clock` field (`"clock": true`, which twig-fuzz sets) makes twig-rs render with the
  oracle's "now".
- `known_mismatch` (optional) marks a vector that still differs. `reason` says what is wrong and where; `signature`
  is the mismatch signature when it was recorded (informational: the replay reports a changed signature but does not
  fail on it).

## Rules

- A vector without `known_mismatch` must replay identically: output bytes, error class and message, template name,
  line and column, previous errors, deprecations and diagnostics. Only the deviations twig-fuzz knows from
  `reference/deviations.md` are accepted (`, called in <file> on line N` suffixes, closure names with file and line,
  `var_dump()` object handles, also in the `string(N)` length of a dumped string that holds a dump, lossy non-UTF-8
  messages, and a warning PHP raised while compiling the template class under a filesystem cache, which silences it,
  where twig-rs raises it when the expression runs).
- A vector with `known_mismatch` must still mismatch. A fix makes the test fail until the marker is removed, which
  turns the vector into a plain regression test (a ratchet, like `tests/twig/known-failures.txt`).
- `twig-fuzz replay --update <files or dirs> [--id ID]...` removes the marker of every marked vector that now
  matches the oracle (only those named by `--id` when given) and changes nothing else in the files. A marked vector
  that still mismatches keeps its marker, also when its signature changed: the command lists it for a person to look
  at (the test only reports such a vector, it does not fail).
- Vectors come from `twig-fuzz drive <batch> --out <dir>` (`<dir>/vectors.json`: one minimised vector per mismatch
  cluster, marked `known_mismatch`). Give each file a topic name (`expressions.json`, `filters/round.json`, ...),
  replace the generic `reason` with the actual diagnosis, and keep each file below ~5 MB.
- Never edit `oracle` by hand: re-run the case through the oracle instead. `twig-fuzz refresh <file> --id ID...`
  does that in place (each case in a PHP process of its own; only the `oracle` values change). A vector recorded
  before the oracle learned something new about the data (for example the `props` of date objects in the typed
  tree) gets it that way.
