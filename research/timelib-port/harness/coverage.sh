#!/usr/bin/env bash
# Line coverage (gcov) of PHP 8.5.13's timelib C sources under a harness corpus: shows which upstream code paths
# the differential corpora reach. Runs in the twig-timelib-tools image (research/timelib-port/docker/Dockerfile).
#
#   research/timelib-port/harness/coverage.sh <PHPSRC>/ext/date/lib <corpus.txt> <outdir>
#
# <outdir> receives the annotated *.gcov files and summary.txt (gcov's per-file "Lines executed" report).
# parse_date.c / parse_iso_intervals.c carry #line directives, so their actions and helpers are reported against
# parse_date.re / parse_iso_intervals.re.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="$(cd "${1:?PHPSRC/ext/date/lib}" && pwd)"
CORPUS="${2:?corpus file}"
OUT="${3:?output directory}"
IMAGE="${TWIG_TIMELIB_TOOLS_IMAGE:-twig-timelib-tools}"
mkdir -p "$OUT"
WORK="$(cd "$(dirname "$CORPUS")" && pwd)"
OUT="$(cd "$OUT" && pwd)"
if command -v cygpath >/dev/null 2>&1; then
    HERE="$(cygpath -m "$HERE")"; LIB="$(cygpath -m "$LIB")"; WORK="$(cygpath -m "$WORK")"; OUT="$(cygpath -m "$OUT")"
fi
MSYS_NO_PATHCONV=1 docker run --rm -i -v "$HERE:/harness:ro" -v "$LIB:/tlib:ro" -v "$WORK:/work:ro" -v "$OUT:/out" \
    "$IMAGE" sh -c '
set -e
mkdir -p /tmp/b/ext/date/lib && cp /tlib/*.c /tlib/*.h /tlib/*.re /tmp/b/ && cp /tlib/*.re /tlib/*.c /tmp/b/ext/date/lib/ && cd /tmp/b
SRCS="parse_date.c parse_iso_intervals.c parse_tz.c timelib.c tm2unixtime.c unixtime2tm.c dow.c interval.c parse_posix.c"
CFLAGS="-O0 --coverage -w -fwrapv -DHAVE_STRTOLL=1 -DHAVE_ATOLL=1 -I."
for f in $SRCS astro.c; do gcc $CFLAGS -c "$f" -o "${f%.c}.o"; done
gcc $CFLAGS -c /harness/harness.c -o harness.o
gcc --coverage *.o -lm -o /tmp/harness
/tmp/harness < "/work/$1" > /dev/null
gcov $SRCS > /out/summary.txt 2>&1 || true
cp *.gcov /out/ 2>/dev/null || true
' sh "$(basename "$CORPUS")"
echo "coverage written to $OUT"
