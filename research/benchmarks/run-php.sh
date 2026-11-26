#!/usr/bin/env bash
# Runs bench.php (PHP Twig 3.31.1 on PHP 8.5.13) in the oracle image over benches/scenarios, twice:
#   results/php.jsonl      default CLI settings (no OPcache), Twig without a cache: compiled templates are eval()'d
#   results/php-jit.jsonl  OPcache with the tracing JIT and Twig's filesystem cache, so that OPcache keeps the
#                          compiled templates (a typical production set-up)
#
#   research/benchmarks/run-php.sh [--time=<seconds per benchmark>] [--only=<scenario>]
set -euo pipefail
IMAGE="${TWIG_ORACLE_IMAGE:-twig-oracle:3.31.1}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
export MSYS_NO_PATHCONV=1   # Git Bash: keep /scenarios and /bench as they are
host() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }

docker image inspect "$IMAGE" >/dev/null 2>&1 || docker build -t "$IMAGE" "$ROOT/research/php-oracle" >&2
mkdir -p "$HERE/results"
run() {
    docker run --rm \
        -v "$(host "$ROOT/benches/scenarios"):/scenarios:ro" \
        -v "$(host "$HERE"):/bench:ro" \
        "$IMAGE" php "$@"
}
php_version="$(run -r 'echo PHP_VERSION;')"
echo "PHP $php_version, $(run -r 'echo php_uname("m"), " ", trim(shell_exec("nproc")), " CPUs";')" >&2

run /bench/bench.php /scenarios "$@" | tee "$HERE/results/php.jsonl"
run -d opcache.enable_cli=1 -d opcache.jit=tracing -d opcache.jit_buffer_size=128M \
    /bench/bench.php /scenarios --cache=/tmp/twig-cache "$@" | tee "$HERE/results/php-jit.jsonl"
