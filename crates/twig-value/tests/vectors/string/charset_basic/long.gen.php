<?php
// long.json: inputs of thousands of characters. glibc converts in rounds of 8160 characters, a round in which
// //IGNORE skipped an unrepresentable character ends the call early (PHP then skips one input byte, see
// php_iconv_string()), PHP's output buffer starts at the input length + 32 and grows by the input length (E2BIG
// restarts the rounds), //TRANSLIT replacements are written whole, and UTF-16/UTF-32 repeat their byte order mark
// before a replacement. The cases put special characters around those boundaries. Inputs are recipes (see
// common.inc.php), outputs [length, fnv1a64].

require __DIR__ . '/pairs.inc.php';

$cases = [];
$add = static function (string $from, string $to, array $segments) use (&$cases): void {
    $cases[] = [name_index($from), name_index($to), $segments, convert($from, $to, recipe($segments), true)];
};
$enc = static fn (string $cs, int ...$cps): string => bin2hex((string) encode_in($cs, $cps));

// [source, target, filler code point, special input (hex, in the source charset)]
$scenarios = [
    ['UTF-8', 'ISO-8859-1', 0x61, bin2hex("\u{20ac}")],
    ['UTF-8', 'ISO-8859-1', 0xE9, bin2hex("\u{20ac}")],
    ['UTF-8', 'ASCII', 0x61, bin2hex("\u{e9}")],
    ['UTF-8', 'ASCII', 0x20AC, bin2hex("\u{4e00}")],
    ['UTF-8', 'UCS-2', 0x61, bin2hex("\u{1F600}")],
    ['UTF-8', 'UCS-2BE', 0xE9, bin2hex("\u{10000}")],
    ['UTF-8', 'UTF-16LE', 0x61, '' . bin2hex("\xf4\x90\x80\x80")],
    ['UTF-8', 'UTF-16', 0x61, bin2hex("\xf4\x90\x80\x80")],
    ['UTF-8', 'UTF-32', 0x61, bin2hex("\xf4\x90\x80\x80")],
    ['UTF-8', 'UTF-16LE', 0x61, 'ff'],
    ['UTF-8', 'ISO-8859-1', 0x61, 'ff'],
    ['UTF-8', 'UTF-8', 0x61, 'c0af'],
    ['UCS-4', 'UTF-8', 0x61, '0000d800'],
    ['UCS-4', 'UTF-16', 0x61, '0000dc00'],
    ['UCS-4', 'UCS-2BE', 0x61, '0000d800'],
    ['UCS-4', 'UCS-2', 0x62, '0000d800'],
    ['UCS-4', 'ISO-8859-1', 0x61, '80000000'],
    ['UCS-4LE', 'ASCII', 0xE9, '00000080'],
    ['UTF-16', 'ISO-8859-1', 0x61, $enc('UTF-16LE', 0x20AC)],
    ['UTF-16BE', 'ASCII', 0x62, $enc('UTF-16BE', 0xE9)],
    ['UTF-16LE', 'UCS-2', 0x63, $enc('UTF-16LE', 0x1F600)],
    ['UTF-16LE', 'ISO-8859-1', 0x63, '00dc'],
    ['UTF-32BE', 'ISO-8859-1', 0x61, $enc('UTF-32BE', 0x20AC)],
    ['UCS-2', 'ASCII', 0x61, $enc('UCS-2', 0xE9)],
    ['UCS-2BE', 'UTF-8', 0x61, 'd800'],
    ['ISO-8859-1', 'ASCII', 0x61, 'e9'],
    ['ASCII', 'ISO-8859-1', 0x61, '80'],
];
$flags = ['//IGNORE', '//TRANSLIT//IGNORE', '//ignore', '//TRANSLIT', '', '//IGNORE//TRANSLIT'];
$round = 8160;
mt_srand(20261126);
foreach ($scenarios as [$from, $to, $filler, $special]) {
    $unit = $enc($from, $filler);
    if (str_starts_with($from, 'UTF-16') || str_starts_with($from, 'UTF-32')) {
        if ($from === 'UTF-16' || $from === 'UTF-32') {
            $unit = $enc($from . 'LE', $filler); // no mark per character
        }
    }
    $positions = [0, 1, $round - 2, $round - 1, $round, $round + 1, 2 * $round - 1, 2 * $round, 2 * $round + 1];
    foreach ($positions as $at) {
        foreach ([$round, 9000, 2 * $round + 5, 20000] as $total) {
            if ($at >= $total) {
                continue;
            }
            $segments = [['rep', $unit, $at], ['hex', $special], ['rep', $unit, $total - $at - 1]];
            foreach ($flags as $flag) {
                $add($from, $to . $flag, $segments);
            }
        }
    }
    // two specials, one per round, and a truncated character at the end
    $add($from, $to . '//IGNORE', [['rep', $unit, 100], ['hex', $special], ['rep', $unit, $round], ['hex', $special], ['rep', $unit, 50]]);
    $add($from, $to . '//IGNORE', [['rep', $unit, $round - 1], ['hex', $special], ['hex', substr($unit, 0, 2)]]);
    $add($from, $to . '//IGNORE', [['rep', $unit, $round], ['hex', $special], ['hex', substr($special, 0, 2)]]);
    // seeded random layouts
    for ($i = 0; $i < 12; $i++) {
        $segments = [];
        for ($n = mt_rand(2, 6); $n > 0; $n--) {
            $segments[] = ['rep', $unit, mt_rand(0, 9000)];
            $segments[] = ['hex', $special];
        }
        $add($from, $to . $flags[mt_rand(0, count($flags) - 1)], $segments);
    }
}

