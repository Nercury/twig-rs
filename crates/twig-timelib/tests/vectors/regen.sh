#!/usr/bin/env bash
# Regenerates the twig-timelib oracle vectors with PHP 8.5.13 (image built by research/php-oracle).
#
#   crates/twig-timelib/tests/vectors/regen.sh            # all topics
#   crates/twig-timelib/tests/vectors/regen.sh parse      # one topic
#
# Each <topic>.gen.php writes <topic>.json; inputs.json holds the string literals used as inputs.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="${TWIG_ORACLE_IMAGE:-twig-oracle:3.31.1}"
MOUNT="$HERE"
if command -v cygpath >/dev/null 2>&1; then MOUNT="$(cygpath -m "$HERE")"; fi
topics=("$@")
if [ ${#topics[@]} -eq 0 ]; then topics=(parse strtotime interval zones iso_interval setters); fi
for t in "${topics[@]}"; do
    MSYS_NO_PATHCONV=1 docker run --rm -v "$MOUNT:/v:ro" "$IMAGE" php -d memory_limit=-1 "/v/$t.gen.php" > "$HERE/$t.json"
    echo "$t.json: $(wc -c < "$HERE/$t.json") bytes"
done
