#!/usr/bin/env bash
# Builds the C harness against PHP 8.5.13's timelib copy and runs it on a corpus inside the twig-timelib-tools
# image (research/timelib-port/docker/Dockerfile).
#
#   research/timelib-port/harness/run.sh <PHPSRC>/ext/date/lib <corpus.txt> <out.txt>
#
# The flags mirror how PHP builds timelib (PHPSRC/ext/date/config0.m4): -fwrapv (signed overflow wraps) and
# HAVE_STRTOLL/HAVE_ATOLL from configure. Set HARNESS_CFLAGS to add flags (e.g. "--coverage -O0").
#
# Compare with the Rust port:
#   cargo run --release -p twig-timelib --example timelib_dump < corpus.txt > rust.txt && cmp out.txt rust.txt
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="$(cd "${1:?PHPSRC/ext/date/lib}" && pwd)"
CORPUS="${2:?corpus file}"
OUT="${3:?output file}"
IMAGE="${TWIG_TIMELIB_TOOLS_IMAGE:-twig-timelib-tools}"
WORK="$(cd "$(dirname "$CORPUS")" && pwd)"
if command -v cygpath >/dev/null 2>&1; then
    HERE="$(cygpath -m "$HERE")"; LIB="$(cygpath -m "$LIB")"; WORK="$(cygpath -m "$WORK")"
fi
MSYS_NO_PATHCONV=1 docker run --rm -i -e "HARNESS_CFLAGS=${HARNESS_CFLAGS:-}" \
    -v "$HERE:/harness:ro" -v "$LIB:/tlib:ro" -v "$WORK:/work" "$IMAGE" sh -c '
set -e
mkdir -p /tmp/b && cp /tlib/*.c /tlib/*.h /tmp/b/ && cd /tmp/b
gcc -O2 -w -fwrapv -DHAVE_STRTOLL=1 -DHAVE_ATOLL=1 $HARNESS_CFLAGS -I. /harness/harness.c parse_date.c \
    parse_iso_intervals.c parse_tz.c timelib.c tm2unixtime.c unixtime2tm.c dow.c interval.c parse_posix.c astro.c \
    -lm -o /tmp/harness
/tmp/harness < "/work/$1" > "/work/$2"
' sh "$(basename "$CORPUS")" "$(basename "$OUT")"
