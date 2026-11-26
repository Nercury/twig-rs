<?php
// codepoints.json: single characters, one iconv() call each, from UCS-4 (so that surrogates and values beyond Unicode
// reach the encoders): every BMP value, every value of planes 1 and 2 and of the tag block, the CJK compatibility
// supplement, every 31st value of planes 3 to 16, and the first values beyond U+10FFFF; into the targets with a
// smaller repertoire, plain, with //TRANSLIT (one character at a time, unlike translit.gen.php, which built the
// tables from batches) and with //IGNORE. Hashed per block of 256 values like sweep.json (token() in common.inc.php).

require __DIR__ . '/common.inc.php';

/** @return list<int> */
function code_points(string $kind): array
{
    if ($kind === 'bmp') {
        return range(0, 0xFFFF);
    }
    $v = range(0x10000, 0x2FFFF);
    for ($c = 0x30000; $c <= 0x10FFFF; $c += 31) {
        $v[] = $c;
    }
    array_push($v, ...range(0xE0000, 0xE01FF), ...range(0x2F800, 0x2FA1F), ...range(0x110000, 0x1100FF));
    return $v;
}

$targets = [
    'bmp' => ['ASCII', 'ASCII//TRANSLIT', 'ASCII//IGNORE', 'ISO-8859-1', 'ISO-8859-1//TRANSLIT', 'UCS-2//TRANSLIT',
        'UTF-8//TRANSLIT', 'UTF-16//TRANSLIT', 'UCS-2BE//IGNORE'],
    'astral' => ['ASCII', 'ASCII//TRANSLIT', 'ISO-8859-1//TRANSLIT', 'UCS-2', 'UCS-2//TRANSLIT', 'UCS-2BE//TRANSLIT',
        'UCS-2BE//IGNORE', 'UTF-16BE//TRANSLIT', 'UTF-32//TRANSLIT//IGNORE'],
];
$sets = [];
foreach ($targets as $kind => $tos) {
    $blocks = array_chunk(code_points($kind), 256);
    foreach ($tos as $to) {
        $hashes = [];
        foreach ($blocks as $block) {
            $t = '';
            foreach ($block as $c) {
                $t .= token('UCS-4', $to, pack('N', $c));
            }
            $hashes[] = hash('fnv1a64', $t);
        }
        $sets[] = ['to' => $to, 'kind' => $kind, 'blocks' => $hashes];
    }
}

echo '{"generator":"codepoints.gen.php","php":"', PHP_VERSION, "\",\n\"sets\":[\n";
foreach ($sets as $k => $s) {
    echo json_encode($s, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $k + 1 < count($sets) ? ",\n" : "\n";
}
echo "]}\n";
