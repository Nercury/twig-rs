#!/usr/bin/env bash
# Runs the criterion benchmarks (benches/) in a Linux container on the same Docker host as run-php.sh, so that both
# sides of the comparison run on the same kernel and CPU. Criterion's estimates end up in results/criterion/.
#
#   research/benchmarks/run-rust.sh [<source dir, default: the repository>] [criterion arguments]
#
# The target directory is the Docker volume TWIG_BENCH_TARGET (default twig-bench-target). Cargo tells changed sources
# by their file times, so a source tree older than the last build in the volume (an earlier commit extracted with
# `git archive`) needs a volume of its own.
set -euo pipefail
IMAGE="${TWIG_RUST_IMAGE:-rust:1.100-slim-trixie}"
TARGET="${TWIG_BENCH_TARGET:-twig-bench-target}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$(cd "${1:-$HERE/../..}" && pwd)"
[ $# -gt 0 ] && shift
export MSYS_NO_PATHCONV=1
host() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }

mkdir -p "$HERE/results"
# The sources are mounted read-only; the target directory and the registry live in named volumes.
docker run --rm \
    -v "$(host "$SRC"):/src:ro" \
    -v "$(host "$HERE/results"):/out" \
    -v "$TARGET:/target" \
    -v twig-bench-cargo:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/target \
    -w /src \
    "$IMAGE" \
    sh -c 'rustc --version && cargo bench --locked --bench "*" -- "$@" && rm -rf /out/criterion && cp -r /target/criterion /out/criterion' \
    sh "$@"
