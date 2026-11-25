#!/usr/bin/env bash
# Host-side wrapper (Windows Git Bash / Linux / macOS): pipes a JSON batch into the oracle container.
#
#   ./oracle.sh [--pretty] < batch.json > results.json
#   echo '[{"templates":{"index.twig":"{{ 1.0 }}"}}]' | ./oracle.sh --pretty
#   ./oracle.sh --build            # (re)build the image first, then read stdin
#   ./oracle.sh suite [integration|all] [phpunit args]   # run Twig's PHPUnit suite
#
# Image name can be overridden with TWIG_ORACLE_IMAGE.
set -euo pipefail
IMAGE="${TWIG_ORACLE_IMAGE:-twig-oracle:3.31.1}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export MSYS_NO_PATHCONV=1   # Git Bash: do not rewrite /oracle/... arguments into Windows paths

if [ "${1:-}" = "--build" ]; then
    shift
    docker build -t "$IMAGE" "$HERE" >&2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    docker build -t "$IMAGE" "$HERE" >&2
fi
if [ "${1:-}" = "suite" ]; then
    shift
    exec docker run --rm "$IMAGE" ./run-suite.sh "$@"
fi
exec docker run --rm -i "$IMAGE" php runner.php "$@"
