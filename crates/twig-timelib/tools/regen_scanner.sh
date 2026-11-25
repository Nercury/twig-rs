#!/usr/bin/env bash
# Regenerates the two re2c scanners of the port with re2rust 4.1 (Docker image built from
# research/timelib-port/docker/Dockerfile):
#   src/parse_date/scanner.rs           from timelib's parse_date.re          (strtotime() & co.)
#   src/parse_iso_intervals/scanner.rs  from timelib's parse_iso_intervals.re (DateInterval/DatePeriod strings)
#
#   crates/twig-timelib/tools/regen_scanner.sh <PHPSRC>/ext/date/lib
#
# Steps: tools/gen_scanner_re.py / tools/gen_iso_scanner_re.py assemble scanner.re (verbatim upstream
# definitions + Rust actions from tools/scanner_rules.re.rs / tools/iso_scanner_rules.re.rs), then re2rust -b
# (bit vectors, as timelib's committed C scanners use) writes scanner.rs. All files are committed; building the
# crate does not need re2c.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB="${1:?path to PHPSRC/ext/date/lib}"
# Accept the old form (path to parse_date.re) too.
if [ -f "$LIB" ]; then LIB="$(dirname "$LIB")"; fi
IMAGE="${TWIG_TIMELIB_TOOLS_IMAGE:-twig-timelib-tools}"
# On Windows (Git Bash) hand native paths to python and docker.
if command -v cygpath >/dev/null 2>&1; then HERE="$(cygpath -m "$HERE")"; LIB="$(cygpath -m "$LIB")"; fi

python -I "$HERE/tools/gen_scanner_re.py" "$LIB/parse_date.re" "$HERE/src/parse_date/scanner.re"
python -I "$HERE/tools/gen_iso_scanner_re.py" "$LIB/parse_iso_intervals.re" "$HERE/src/parse_iso_intervals/scanner.re"
for dir in parse_date parse_iso_intervals; do
    MSYS_NO_PATHCONV=1 docker run --rm -v "$HERE/src/$dir:/w" "$IMAGE" \
        sh -c 're2rust -b --no-generation-date -W -o /w/scanner.rs /w/scanner.re'
    echo "wrote $HERE/src/$dir/scanner.rs"
done
