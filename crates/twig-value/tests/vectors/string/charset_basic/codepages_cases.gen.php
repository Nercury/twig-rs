<?php
// codepages_cases.json: conversions from and into the codepages (the single-byte charsets with tables):
//   - every codepage into itself, six other codepages, UTF-8, UTF-16, ISO-8859-1 and ASCII, plain, //TRANSLIT and
//     //IGNORE, on a pool of inputs per source (all its characters, invalid bytes, seeded random strings);
//   - UTF-8 into every codepage on a pool of characters (transliterations, unrepresentable and tag characters, values
//     beyond Unicode), with every suffix;
//   - the decoders of CP1255 and CP1258, which hold back each character that a following one may combine with:
//     chains of marks, invalid bytes between a letter and its mark, the held character written by PHP's flush (with
//     a byte order mark, as a transliteration, or failing), E2BIG during the flush;
//   - long inputs: held characters, combinations and unrepresentable characters around the rounds of 8160 characters
//     and around the points where PHP's output buffer grows (E2BIG), where //IGNORE ends a call early and PHP skips
//     an input byte, and codepages as targets of //TRANSLIT replacements and of letters written with their marks.
// Inputs are recipes (see common.inc.php); the outputs of inputs longer than 64 bytes are [length, fnv1a64].

require __DIR__ . '/common.inc.php';

$cases = [];
$add = static function (string $from, string $to, array $segments) use (&$cases): void {
    $input = recipe($segments);
    $cases[] = [name_index($from), name_index($to), $segments, convert($from, $to, $input, strlen($input) > 64)];
};
$hex = static fn (string $s): array => [['hex', bin2hex($s)]];

/** The bytes charset $cs decodes on their own, and the characters they give. */
function repertoire(string $cs): array
{
    $bytes = [];
    for ($b = 0; $b < 256; $b++) {
        $GLOBALS['__diags'] = [];
        $r = iconv($cs, 'UCS-4BE', chr($b));
        if ($r !== false) {
            $bytes[$b] = unpack('N', $r)[1];
        }
    }
    return $bytes;
}

/** $cps in charset $cs, null when one of them cannot be represented. */
function encode_cps(string $cs, array $cps): ?string
{
    if ($cps === []) {
        return '';
    }
    $GLOBALS['__diags'] = [];
    $r = iconv('UCS-4BE', $cs, ucs4(...$cps));
    return $r === false ? null : $r;
}

mt_srand(20261127);

// every codepage into itself, six others (at distances 1, 3, 7, 15, 31 and 63 in the list) and the main Unicode
// charsets
foreach (CODEPAGES as $i => $from) {
    $targets = ['UTF-8', 'UTF-16', 'ISO-8859-1', 'ASCII'];
    foreach ([0, 1, 3, 7, 15, 31, 63] as $d) {
        $targets[] = CODEPAGES[($i + $d) % count(CODEPAGES)];
    }
    $rep = repertoire($from);
    $valid = array_keys($rep);
    $invalid = array_values(array_diff(range(0, 255), $valid));
    $pool = [implode('', array_map('chr', $valid))];
    for ($i = 0; $i < 4; $i++) {
        $s = '';
        for ($n = mt_rand(1, 10); $n > 0; $n--) {
            $s .= chr($valid[mt_rand(0, count($valid) - 1)]);
        }
        $pool[] = $s;
    }
    $s = 'a';
    foreach (array_slice($invalid, 0, 3) as $b) {
        $s .= chr($b) . 'b';
    }
    $pool[] = $s;
    foreach ($targets as $to) {
        foreach (['', '//TRANSLIT', '//IGNORE'] as $suffix) {
            foreach ($pool as $input) {
                $add($from, $to . $suffix, $hex($input));
            }
        }
    }
}

// UTF-8 into every codepage
$samples = [[0x61, 0xE9, 0x62], [0x20AC], [0xA0], [0xBD], [0x2026], [0x2018, 0x2019, 0x201C, 0x201D], [0x339F],
    [0x20BD], [0x20B4], [0x1F00, 0x1F71], [0x0401, 0x0451, 0x0490], [0x05D0, 0x05B7, 0xFB2E, 0xFB2C], [0x00C3, 0x1EA0],
    [0x0340, 0x0341], [0x4E00], [0x1F600], [0x1D400], [0xE0041, 0x61], [0xFEFF, 0x61], [0x200B], [0x0300, 0x0301],
    [0x41, 0x0300], [0xFB01], [0xF8FF, 0xE01E], [0x2206, 0x0394], [0x10FFFF], [0x110000], [0xD800]];
