<?php
// Generates float_format.json: PHP float-to-string conversions of a deterministic set of doubles.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/float_format.gen.php > float_format.json
//
// cases: [IEEE bits (hex, big-endian), (string) at precision=14, var_export() at serialize_precision=-1]
// json:  [bits, json_encode() or null] for every 7th value.

declare(strict_types=1);
require __DIR__.'/float_values.inc.php';
ini_set('precision', '14');
ini_set('serialize_precision', '-1');

$rows = [];
$json = [];
$i = 0;
foreach (float_values(1000) as $hex => $f) {
    $hex = (string) $hex;
    $rows[] = [$hex, @(string) $f, var_export($f, true)];
    if ($i++ % 7 === 0) {
        $j = json_encode($f);
        $json[] = [$hex, false === $j ? null : $j];
    }
}
echo "{\"generator\":\"float_format.gen.php\",\"php\":\"", PHP_VERSION, "\",\n\"cases\":[\n";
emit_rows($rows);
echo "],\n\"json\":[\n";
emit_rows($json);
echo "]}\n";