// //TRANSLIT replacements longer than their input around the E2BIG points of PHP's buffer
foreach ([[0xBD, 'ASCII'], [0x2026, 'ASCII'], [0x33AF, 'ASCII'], [0x339F, 'ISO-8859-1'], [0x1F600, 'UCS-2']] as [$cp, $to]) {
    foreach ([30, 31, 32, 33, 34, 100, 1000, 8159, 8160, 8161] as $n) {
        $add('UTF-8', "$to//TRANSLIT", [['rep', bin2hex(utf8($cp)), $n]]);
        $add('UTF-8', "$to//TRANSLIT", [['rep', '61', $n], ['rep', bin2hex(utf8($cp)), $n]]);
    }
    for ($i = 0; $i < 20; $i++) {
        $add('UTF-8', "$to//TRANSLIT//IGNORE", [['rep', '61', mt_rand(0, 200)], ['rep', bin2hex(utf8($cp)), mt_rand(1, 300)], ['hex', 'ff'], ['rep', bin2hex("\u{4e00}"), mt_rand(0, 50)]]);
    }
}

// the byte order mark written again before a replacement, where E2BIG cuts it off
foreach (['UTF-16', 'UTF-32', 'UTF-16LE'] as $to) {
    foreach ([1, 2, 3, 30, 31, 32, 33, 34, 35, 60, 64, 100] as $a) {
        foreach ([1, 2, 3, 10] as $b) {
            $add('UTF-8', "$to//TRANSLIT", [['rep', '61', $a], ['rep', 'f4908080', $b], ['rep', '61', $a]]);
        }
    }
    for ($i = 0; $i < 30; $i++) {
        $segments = [];
        for ($n = mt_rand(1, 8); $n > 0; $n--) {
            $segments[] = ['rep', '61', mt_rand(0, 120)];
            $segments[] = ['rep', 'f4908080', mt_rand(1, 4)];
        }
        $add('UTF-8', "$to//TRANSLIT", $segments);
        $add('UCS-4', "$to//TRANSLIT//IGNORE", [['rep', '00000061', mt_rand(0, 9000)], ['rep', '00110000', mt_rand(1, 3)], ['hex', '0000d800'], ['rep', '00000062', mt_rand(0, 9000)]]);
    }
}

// mixed random content in long strings, every target, //IGNORE
$fillers = [0x61, 0xE9, 0x20AC, 0x1F600, 0x4E00, 0xBD];
foreach (CHARSETS as $to) {
    for ($i = 0; $i < 8; $i++) {
        $segments = [];
        for ($n = mt_rand(3, 10); $n > 0; $n--) {
            $segments[] = ['rep', bin2hex(utf8($fillers[mt_rand(0, 5)])), mt_rand(1, 4000)];
            $segments[] = ['hex', ['ff', 'c3', 'f4908080', 'eda080', ''][mt_rand(0, 4)]];
        }
        $add('UTF-8', $to . ['//IGNORE', '//TRANSLIT//IGNORE', '//ignore'][mt_rand(0, 2)], $segments);
    }
}

emit('long.gen.php', $cases);