foreach (CODEPAGES as $to) {
    foreach (['', '//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE'] as $suffix) {
        foreach ($samples as $cps) {
            $add('UTF-8', $to . $suffix, $hex(implode('', array_map('utf8', $cps))));
        }
        $add('UCS-4BE', $to . $suffix, $hex(ucs4(0x61, 0xD800, 0x7FFFFFFF, 0x62)));
    }
}

// the combining decoders
$composing = [
    'CP1258' => ['bases' => ["A", "a", "\xc2", "\xd5", "\xa8", "\xdd"], 'marks' => ["\xcc", "\xec", "\xde", "\xd2", "\xf2"],
        'plain' => ["1", "\x80", "\xe9", " "], 'invalid' => "\x81", 'unmappable' => "\xd5"],
    'CP1255' => ['bases' => ["\xf9", "\xe0", "\xe9", "\xd6", "\xe5"], 'marks' => ["\xcc", "\xd1", "\xd2", "\xc7", "\xc4", "\xc9", "\xcf"],
        'plain' => ["1", "\x80", "A", " "], 'invalid' => "\xca", 'unmappable' => "\xf9"],
];
$flushTargets = ['UTF-8', 'UTF-16', 'UTF-32', 'UCS-2', 'ISO-8859-1', 'ISO-8859-1//TRANSLIT', 'ISO-8859-1//IGNORE',
    'ASCII//TRANSLIT', 'ASCII//TRANSLIT//IGNORE', 'UTF-16//TRANSLIT', 'CP1252//IGNORE', 'UTF-8//IGNORE'];
foreach ($composing as $cs => $c) {
    $short = [];
    foreach ($c['bases'] as $base) {
        $short[] = $base;
        foreach ($c['marks'] as $mark) {
            $short[] = $base . $mark;
            $short[] = $base . $mark . $mark;
            $short[] = $mark . $base;
            $short[] = $base . $c['invalid'] . $mark;
            $short[] = $base . $mark . 'z';
            foreach ($c['marks'] as $mark2) {
                $short[] = $base . $mark . $mark2;
            }
        }
        foreach ($c['plain'] as $p) {
            $short[] = $base . $p;
            $short[] = $p . $base;
        }
        $short[] = $base . $base;
        $short[] = $base . $c['invalid'];
        $short[] = $c['invalid'] . $base;
    }
    foreach ($c['marks'] as $mark) {
        $short[] = $mark;
        $short[] = $mark . $mark;
    }
    foreach ($flushTargets as $to) {
        foreach (array_values(array_unique($short)) as $s) {
            $add($cs, $to, $hex($s));
        }
    }
    // the held character as the last of a long run and the output buffer full when PHP flushes: 34 three-byte
    // characters make the output one input length (+ 32) longer than the input, so that the buffer is exactly full
    foreach ([0, 1, 2, 5, 40] as $two) {
        foreach ([32, 33, 34, 35, 36] as $three) {
            foreach (['UTF-8', 'UTF-8//TRANSLIT', 'UTF-16LE'] as $to) {
                $add($cs, $to, [['rep', '80', $three], ['rep', bin2hex("\xa9"), $two], ['hex', bin2hex($c['bases'][0])]]);
                $add($cs, $to, [['rep', '80', $three], ['rep', bin2hex("\xa9"), $two], ['hex', bin2hex($c['bases'][0] . $c['marks'][0])]]);
            }
        }
    }
}

