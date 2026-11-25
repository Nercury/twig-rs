<?php
// Generates float_precision.json: (string) of doubles at ini precision 1..17 (zend_gcvt mode 2).
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/float_precision.gen.php > float_precision.json
//
// cases: [IEEE bits (hex, big-endian), precision 1, ..., precision 17]; a subset (every 19th value) of
// the float_format.json set.

declare(strict_types=1);
require __DIR__.'/float_values.inc.php';

$rows = [];
$i = 0;
foreach (float_values(1000) as $hex => $f) {
    if ($i++ % 19 !== 0) {
        continue;
    }
    $r = [(string) $hex];
    for ($p = 1; $p <= 17; $p++) {
        ini_set('precision', (string) $p);
        $r[] = @(string) $f;
    }
    $rows[] = $r;
}
echo "{\"generator\":\"float_precision.gen.php\",\"php\":\"", PHP_VERSION, "\",\n\"cases\":[\n";
emit_rows($rows);
echo "]}\n";
