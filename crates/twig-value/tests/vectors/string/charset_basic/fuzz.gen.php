<?php
// fuzz.json: seeded random conversions mixing everything: charset names and aliases in random case, suffix variants,
// inputs built from valid characters of every class, raw bytes, invalid and truncated sequences and long runs (up to
// tens of thousands of characters, so that rounds, E2BIG and PHP's //IGNORE byte skipping interact). Inputs are
// recipes (see common.inc.php); outputs longer than 256 bytes are [length, fnv1a64].

require __DIR__ . '/pairs.inc.php';

// arguments (exploratory rounds): seed and number of cases; fuzz.json uses the defaults
mt_srand((int) ($argv[1] ?? 4242));
$count = (int) ($argv[2] ?? 12000);
$names = [
    'UTF-8' => ['UTF-8', 'utf8', 'ISO-10646/UTF8', 'Utf-8', ''],
    'UTF-16' => ['UTF-16', 'utf16'],
    'UTF-16BE' => ['UTF-16BE', 'utf16be'],
    'UTF-16LE' => ['UTF-16LE', 'UTF16LE'],
    'UTF-32' => ['UTF-32', 'utf32'],
    'UTF-32BE' => ['UTF-32BE', 'utf32be'],
    'UTF-32LE' => ['UTF-32LE', 'Utf32Le'],
    'UCS-2' => ['UCS-2', 'ucs2', 'ISO-10646/UCS2', 'osf00010100'],
    'UCS-2BE' => ['UCS-2BE', 'unicodebig'],
    'UCS-2LE' => ['UCS-2LE', 'unicodelittle'],
    'UCS-4' => ['UCS-4', 'ucs4', 'iso-10646', '10646-1:1993/ucs4'],
    'UCS-4BE' => ['UCS-4BE', 'csucs4'],
    'UCS-4LE' => ['UCS-4LE', 'ucs-4le'],
    'ASCII' => ['ASCII', 'us-ascii', 'ANSI_X3.4-1968', 'iso646-us', 'cp367', 'IBM891', 'csibm903'],
    'ISO-8859-1' => ['ISO-8859-1', 'latin1', 'L1', 'iso_8859-1:1987', 'cp819', '8859_1'],
];
$suffixes = ['', '', '//TRANSLIT', '//IGNORE', '//IGNORE', '//TRANSLIT//IGNORE', '//IGNORE//TRANSLIT', '//ignore',
    '//translit', '//TRANSLIT,IGNORE', '//IGNORE//', '/', '//IGNORE,TRANSLIT', '//X//IGNORE'];
$specials = [0xE9, 0x20AC, 0xBD, 0x2026, 0x1F600, 0x4E00, 0x200B, 0xFEFF, 0xFFFE, 0x10FFFF, 0x110000, 0x7FFFFFFF,
    0xD800, 0xDFFF, 0xE0001, 0x339F, 0x2F800, 0x1D400, 0xFFFD, 0x00, 0x7F, 0x80, 0xFF];
$invalid = [
    'UTF-8' => ['ff', 'c3', 'e282', 'c0af', 'eda080', 'f8888080', 'fe', '80', 'e0'],
    'UTF-16' => ['00dc', 'dc00', '00d8', 'd800', '00', 'fffe', 'feff'], 'UTF-16BE' => ['dc00', 'd800', '00', 'd800d800'],
    'UTF-16LE' => ['00dc', '00d8', '00'], 'UTF-32' => ['00001100', '00110000', '0000d800', '00', '0000feff', 'fffe0000'],
    'UTF-32BE' => ['00110000', '0000dc00', '0000'], 'UTF-32LE' => ['00001100', '00d80000', '000000'],
    'UCS-2' => ['00d8', '00dc', '00'], 'UCS-2BE' => ['d800', 'dfff', '00'], 'UCS-2LE' => ['00d8', '61'],
    'UCS-4' => ['80000000', 'ffffffff', '0000', '0000d800'], 'UCS-4BE' => ['80000001', '00'],
    'UCS-4LE' => ['00000080', '000000'], 'ASCII' => ['80', 'ff', 'e9'], 'ISO-8859-1' => ['ff', '80'],
];

$cases = [];
for ($i = 0; $i < $count; $i++) {
    $fromCs = CHARSETS[mt_rand(0, count(CHARSETS) - 1)];
    $toCs = CHARSETS[mt_rand(0, count(CHARSETS) - 1)];
    $from = $names[$fromCs][mt_rand(0, count($names[$fromCs]) - 1)];
    $to = $names[$toCs][mt_rand(0, count($names[$toCs]) - 1)] . $suffixes[mt_rand(0, count($suffixes) - 1)];
    $long = mt_rand(0, 9) === 0;
    $segments = [];
    for ($n = mt_rand(1, $long ? 8 : 6); $n > 0; $n--) {
        $r = mt_rand(0, 99);
        if ($r < 45) {
            $cps = [];
            for ($k = mt_rand(1, 6); $k > 0; $k--) {
                $cps[] = match (mt_rand(0, 4)) {
                    0, 1 => mt_rand(0x20, 0x7E),
                    2 => mt_rand(0x80, 0x2FF),
                    3 => mt_rand(0x300, 0xFFFD),
                    default => $specials[mt_rand(0, count($specials) - 1)],
                };
            }
            $cps = array_values(array_filter($cps, static fn (int $c): bool => representable($fromCs, $c)));
            $s = encode_in($fromCs, $cps);
            if ($s === null || $s === '') {
                continue;
            }
            if (($fromCs === 'UTF-16' || $fromCs === 'UTF-32') && $segments !== []) {
                $s = substr($s, $fromCs === 'UTF-16' ? 2 : 4); // the mark only at the start
            }
            $segments[] = $long && mt_rand(0, 2) === 0 ? ['rep', bin2hex($s), mt_rand(100, 9000)] : ['hex', bin2hex($s)];
        } elseif ($r < 70) {
            $list = $invalid[$fromCs];
            $segments[] = ['hex', $list[mt_rand(0, count($list) - 1)]];
        } elseif ($r < 85) {
            $s = '';
            for ($k = mt_rand(1, 6); $k > 0; $k--) {
                $s .= chr(mt_rand(0, 255));
            }
            $segments[] = ['hex', bin2hex($s)];
        } else {
            $unit = encode_in($fromCs, [mt_rand(0x20, 0x7E)]);
            if ($fromCs === 'UTF-16' || $fromCs === 'UTF-32') {
                $unit = substr((string) $unit, $fromCs === 'UTF-16' ? 2 : 4);
            }
            $segments[] = ['rep', bin2hex((string) $unit), $long ? mt_rand(1000, 12000) : mt_rand(1, 40)];
        }
    }
    $input = recipe($segments);
    $cases[] = [name_index($from), name_index($to), $segments, convert($from, $to, $input, strlen($input) > 256)];
}
emit('fuzz.gen.php', $cases);