// long inputs from the combining decoders: held characters and marks around the rounds, E2BIG and //IGNORE stops
$round = 8160;
foreach ($composing as $cs => $c) {
    $base = $c['bases'][0];
    $mark = $c['marks'][0];
    $fillers = ['31', bin2hex($base), bin2hex($base . $mark), bin2hex($mark), bin2hex($base . '1'), '80'];
    $specials = [bin2hex($c['unmappable']), '80', bin2hex($c['invalid']), bin2hex($base . $mark),
        bin2hex($c['invalid'] . $mark), bin2hex($base . $c['invalid'] . $mark), bin2hex($mark . $mark)];
    $flags = ['ISO-8859-1//IGNORE', 'ISO-8859-1//TRANSLIT', 'UTF-8', 'UTF-8//IGNORE', 'UTF-16//TRANSLIT',
        'ASCII//TRANSLIT//IGNORE', 'ISO-8859-1//ignore', 'CP1252//IGNORE'];
    foreach ($fillers as $filler) {
        // characters per filler: a letter and a plain character give two, anything else one
        $perFiller = $filler === bin2hex($base . '1') ? 2 : 1;
        foreach ([$round - 3, $round - 2, $round - 1, $round, $round + 1, 2 * $round - 1, 2 * $round] as $chars) {
            $count = intdiv($chars, $perFiller);
            foreach ($specials as $special) {
                foreach (['ISO-8859-1//IGNORE', 'UTF-8//IGNORE', 'ASCII//TRANSLIT//IGNORE'] as $to) {
                    $add($cs, $to, [['hex', '80'], ['rep', $filler, $count], ['hex', $special], ['rep', $filler, 50], ['hex', $special]]);
                    $add($cs, $to, [['rep', $filler, $count], ['hex', $special], ['rep', '31', 20]]);
                }
            }
        }
        for ($i = 0; $i < 10; $i++) {
            $segments = [];
            for ($n = mt_rand(2, 6); $n > 0; $n--) {
                $segments[] = ['rep', $filler, mt_rand(0, 9000)];
                $segments[] = ['hex', $specials[mt_rand(0, count($specials) - 1)]];
            }
            $add($cs, $flags[mt_rand(0, count($flags) - 1)], $segments);
        }
    }
    // E2BIG: three-byte UTF-8 output for one input byte; the buffer grows by the input length
    for ($i = 0; $i < 40; $i++) {
        $segments = [];
        for ($n = mt_rand(2, 8); $n > 0; $n--) {
            $segments[] = ['rep', ['80', bin2hex($base), bin2hex($base . $mark), bin2hex($mark), '31', 'a9'][mt_rand(0, 5)], mt_rand(1, 3000)];
            if (mt_rand(0, 1) === 1) {
                $segments[] = ['hex', $specials[mt_rand(0, count($specials) - 1)]];
            }
        }
        $add($cs, ['UTF-8', 'UTF-8//IGNORE', 'UTF-16', 'UTF-32//TRANSLIT', 'UCS-2'][mt_rand(0, 4)], $segments);
    }
}

// E2BIG at the first character of a call's second round, which starts with a held character: the first round writes
// 8160 four-byte characters (after a mark of four bytes for UTF-32) and leaves less room than a character in PHP's
// buffer of the input length + 32 bytes; the held character must survive into the next call
foreach (['CP1258' => 'AB', 'CP1255' => "àá"] as $cs => $pair) {
    foreach (['UCS-4BE' => 0, 'UTF-32' => 4, 'UCS-4BE//IGNORE' => 0, 'UTF-32//TRANSLIT' => 4] as $to => $mark) {
        for ($n = 32604 + $mark; $n <= 32614 + $mark; $n++) {
            $add($cs, $to, [['rep', '31', $round - 1], ['hex', bin2hex($pair)], ['rep', '31', $n - $round - 1]]);
        }
    }
}

// codepages as targets in long inputs: //TRANSLIT replacements and letters written with their marks around E2BIG
foreach ([['ISO-8859-1', 'CP1258', 'c3'], ['ISO-8859-1', 'CP1258', bin2hex("\xc3\xd5")], ['UTF-8', 'CP1255', bin2hex("\u{fb2c}")],
    ['UTF-8', 'KOI8-R//TRANSLIT', bin2hex("\u{20bd}")], ['UTF-8', 'ISO-8859-7//TRANSLIT', bin2hex("\u{1f00}")],
    ['ISO-8859-1', 'CP437//TRANSLIT', 'bc'], ['ISO-8859-1', 'MACINTOSH//TRANSLIT', 'bd'], ['UCS-2LE', 'CP1258//TRANSLIT', '1f00'],
    ['UTF-8', 'CP1251//IGNORE', bin2hex("\u{20ac}\u{e9}")], ['ISO-8859-1', 'CP1258//IGNORE', bin2hex("\xc3\xbd")]] as [$from, $to, $unit]) {
    foreach ([30, 31, 32, 33, 34, 100, 4000, 8159, 8160, 8161] as $n) {
        $add($from, $to, [['rep', $unit, $n]]);
        $add($from, $to, [['rep', '61', $n], ['rep', $unit, $n]]);
    }
    for ($i = 0; $i < 10; $i++) {
        $add($from, $to, [['rep', '61', mt_rand(0, 9000)], ['rep', $unit, mt_rand(1, 9000)], ['rep', '62', mt_rand(0, 100)]]);
    }
}

emit('codepages_cases.gen.php', $cases);
