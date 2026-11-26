#!/usr/bin/env bash
# Regenerates the twig-date oracle vectors with PHP 8.5.13 (image built by research/php-oracle).
#
#   crates/twig-date/tests/vectors/regen.sh            # all topics
#   crates/twig-date/tests/vectors/regen.sh format     # one topic
#
# Each <topic>.gen.php writes <topic>.json. PHP's clock is faked with fakenow.c (LD_PRELOAD; the generators set
# FAKE_SEC / FAKE_USEC with putenv()) so results that depend on "now" are reproducible.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="${TWIG_ORACLE_IMAGE:-twig-oracle:3.31.1}"
MOUNT="$HERE"
if command -v cygpath >/dev/null 2>&1; then MOUNT="$(cygpath -m "$HERE")"; fi
topics=("$@")
if [ ${#topics[@]} -eq 0 ]; then
    topics=(format construct modify diff interval timezone dumps methods functions period method_params)
fi
for t in "${topics[@]}"; do
    MSYS_NO_PATHCONV=1 docker run --rm -v "$MOUNT:/v:ro" "$IMAGE" sh -c \
        "gcc -shared -fPIC -O2 -o /tmp/fakenow.so /v/fakenow.c && LD_PRELOAD=/tmp/fakenow.so php -d memory_limit=-1 /v/$t.gen.php" \
        > "$HERE/$t.json"
    echo "$t.json: $(wc -c < "$HERE/$t.json") bytes"
done
