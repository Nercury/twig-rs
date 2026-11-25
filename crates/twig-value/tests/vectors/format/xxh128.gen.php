<?php
// Generates format/xxh128.json: hash('xxh128', ...) against PHP 8.5.13 (bundled xxHash 0.8.1).
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/format/xxh128.gen.php > format/xxh128.json
//
// lengths: [length, hash] for the input of `length` bytes ((i * 31 + length * 7) & 0xff) at i,
//          covering every code path of XXH3-128 (0-16, 17-128, 129-240 bytes, stripes, blocks)
// strings: [input (JSON text or {"hex"}), hash] for templates and other explicit inputs

declare(strict_types=0);
require __DIR__.'/classes.inc.php';

function input(int $length): string
{
    $s = '';
    for ($i = 0; $i < $length; $i++) {
        $s .= chr(($i * 31 + $length * 7) & 0xff);
    }
    return $s;
}

$lengths = range(0, 1100);
for ($n = 1101; $n < 20000; $n += 37) {
    $lengths[] = $n;
}
foreach ([65535, 65536, 65537, 100000, 1048576] as $n) {
    $lengths[] = $n;
}
$rows = [];
foreach ($lengths as $n) {
    $rows[] = [$n, hash('xxh128', input($n))];
}

$strings = [];
foreach (['', 'abc', 'Hi {{ x }', '{{ "Hello" }}', "{% if a %}\n{{ b|upper }}\n{% endif %}", "\0\xff\xfe",
    "\u{1f600} unicode \u{e9}", str_repeat('{{ x }}', 100)] as $s) {
    $strings[] = [out_enc($s), hash('xxh128', $s)];
}

echo json_encode(['generator' => 'format/xxh128.gen.php', 'php' => PHP_VERSION, 'lengths' => $rows,
    'strings' => $strings], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";
