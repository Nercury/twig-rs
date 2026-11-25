#!/usr/bin/env bash
# Full differential test of crates/twig-timelib against PHP 8.5.13's timelib (C, built like PHP builds it):
# generates every corpus with fixed seeds, runs the C harness (Docker image twig-timelib-tools) and the Rust
# dump tool (release build with overflow checks, so an arithmetic overflow aborts instead of wrapping silently),
# and compares the outputs line by line. Optionally measures gcov line coverage of the C sources.
#
#   research/timelib-port/run-differential.sh <PHPSRC> <workdir> [--coverage]
#
# <PHPSRC> is a php-src 8.5.13 checkout (ext/date/lib and ext/date/tests are used). Needs python, docker and
# cargo. Prints a per-command summary; exits non-zero if any line differs.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PHPSRC="$(cd "${1:?path to php-src 8.5.13}" && pwd)"
WORK="${2:?work directory}"
mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd)"
TOOLS="$ROOT/research/timelib-port/tools"
TESTS="$PHPSRC/ext/date/tests"
if command -v cygpath >/dev/null 2>&1; then TESTS="$(cygpath -m "$TESTS")"; fi

gen() { python -I "$TOOLS/gen_corpus.py" "$@"; }
: > "$WORK/in.txt"
#        mode     count   seed  (literals from php-src's ext/date/tests for parse/init/modify/ival/raw)
while read -r mode count seed lit; do
    src="-"
    if [ "$lit" = "t" ]; then src="$TESTS"; fi
    gen "$src" "$mode" "$count" "$seed" >> "$WORK/in.txt"
done <<'EOF'
parse    300000  1  t
init     200000  2  t
modify   200000  3  t
format   200000  4  -
local    299000  5  -
fmtinit  100000  6  -
zone      20000  7  -
diff     100000  8  -
add      100000  9  -
mk       100000 10  -
info      59800 11  -
tzfile     3000 12  -
dow      100000 13  -
abbr      20000 14  -
ival      50000 15  t
raw       60000 16  t
posix     30000 17  -
hms       30000 18  -
formatx  100000 19  -
trans    200000 20  -
extreme   60000 21  -
special       0  0  -
EOF
echo "corpus: $(wc -l < "$WORK/in.txt") lines"

bash "$ROOT/research/timelib-port/harness/run.sh" "$PHPSRC/ext/date/lib" "$WORK/in.txt" "$WORK/c.txt"
# Target directory (relative to the repository root) for the overflow-checked build.
TD="${TIMELIB_DIFF_TARGET_DIR:-target/timelib-differential}"
(cd "$ROOT" && RUSTFLAGS="-C overflow-checks=on" CARGO_TARGET_DIR="$TD" \
    cargo +stable build --release -q -p twig-timelib --example timelib_dump)
"$ROOT/$TD/release/examples/timelib_dump" < "$WORK/in.txt" > "$WORK/rust.txt"
python -I "$TOOLS/compare_outputs.py" "$WORK/in.txt" "$WORK/c.txt" "$WORK/rust.txt"

if [ "${3:-}" = "--coverage" ]; then
    bash "$ROOT/research/timelib-port/harness/coverage.sh" "$PHPSRC/ext/date/lib" "$WORK/in.txt" "$WORK/coverage"
    python -I "$TOOLS/gcov_summary.py" "$WORK/coverage"
fi
